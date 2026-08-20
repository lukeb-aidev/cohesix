// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Adapt admitted network devices to the isolated console-network child.
// Author: Lukas Bower

//! Root NIC adapter for the isolated console-network service.
//!
//! Root retains the admitted NIC and console policy. Ethernet, IP, TCP,
//! authentication, and framing are owned by the compiler-declared child. Every
//! crossing copies through the four fixed ABI pages; no root pointer or NIC cap
//! enters the child.

use core::fmt;
use core::fmt::Write as _;

use console_network_abi::{
    ExchangeKind, SendBatchBuilder, CONSOLE_PAYLOAD_BYTES, SEND_BATCH_MAX_RECORDS,
};
use heapless::{Deque, String as HeaplessString, Vec as HeaplessVec};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, Ipv4Address};

use super::{
    select_isolated_network_turn, select_isolated_response_turn, ConsoleLine,
    IsolatedNetworkLowerCursor, IsolatedNetworkLowerUnit, IsolatedNetworkTurnOutcome,
    IsolatedNetworkTurnSelection, IsolatedNetworkTurnUnit, NetConsoleDisconnectReason,
    NetConsoleEvent, NetCounters, NetDevice, NetPoller, NetStatusReport, NetTelemetry,
};
#[cfg(feature = "net-backend-virtio")]
use super::{ConsoleNetConfig, NET_STAGE};
use crate::console_network_service::{BoundaryError, ConsoleNetworkContainmentTurn, ServiceState};
use crate::drivers::driver_task_net::Cyw43DriverTaskDevice;
#[cfg(feature = "net-backend-virtio")]
use crate::drivers::virtio::net::{DriverError as VirtioDriverError, VirtioNetStatic};
use crate::hal::console_network::ConsoleNetworkRuntime;
use crate::hal::driver_task::{DriverServiceBudget, DriverServiceBudgetError, DriverTaskContract};
use crate::hal::{HalError, KernelHal};
use crate::observe::IngestSnapshot;
#[cfg(feature = "net-backend-virtio")]
use crate::rust_alloc::boxed::Box;
use crate::serial::DEFAULT_LINE_CAPACITY;

const LINE_QUEUE_DEPTH: usize = 8;
const EVENT_QUEUE_DEPTH: usize = 8;
const ISOLATED_NETWORK_TURN_FRAMES: u16 = 2;
const ISOLATED_NETWORK_TURN_BYTES: u32 =
    (console_network_abi::CONSOLE_PAYLOAD_BYTES + console_network_abi::ETHERNET_FRAME_BYTES) as u32;

