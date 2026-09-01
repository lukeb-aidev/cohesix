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
    CommandBatchCursor, ExchangeKind, SendBatchBuilder, CONSOLE_PAYLOAD_BYTES,
    SEND_BATCH_MAX_RECORDS,
};
use heapless::{Deque, String as HeaplessString, Vec as HeaplessVec};
#[cfg(feature = "net-backend-virtio")]
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, Ipv4Address};

#[cfg(any(test, feature = "release-pi4"))]
use super::isolated_seam::{isolated_seam_observation_ms, IsolatedSeamDiagnostics};
use super::isolated_self_test::{
    finish_poll_with_self_test, IsolatedSelfTestObservation, IsolatedSelfTestState,
};
#[cfg(feature = "net-backend-virtio")]
use super::ConsoleNetConfig;
#[cfg(feature = "net-backend-virtio")]
use super::NetDeviceCounters;
use super::{
    direct_genet_causal_stage_drain_observed, select_isolated_direct_network_turn_for_contract,
    select_isolated_direct_response_turn, select_isolated_network_turn,
    select_isolated_response_turn, ConsoleLine, DirectGenetCausalStageDrainEvidence,
    DirectGenetCommandControlDeferReason, DirectGenetCommandControlOutcome,
    IsolatedConsoleDiagnostics, IsolatedNetworkLowerCursor, IsolatedNetworkLowerUnit,
    IsolatedNetworkTurnOutcome, IsolatedNetworkTurnSelection, IsolatedNetworkTurnUnit,
    NetConsoleDisconnectReason, NetConsoleEvent, NetCounters, NetDevice, NetPoller,
    NetSelfTestReport, NetSelfTestStartResult, NetStatusReport, NetTelemetry,
};
use crate::console_network_service::{BoundaryError, ConsoleNetworkContainmentTurn, ServiceState};
use crate::drivers::driver_task_net::Cyw43DriverTaskDevice;
#[cfg(feature = "net-backend-virtio")]
use crate::drivers::virtio::net::DriverError as VirtioDriverError;
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
#[cfg(feature = "net-backend-virtio")]
const QEMU_VIRTIO_MAC: [u8; 6] = [0x52, 0x55, 0x00, 0xd1, 0x55, 0x01];

fn isolated_wifi_connection_generation<D: NetDevice>() -> u64 {
    let contract = D::driver_task_contract();
    if contract != crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT {
        return super::projected_wifi_connection_generation(contract, 0);
    }
    let live_generation = {
        #[cfg(feature = "kernel")]
        {
            u64::from(crate::drivers::driver_task_net::cyw43_connection_generation())
        }
        #[cfg(not(feature = "kernel"))]
        {
            0
        }
    };
    super::projected_wifi_connection_generation(contract, live_generation)
}

fn refresh_isolated_device_counters<D: NetDevice>(counters: &mut NetCounters, device: &D) {
    counters.apply_device_snapshot(
        device.counters(),
        isolated_wifi_connection_generation::<D>(),
    );
}

const fn console_service_local_fault_state_pending(
    faulted: bool,
    graceful_teardown_pending: bool,
    containment_active: bool,
    terminal: bool,
    terminal_diagnostic_pending: bool,
) -> bool {
    faulted
        || graceful_teardown_pending
        || containment_active
        || (terminal && terminal_diagnostic_pending)
}

const fn console_service_local_containment_state_pending(
    faulted: bool,
    graceful_teardown_pending: bool,
    containment_active: bool,
    terminal: bool,
) -> bool {
    !terminal && (faulted || graceful_teardown_pending || containment_active)
}

