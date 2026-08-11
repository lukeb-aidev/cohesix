// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Adapt the QEMU virtual NIC to the isolated console-network child.
// Author: Lukas Bower

//! QEMU-only root adapter for the isolated console-network service.
//!
//! Root retains the virtual NIC and console policy. Ethernet, IP, TCP,
//! authentication, and framing are owned by the compiler-declared child. Every
//! crossing copies through the four fixed ABI pages; no root pointer or NIC cap
//! enters the child.

use core::fmt;

use console_network_abi::ExchangeKind;
use heapless::{Deque, String as HeaplessString, Vec as HeaplessVec};
use smoltcp::phy::{Device, RxToken, TxToken};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, Ipv4Address};

use super::{
    ConsoleLine, ConsoleNetConfig, NetConsoleDisconnectReason, NetConsoleEvent, NetCounters,
    NetDevice, NetPoller, NetStatusReport, NetTelemetry, NET_STAGE,
};
use crate::console_network_service::{BoundaryError, ConsoleNetworkContainmentProof, ServiceState};
use crate::drivers::virtio::net::{DriverError as VirtioDriverError, VirtioNetStatic};
use crate::hal::console_network::ConsoleNetworkRuntime;
use crate::hal::driver_task::DriverTaskContract;
use crate::hal::{HalError, KernelHal};
use crate::observe::IngestSnapshot;
use crate::rust_alloc::boxed::Box;
use crate::serial::DEFAULT_LINE_CAPACITY;

const LINE_QUEUE_DEPTH: usize = 8;
const EVENT_QUEUE_DEPTH: usize = 8;

/// Construction failure for the QEMU isolated console-network adapter.
#[derive(Debug)]
pub enum IsolatedConsoleInitError {
    /// Virtual NIC discovery or construction failed.
    Driver(VirtioDriverError),
    /// Child object, mapping, capability, or MCS construction failed.
    Hal(HalError),
    /// QEMU configuration disagrees with the generated child contract.
    InvalidConfig(&'static str),
}

impl fmt::Display for IsolatedConsoleInitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Driver(error) => write!(formatter, "{error}"),
            Self::Hal(error) => write!(formatter, "{error}"),
            Self::InvalidConfig(reason) => formatter.write_str(reason),
        }
    }
}

/// Sole QEMU network-console owner visible through [`NetPoller`].
pub struct IsolatedVirtioConsole {
    device: VirtioNetStatic,
    runtime: ConsoleNetworkRuntime,
    mac: EthernetAddress,
    ip: Ipv4Address,
    prefix_len: u8,
    gateway: Option<Ipv4Address>,
    listen_port: u16,
    lines: Deque<ConsoleLine, LINE_QUEUE_DEPTH>,
    events: Deque<NetConsoleEvent, EVENT_QUEUE_DEPTH>,
    output: Deque<HeaplessString<DEFAULT_LINE_CAPACITY>, LINE_QUEUE_DEPTH>,
    pending_egress: Option<HeaplessVec<u8, { console_network_abi::ETHERNET_FRAME_BYTES }>>,
    active_connection: Option<u64>,
    authenticated_connection: Option<u64>,
    listener_ready: bool,
    disconnect_requested: bool,
    output_issued: bool,
    faulted: bool,
    terminal: bool,
    last_now_ms: u64,
    telemetry: NetTelemetry,
    counters: NetCounters,
    connection_bytes_read: u64,
    connection_bytes_written: u64,
    ingest_backpressure: u64,
    ingest_dropped: u64,
}

impl IsolatedVirtioConsole {
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
        Ok(Box::new(Self {
            device,
            runtime,
            mac,
            ip,
            prefix_len: config.address.prefix_len,
            gateway,
            listen_port: config.listen_port,
            lines: Deque::new(),
            events: Deque::new(),
            output: Deque::new(),
            pending_egress: None,
            active_connection: None,
            authenticated_connection: None,
            listener_ready: false,
            disconnect_requested: false,
            output_issued: false,
            faulted: false,
            terminal: false,
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
        }))
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