#[derive(Clone, Debug, Eq, PartialEq)]
struct QueuedConsoleOutput {
    line: HeaplessString<DEFAULT_LINE_CAPACITY>,
    terminal: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ResponseLane {
    generation: u64,
    connection_id: u64,
    awaiting_batch_sequence: Option<u64>,
    terminal_sequence: Option<u64>,
    terminal_control_completed: bool,
    terminal_output_drained: bool,
    terminal_queued: bool,
}

impl ResponseLane {
    const fn new(generation: u64, connection_id: u64) -> Self {
        Self {
            generation,
            connection_id,
            awaiting_batch_sequence: None,
            terminal_sequence: None,
            terminal_control_completed: false,
            terminal_output_drained: false,
            terminal_queued: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConsoleNetworkContainmentDiagnostic {
    Fault {
        expected_generation: u64,
        observed_generation: u32,
        fault_class: crate::critical_tcb::FaultClass,
        sequence: u64,
    },
    LocalFault {
        generation: u64,
    },
    InvalidMailbox {
        generation: u64,
    },
    ContainmentFailed {
        generation: u64,
    },
    IncompleteProof {
        generation: u64,
    },
    Teardown {
        generation: u64,
    },
}

impl ConsoleNetworkContainmentDiagnostic {
    fn render(self) -> Result<HeaplessString<DEFAULT_LINE_CAPACITY>, fmt::Error> {
        let mut line = HeaplessString::new();
        match self {
            Self::Fault {
                expected_generation,
                observed_generation,
                fault_class,
                sequence,
            } if expected_generation == u64::from(observed_generation) => write!(
                line,
                "[console-network] generation={expected_generation} terminal-fault class={fault_class:?} sequence={sequence}"
            )?,
            Self::Fault {
                expected_generation,
                observed_generation,
                ..
            } => write!(
                line,
                "[console-network] fault generation mismatch expected={expected_generation} observed={observed_generation}"
            )?,
            Self::LocalFault { generation } => write!(
                line,
                "[console-network] generation={generation} terminal-fault source=local"
            )?,
            Self::InvalidMailbox { generation } => write!(
                line,
                "[console-network] generation={generation} fault-mailbox-invalid action=contain"
            )?,
            Self::ContainmentFailed { generation } => write!(
                line,
                "[console-network] terminal containment failed generation={generation} action=quarantine-no-replacement"
            )?,
            Self::IncompleteProof { generation } => write!(
                line,
                "[console-network] terminal containment proof incomplete generation={generation} action=quarantine-no-replacement"
            )?,
            Self::Teardown { generation } => write!(
                line,
                "CONSOLE_NETWORK_TEARDOWN generation={generation} tcb_suspended=yes scheduling_context_unbound=yes mappings_scrubbed=yes capabilities_revoked=yes objects_deleted=yes generation_fenced=yes state=terminal"
            )?,
        }
        Ok(line)
    }
}

/// Construction failure for the QEMU isolated console-network adapter.
#[derive(Debug)]
pub enum IsolatedConsoleInitError {
    /// Virtual NIC discovery or construction failed.
    #[cfg(feature = "net-backend-virtio")]
    Driver(VirtioDriverError),
    /// Child object, mapping, capability, or MCS construction failed.
    Hal(HalError),
    /// QEMU configuration disagrees with the generated child contract.
    InvalidConfig(&'static str),
}

impl fmt::Display for IsolatedConsoleInitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(feature = "net-backend-virtio")]
            Self::Driver(error) => write!(formatter, "{error}"),
            Self::Hal(error) => write!(formatter, "{error}"),
            Self::InvalidConfig(reason) => formatter.write_str(reason),
        }
    }
}

/// Sole network-console owner visible through [`NetPoller`].
pub struct IsolatedNetworkConsole<D: NetDevice> {
    device: D,
    runtime: ConsoleNetworkRuntime,
    mac: EthernetAddress,
    ip: Ipv4Address,
    prefix_len: u8,
    gateway: Option<Ipv4Address>,
    listen_port: u16,
    lines: Deque<ConsoleLine, LINE_QUEUE_DEPTH>,
    events: Deque<NetConsoleEvent, EVENT_QUEUE_DEPTH>,
    output: Deque<QueuedConsoleOutput, LINE_QUEUE_DEPTH>,
    response_lane: Option<ResponseLane>,
    pending_egress: Option<HeaplessVec<u8, { console_network_abi::ETHERNET_FRAME_BYTES }>>,
    lower_cursor: IsolatedNetworkLowerCursor,
    active_connection: Option<u64>,
    authenticated_connection: Option<u64>,
    listener_ready: bool,
    disconnect_requested: bool,
    disconnect_issued: bool,
    output_issued: bool,
    faulted: bool,
    graceful_teardown_pending: bool,
    terminal: bool,
    pending_containment_fault_diagnostic: Option<ConsoleNetworkContainmentDiagnostic>,
    pending_containment_failure_diagnostic: Option<ConsoleNetworkContainmentDiagnostic>,
    pending_containment_teardown_diagnostic: Option<ConsoleNetworkContainmentDiagnostic>,
    last_now_ms: u64,
    telemetry: NetTelemetry,
    counters: NetCounters,
    connection_bytes_read: u64,
    connection_bytes_written: u64,
    ingest_backpressure: u64,
    ingest_dropped: u64,
    profile_backend: &'static str,
    backend: &'static str,
    active_driver: &'static str,
    mode: &'static str,
    interface_policy: &'static str,
    active_interface: &'static str,
    address_source: &'static str,
    dhcp_phase: &'static str,
}

/// QEMU VirtIO specialization retaining its exact bounded driver seams.
#[cfg(feature = "net-backend-virtio")]
pub type IsolatedVirtioConsole = IsolatedNetworkConsole<VirtioNetStatic>;

/// Physical Pi CYW43 specialization used after root-only DHCP bootstrap.
pub type IsolatedCyw43Console = IsolatedNetworkConsole<Cyw43DriverTaskDevice>;

#[cfg(feature = "net-backend-virtio")]
impl IsolatedNetworkConsole<VirtioNetStatic> {
    /// Construct the NIC plus suspended compiler-declared child generation.
    pub fn new(
        hal: &mut KernelHal<'_>,
        config: ConsoleNetConfig,
    ) -> Result<Box<Self>, IsolatedConsoleInitError> {
        if config.listen_port != crate::console_network_service::generated_config().listener_port {
            return Err(IsolatedConsoleInitError::InvalidConfig(
                "console listener port disagrees with generated child",
            ));
        }
        if config.address.ip == [0; 4]
            || config.address.prefix_len == 0
            || config.address.prefix_len > 32
        {
            return Err(IsolatedConsoleInitError::InvalidConfig(
                "isolated QEMU console requires a static IPv4 address",
            ));
        }
        let mut device = VirtioNetStatic::create_with_stage(hal, &config, NET_STAGE)
            .map_err(IsolatedConsoleInitError::Driver)?;
        let mac = device.mac();
        let ip = Ipv4Address::from(config.address.ip);
        device.set_assigned_ipv4(ip);
        let runtime = hal
            .construct_console_network_runtime(
                1,
                mac.0,
                config.address.ip,
                config.address.prefix_len,
                config.address.gateway.unwrap_or([0; 4]),
                config.auth_token,
            )
            .map_err(IsolatedConsoleInitError::Hal)?;
        let gateway = config.address.gateway.map(Ipv4Address::from);
        Ok(Box::new(Self::from_existing(
            device,
            runtime,
            mac,
            ip,
            config.address.prefix_len,
            gateway,
            config.listen_port,
            "virtio-net",
            "virtio-net",
            "virtio-net",
            "static",
            "wired",
            "wired",
            "dev-virt-isolated-child",
            "disabled",
        )))
    }
}

impl<D: NetDevice> IsolatedNetworkConsole<D> {
    /// Bind an already-admitted NIC and finalized child generation.
    #[allow(clippy::too_many_arguments)]
    pub fn from_existing(
        device: D,
        runtime: ConsoleNetworkRuntime,
        mac: EthernetAddress,
        ip: Ipv4Address,
        prefix_len: u8,
        gateway: Option<Ipv4Address>,
        listen_port: u16,
        profile_backend: &'static str,
        backend: &'static str,
        active_driver: &'static str,
        mode: &'static str,
        interface_policy: &'static str,
        active_interface: &'static str,
        address_source: &'static str,
        dhcp_phase: &'static str,
    ) -> Self {
        Self {
            device,
            runtime,
            mac,
            ip,
            prefix_len,
            gateway,
            listen_port,
            lines: Deque::new(),
            events: Deque::new(),
            output: Deque::new(),
            response_lane: None,
            pending_egress: None,
            lower_cursor: IsolatedNetworkLowerCursor::new(),
            active_connection: None,
            authenticated_connection: None,
            listener_ready: false,
            disconnect_requested: false,
            disconnect_issued: false,
            output_issued: false,
            faulted: false,
            graceful_teardown_pending: false,
            terminal: false,
            pending_containment_fault_diagnostic: None,
            pending_containment_failure_diagnostic: None,
            pending_containment_teardown_diagnostic: None,
            last_now_ms: 0,
            telemetry: NetTelemetry {
                link_up: true,
                tx_drops: 0,
                last_poll_ms: 0,
            },
            counters: NetCounters::default(),
            connection_bytes_read: 0,
            connection_bytes_written: 0,
            ingest_backpressure: 0,
            ingest_dropped: 0,
            profile_backend,
            backend,
            active_driver,
            mode,
            interface_policy,
            active_interface,
            address_source,
            dhcp_phase,
        }
    }