/// Whether one successful response StageOutput may immediately consume its
/// exact direct-GENET child publication and causal ACK.
///
/// The StageOutput signal has already transferred execution to the same-core
/// child. A stable completion published before that child blocks is therefore
/// the only eligible successor: this adds no root device operation and no new
/// control record. QEMU, copied WiFi, a failed stage, and a child without the
/// exact GENET transport retain their established later ObserveChild turn.
const fn direct_genet_stage_completion_observation_due(
    exact_genet_contract: bool,
    runtime_direct_genet: bool,
    stage_committed: bool,
) -> bool {
    exact_genet_contract && runtime_direct_genet && stage_committed
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct QueuedConsoleOutput {
    line: HeaplessString<DEFAULT_LINE_CAPACITY>,
    terminal: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PendingResponseBatch {
    sequence: u64,
    terminal_count: u16,
    #[cfg(any(test, feature = "release-pi4"))]
    staged_ms: u64,
    control_completed: bool,
    output_drained: bool,
}

impl PendingResponseBatch {
    const fn new(sequence: u64, terminal_count: u16, _staged_ms: u64) -> Self {
        Self {
            sequence,
            terminal_count,
            #[cfg(any(test, feature = "release-pi4"))]
            staged_ms: _staged_ms,
            control_completed: false,
            output_drained: false,
        }
    }

    const fn complete(self) -> bool {
        self.control_completed && self.output_drained
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ResponseLane {
    generation: u64,
    connection_id: u64,
    awaiting_batch: Option<PendingResponseBatch>,
    producer_open: bool,
    completed_responses: u16,
}

const fn console_response_batch_debt(
    lane: Option<ResponseLane>,
) -> Option<super::ConsoleResponseBatchDebt> {
    let Some(lane) = lane else {
        return None;
    };
    let Some(batch) = lane.awaiting_batch else {
        return None;
    };
    Some(super::ConsoleResponseBatchDebt {
        generation: lane.generation,
        connection_id: lane.connection_id,
        sequence: batch.sequence,
        control_completed: batch.control_completed,
        output_drained: batch.output_drained,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct IsolatedTurnTelemetry {
    last_progress_ms: u64,
    last_unit: &'static str,
    turns: u64,
    progress_turns: u64,
    observe_child_turns: u64,
    stage_output_turns: u64,
    stage_output_successes: u64,
    disconnect_turns: u64,
    ingress_turns: u64,
    service_tick_turns: u64,
    transmit_egress_turns: u64,
    deferred_diagnostic_turns: u64,
}

impl IsolatedTurnTelemetry {
    const fn new() -> Self {
        Self {
            last_progress_ms: 0,
            last_unit: "none",
            turns: 0,
            progress_turns: 0,
            observe_child_turns: 0,
            stage_output_turns: 0,
            stage_output_successes: 0,
            disconnect_turns: 0,
            ingress_turns: 0,
            service_tick_turns: 0,
            transmit_egress_turns: 0,
            deferred_diagnostic_turns: 0,
        }
    }

    fn record(&mut self, now_ms: u64, unit: IsolatedNetworkTurnUnit, progress: bool) {
        self.turns = self.turns.saturating_add(1);
        self.last_unit = match unit {
            IsolatedNetworkTurnUnit::DeferredDiagnostic => {
                self.deferred_diagnostic_turns = self.deferred_diagnostic_turns.saturating_add(1);
                "diagnostic"
            }
            IsolatedNetworkTurnUnit::TransmitEgress => {
                self.transmit_egress_turns = self.transmit_egress_turns.saturating_add(1);
                "egress"
            }
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::ObserveChild) => {
                self.observe_child_turns = self.observe_child_turns.saturating_add(1);
                "observe"
            }
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::StageOutput) => {
                self.stage_output_turns = self.stage_output_turns.saturating_add(1);
                if progress {
                    self.stage_output_successes = self.stage_output_successes.saturating_add(1);
                }
                "output"
            }
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::Disconnect) => {
                self.disconnect_turns = self.disconnect_turns.saturating_add(1);
                "disconnect"
            }
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::Ingress) => {
                self.ingress_turns = self.ingress_turns.saturating_add(1);
                "ingress"
            }
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::ServiceTick) => {
                self.service_tick_turns = self.service_tick_turns.saturating_add(1);
                "tick"
            }
        };
        if progress {
            self.progress_turns = self.progress_turns.saturating_add(1);
            self.last_progress_ms = now_ms;
        }
    }
}

impl ResponseLane {
    const fn new(generation: u64, connection_id: u64) -> Self {
        Self {
            generation,
            connection_id,
            awaiting_batch: None,
            producer_open: false,
            completed_responses: 0,
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
        fault_label: u64,
        fault_length: u16,
        fault_mr0: u64,
        fault_mr1: u64,
    },
    LocalFault {
        generation: u64,
        reason: &'static str,
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
                fault_label,
                fault_length,
                fault_mr0,
                fault_mr1,
            } if expected_generation == u64::from(observed_generation) => write!(
                line,
                "[console-network] generation={expected_generation} terminal-fault class={fault_class:?} sequence={sequence} label={fault_label} length={fault_length} mr0=0x{fault_mr0:016x} mr1=0x{fault_mr1:016x}"
            )?,
            Self::Fault {
                expected_generation,
                observed_generation,
                ..
            } => write!(
                line,
                "[console-network] fault generation mismatch expected={expected_generation} observed={observed_generation}"
            )?,
            Self::LocalFault { generation, reason } => write!(
                line,
                "[console-network] generation={generation} terminal-fault source=local reason={reason}"
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
    ready_published_ms: Option<u64>,
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
    direct_service_tick_ms: Option<u64>,
    telemetry: NetTelemetry,
    counters: NetCounters,
    connection_bytes_read: u64,
    connection_bytes_written: u64,
    ingest_backpressure: u64,
    ingest_dropped: u64,
    response_drains: u64,
    self_test: IsolatedSelfTestState,
    turn_telemetry: IsolatedTurnTelemetry,
    #[cfg(any(test, feature = "release-pi4"))]
    seam_telemetry: IsolatedSeamDiagnostics,
    #[cfg(any(test, feature = "release-pi4"))]
    response_dispatch_ms: u64,
    profile_backend: &'static str,
    backend: &'static str,
    active_driver: &'static str,
    mode: &'static str,
    interface_policy: &'static str,
    active_interface: &'static str,
    address_source: &'static str,
    dhcp_phase: &'static str,
}

/// Root-side policy marker for a NIC owned directly by the QEMU child.
#[cfg(feature = "net-backend-virtio")]
pub struct DirectVirtioChildDevice {
    mac: EthernetAddress,
}

#[cfg(feature = "net-backend-virtio")]
pub struct DirectVirtioRxToken;

#[cfg(feature = "net-backend-virtio")]
impl RxToken for DirectVirtioRxToken {
    fn consume<R, F>(self, operation: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        operation(&[])
    }
}

#[cfg(feature = "net-backend-virtio")]
pub struct DirectVirtioTxToken;

#[cfg(feature = "net-backend-virtio")]
impl TxToken for DirectVirtioTxToken {
    fn consume<R, F>(self, _len: usize, operation: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        operation(&mut [])
    }
}

#[cfg(feature = "net-backend-virtio")]
impl Device for DirectVirtioChildDevice {
    type RxToken<'a>
        = DirectVirtioRxToken
    where
        Self: 'a;
    type TxToken<'a>
        = DirectVirtioTxToken
    where
        Self: 'a;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        None
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        None
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut capabilities = DeviceCapabilities::default();
        capabilities.medium = Medium::Ethernet;
        capabilities.max_transmission_unit = 1500;
        capabilities
    }
}

#[cfg(feature = "net-backend-virtio")]
impl NetDevice for DirectVirtioChildDevice {
    type Error = VirtioDriverError;

    fn create<H>(_hal: &mut H) -> Result<Self, Self::Error>
    where
        H: crate::hal::Hardware<Error = crate::hal::HalError>,
    {
        Err(VirtioDriverError::NoDevice)
    }

    fn mac(&self) -> EthernetAddress {
        self.mac
    }

    fn tx_drop_count(&self) -> u32 {
        0
    }

    fn name() -> &'static str {
        "virtio-net-direct-child"
    }

    fn driver_task_contract() -> DriverTaskContract {
        crate::hal::driver_task::VIRTIO_NET_DRIVER_TASK_CONTRACT
    }

    fn debug_snapshot(&mut self) {}

    fn counters(&self) -> NetDeviceCounters {
        NetDeviceCounters::default()
    }
}

/// QEMU specialization: root owns policy; the child owns the NIC data path.
#[cfg(feature = "net-backend-virtio")]
pub type IsolatedVirtioConsole = IsolatedNetworkConsole<DirectVirtioChildDevice>;

/// Physical Pi CYW43 specialization used after root-only DHCP bootstrap.
pub type IsolatedCyw43Console = IsolatedNetworkConsole<Cyw43DriverTaskDevice>;

#[cfg(feature = "net-backend-virtio")]
impl IsolatedNetworkConsole<DirectVirtioChildDevice> {
    /// Admit the NIC directly to the suspended compiler-declared child.
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
        let device = DirectVirtioChildDevice {
            mac: EthernetAddress(QEMU_VIRTIO_MAC),
        };
        let mac = device.mac;
        let ip = Ipv4Address::from(config.address.ip);
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
            "virtio-net-direct-child",
            "virtio-net-direct-child",
            "static",
            "wired",
            "wired",
            "dev-virt-isolated-child",
            "disabled",
            true,
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
        self_test_enabled: bool,
    ) -> Self {
        let mut counters = NetCounters::default();
        refresh_isolated_device_counters(&mut counters, &device);
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
            ready_published_ms: None,
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
            direct_service_tick_ms: None,
            telemetry: NetTelemetry {
                link_up: true,
                tx_drops: 0,
                last_poll_ms: 0,
            },
            counters,
            connection_bytes_read: 0,
            connection_bytes_written: 0,
            ingest_backpressure: 0,
            ingest_dropped: 0,
            response_drains: 0,
            self_test: IsolatedSelfTestState::new(self_test_enabled),
            turn_telemetry: IsolatedTurnTelemetry::new(),
            #[cfg(any(test, feature = "release-pi4"))]
            seam_telemetry: IsolatedSeamDiagnostics::default(),
            #[cfg(any(test, feature = "release-pi4"))]
            response_dispatch_ms: 0,
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

    /// Whether the child and its NIC peer own the complete packet data plane.
    #[must_use]
    pub(crate) const fn direct_data_plane(&self) -> bool {
        self.runtime.direct_data_plane()
    }

    /// Copy the passive Pi MCS seam aggregates without changing service state.
    #[cfg(any(test, feature = "release-pi4"))]
    #[must_use]
    pub(crate) const fn isolated_seam_diagnostics(&self) -> IsolatedSeamDiagnostics {
        self.seam_telemetry
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

    /// Fence the console half of a direct NIC pair after the physical owner has
    /// completed its independent supervisor containment. This path does not
    /// wait for a second console fault: it records the coupled fault locally and
    /// starts the existing exact console teardown cursor immediately.
    pub(crate) fn begin_paired_driver_fault_containment(&mut self) -> Result<bool, HalError> {
        if self.terminal {
            return Ok(false);
        }
        let generation = self.runtime.generation();
        self.faulted = true;
        self.listener_ready = false;
        self.pending_containment_fault_diagnostic =
            Some(ConsoleNetworkContainmentDiagnostic::LocalFault {
                generation,
                reason: "direct-nic-peer-fault",
            });
        if !self.runtime.containment_active() {
            self.runtime.begin_containment()?;
        }
        Ok(true)
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
            let mut diagnostic = faulted.then(|| {
                self.pending_containment_fault_diagnostic.unwrap_or(
                    ConsoleNetworkContainmentDiagnostic::LocalFault {
                        generation,
                        reason: "runtime-boundary",
                    },
                )
            });
            match crate::hal::critical_tcb::take_target_service_fault(
                crate::console_network_service::SERVICE_TASK_ID,
            ) {
                Ok(Some(record)) => {
                    diagnostic = Some(ConsoleNetworkContainmentDiagnostic::Fault {
                        expected_generation: generation,
                        observed_generation: record.identity.supervisor_generation,
                        fault_class: record.fault_class,
                        sequence: record.sequence,
                        fault_label: record.fault_label,
                        fault_length: record.fault_length,
                        fault_mr0: record.fault_mr0,
                        fault_mr1: record.fault_mr1,
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

    /// Acquire the most recent exact child-owned direct-GENET snapshot.
    #[must_use]
    pub fn direct_genet_runtime_diagnostic(
        &self,
    ) -> Option<console_network_abi::DirectGenetRuntimeDiagnostic> {
        self.runtime.direct_genet_runtime_diagnostic()
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
        if self.pending_containment_fault_diagnostic.is_none() {
            self.pending_containment_fault_diagnostic =
                Some(ConsoleNetworkContainmentDiagnostic::LocalFault {
                    generation: self.runtime.generation(),
                    reason,
                });
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
            .unwrap_or_else(|| ResponseLane::new(generation, connection_id));
        if lane.generation != generation || lane.connection_id != connection_id {
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
        let lane = self
            .response_lane
            .get_or_insert_with(|| ResponseLane::new(generation, connection_id));
        lane.producer_open = !terminal;
        if terminal {
            lane.completed_responses = lane.completed_responses.saturating_add(1);
        }
        true
    }

    fn settle_completed_response_batch(&mut self) {
        let terminal_count = self
            .response_lane
            .and_then(|lane| lane.awaiting_batch)
            .filter(|batch| batch.complete())
            .map(|batch| batch.terminal_count);
        let Some(terminal_count) = terminal_count else {
            return;
        };
        let Some(lane) = self.response_lane.as_mut() else {
            return;
        };
        if lane.completed_responses < terminal_count {
            self.fail_closed("response-terminal-underflow");
            return;
        }
        lane.completed_responses -= terminal_count;
        lane.awaiting_batch = None;
    }

    fn complete_response_lane_if_drained(&mut self) {
        self.settle_completed_response_batch();
        let complete = self.response_lane.is_some_and(|lane| {
            !lane.producer_open
                && lane.completed_responses == 0
                && lane.awaiting_batch.is_none()
                && self.output.is_empty()
                && !self.runtime.publication_ack_pending()
                && self.pending_egress.is_none()
        });
        if complete {
            self.response_lane = None;
            self.output_issued = false;
        }
    }

    fn handle_control_completed(&mut self, sequence: u64) {
        if let Some(lane) = self.response_lane.as_mut() {
            let Some(batch) = lane.awaiting_batch.as_mut() else {
                self.fail_closed("response-completion-sequence");
                return;
            };
            if batch.sequence != sequence {
                self.fail_closed("response-completion-sequence");
                return;
            }
            batch.control_completed = true;
            #[cfg(any(test, feature = "release-pi4"))]
            self.seam_telemetry.record_control_completed(
                batch.staged_ms,
                isolated_seam_observation_ms(self.last_now_ms),
            );
        }
        self.settle_completed_response_batch();
    }

    fn handle_event(&mut self, event: crate::console_network_service::ConsoleNetworkEvent) {
        let connection_id = event.connection_id();
        match event.kind() {
            ExchangeKind::Ready => {
                self.listener_ready = true;
                self.ready_published_ms = Some(event.now_ms());
                self.disconnect_requested = false;
                self.disconnect_issued = false;
            }
            ExchangeKind::Connected => {
                // A new child connection identity cannot inherit commands
                // retained for any earlier authenticated peer.
                self.lines.clear();
                #[cfg(any(test, feature = "release-pi4"))]
                {
                    self.response_dispatch_ms = 0;
                }
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
                if line.push_str(payload).is_err() {
                    self.ingest_backpressure = self.ingest_backpressure.saturating_add(1);
                    self.fail_closed("command-queue-backpressure");
                    return;
                }
                let console_line = ConsoleLine::for_connection(line, event.now_ms(), connection_id);
                #[cfg(any(test, feature = "release-pi4"))]
                let root_observed_ms = isolated_seam_observation_ms(self.last_now_ms);
                #[cfg(any(test, feature = "release-pi4"))]
                let console_line = console_line.with_root_observed_ms(root_observed_ms);
                if self.lines.push_back(console_line).is_err() {
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
                #[cfg(any(test, feature = "release-pi4"))]
                self.seam_telemetry
                    .record_command_or_batch_observed(event.now_ms(), root_observed_ms);
            }
            ExchangeKind::CommandBatch => {
                if self.authenticated_connection != Some(connection_id) {
                    self.fail_closed("command-batch-before-authentication");
                    return;
                }
                let payload = event.payload_bytes();
                let Ok(mut cursor) = CommandBatchCursor::validate(payload) else {
                    self.fail_closed("command-batch-record");
                    return;
                };
                if cursor.remaining() > LINE_QUEUE_DEPTH.saturating_sub(self.lines.len()) {
                    self.ingest_backpressure = self.ingest_backpressure.saturating_add(1);
                    self.fail_closed("command-batch-queue-backpressure");
                    return;
                }
                #[cfg(any(test, feature = "release-pi4"))]
                let root_observed_ms = isolated_seam_observation_ms(self.last_now_ms);
                loop {
                    let command = match cursor.next_command(payload) {
                        Ok(Some(command)) => command,
                        Ok(None) => break,
                        Err(_) => {
                            self.fail_closed("command-batch-record");
                            return;
                        }
                    };
                    let (now_ms, command) = command;
                    let mut line = HeaplessString::new();
                    if line.push_str(command).is_err() {
                        self.fail_closed("command-batch-admission");
                        return;
                    }
                    let console_line = ConsoleLine::for_connection(line, now_ms, connection_id);
                    #[cfg(any(test, feature = "release-pi4"))]
                    let console_line = console_line.with_root_observed_ms(root_observed_ms);
                    if self.lines.push_back(console_line).is_err() {
                        self.fail_closed("command-batch-admission");
                        return;
                    }
                    self.connection_bytes_read = self
                        .connection_bytes_read
                        .saturating_add(command.len() as u64);
                    self.counters.tcp_rx_bytes = self
                        .counters
                        .tcp_rx_bytes
                        .saturating_add(command.len() as u64);
                    self.counters.tcp_console_recv_ready =
                        self.counters.tcp_console_recv_ready.saturating_add(1);
                }
                #[cfg(any(test, feature = "release-pi4"))]
                self.seam_telemetry
                    .record_command_or_batch_observed(event.now_ms(), root_observed_ms);
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
                #[cfg(any(test, feature = "release-pi4"))]
                {
                    self.response_dispatch_ms = 0;
                }
            }
            ExchangeKind::Backpressure => self.fail_closed("child-event-backpressure"),
            ExchangeKind::ControlCompleted => {
                self.handle_control_completed(event.related_sequence());
            }
            ExchangeKind::OutputDrained => {
                if let Some(lane) = self.response_lane.as_mut() {
                    if lane.generation != self.runtime.generation()
                        || lane.connection_id != connection_id
                    {
                        self.fail_closed("response-drain-identity");
                        return;
                    }
                    let Some(batch) = lane.awaiting_batch.as_mut() else {
                        self.fail_closed("response-drain-sequence");
                        return;
                    };
                    if batch.sequence != event.related_sequence() {
                        self.fail_closed("response-drain-sequence");
                        return;
                    }
                    batch.output_drained = true;
                    #[cfg(any(test, feature = "release-pi4"))]
                    self.seam_telemetry.record_output_drained(
                        batch.staged_ms,
                        event.now_ms(),
                        isolated_seam_observation_ms(self.last_now_ms),
                    );
                }
                self.response_drains = self.response_drains.saturating_add(1);
                self.settle_completed_response_batch();
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
                #[cfg(any(test, feature = "release-pi4"))]
                {
                    self.response_dispatch_ms = 0;
                }
            }
            ExchangeKind::SendLine | ExchangeKind::SendBatch | ExchangeKind::Disconnect => {
                self.fail_closed("child-published-root-control-kind")
            }
        }
    }

    fn poll_child_output(&mut self) -> IsolatedNetworkTurnOutcome {
        if self.runtime.publication_ack_pending() {
            // A deadline-only observation retained the complete publication
            // without waking the child. Its next ordinary child-output unit
            // spends exactly that one deferred credit before reading again.
            return self.acknowledge_child_publication(true);
        }
        self.observe_child_output(true)
    }

    fn poll_child_output_without_ack(&mut self) -> IsolatedNetworkTurnOutcome {
        if self.runtime.publication_ack_pending() {
            // Repeated deadline observations cannot reread or credit the same
            // retained publication. The ordinary Network rotor owns its ACK.
            return IsolatedNetworkTurnOutcome::complete(false);
        }
        self.observe_child_output(false)
    }

    fn observe_child_output(
        &mut self,
        acknowledge_publication: bool,
    ) -> IsolatedNetworkTurnOutcome {
        let turn = match self.runtime.poll_turn() {
            Ok(turn) => turn,
            Err(error) => {
                self.fail_closed(error.reason());
                return IsolatedNetworkTurnOutcome::complete(false);
            }
        };
        let publication_observed = turn.publication_observed();
        let mut activity = turn.input_progress_observed();
        if let Some(sequence) = turn.input_completions.control_sequence {
            self.handle_control_completed(sequence);
            if self.faulted {
                return IsolatedNetworkTurnOutcome::complete(activity);
            }
        }
        if let Some(event) = turn.event {
            activity = true;
            self.handle_event(event);
            if self.faulted {
                return IsolatedNetworkTurnOutcome::complete(activity);
            }
            if self.graceful_teardown_pending {
                if turn.egress.is_some() {
                    // One global credit cannot authorize ShutdownComplete and
                    // an egress publication together. Treat the copied pair as
                    // a protocol violation and contain it without ACK.
                    self.fail_closed("terminal-egress-coalescing");
                    return IsolatedNetworkTurnOutcome::complete(activity);
                }
                if self.runtime.retire_terminal_publication().is_err() {
                    self.fail_closed("terminal-publication-retirement");
                    return IsolatedNetworkTurnOutcome::complete(activity);
                }
                if self.runtime.begin_containment().is_err() {
                    self.fail_closed("terminal-containment-start");
                }
                return IsolatedNetworkTurnOutcome::complete(activity);
            }
        }
        if let Some(egress) = turn.egress {
            activity = true;
            if self.runtime.direct_data_plane() {
                self.fail_closed("direct-root-egress-publication");
                return IsolatedNetworkTurnOutcome::complete(activity);
            }
            if self.pending_egress.is_some() {
                self.fail_closed("egress-overwrite");
                return IsolatedNetworkTurnOutcome::complete(activity);
            }
            self.pending_egress = Some(egress);
        }
        if publication_observed {
            activity = true;
            if acknowledge_publication {
                return self.acknowledge_child_publication(activity);
            }
        }
        IsolatedNetworkTurnOutcome::complete(activity)
    }

    fn acknowledge_child_publication(&mut self, activity: bool) -> IsolatedNetworkTurnOutcome {
        if self.runtime.acknowledge_publication().is_err() {
            self.fail_closed("publication-ack");
            return IsolatedNetworkTurnOutcome::complete(activity);
        }
        IsolatedNetworkTurnOutcome::child_signaled(activity)
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
                .is_some_and(|lane| lane.awaiting_batch.is_some())
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
        let mut terminal_count = 0u16;
        for queued in self.output.iter().take(SEND_BATCH_MAX_RECORDS) {
            match builder.try_push_line(queued.line.as_str()) {
                Ok(true) => {
                    count = count.saturating_add(1);
                    bytes = bytes.saturating_add(queued.line.len());
                    terminal_count = terminal_count.saturating_add(u16::from(queued.terminal));
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
                #[cfg(any(test, feature = "release-pi4"))]
                let staged_ms = isolated_seam_observation_ms(self.last_now_ms);
                #[cfg(any(test, feature = "release-pi4"))]
                if self.response_dispatch_ms != 0 {
                    self.seam_telemetry
                        .record_dispatch_to_stage(self.response_dispatch_ms, staged_ms);
                    self.response_dispatch_ms = 0;
                }
                for _ in 0..count {
                    let _ = self.output.pop_front();
                }
                if let Some(lane) = self.response_lane.as_mut() {
                    lane.awaiting_batch =
                        Some(PendingResponseBatch::new(sequence, terminal_count, {
                            #[cfg(any(test, feature = "release-pi4"))]
                            {
                                staged_ms
                            }
                            #[cfg(not(any(test, feature = "release-pi4")))]
                            {
                                self.last_now_ms
                            }
                        }));
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

    fn disconnect_stage_ready(&self) -> bool {
        let Some(connection_id) = self.active_connection else {
            return false;
        };
        self.disconnect_requested
            && !self.disconnect_issued
            && self.response_lane.is_none()
            && self.output.is_empty()
            && self.runtime.control_available()
            && (!self.output_issued || self.runtime.console_output_drained(connection_id))
    }

    fn refresh_device_counters(&mut self) {
        self.telemetry.tx_drops = self.device.tx_drop_count();
        refresh_isolated_device_counters(&mut self.counters, &self.device);
    }

    fn service_self_test(&mut self) -> bool {
        if !self.self_test.running() {
            return false;
        }
        let authenticated_connection = self.authenticated_connection;
        let output_drained = authenticated_connection.is_some_and(|connection_id| {
            self.output.is_empty()
                && self.pending_egress.is_none()
                && self.response_lane.is_none()
                && self.runtime.console_output_drained(connection_id)
        });
        let observation = IsolatedSelfTestObservation {
            now_ms: self.last_now_ms,
            direct_data_plane: self.runtime.direct_data_plane(),
            tx_complete: self.counters.tx_complete,
            rx_packets: self.counters.rx_packets,
            tcp_rx_bytes: self.counters.tcp_rx_bytes,
            connection_bytes_read: self.connection_bytes_read,
            connection_bytes_written: self.connection_bytes_written,
            response_drains: self.response_drains,
            authenticated_connection,
            listener_ready: self.listener_ready,
            output_drained,
        };
        let Some(result) = self.self_test.observe(observation) else {
            return false;
        };
        log::info!(
            "[net-selftest] result generation={} run_generation={} tx_ok={} udp_echo_ok={} tcp_ok={} console_ok={} peer_assisted_ok={} result={}",
            self.runtime.generation(),
            self.self_test.run_generation(),
            result.tx_ok,
            result.udp_echo_ok,
            result.tcp_ok,
            result.console_ok,
            result.peer_assisted_ok,
            result.verdict(),
        );
        true
    }

    fn isolated_console_diagnostics(&self) -> IsolatedConsoleDiagnostics {
        let (awaiting_batch_drain, producer_open) =
            self.response_lane.map_or((false, false), |lane| {
                (lane.awaiting_batch.is_some(), lane.producer_open)
            });
        let yield_accounting = self.runtime.direct_genet_yield_accounting();
        IsolatedConsoleDiagnostics {
            generation: self.runtime.generation(),
            last_poll_ms: self.last_now_ms,
            last_progress_ms: self.turn_telemetry.last_progress_ms,
            last_unit: self.turn_telemetry.last_unit,
            turns: self.turn_telemetry.turns,
            progress_turns: self.turn_telemetry.progress_turns,
            observe_child_turns: self.turn_telemetry.observe_child_turns,
            stage_output_turns: self.turn_telemetry.stage_output_turns,
            stage_output_successes: self.turn_telemetry.stage_output_successes,
            disconnect_turns: self.turn_telemetry.disconnect_turns,
            ingress_turns: self.turn_telemetry.ingress_turns,
            service_tick_turns: self.turn_telemetry.service_tick_turns,
            transmit_egress_turns: self.turn_telemetry.transmit_egress_turns,
            deferred_diagnostic_turns: self.turn_telemetry.deferred_diagnostic_turns,
            command_queue: self.lines.len(),
            output_queue: self.output.len(),
            pending_egress: self.pending_egress.is_some(),
            awaiting_batch_drain,
            producer_open,
            response_drains: self.response_drains,
            ingress_backpressure: self.ingest_backpressure,
            ingress_dropped: self.ingest_dropped,
            direct_genet_yield_calls: yield_accounting.yield_calls,
            direct_genet_yield_counter_hz: yield_accounting.counter_hz,
            direct_genet_yield_call_wall_scaled: yield_accounting.call_wall_scaled,
            direct_genet_yield_child_credit_scaled: yield_accounting.credited_child_scaled,
            direct_genet_yield_invalid_reasons: yield_accounting.invalid_reasons,
        }
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
        self.poll_child_output()
    }

    #[inline(never)]
    fn poll_stage_output_unit(&mut self) -> IsolatedNetworkTurnOutcome {
        let stage_committed = self.stage_one_output();
        if !direct_genet_stage_completion_observation_due(
            D::driver_task_contract() == crate::hal::driver_task::GENET_DRIVER_TASK_CONTRACT,
            self.runtime.direct_genet(),
            stage_committed,
        ) {
            return IsolatedNetworkTurnOutcome::child_signal_attempt(stage_committed);
        }

        let expected_batch = self.response_lane.and_then(|lane| {
            lane.awaiting_batch.map(|batch| {
                (
                    lane.generation,
                    lane.connection_id,
                    batch.sequence,
                    self.response_drains,
                )
            })
        });

        // Observe without granting publication credit. Only an exact
        // OutputDrained transition for the batch just staged above may use the
        // second same-core YieldTo. An unrelated event or no publication keeps
        // the ACK owed for the ordinary ObserveChild unit.
        let _observed = self.poll_child_output_without_ack();
        let causal_drain_observed = expected_batch.is_some_and(
            |(generation, connection_id, batch_sequence, response_drains_before)| {
                let observed_lane = self.response_lane;
                direct_genet_causal_stage_drain_observed(DirectGenetCausalStageDrainEvidence {
                    exact_genet_contract: D::driver_task_contract()
                        == crate::hal::driver_task::GENET_DRIVER_TASK_CONTRACT,
                    runtime_direct_genet: self.runtime.direct_genet(),
                    stage_committed,
                    expected_generation: generation,
                    expected_connection_id: connection_id,
                    expected_batch_sequence: batch_sequence,
                    observed_generation: self.runtime.generation(),
                    observed_lane_generation: observed_lane.map(|lane| lane.generation),
                    observed_lane_connection_id: observed_lane.map(|lane| lane.connection_id),
                    observed_batch_sequence: observed_lane
                        .and_then(|lane| lane.awaiting_batch.map(|batch| batch.sequence)),
                    observed_batch_output_drained: observed_lane
                        .and_then(|lane| lane.awaiting_batch)
                        .is_some_and(|batch| batch.output_drained),
                    response_drains_before,
                    response_drains_after: self.response_drains,
                    publication_ack_pending: self.runtime.publication_ack_pending(),
                    faulted: self.faulted,
                    terminal: self.terminal,
                    graceful_teardown_pending: self.graceful_teardown_pending,
                })
            },
        );
        if causal_drain_observed {
            let _causal_ack = self.acknowledge_child_publication(true);
        }
        IsolatedNetworkTurnOutcome::child_signaled(true)
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
        if self.runtime.direct_data_plane() {
            if self.direct_service_tick_ms == Some(self.last_now_ms) {
                return IsolatedNetworkTurnOutcome::complete(false);
            }
            self.direct_service_tick_ms = Some(self.last_now_ms);
        }
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
            && lane.is_some_and(|lane| lane.awaiting_batch.is_none());
        let response_progress_outstanding = lane.is_some_and(|lane| {
            lane.awaiting_batch.is_some()
                || lane.completed_responses != 0
                || lane.producer_open
                || !self.output.is_empty()
        });
        let selection = if self.runtime.direct_data_plane() {
            if self.pending_egress.is_some() {
                self.fail_closed("direct-egress-publication");
                return false;
            }
            select_isolated_direct_response_turn(
                stage_output_ready,
                response_progress_outstanding,
                self.lower_cursor,
            )
        } else {
            select_isolated_response_turn(
                self.pending_egress.is_some(),
                stage_output_ready,
                response_progress_outstanding,
                self.lower_cursor,
            )
        };
        self.lower_cursor = selection.successor();
        let outcome = match selection.unit() {
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
        self.turn_telemetry
            .record(now_ms, selection.unit(), activity);
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
        let self_test_progress = self.service_self_test();
        (activity || self_test_progress) && !self.faulted
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

        let selection: IsolatedNetworkTurnSelection = if self.runtime.direct_data_plane() {
            if self.pending_egress.is_some()
                || self.device.isolated_deferred_tx_diagnostic_pending()
            {
                self.fail_closed("direct-root-data-plane-state");
                return false;
            }
            let stage_output_ready = !self.output.is_empty()
                && self.runtime.control_available()
                && self
                    .response_lane
                    .is_none_or(|lane| lane.awaiting_batch.is_none());
            select_isolated_direct_network_turn_for_contract(
                D::driver_task_contract() == crate::hal::driver_task::GENET_DRIVER_TASK_CONTRACT,
                stage_output_ready,
                self.disconnect_stage_ready(),
                self.lower_cursor,
            )
        } else {
            select_isolated_network_turn(
                self.device.isolated_deferred_tx_diagnostic_pending(),
                self.pending_egress.is_some(),
                self.lower_cursor,
            )
        };
        // Commit the ordinary successor before any selected unit can block or
        // fault. Only a successful child notification may force ObserveChild.
        self.lower_cursor = selection.successor();
        let outcome = match selection.unit() {
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
        self.turn_telemetry
            .record(now_ms, selection.unit(), activity);
        self.lower_cursor = lower_cursor;
        self.complete_response_lane_if_drained();
        if self.faulted {
            self.refresh_device_counters();
            return false;
        }
        self.refresh_device_counters();
        finish_poll_with_self_test(activity, || self.service_self_test())
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

    fn flush_tcp_with_budget(
        &mut self,
        now_ms: u64,
        budget: &mut DriverServiceBudget,
    ) -> Result<bool, DriverServiceBudgetError> {
        // A retained authenticated response uses the same one-turn resource
        // contract as ordinary service, but selects only useful response work.
        // The ordinary strict rotor remains authoritative outside this lane.
        budget.charge_ops(1)?;
        budget.charge_frames(ISOLATED_NETWORK_TURN_FRAMES)?;
        budget.charge_bytes(ISOLATED_NETWORK_TURN_BYTES)?;
        Ok(self.poll_response_turn(now_ms))
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

    fn service_direct_genet_command_control_with_budget(
        &mut self,
        expected: super::ConsoleResponseIdentity,
        now_ms: u64,
        budget: &mut DriverServiceBudget,
    ) -> Result<DirectGenetCommandControlOutcome, DriverServiceBudgetError> {
        if D::driver_task_contract() != crate::hal::driver_task::GENET_DRIVER_TASK_CONTRACT
            || !self.runtime.direct_genet()
            || !self.runtime.direct_data_plane()
        {
            return Ok(DirectGenetCommandControlOutcome::Unsupported);
        }
        let identity_exact = expected.generation != 0
            && expected.connection_id != 0
            && self.runtime.generation() == expected.generation
            && self.active_connection == Some(expected.connection_id)
            && self.authenticated_connection == Some(expected.connection_id)
            && self.bounded_console_response_identity() == Some(expected);
        if !identity_exact || self.faulted || self.terminal || self.graceful_teardown_pending {
            self.fail_closed("direct-genet-command-control-identity");
            return Ok(DirectGenetCommandControlOutcome::Fault);
        }
        budget.charge_ops(1)?;
        budget.charge_frames(ISOLATED_NETWORK_TURN_FRAMES)?;
        budget.charge_bytes(ISOLATED_NETWORK_TURN_BYTES)?;
        self.last_now_ms = now_ms;
        self.telemetry.last_poll_ms = now_ms;

        let Some(response_lane) = self.response_lane else {
            return Ok(DirectGenetCommandControlOutcome::Deferred(
                DirectGenetCommandControlDeferReason::OutputMissing,
            ));
        };
        if response_lane.generation != expected.generation
            || response_lane.connection_id != expected.connection_id
        {
            self.fail_closed("direct-genet-command-control-response-lane");
            return Ok(DirectGenetCommandControlOutcome::Fault);
        }
        if response_lane.awaiting_batch.is_some() {
            return Ok(DirectGenetCommandControlOutcome::Deferred(
                DirectGenetCommandControlDeferReason::PriorBatch,
            ));
        }
        if self.output.is_empty() {
            return Ok(DirectGenetCommandControlOutcome::Deferred(
                DirectGenetCommandControlDeferReason::OutputMissing,
            ));
        }
        if !self.runtime.control_available() {
            return Ok(DirectGenetCommandControlOutcome::Deferred(
                DirectGenetCommandControlDeferReason::ControlBusy,
            ));
        }
        let stage_published = self.stage_one_output();
        self.turn_telemetry.record(
            now_ms,
            IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::StageOutput),
            stage_published,
        );
        if self.faulted {
            return Ok(DirectGenetCommandControlOutcome::Fault);
        }
        if stage_published {
            self.lower_cursor = IsolatedNetworkLowerCursor::new();
            return Ok(DirectGenetCommandControlOutcome::StagePublished);
        }

        // A notification-only service tick is not causally distinguishable
        // from an older coalesced WAKE_CONTROL. Keep the child quiesced until
        // an exact newly sequenced response control record is stageable.
        Ok(DirectGenetCommandControlOutcome::Deferred(
            DirectGenetCommandControlDeferReason::StageBackpressure,
        ))
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

    #[cfg(feature = "release-pi4")]
    fn note_console_response_dispatch(&mut self, connection_id: u64, dispatch_ms: u64) {
        self.response_dispatch_ms = if dispatch_ms != 0
            && self.active_connection == Some(connection_id)
            && self.authenticated_connection == Some(connection_id)
        {
            // The caller has already sampled the common absolute Pi seam
            // epoch. Preserve that exact instant for both recorder consumers;
            // sampling again here would manufacture dispatch skew.
            dispatch_ms
        } else {
            0
        };
    }

    fn console_response_lane(&self) -> Option<super::ConsoleResponseLane> {
        let lane = self.response_lane?;
        Some(super::ConsoleResponseLane {
            generation: lane.generation,
            connection_id: lane.connection_id,
            queued_lines: self.output.len(),
            available_lines: LINE_QUEUE_DEPTH.saturating_sub(self.output.len()),
            awaiting_batch_drain: lane.awaiting_batch.is_some(),
            terminal_queued: lane.completed_responses != 0,
            producer_open: lane.producer_open,
            completed_responses: usize::from(lane.completed_responses),
        })
    }

    fn console_child_publication_pending(&self) -> Option<bool> {
        self.runtime.child_publication_pending().ok()
    }

    fn console_child_control_publication_owed(
        &self,
    ) -> Option<crate::console_network_service::ConsoleNetworkControlPublication> {
        self.runtime.child_control_publication_owed()
    }

    fn console_response_batch_debt(&self) -> Option<super::ConsoleResponseBatchDebt> {
        console_response_batch_debt(self.response_lane)
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

    fn console_service_local_fault_pending(&self) -> bool {
        console_service_local_fault_state_pending(
            self.faulted(),
            self.graceful_teardown_pending,
            self.runtime.containment_active(),
            self.terminal,
            self.pending_containment_diagnostic().is_some(),
        )
    }

    fn console_service_local_containment_pending(&self) -> bool {
        console_service_local_containment_state_pending(
            self.faulted(),
            self.graceful_teardown_pending,
            self.runtime.containment_active(),
            self.terminal,
        )
    }

    fn console_listen_port(&self) -> u16 {
        self.listen_port
    }

    fn console_listener_ready(&self) -> bool {
        self.listener_ready && !self.faulted && !self.terminal
    }

    #[inline(never)]
    fn poll_isolated_child_publication_only(&mut self) -> bool {
        if self.faulted || self.terminal || !self.runtime.activated() {
            return false;
        }
        self.poll_child_output_without_ack().activity()
    }

    fn isolated_child_ready_published_ms(&self) -> Option<u64> {
        self.ready_published_ms
    }

    fn start_self_test(&mut self, now_ms: u64) -> NetSelfTestStartResult {
        if !self.self_test.enabled() {
            return NetSelfTestStartResult::SelfTestDisabled;
        }
        if self.faulted || self.terminal {
            return NetSelfTestStartResult::PolicyDisabled;
        }
        if self.ip.is_unspecified() || (self.mode == "dhcp" && self.dhcp_phase != "bound") {
            return NetSelfTestStartResult::DhcpPending;
        }
        if !self.runtime.activated() || !self.listener_ready {
            return NetSelfTestStartResult::NotReadyBootstrapCommit;
        }
        self.refresh_device_counters();
        if self.self_test.start(
            now_ms,
            self.counters.tx_complete,
            self.counters.rx_packets,
            self.counters.tcp_rx_bytes,
            self.connection_bytes_read,
            self.connection_bytes_written,
            self.response_drains,
            self.authenticated_connection,
        ) {
            NetSelfTestStartResult::Started
        } else {
            NetSelfTestStartResult::SelfTestDisabled
        }
    }

    fn self_test_report(&self) -> NetSelfTestReport {
        let mut udp_target = HeaplessString::new();
        let _ = udp_target.push_str("peer-assisted");
        let mut tcp_target = HeaplessString::new();
        let _ = write!(tcp_target, "{}:{}", self.ip, self.listen_port);
        NetSelfTestReport {
            enabled: self.self_test.enabled(),
            running: self.self_test.running(),
            run_generation: self.self_test.run_generation(),
            last_result: self.self_test.last_result(),
            backend: self.active_driver,
            udp_target,
            tcp_target,
        }
    }

    fn isolated_console_diagnostics(&self) -> Option<IsolatedConsoleDiagnostics> {
        Some(IsolatedNetworkConsole::isolated_console_diagnostics(self))
    }

    #[cfg(feature = "release-pi4")]
    fn isolated_seam_diagnostics(&self) -> Option<IsolatedSeamDiagnostics> {
        Some(IsolatedNetworkConsole::isolated_seam_diagnostics(self))
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
            tcp_ready: super::stack::net_status_tcp_ready(
                self.console_listener_ready(),
                self.active_driver,
                self.active_interface,
                self.counters,
            ),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_committed_direct_genet_stage_observes_its_causal_completion() {
        assert!(direct_genet_stage_completion_observation_due(
            true, true, true,
        ));
        for evidence in [
            (false, true, true),
            (true, false, true),
            (true, true, false),
            (false, false, true),
            (false, true, false),
            (true, false, false),
            (false, false, false),
        ] {
            assert!(
                !direct_genet_stage_completion_observation_due(
                    evidence.0, evidence.1, evidence.2,
                ),
                "QEMU, WiFi, non-direct, and backpressured stages retain the ordinary observation boundary: {evidence:?}",
            );
        }
    }

    #[test]
    fn response_batch_debt_preserves_exact_control_and_drain_state() {
        let mut batch = PendingResponseBatch::new(19, 1, 0);
        batch.control_completed = true;
        let lane = ResponseLane {
            generation: 7,
            connection_id: 41,
            awaiting_batch: Some(batch),
            producer_open: false,
            completed_responses: 1,
        };
        assert_eq!(
            console_response_batch_debt(Some(lane)),
            Some(super::super::ConsoleResponseBatchDebt {
                generation: 7,
                connection_id: 41,
                sequence: 19,
                control_completed: true,
                output_drained: false,
            }),
        );
        let drained = ResponseLane {
            awaiting_batch: lane.awaiting_batch.map(|mut batch| {
                batch.output_drained = true;
                batch
            }),
            ..lane
        };
        assert!(
            console_response_batch_debt(Some(drained)).is_some_and(|debt| debt.output_drained),
            "the exact terminal stays observable until ordinary settlement consumes it",
        );
        assert!(console_response_batch_debt(None).is_none());
        assert!(console_response_batch_debt(Some(ResponseLane {
            awaiting_batch: None,
            ..lane
        }))
        .is_none());
    }

    #[test]
    fn local_fault_hint_covers_fault_containment_and_unreported_terminal_only() {
        assert!(!console_service_local_fault_state_pending(
            false, false, false, false, false
        ));
        assert!(console_service_local_fault_state_pending(
            true, false, false, false, false
        ));
        assert!(console_service_local_fault_state_pending(
            false, true, false, false, false
        ));
        assert!(console_service_local_fault_state_pending(
            false, false, true, false, false
        ));
        assert!(console_service_local_fault_state_pending(
            false, false, false, true, true
        ));
        assert!(!console_service_local_fault_state_pending(
            false, false, false, true, false
        ));
        assert!(console_service_local_containment_state_pending(
            true, false, false, false,
        ));
        assert!(console_service_local_containment_state_pending(
            false, true, false, false,
        ));
        assert!(console_service_local_containment_state_pending(
            false, false, true, false,
        ));
        assert!(
            !console_service_local_containment_state_pending(false, false, false, true),
            "a terminal diagnostic fences passive admission but must let ordinary output run",
        );
    }
}