    /// Contain and revoke the exact child generation.
    pub fn contain(
        &mut self,
        hal: &mut KernelHal<'_>,
    ) -> Result<ConsoleNetworkContainmentProof, HalError> {
        let proof = self.runtime.contain(hal)?;
        self.faulted = true;
        self.terminal = true;
        self.listener_ready = false;
        self.active_connection = None;
        self.authenticated_connection = None;
        self.lines.clear();
        self.events.clear();
        self.output.clear();
        self.pending_egress = None;
        Ok(proof)
    }

    /// Consume the critical service mailbox and contain a faulted generation.
    pub fn contain_if_faulted(&mut self, hal: &mut KernelHal<'_>) -> Result<bool, HalError> {
        if self.terminal {
            return Ok(false);
        }
        match crate::hal::critical_tcb::take_target_service_fault(
            crate::console_network_service::SERVICE_TASK_ID,
        ) {
            Ok(Some(record)) => {
                if u64::from(record.identity.supervisor_generation) != self.runtime.generation() {
                    self.runtime.record_supervisor_fault();
                    self.faulted = true;
                    return self.contain(hal).map(|_| true);
                }
                log::error!(
                    "[console-network] generation={} terminal-fault class={:?} sequence={}",
                    self.runtime.generation(),
                    record.fault_class,
                    record.sequence,
                );
                self.runtime.record_supervisor_fault();
                self.faulted = true;
            }
            Ok(None) => {}
            Err(crate::hal::critical_tcb::CriticalTcbConstructionError::FaultHandoff(
                crate::critical_tcb::FaultHandoffError::Contended,
            )) => {
                // The critical mailbox is durable. Contention means another
                // bounded root-control turn owns its lock, so retry without
                // manufacturing a service fault or losing the record.
            }
            Err(error) => {
                log::error!(
                    "[console-network] generation={} fault-mailbox-invalid error={error:?}",
                    self.runtime.generation(),
                );
                self.runtime.record_supervisor_fault();
                self.faulted = true;
            }
        }
        if self.faulted {
            self.contain(hal).map(|_| true)
        } else {
            Ok(false)
        }
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
        self.lines.clear();
        self.events.clear();
        self.output.clear();
        self.pending_egress = None;
    }