    /// Resume the child only after the exact target fault registry is sealed.
    pub fn activate(&mut self) -> Result<(), HalError> {
        self.runtime.activate()?;
        self.telemetry.link_up = true;
        Ok(())
    }

    /// Whether a protocol, ABI, timeout, or critical-lane fault needs teardown.
    #[must_use]
    pub const fn faulted(&self) -> bool {
        self.faulted || matches!(self.runtime.boundary().state(), ServiceState::Faulted)
    }

    /// Whether a fault or validated graceful terminal still needs containment.
    #[must_use]
    pub const fn containment_required(&self) -> bool {
        !self.terminal
            && (self.faulted()
                || self.graceful_teardown_pending
                || self.runtime.containment_active())
    }

    /// Advance the exact child generation by one containment unit.
    pub fn contain_one_turn(
        &mut self,
        hal: &mut KernelHal<'_>,
    ) -> Result<ConsoleNetworkContainmentTurn, HalError> {
        let generation = self.runtime.generation();
        let turn = match self.runtime.contain_one_turn(hal) {
            Ok(turn) => turn,
            Err(error) => {
                if self.pending_containment_failure_diagnostic.is_none() {
                    self.pending_containment_failure_diagnostic =
                        Some(ConsoleNetworkContainmentDiagnostic::ContainmentFailed { generation });
                }
                return Err(error);
            }
        };
        match turn {
            ConsoleNetworkContainmentTurn::Complete(proof) if proof.complete() => {
                self.graceful_teardown_pending = false;
                self.terminal = true;
                self.listener_ready = false;
                if self.pending_containment_teardown_diagnostic.is_none() {
                    self.pending_containment_teardown_diagnostic =
                        Some(ConsoleNetworkContainmentDiagnostic::Teardown { generation });
                }
            }
            ConsoleNetworkContainmentTurn::Complete(_) => {
                if self.pending_containment_failure_diagnostic.is_none() {
                    self.pending_containment_failure_diagnostic =
                        Some(ConsoleNetworkContainmentDiagnostic::IncompleteProof { generation });
                }
            }
            ConsoleNetworkContainmentTurn::Idle
            | ConsoleNetworkContainmentTurn::Retry
            | ConsoleNetworkContainmentTurn::InProgress => {}
        }
        Ok(turn)
    }

    /// Consume the critical mailbox and advance one containment unit.
    pub fn contain_if_faulted(
        &mut self,
        hal: &mut KernelHal<'_>,
    ) -> Result<ConsoleNetworkContainmentTurn, HalError> {
        if self.terminal {
            return Ok(ConsoleNetworkContainmentTurn::Idle);
        }
        if !self.runtime.containment_active() {
            let generation = self.runtime.generation();
            let mut faulted = self.faulted();
            let mut diagnostic =
                faulted.then_some(ConsoleNetworkContainmentDiagnostic::LocalFault { generation });
            match crate::hal::critical_tcb::take_target_service_fault(
                crate::console_network_service::SERVICE_TASK_ID,
            ) {
                Ok(Some(record)) => {
                    diagnostic = Some(ConsoleNetworkContainmentDiagnostic::Fault {
                        expected_generation: generation,
                        observed_generation: record.identity.supervisor_generation,
                        fault_class: record.fault_class,
                        sequence: record.sequence,
                    });
                    faulted = true;
                }
                Ok(None) => {}
                Err(crate::hal::critical_tcb::CriticalTcbConstructionError::FaultHandoff(
                    crate::critical_tcb::FaultHandoffError::Contended,
                )) => {
                    // The critical mailbox is durable. Contention means another
                    // bounded root-control turn owns its lock, so retry without
                    // manufacturing a service fault or losing the record. A
                    // simultaneous local fault cannot bypass this retry: once
                    // latched, later turns intentionally stop taking mail.
                    return Ok(ConsoleNetworkContainmentTurn::Retry);
                }
                Err(_) => {
                    diagnostic =
                        Some(ConsoleNetworkContainmentDiagnostic::InvalidMailbox { generation });
                    faulted = true;
                }
            }
            if !faulted {
                return Ok(ConsoleNetworkContainmentTurn::Idle);
            }
            self.faulted = true;
            self.listener_ready = false;
            if self.pending_containment_fault_diagnostic.is_none() {
                self.pending_containment_fault_diagnostic = diagnostic;
            }
            if let Err(error) = self.runtime.begin_containment() {
                if self.pending_containment_failure_diagnostic.is_none() {
                    self.pending_containment_failure_diagnostic =
                        Some(ConsoleNetworkContainmentDiagnostic::ContainmentFailed { generation });
                }
                return Err(error);
            }
            return Ok(ConsoleNetworkContainmentTurn::InProgress);
        }
        self.contain_one_turn(hal)
    }

    fn pending_containment_diagnostic(&self) -> Option<ConsoleNetworkContainmentDiagnostic> {
        self.pending_containment_fault_diagnostic
            .or(self.pending_containment_failure_diagnostic)
            .or(self.pending_containment_teardown_diagnostic)
    }

    fn pending_containment_diagnostic_line(&self) -> Option<HeaplessString<DEFAULT_LINE_CAPACITY>> {
        self.pending_containment_diagnostic()?.render().ok()
    }

    fn commit_containment_diagnostic(&mut self, expected_line: &str) -> bool {
        let Some(expected) = self.pending_containment_diagnostic() else {
            return false;
        };
        let Ok(rendered) = expected.render() else {
            return false;
        };
        if rendered.as_str() != expected_line {
            return false;
        }
        if self.pending_containment_fault_diagnostic == Some(expected) {
            self.pending_containment_fault_diagnostic = None;
        } else if self.pending_containment_failure_diagnostic == Some(expected) {
            self.pending_containment_failure_diagnostic = None;
        } else if self.pending_containment_teardown_diagnostic == Some(expected) {
            self.pending_containment_teardown_diagnostic = None;
        } else {
            return false;
        }
        true
    }

    /// Exact active child generation.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.runtime.generation()
    }

    /// Ethernet address owned by the admitted virtual NIC.
    #[must_use]
    pub const fn hardware_address(&self) -> EthernetAddress {
        self.mac
    }

    /// Static QEMU IPv4 address passed to the sealed child descriptor.
    #[must_use]
    pub const fn ipv4_address(&self) -> Ipv4Address {
        self.ip
    }

    /// Static QEMU prefix length.
    #[must_use]
    pub const fn prefix_len(&self) -> u8 {
        self.prefix_len
    }

    /// Static QEMU gateway, when configured.
    #[must_use]
    pub const fn gateway(&self) -> Option<Ipv4Address> {
        self.gateway
    }

    fn fail_closed(&mut self, reason: &'static str) {
        if self.faulted {
            return;
        }
        log::error!(
            "[console-network] generation={} fail-closed reason={reason}",
            self.runtime.generation()
        );
        let _ = self.runtime.signal_revoke();
        self.faulted = true;
        self.listener_ready = false;
        self.active_connection = None;
        self.authenticated_connection = None;
        self.disconnect_requested = false;
        self.disconnect_issued = false;
        self.lines.clear();
        self.events.clear();
        self.output.clear();
        self.response_lane = None;
        self.pending_egress = None;
    }