    fn handle_event(&mut self, event: crate::console_network_service::ConsoleNetworkEvent) {
        let connection_id = event.connection_id();
        match event.kind() {
            ExchangeKind::Ready => self.listener_ready = true,
            ExchangeKind::Connected => {
                self.active_connection = Some(connection_id);
                self.authenticated_connection = None;
                self.disconnect_requested = false;
                self.output_issued = false;
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
                        .push_back(ConsoleLine::new(line, event.now_ms()))
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
                self.output_issued = false;
                self.output.clear();
            }
            ExchangeKind::Backpressure => self.fail_closed("child-event-backpressure"),
            ExchangeKind::Rejected
            | ExchangeKind::PacketConsumed
            | ExchangeKind::ControlCompleted
            | ExchangeKind::OutputDrained => {}
            ExchangeKind::ShutdownComplete => {
                self.terminal = true;
                self.listener_ready = false;
                self.active_connection = None;
                self.authenticated_connection = None;
            }
            ExchangeKind::SendLine | ExchangeKind::Disconnect => {
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
        }
        if let Some(egress) = turn.egress {
            activity = true;
            if self.pending_egress.replace(egress).is_some() {
                self.fail_closed("egress-overwrite");
            }
        }
        activity
    }

    fn transmit_pending_egress(&mut self, timestamp: Instant) -> bool {
        let Some(frame) = self.pending_egress.take() else {
            return false;
        };
        let Some(token) = self.device.transmit(timestamp) else {
            self.pending_egress = Some(frame);
            return false;
        };
        let length = frame.len();
        token.consume(length, |output| output.copy_from_slice(frame.as_slice()));
        true
    }

    fn stage_one_ingress(&mut self, timestamp: Instant) -> bool {
        if self.pending_egress.is_some() || !self.runtime.ingress_available() {
            return false;
        }
        self.device.begin_smoltcp_rx_transaction();
        let staged = if let Some((receive, transmit)) = self.device.receive(timestamp) {
            let runtime = &mut self.runtime;
            let result = receive.consume(|packet| {
                if packet.is_empty() {
                    Ok(None)
                } else {
                    runtime.stage_ingress(packet).map(Some)
                }
            });
            drop(transmit);
            Some(result)
        } else {
            None
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
        if !self.runtime.control_available() {
            return false;
        }
        let Some(line) = self.output.front() else {
            return false;
        };
        match self.runtime.stage_authorized_line(line, self.last_now_ms) {
            Ok(_) => {
                let length = line.len();
                let _ = self.output.pop_front();
                self.output_issued = true;
                self.connection_bytes_written =
                    self.connection_bytes_written.saturating_add(length as u64);
                self.counters.tcp_tx_bytes =
                    self.counters.tcp_tx_bytes.saturating_add(length as u64);
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
            return false;
        };
        if !self.disconnect_requested
            || !self.output.is_empty()
            || !self.runtime.control_available()
        {
            return false;
        }
        if self.output_issued && !self.runtime.console_output_drained(connection_id) {
            return false;
        }
        match self.runtime.stage_disconnect(self.last_now_ms) {
            Ok(_) => true,
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
}

impl NetPoller for IsolatedVirtioConsole {
    fn poll(&mut self, now_ms: u64) -> bool {
        self.last_now_ms = now_ms;
        self.telemetry.last_poll_ms = now_ms;
        if self.faulted || self.terminal || !self.runtime.activated() {
            self.telemetry.link_up = false;
            return false;
        }
        self.counters.smoltcp_polls = self.counters.smoltcp_polls.saturating_add(1);
        let timestamp = Instant::from_millis(now_ms.min(i64::MAX as u64) as i64);
        let mut activity = self.poll_child_output();
        if self.faulted {
            return activity;
        }
        activity |= self.transmit_pending_egress(timestamp);
        if self.pending_egress.is_none() {
            activity |= self.stage_one_output();
            activity |= self.stage_disconnect_if_drained();
            activity |= self.stage_one_ingress(timestamp);
            if self.runtime.service_tick().is_err() {
                self.fail_closed("service-tick");
            }
        }
        self.refresh_device_counters();
        activity
    }

    fn driver_task_contract(&self) -> DriverTaskContract {
        VirtioNetStatic::driver_task_contract()
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
        if self.faulted || self.terminal || self.authenticated_connection.is_none() {
            return !self.faulted && !self.terminal;
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            return true;
        }
        let mut bounded = HeaplessString::new();
        if bounded.push_str(line).is_err() || self.output.push_back(bounded).is_err() {
            return false;
        }
        true
    }

    fn request_disconnect(&mut self) {
        if self.active_connection.is_some() && !self.faulted && !self.terminal {
            self.disconnect_requested = true;
        }
    }

    fn console_output_drained(&self, connection_id: u64) -> bool {
        !self.faulted
            && self.output.is_empty()
            && self.runtime.console_output_drained(connection_id)
    }

    fn drain_console_events(&mut self, visitor: &mut dyn FnMut(NetConsoleEvent)) {
        while let Some(event) = self.events.pop_front() {
            visitor(event);
        }
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
            profile_backend: "virtio-net",
            backend: "virtio-net",
            active_driver: "virtio-net",
            mode: "static",
            interface_policy: "wired",
            active_interface: "wired",
            standby_interface: "none",
            address_source: "dev-virt-isolated-child",
            ip,
            gateway,
            dhcp_phase: "disabled",
            tcp_ready: self.console_listener_ready(),
        }
    }

    fn contain_faulted_console_service(
        &mut self,
        hal: &mut KernelHal<'_>,
    ) -> Result<bool, HalError> {
        self.contain_if_faulted(hal)
    }
}