    fn queue_console_output(&mut self, line: &str, terminal: bool) -> bool {
        if self.faulted || self.terminal {
            return false;
        }
        let connection_id = match self.authenticated_connection {
            Some(connection_id) => connection_id,
            None => return false,
        };
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            return true;
        }
        let generation = self.runtime.generation();
        let lane = self
            .response_lane
            .get_or_insert_with(|| ResponseLane::new(generation, connection_id));
        if lane.generation != generation
            || lane.connection_id != connection_id
            || lane.terminal_queued
        {
            return false;
        }
        let mut bounded = HeaplessString::new();
        if bounded.push_str(line).is_err()
            || self
                .output
                .push_back(QueuedConsoleOutput {
                    line: bounded,
                    terminal,
                })
                .is_err()
        {
            return false;
        }
        if terminal {
            if let Some(lane) = self.response_lane.as_mut() {
                lane.terminal_queued = true;
            }
        }
        true
    }

    fn complete_response_lane_if_drained(&mut self) {
        let complete = self.response_lane.is_some_and(|lane| {
            lane.terminal_sequence.is_some()
                && lane.terminal_control_completed
                && lane.terminal_output_drained
                && lane.awaiting_batch_sequence.is_none()
                && self.output.is_empty()
                && !self.runtime.publication_ack_pending()
                && self.pending_egress.is_none()
        });
        if complete {
            self.response_lane = None;
            self.output_issued = false;
        }
    }

    fn handle_event(&mut self, event: crate::console_network_service::ConsoleNetworkEvent) {
        let connection_id = event.connection_id();
        match event.kind() {
            ExchangeKind::Ready => {
                self.listener_ready = true;
                self.disconnect_requested = false;
                self.disconnect_issued = false;
            }
            ExchangeKind::Connected => {
                // A new child connection identity cannot inherit commands
                // retained for any earlier authenticated peer.
                self.lines.clear();
                self.active_connection = Some(connection_id);
                self.authenticated_connection = None;
                self.disconnect_requested = false;
                self.disconnect_issued = false;
                self.output_issued = false;
                self.response_lane = None;
                self.connection_bytes_read = 0;
                self.connection_bytes_written = 0;
                if self
                    .events
                    .push_back(NetConsoleEvent::Connected {
                        conn_id: connection_id,
                        peer: None,
                    })
                    .is_err()
                {
                    self.fail_closed("connection-event-backpressure");
                }
                self.counters.tcp_accepts = self.counters.tcp_accepts.saturating_add(1);
            }
            ExchangeKind::Authenticated => {
                if self.active_connection != Some(connection_id) {
                    self.fail_closed("authenticated-connection-mismatch");
                    return;
                }
                self.authenticated_connection = Some(connection_id);
                if self
                    .events
                    .push_back(NetConsoleEvent::Authenticated {
                        conn_id: connection_id,
                    })
                    .is_err()
                {
                    self.fail_closed("authentication-event-backpressure");
                }
                self.counters.tcp_auth_sessions = self.counters.tcp_auth_sessions.saturating_add(1);
            }
            ExchangeKind::Command => {
                if self.authenticated_connection != Some(connection_id) {
                    self.fail_closed("command-before-authentication");
                    return;
                }
                let Ok(payload) = event.payload() else {
                    self.fail_closed("command-utf8");
                    return;
                };
                let mut line = HeaplessString::new();
                if line.push_str(payload).is_err()
                    || self
                        .lines
                        .push_back(ConsoleLine::for_connection(
                            line,
                            event.now_ms(),
                            connection_id,
                        ))
                        .is_err()
                {
                    self.ingest_backpressure = self.ingest_backpressure.saturating_add(1);
                    self.fail_closed("command-queue-backpressure");
                    return;
                }
                self.connection_bytes_read = self
                    .connection_bytes_read
                    .saturating_add(payload.len() as u64);
                self.counters.tcp_rx_bytes = self
                    .counters
                    .tcp_rx_bytes
                    .saturating_add(payload.len() as u64);
                self.counters.tcp_console_recv_ready =
                    self.counters.tcp_console_recv_ready.saturating_add(1);
            }
            ExchangeKind::Disconnected => {
                // Root observes lifecycle events before command lines. Retire
                // every not-yet-dispatched command with the disconnected
                // identity so it cannot execute after a replacement connects.
                self.lines.clear();
                let reason = if self.disconnect_requested {
                    NetConsoleDisconnectReason::Quit
                } else {
                    NetConsoleDisconnectReason::Eof
                };
                if self
                    .events
                    .push_back(NetConsoleEvent::Disconnected {
                        conn_id: connection_id,
                        reason,
                        bytes_read: self.connection_bytes_read,
                        bytes_written: self.connection_bytes_written,
                    })
                    .is_err()
                {
                    self.fail_closed("disconnect-event-backpressure");
                    return;
                }
                self.active_connection = None;
                self.authenticated_connection = None;
                self.disconnect_requested = false;
                self.disconnect_issued = false;
                self.output_issued = false;
                self.output.clear();
                self.response_lane = None;
            }
            ExchangeKind::Backpressure => self.fail_closed("child-event-backpressure"),
            ExchangeKind::ControlCompleted => {
                if let Some(lane) = self.response_lane.as_mut() {
                    if lane.awaiting_batch_sequence != Some(event.related_sequence()) {
                        self.fail_closed("response-completion-sequence");
                        return;
                    }
                    if lane.terminal_sequence == Some(event.related_sequence()) {
                        lane.terminal_control_completed = true;
                    }
                }
            }
            ExchangeKind::OutputDrained => {
                if let Some(lane) = self.response_lane.as_mut() {
                    if lane.generation != self.runtime.generation()
                        || lane.connection_id != connection_id
                    {
                        self.fail_closed("response-drain-identity");
                        return;
                    }
                    if lane.awaiting_batch_sequence != Some(event.related_sequence()) {
                        self.fail_closed("response-drain-sequence");
                        return;
                    }
                    lane.awaiting_batch_sequence = None;
                    if lane.terminal_sequence == Some(event.related_sequence()) {
                        lane.terminal_output_drained = true;
                    }
                }
            }
            ExchangeKind::Rejected | ExchangeKind::PacketConsumed => {}
            ExchangeKind::ShutdownComplete => {
                // Terminal child shutdown is also an input-authority boundary.
                self.lines.clear();
                // The child has consumed its last credit and parked, but its
                // TCB/SC/mappings/caps are not terminal until bounded root
                // containment publishes the complete teardown proof.
                self.graceful_teardown_pending = true;
                self.listener_ready = false;
                self.active_connection = None;
                self.authenticated_connection = None;
                self.disconnect_requested = false;
                self.disconnect_issued = false;
                self.response_lane = None;
            }
            ExchangeKind::SendLine | ExchangeKind::SendBatch | ExchangeKind::Disconnect => {
                self.fail_closed("child-published-root-control-kind")
            }
        }
    }

    fn poll_child_output(&mut self) -> bool {
        let turn = match self.runtime.poll_turn() {
            Ok(turn) => turn,
            Err(_) => {
                self.fail_closed("child-output-record");
                return false;
            }
        };
        let mut activity = false;
        if let Some(event) = turn.event {
            activity = true;
            self.handle_event(event);
            if self.faulted {
                return activity;
            }
            if self.graceful_teardown_pending {
                if turn.egress.is_some() {
                    // One global credit cannot authorize ShutdownComplete and
                    // an egress publication together. Treat the copied pair as
                    // a protocol violation and contain it without ACK.
                    self.fail_closed("terminal-egress-coalescing");
                    return activity;
                }
                if self.runtime.retire_terminal_publication().is_err() {
                    self.fail_closed("terminal-publication-retirement");
                    return activity;
                }
                if self.runtime.begin_containment().is_err() {
                    self.fail_closed("terminal-containment-start");
                }
                return activity;
            }
        }
        if let Some(egress) = turn.egress {
            activity = true;
            if self.pending_egress.is_some() {
                self.fail_closed("egress-overwrite");
                return activity;
            }
            self.pending_egress = Some(egress);
        }
        activity
    }

    fn transmit_pending_egress(&mut self, timestamp: Instant) -> bool {
        let Some(frame) = self.pending_egress.take() else {
            return false;
        };
        if !self
            .device
            .transmit_isolated_frame(timestamp, frame.as_slice())
        {
            self.pending_egress = Some(frame);
            return false;
        }
        true
    }

    fn stage_one_ingress(&mut self) -> bool {
        if self.pending_egress.is_some() || !self.runtime.ingress_available() {
            return false;
        }
        self.device.begin_smoltcp_rx_transaction();
        let staged = {
            let runtime = &mut self.runtime;
            self.device.consume_isolated_rx(
                Instant::from_millis(self.last_now_ms.min(i64::MAX as u64) as i64),
                |packet| {
                    if packet.is_empty() {
                        Ok(None)
                    } else {
                        runtime.stage_ingress(packet).map(Some)
                    }
                },
            )
        };
        self.device.end_smoltcp_rx_transaction();
        match staged {
            Some(Ok(Some(_))) => true,
            Some(Ok(None)) | None => false,
            Some(Err(BoundaryError::Backpressure)) => {
                self.ingest_backpressure = self.ingest_backpressure.saturating_add(1);
                false
            }
            Some(Err(_)) => {
                self.ingest_dropped = self.ingest_dropped.saturating_add(1);
                self.fail_closed("ingress-publication");
                false
            }
        }
    }

    fn stage_one_output(&mut self) -> bool {
        if !self.runtime.control_available()
            || self
                .response_lane
                .is_some_and(|lane| lane.awaiting_batch_sequence.is_some())
        {
            return false;
        }
        if self.output.is_empty() {
            return false;
        }
        let mut storage = [0u8; CONSOLE_PAYLOAD_BYTES];
        let mut builder = SendBatchBuilder::new(&mut storage);
        let mut count = 0usize;
        let mut bytes = 0usize;
        let mut terminal = false;
        for queued in self.output.iter().take(SEND_BATCH_MAX_RECORDS) {
            match builder.try_push_line(queued.line.as_str()) {
                Ok(true) => {
                    count = count.saturating_add(1);
                    bytes = bytes.saturating_add(queued.line.len());
                    terminal |= queued.terminal;
                }
                Ok(false) => break,
                Err(_) => {
                    self.fail_closed("output-batch-encoding");
                    return false;
                }
            }
        }
        let payload = match builder.finish() {
            Ok(payload) => payload,
            Err(_) => {
                self.fail_closed("output-batch-empty");
                return false;
            }
        };
        match self
            .runtime
            .stage_authorized_batch(payload, self.last_now_ms)
        {
            Ok(sequence) => {
                for _ in 0..count {
                    let _ = self.output.pop_front();
                }
                if let Some(lane) = self.response_lane.as_mut() {
                    lane.awaiting_batch_sequence = Some(sequence);
                    if terminal {
                        lane.terminal_sequence = Some(sequence);
                    }
                }
                self.output_issued = true;
                self.connection_bytes_written =
                    self.connection_bytes_written.saturating_add(bytes as u64);
                self.counters.tcp_tx_bytes =
                    self.counters.tcp_tx_bytes.saturating_add(bytes as u64);
                true
            }
            Err(BoundaryError::Backpressure) => false,
            Err(_) => {
                self.fail_closed("output-publication");
                false
            }
        }
    }

    fn stage_disconnect_if_drained(&mut self) -> bool {
        let Some(connection_id) = self.active_connection else {
            self.disconnect_requested = false;
            self.disconnect_issued = false;
            return false;
        };
        if !self.disconnect_requested
            || self.disconnect_issued
            || self.response_lane.is_some()
            || !self.output.is_empty()
            || !self.runtime.control_available()
        {
            return false;
        }
        if self.output_issued && !self.runtime.console_output_drained(connection_id) {
            return false;
        }
        match self.runtime.stage_disconnect(self.last_now_ms) {
            Ok(_) => {
                self.disconnect_issued = true;
                true
            }
            Err(BoundaryError::Backpressure) => false,
            Err(_) => {
                self.fail_closed("disconnect-publication");
                false
            }
        }
    }

    fn refresh_device_counters(&mut self) {
        let device = self.device.counters();
        self.telemetry.tx_drops = self.device.tx_drop_count();
        self.counters.rx_packets = device.rx_packets;
        self.counters.tx_packets = device.tx_packets;
        self.counters.rx_used_advances = device.rx_used_advances;
        self.counters.tx_used_advances = device.tx_used_advances;
        self.counters.tx_submit = device.tx_submit;
        self.counters.tx_complete = device.tx_complete;
        self.counters.tx_free = device.tx_free;
        self.counters.tx_in_flight = device.tx_in_flight;
        self.counters.tx_double_submit = device.tx_double_submit;
        self.counters.tx_zero_len_attempt = device.tx_zero_len_attempt;
    }

    #[inline(never)]
    fn poll_deferred_diagnostic_unit(&mut self) -> IsolatedNetworkTurnOutcome {
        // A successful TX owned the prior Network visit. Drain its one compact
        // routine record before another operation can overwrite that slot.
        let emitted = self.device.emit_one_isolated_deferred_tx_diagnostic();
        debug_assert!(emitted, "selected deferred diagnostic must exist");
        IsolatedNetworkTurnOutcome::complete(emitted)
    }

    #[inline(never)]
    fn poll_transmit_egress_unit(&mut self, now_ms: u64) -> IsolatedNetworkTurnOutcome {
        // One bounded reclaim plus one atomic publish/notify attempt. Success
        // and backpressure both return through the outer replenishment seam.
        let timestamp = Instant::from_millis(now_ms.min(i64::MAX as u64) as i64);
        IsolatedNetworkTurnOutcome::complete(self.transmit_pending_egress(timestamp))
    }

    #[inline(never)]
    fn poll_observe_child_unit(&mut self) -> IsolatedNetworkTurnOutcome {
        IsolatedNetworkTurnOutcome::complete(self.poll_child_output())
    }

    #[inline(never)]
    fn poll_acknowledge_publication_unit(&mut self) -> IsolatedNetworkTurnOutcome {
        match self.runtime.acknowledge_publication() {
            Ok(()) => IsolatedNetworkTurnOutcome::child_signaled(false),
            Err(_) => {
                self.fail_closed("publication-ack");
                IsolatedNetworkTurnOutcome::complete(false)
            }
        }
    }

    #[inline(never)]
    fn poll_stage_output_unit(&mut self) -> IsolatedNetworkTurnOutcome {
        IsolatedNetworkTurnOutcome::child_signal_attempt(self.stage_one_output())
    }

    #[inline(never)]
    fn poll_disconnect_unit(&mut self) -> IsolatedNetworkTurnOutcome {
        IsolatedNetworkTurnOutcome::child_signal_attempt(self.stage_disconnect_if_drained())
    }

    #[inline(never)]
    fn poll_ingress_unit(&mut self) -> IsolatedNetworkTurnOutcome {
        IsolatedNetworkTurnOutcome::child_signal_attempt(self.stage_one_ingress())
    }

    #[inline(never)]
    fn poll_service_tick_unit(&mut self) -> IsolatedNetworkTurnOutcome {
        match self.runtime.service_tick() {
            Ok(()) => IsolatedNetworkTurnOutcome::child_signaled(false),
            Err(_) => {
                self.fail_closed("service-tick");
                IsolatedNetworkTurnOutcome::complete(false)
            }
        }
    }

    #[inline(never)]
    fn poll_response_turn(&mut self, now_ms: u64) -> bool {
        self.last_now_ms = now_ms;
        self.telemetry.last_poll_ms = now_ms;
        if self.faulted || self.terminal || !self.runtime.activated() {
            self.telemetry.link_up = false;
            return false;
        }
        self.counters.smoltcp_polls = self.counters.smoltcp_polls.saturating_add(1);

        let lane = self.response_lane;
        let stage_output_ready = !self.output.is_empty()
            && self.runtime.control_available()
            && lane.is_some_and(|lane| lane.awaiting_batch_sequence.is_none());
        let response_progress_outstanding =
            lane.is_some_and(|lane| lane.awaiting_batch_sequence.is_some() || lane.terminal_queued);
        let selection = select_isolated_response_turn(
            self.pending_egress.is_some(),
            self.runtime.publication_ack_pending(),
            stage_output_ready,
            response_progress_outstanding,
            self.lower_cursor,
        );
        self.lower_cursor = selection.successor();
        let outcome = match selection.unit() {
            IsolatedNetworkTurnUnit::AcknowledgePublication => {
                self.poll_acknowledge_publication_unit()
            }
            IsolatedNetworkTurnUnit::TransmitEgress => self.poll_transmit_egress_unit(now_ms),
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::ObserveChild) => {
                self.poll_observe_child_unit()
            }
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::StageOutput) => {
                self.poll_stage_output_unit()
            }
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::Ingress) => {
                self.poll_ingress_unit()
            }
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::ServiceTick) => {
                self.poll_service_tick_unit()
            }
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::Disconnect)
            | IsolatedNetworkTurnUnit::DeferredDiagnostic => {
                self.fail_closed("invalid-response-turn-unit");
                IsolatedNetworkTurnOutcome::complete(false)
            }
        };
        let (lower_cursor, activity) = selection.finish(outcome);
        self.lower_cursor =
            if selection.unit() == IsolatedNetworkTurnUnit::TransmitEgress && activity {
                // A retained child TCP egress publication has now crossed the NIC
                // boundary. The only useful unobserved reciprocal work is exact
                // host ingress; skip a blind child-notification observation.
                IsolatedNetworkLowerCursor::for_unit(IsolatedNetworkLowerUnit::Ingress)
            } else {
                lower_cursor
            };
        self.complete_response_lane_if_drained();
        self.refresh_device_counters();
        activity && !self.faulted
    }
}

impl<D: NetDevice> NetPoller for IsolatedNetworkConsole<D> {
    #[inline(never)]
    fn poll(&mut self, now_ms: u64) -> bool {
        self.last_now_ms = now_ms;
        self.telemetry.last_poll_ms = now_ms;
        if self.faulted || self.terminal || !self.runtime.activated() {
            self.telemetry.link_up = false;
            return false;
        }
        self.counters.smoltcp_polls = self.counters.smoltcp_polls.saturating_add(1);

        let selection: IsolatedNetworkTurnSelection = select_isolated_network_turn(
            self.device.isolated_deferred_tx_diagnostic_pending(),
            self.pending_egress.is_some(),
            self.runtime.publication_ack_pending(),
            self.lower_cursor,
        );
        // Commit the ordinary successor before any selected unit can block or
        // fault. Only a successful child notification may force ObserveChild.
        self.lower_cursor = selection.successor();
        let outcome = match selection.unit() {
            IsolatedNetworkTurnUnit::AcknowledgePublication => {
                self.poll_acknowledge_publication_unit()
            }
            IsolatedNetworkTurnUnit::DeferredDiagnostic => self.poll_deferred_diagnostic_unit(),
            IsolatedNetworkTurnUnit::TransmitEgress => self.poll_transmit_egress_unit(now_ms),
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::ObserveChild) => {
                self.poll_observe_child_unit()
            }
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::StageOutput) => {
                self.poll_stage_output_unit()
            }
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::Disconnect) => {
                self.poll_disconnect_unit()
            }
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::Ingress) => {
                self.poll_ingress_unit()
            }
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::ServiceTick) => {
                self.poll_service_tick_unit()
            }
        };
        let (lower_cursor, activity) = selection.finish(outcome);
        self.lower_cursor = lower_cursor;
        self.complete_response_lane_if_drained();
        if self.faulted {
            self.refresh_device_counters();
            return false;
        }
        self.refresh_device_counters();
        activity
    }

    fn poll_with_budget(
        &mut self,
        now_ms: u64,
        budget: &mut DriverServiceBudget,
    ) -> Result<bool, DriverServiceBudgetError> {
        // One child observation may coalesce one event and one egress record.
        // Charge both bounded records before any observation or device side
        // effect; every other selected unit is conservatively covered and
        // returns through the outer yield.
        budget.charge_ops(1)?;
        budget.charge_frames(ISOLATED_NETWORK_TURN_FRAMES)?;
        budget.charge_bytes(ISOLATED_NETWORK_TURN_BYTES)?;
        Ok(self.poll(now_ms))
    }

    fn poll_console_response_with_budget(
        &mut self,
        now_ms: u64,
        budget: &mut DriverServiceBudget,
    ) -> Result<bool, DriverServiceBudgetError> {
        budget.charge_ops(1)?;
        budget.charge_frames(ISOLATED_NETWORK_TURN_FRAMES)?;
        budget.charge_bytes(ISOLATED_NETWORK_TURN_BYTES)?;
        Ok(self.poll_response_turn(now_ms))
    }

    fn driver_task_contract(&self) -> DriverTaskContract {
        D::driver_task_contract()
    }

    fn telemetry(&self) -> NetTelemetry {
        self.telemetry
    }

    fn stats(&self) -> NetCounters {
        self.counters
    }

    fn drain_console_lines(&mut self, now_ms: u64, visitor: &mut dyn FnMut(ConsoleLine)) {
        let _ = self.drain_console_lines_bounded(now_ms, usize::MAX, visitor);
    }

    fn drain_console_lines_bounded(
        &mut self,
        _now_ms: u64,
        max_lines: usize,
        visitor: &mut dyn FnMut(ConsoleLine),
    ) -> usize {
        let mut count = 0usize;
        while count < max_lines {
            let Some(line) = self.lines.pop_front() else {
                break;
            };
            visitor(line);
            count = count.saturating_add(1);
        }
        count
    }

    fn send_console_line(&mut self, line: &str) -> bool {
        self.queue_console_output(line, false)
    }

    fn send_console_terminal_line(&mut self, line: &str) -> bool {
        self.queue_console_output(line, true)
    }

    fn bounded_console_response_identity(&self) -> Option<super::ConsoleResponseIdentity> {
        let connection_id = self.authenticated_connection?;
        if self.faulted
            || self.terminal
            || self.active_connection != Some(connection_id)
            || !self.runtime.activated()
        {
            return None;
        }
        Some(super::ConsoleResponseIdentity {
            generation: self.runtime.generation(),
            connection_id,
        })
    }

    fn console_response_lane(&self) -> Option<super::ConsoleResponseLane> {
        let lane = self.response_lane?;
        Some(super::ConsoleResponseLane {
            generation: lane.generation,
            connection_id: lane.connection_id,
            queued_lines: self.output.len(),
            available_lines: LINE_QUEUE_DEPTH.saturating_sub(self.output.len()),
            awaiting_batch_drain: lane.awaiting_batch_sequence.is_some(),
            terminal_queued: lane.terminal_queued,
        })
    }

    fn request_disconnect(&mut self) {
        if self.active_connection.is_some() && !self.faulted && !self.terminal {
            self.disconnect_requested = true;
        }
    }

    fn console_output_drained(&self, connection_id: u64) -> bool {
        !self.faulted
            && self.output.is_empty()
            && self.pending_egress.is_none()
            && self.response_lane.is_none()
            && self.runtime.console_output_drained(connection_id)
    }

    fn drain_console_events(&mut self, visitor: &mut dyn FnMut(NetConsoleEvent)) {
        while let Some(event) = self.events.pop_front() {
            visitor(event);
        }
    }

    fn take_console_event(&mut self) -> Option<NetConsoleEvent> {
        self.events.pop_front()
    }

    fn console_event_pending(&self) -> bool {
        !self.events.is_empty()
    }

    fn ingest_snapshot(&self) -> IngestSnapshot {
        IngestSnapshot {
            backpressure: self.ingest_backpressure,
            dropped: self.ingest_dropped,
            queued: self.lines.len() as u32,
            ..IngestSnapshot::default()
        }
    }

    fn buffered_console_lines_pending(&self) -> bool {
        !self.lines.is_empty()
    }

    fn active_console_conn_id(&self) -> Option<u64> {
        self.active_connection
    }

    fn authenticated_console_conn_id(&self) -> Option<u64> {
        self.authenticated_connection
    }

    fn console_service_pending(&self) -> bool {
        self.pending_egress.is_some()
            || !self.output.is_empty()
            || self.disconnect_requested
            || !self.lines.is_empty()
    }

    fn console_listen_port(&self) -> u16 {
        self.listen_port
    }

    fn console_listener_ready(&self) -> bool {
        self.listener_ready && !self.faulted && !self.terminal
    }

    fn status_report(&self) -> NetStatusReport {
        use core::fmt::Write as _;

        let mut ip = HeaplessString::new();
        let _ = write!(ip, "{}", self.ip);
        let mut gateway = HeaplessString::new();
        let _ = write!(
            gateway,
            "{}",
            self.gateway.unwrap_or(Ipv4Address::UNSPECIFIED)
        );
        NetStatusReport {
            profile_backend: self.profile_backend,
            backend: self.backend,
            active_driver: self.active_driver,
            mode: self.mode,
            interface_policy: self.interface_policy,
            active_interface: self.active_interface,
            standby_interface: "none",
            address_source: self.address_source,
            ip,
            gateway,
            dhcp_phase: self.dhcp_phase,
            tcp_ready: self.console_listener_ready(),
        }
    }

    fn contain_faulted_console_service(
        &mut self,
        hal: &mut KernelHal<'_>,
    ) -> Result<ConsoleNetworkContainmentTurn, HalError> {
        self.contain_if_faulted(hal)
    }

    fn pending_console_network_containment_diagnostic(
        &self,
    ) -> Option<HeaplessString<DEFAULT_LINE_CAPACITY>> {
        self.pending_containment_diagnostic_line()
    }

    fn console_network_containment_diagnostic_pending(&self) -> bool {
        self.pending_containment_diagnostic().is_some()
    }

    fn commit_console_network_containment_diagnostic(&mut self, expected_line: &str) -> bool {
        self.commit_containment_diagnostic(expected_line)
    }
}
