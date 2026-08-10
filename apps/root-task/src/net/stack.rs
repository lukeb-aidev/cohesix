// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Smoltcp-backed TCP console stack for the in-VM root task.
// Author: Lukas Bower

//! Smoltcp-backed TCP console stack for the in-VM root task.
//!
//! Feature toggles:
//! - `net-trace-31337` (default for `dev-virt`) logs virtio RX/TX frames and TCP
//!   console socket activity for port 31337.
//! - `tcp-echo-31337` bypasses console authentication and echoes any bytes
//!   received on port 31337 back to the sender for plumbing checks (`nc
//!   127.0.0.1 31337`).
//!
//! Host sanity checks:
//! - With `tcp-echo-31337`, run `nc 127.0.0.1 31337` and type input; expect
//!   echoed bytes plus `[net-trace]` RX/TX lines for port 31337.
//! - With tracing enabled, `./cohsh --transport tcp --tcp-port 31337 --role queen`
//!   should emit auth telemetry describing frame length and session state without
//!   disclosing credentials.
#![allow(unsafe_code)]
#![cfg(any(test, feature = "kernel"))]

use core::fmt::{self, Write as FmtWrite};
use core::mem;
use core::ops::Range;
use heapless::{String as HeaplessString, Vec as HeaplessVec};
use log::{debug, error, info, trace, warn};
use portable_atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use smoltcp::config::IFACE_NEIGHBOR_CACHE_COUNT;
use smoltcp::iface::{
    Config as IfaceConfig, Interface, PollIngressSingleResult, PollResult, SocketHandle, SocketSet,
    SocketStorage,
};
use smoltcp::socket::raw::{
    PacketBuffer as RawPacketBuffer, PacketMetadata as RawPacketMetadata,
    RecvError as RawRecvError, SendError as RawSendError, Socket as RawSocket,
};
use smoltcp::socket::tcp::{
    ConnectError as TcpConnectError, RecvError as TcpRecvError, Socket as TcpSocket,
    SocketBuffer as TcpSocketBuffer, State as TcpState,
};
use smoltcp::socket::udp::{
    BindError as UdpBindError, PacketBuffer as UdpPacketBuffer,
    PacketMetadata as UdpPacketMetadata, RecvError as UdpRecvError, SendError as UdpSendError,
    Socket as UdpSocket,
};
use smoltcp::time::Instant;
use smoltcp::wire::{
    EthernetAddress, HardwareAddress, IpAddress, IpCidr, IpEndpoint, IpListenEndpoint, IpProtocol,
    IpVersion, Ipv4Address,
};

use super::{
    console_srv::{SessionEvent, TcpConsoleServer},
    dhcp::{DhcpClient, DhcpEvent, DhcpLease, DhcpPhase, DHCP_CLIENT_PORT, DHCP_SERVER_PORT},
    outbound::{OutboundCoalescer, OutboundLane, SendError},
    parse_icmp_echo_request, ConsoleLine, ConsoleNetConfig, NetBackend, NetConsoleDisconnectReason,
    NetConsoleEvent, NetCounters, NetDevice, NetDriverError, NetInterfacePolicy, NetMode,
    NetPoller, NetSelfTestReport, NetSelfTestResult, NetSelfTestStartResult, NetStage,
    NetStatusReport, NetTelemetry, WifiCredentials, DEV_VIRT_GATEWAY, DEV_VIRT_IP, DEV_VIRT_PREFIX,
    MAX_FRAME_LEN, NET_DIAG, NET_STAGE,
};
use crate::bootstrap::bootinfo_snapshot::{BootInfoCanaryError, BootInfoState};
use crate::debug::maybe_report_str_write;
use crate::drivers::driver_task_net::{
    Cyw43DriverTaskDevice, DriverTaskNetError, GenetDriverTaskDevice,
};
use crate::drivers::rtl8139::{DriverError as Rtl8139DriverError, Rtl8139Device};
#[cfg(feature = "net-backend-virtio")]
use crate::drivers::virtio::net::{DriverError as VirtioDriverError, VirtioNetStatic};
use crate::hal::driver_task::{
    DriverServiceBudget, DriverServiceBudgetError, CYW43_WIFI_DRIVER_TASK_CONTRACT,
};
use crate::hal::{HalError, Hardware};
use crate::observe::IngestSnapshot;
use crate::readiness;
use crate::rust_alloc::boxed::Box;
use crate::sel4::BOOTINFO_WINDOW_GUARD;
use crate::serial::DEFAULT_LINE_CAPACITY;
use cohesix_proto::{REASON_INACTIVITY_TIMEOUT, REASON_RECV_ERROR};
use spin::Mutex;

const TCP_RX_BUFFER: usize = 32 * 1024;
const TCP_TX_BUFFER: usize = 32 * 1024;
const MAX_CONSOLE_FRAMES_PER_POLL: u32 = 32;
const MAX_CONSOLE_BYTES_PER_POLL: usize = 20 * 1024;
const SAME_TICK_STALL_WARN_POLLS: u16 = 256;
const MAX_DHCP_RX_PACKETS_PER_POLL: usize = 2;
const MAX_UDP_ECHO_PACKETS_PER_POLL: usize = 2;
const TCP_CONSOLE_RECV_CHUNK_BYTES: usize = DEFAULT_LINE_CAPACITY + 4;
const MAX_TCP_CONSOLE_RECV_CHUNKS_PER_POLL: usize = 64;
const MAX_TCP_CONSOLE_RECV_BYTES_PER_POLL: usize = 20 * 1024;
const TCP_SERVICE_BYTES_PER_TURN: u32 =
    (MAX_TCP_CONSOLE_RECV_BYTES_PER_POLL + MAX_CONSOLE_BYTES_PER_POLL) as u32;
const MAX_TCP_SMOKE_RECV_CHUNKS_PER_POLL: usize = 2;
const TCP_SMOKE_RX_BUFFER: usize = 256;
const TCP_SMOKE_TX_BUFFER: usize = 256;

/// Passive receipt for the exact WiFi DHCP start-to-bound transition.
#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Cyw43OldgoodDhcpReceipt {
    pub generation: u32,
    pub transaction_id: u32,
    pub start_now_ms: u64,
    pub ip: [u8; 4],
    pub prefix_len: u8,
    pub gateway: [u8; 4],
    pub server_id: [u8; 4],
    pub lease_seconds: u32,
    pub bound: bool,
}

#[cfg(feature = "kernel")]
static CYW43_OLDGOOD_DHCP_RECEIPT: Mutex<Option<Cyw43OldgoodDhcpReceipt>> = Mutex::new(None);

#[cfg(feature = "kernel")]
fn clear_cyw43_oldgood_dhcp_receipt() {
    *CYW43_OLDGOOD_DHCP_RECEIPT.lock() = None;
}

#[cfg(feature = "kernel")]
fn record_cyw43_oldgood_dhcp_start(generation: u32, transaction_id: u32, now_ms: u64) {
    *CYW43_OLDGOOD_DHCP_RECEIPT.lock() = (generation != 0).then_some(Cyw43OldgoodDhcpReceipt {
        generation,
        transaction_id,
        start_now_ms: now_ms,
        ip: [0; 4],
        prefix_len: 0,
        gateway: [0; 4],
        server_id: [0; 4],
        lease_seconds: 0,
        bound: false,
    });
}

#[cfg(feature = "kernel")]
fn record_cyw43_oldgood_dhcp_bound(generation: u32, lease: &DhcpLease) {
    let mut slot = CYW43_OLDGOOD_DHCP_RECEIPT.lock();
    let Some(receipt) = slot.as_mut() else {
        return;
    };
    let gateway = lease.gateway.unwrap_or([0; 4]);
    if generation == 0
        || receipt.generation != generation
        || lease.ip == [0; 4]
        || gateway == [0; 4]
    {
        *slot = None;
        return;
    }
    receipt.ip = lease.ip;
    receipt.prefix_len = lease.prefix_len;
    receipt.gateway = gateway;
    receipt.server_id = lease.server_id;
    receipt.lease_seconds = lease.lease_seconds;
    receipt.bound = true;
}

/// Return the current complete WiFi DHCP receipt without mutating the stack.
#[cfg(feature = "kernel")]
pub(crate) fn cyw43_oldgood_dhcp_receipt() -> Option<Cyw43OldgoodDhcpReceipt> {
    let receipt = (*CYW43_OLDGOOD_DHCP_RECEIPT.lock())?;
    (receipt.bound
        && receipt.generation != 0
        && receipt.generation == crate::drivers::driver_task_net::cyw43_connection_generation())
    .then_some(receipt)
}
// Full networking can concurrently own one raw ICMP responder, two console
// acceptors, DHCP, UDP beacon/echo, inbound/outbound smoke sockets, and the
// outbound probe.
const SOCKET_CAPACITY: usize = 9;
const ICMP_ECHO_RX_METADATA_CAPACITY: usize = 2;
const ICMP_ECHO_TX_METADATA_CAPACITY: usize = 1;
const ICMP_ECHO_RX_PAYLOAD_CAPACITY: usize = MAX_FRAME_LEN * ICMP_ECHO_RX_METADATA_CAPACITY;
const ICMP_ECHO_TX_PAYLOAD_CAPACITY: usize = MAX_FRAME_LEN;
const ICMP_ECHO_REPLY_DEADLINE_MS: u64 = 3_000;
const ICMP_ECHO_NEIGHBOR_RETRY_MS: u64 = 1_000;
const ICMP_ECHO_TX_AVAILABILITY_RETRY_MS: u64 = 1;
const FLUSH_BLOCKED_HEARTBEAT_MS: u64 = 2_000;
const RANDOM_SEED: u64 = 0x5a5a_5a5a_1234_5678;
const ECHO_MODE: bool = cfg!(feature = "tcp-echo-31337");
const ERR_AUTH_REASON_TIMEOUT: &str = "ERR AUTH reason=timeout";
const ERR_CONSOLE_REASON_TIMEOUT: &str = "ERR CONSOLE reason=timeout";
const UDP_METADATA_CAPACITY: usize = 8;
const UDP_PAYLOAD_CAPACITY: usize = 512;
const DHCP_PAYLOAD_CAPACITY: usize = 576;
const DHCP_METADATA_CAPACITY: usize = 4;
const DHCP_RESTART_BACKOFF_MS: u64 = if cfg!(feature = "timers-arch-counter") {
    4_000
} else {
    64_000
};
const CYW43_DHCP_POST_SECURE_EAPOL_QUIET_MS: u64 = 0;
const CYW43_DHCP_POST_SECURE_EAPOL_OVERSHOOT_LOG_MS: u64 = 1_000;
const CYW43_DHCP_RX_ADMISSION_RETRY_MS: u64 = 250;
// CYW43 host-EAPOL urgency is retained by the linked-runtime cursor. The
// ordinary EventPump grants one child-runtime operation per outer turn.
const CYW43_HOST_EAPOL_BUDGETED_SERVICE_POLLS: usize = 1;
const UDP_ECHO_PORT: u16 = 31_338;
const UDP_BEACON_PORT: u16 = 40_000;
pub(crate) const TCP_SMOKE_PORT: u16 = 31_339;
const TCP_SMOKE_OUT_LOCAL_PORT: u16 = 31_340;
const TCP_CONSOLE_SELFTEST_LOCAL_PORT: u16 = 31_341;
const CONSOLE_SELFTEST_RECOVERY_DEADLINE_MS: u64 = 3_000;
const CONSOLE_SELFTEST_RETRY_MS: u64 = 250;
const DISCONNECT_DRAIN_DEADLINE_MS: u64 = 10_000;
const DISCONNECT_PEER_CLOSE_GRACE_MS: u64 = 1_000;
const DISCONNECT_CLOSE_DEADLINE_MS: u64 = 10_000;
const CONSOLE_HANDOFF_PENDING_DEADLINE_MS: u64 =
    DISCONNECT_DRAIN_DEADLINE_MS + DISCONNECT_PEER_CLOSE_GRACE_MS + DISCONNECT_CLOSE_DEADLINE_MS;
const BOOTINFO_NET_LOGGER_PREFIX_BUDGET: usize = 48;
const BOOTINFO_NET_LOGGER_FRAME_LIMIT: usize = 192;
#[cfg(feature = "net-outbound-probe")]
const TCP_PROBE_PORT: u16 = TCP_SMOKE_PORT;
#[cfg(feature = "net-outbound-probe")]
const TCP_PROBE_BUFFER: usize = 128;
#[cfg(feature = "net-outbound-probe")]
const TCP_PROBE_RETRY_MS: u64 = 1_000;
#[cfg(feature = "net-outbound-probe")]
const TCP_PROBE_PAYLOAD: &[u8] = b"COHESIX-PING\n";
const NEIGHBOR_CACHE_SIZE: usize = IFACE_NEIGHBOR_CACHE_COUNT;
const SELF_TEST_BEACON_INTERVAL_MS: u64 = 250;
const SELF_TEST_BEACON_WINDOW_MS: u64 = 5_000;
const SELF_TEST_WINDOW_MS: u64 = 15_000;
const SELF_TEST_PEER_ASSISTED_MIN_MS: u64 = 500;
const SELF_TEST_TX_WRAP_BURST: u32 = 72;
const NET_INIT_TAG: &str = "net-console:init";
static STORAGE_ADDRESS_LOGGED: AtomicBool = AtomicBool::new(false);
static NET_WATCH_LOGGED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SelfTestLogSeverity {
    Warn,
    Debug,
    Trace,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConsoleDisconnectPhase {
    Idle,
    Draining,
    PeerCloseWait,
    Closing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConsoleDisconnectAction {
    Wait,
    StartPeerCloseWait,
    StartClose,
    ContinueClose,
    Complete,
    Abort,
}

fn console_disconnect_action(
    phase: ConsoleDisconnectPhase,
    tcp_state: TcpState,
    application_output_drained: bool,
    tcp_send_queue_empty: bool,
    peer_close_first: bool,
    now_ms: u64,
    phase_started_ms: u64,
) -> ConsoleDisconnectAction {
    match phase {
        ConsoleDisconnectPhase::Idle => ConsoleDisconnectAction::Wait,
        ConsoleDisconnectPhase::Draining => {
            if matches!(tcp_state, TcpState::Closed | TcpState::TimeWait) {
                ConsoleDisconnectAction::Complete
            } else if matches!(
                tcp_state,
                TcpState::FinWait1 | TcpState::FinWait2 | TcpState::Closing | TcpState::LastAck
            ) {
                ConsoleDisconnectAction::ContinueClose
            } else if application_output_drained && tcp_send_queue_empty {
                if peer_close_first && tcp_state == TcpState::Established {
                    ConsoleDisconnectAction::StartPeerCloseWait
                } else {
                    ConsoleDisconnectAction::StartClose
                }
            } else if now_ms.saturating_sub(phase_started_ms) >= DISCONNECT_DRAIN_DEADLINE_MS {
                ConsoleDisconnectAction::Abort
            } else {
                ConsoleDisconnectAction::Wait
            }
        }
        ConsoleDisconnectPhase::PeerCloseWait => {
            if matches!(tcp_state, TcpState::Closed | TcpState::TimeWait) {
                ConsoleDisconnectAction::Complete
            } else if matches!(
                tcp_state,
                TcpState::FinWait1 | TcpState::FinWait2 | TcpState::Closing | TcpState::LastAck
            ) {
                ConsoleDisconnectAction::ContinueClose
            } else if tcp_state == TcpState::CloseWait {
                ConsoleDisconnectAction::StartClose
            } else if now_ms.saturating_sub(phase_started_ms) >= DISCONNECT_PEER_CLOSE_GRACE_MS {
                ConsoleDisconnectAction::Abort
            } else {
                ConsoleDisconnectAction::Wait
            }
        }
        ConsoleDisconnectPhase::Closing => {
            if matches!(tcp_state, TcpState::Closed | TcpState::TimeWait) {
                ConsoleDisconnectAction::Complete
            } else if now_ms.saturating_sub(phase_started_ms) >= DISCONNECT_CLOSE_DEADLINE_MS {
                ConsoleDisconnectAction::Abort
            } else {
                ConsoleDisconnectAction::Wait
            }
        }
    }
}

const fn arm_console_disconnect_phase_deadline(
    phase: ConsoleDisconnectPhase,
    phase_started_ms: Option<u64>,
    now_ms: u64,
) -> Option<u64> {
    match phase {
        ConsoleDisconnectPhase::Idle => None,
        ConsoleDisconnectPhase::Draining
        | ConsoleDisconnectPhase::PeerCloseWait
        | ConsoleDisconnectPhase::Closing => Some(match phase_started_ms {
            Some(started_ms) => started_ms,
            None => now_ms,
        }),
    }
}

const fn console_disconnect_terminal_reason(
    action: ConsoleDisconnectAction,
    origin: NetConsoleDisconnectReason,
) -> NetConsoleDisconnectReason {
    if matches!(action, ConsoleDisconnectAction::Abort) {
        NetConsoleDisconnectReason::Error
    } else {
        origin
    }
}

const fn console_output_admitted_during_disconnect(phase: ConsoleDisconnectPhase) -> bool {
    !matches!(
        phase,
        ConsoleDisconnectPhase::PeerCloseWait | ConsoleDisconnectPhase::Closing
    )
}

const fn console_disconnect_application_queues_drained(
    server_outbound_pending: bool,
    coalescer_output_pending: bool,
    inbound_queued: u32,
    entered_draining_this_turn: bool,
) -> bool {
    !entered_draining_this_turn
        && !server_outbound_pending
        && !coalescer_output_pending
        && inbound_queued == 0
}

fn begin_console_disconnect(
    phase: &mut ConsoleDisconnectPhase,
    phase_started_ms: &mut Option<u64>,
    active_reason: &mut NetConsoleDisconnectReason,
    entered_this_turn: &mut bool,
    reason: NetConsoleDisconnectReason,
) -> bool {
    if !matches!(*phase, ConsoleDisconnectPhase::Idle) {
        return false;
    }
    *phase = ConsoleDisconnectPhase::Draining;
    *phase_started_ms = None;
    *active_reason = reason;
    *entered_this_turn = true;
    true
}

const fn console_standby_should_arm(phase: ConsoleDisconnectPhase) -> bool {
    !matches!(phase, ConsoleDisconnectPhase::Idle)
}

const fn console_standby_pending_state(state: TcpState) -> bool {
    matches!(state, TcpState::SynReceived | TcpState::Established)
}

const fn console_standby_promotable_state(state: TcpState) -> bool {
    matches!(
        state,
        TcpState::Listen | TcpState::SynReceived | TcpState::Established
    )
}

const fn console_active_terminal_state(state: TcpState) -> bool {
    matches!(state, TcpState::Closed | TcpState::TimeWait)
}

const fn console_standby_pending_expired(pending_since_ms: Option<u64>, now_ms: u64) -> bool {
    match pending_since_ms {
        Some(started_ms) => {
            now_ms.saturating_sub(started_ms) >= CONSOLE_HANDOFF_PENDING_DEADLINE_MS
        }
        None => false,
    }
}

const fn console_handoff_authority_cleared(
    phase: ConsoleDisconnectPhase,
    session_active: bool,
    active_client_present: bool,
    authenticated: bool,
    auth_state: AuthState,
    peer_present: bool,
    inbound_queued: u32,
    coalescer_pending: bool,
) -> bool {
    matches!(phase, ConsoleDisconnectPhase::Idle)
        && !session_active
        && !active_client_present
        && !authenticated
        && matches!(auth_state, AuthState::Start)
        && !peer_present
        && inbound_queued == 0
        && !coalescer_pending
}

const fn console_socket_service_pending(
    active_state: TcpState,
    active_recv_queue: usize,
    active_send_queue: usize,
    standby_state: TcpState,
    server_outbound_pending: bool,
    coalescer_pending: bool,
    disconnect_phase: ConsoleDisconnectPhase,
) -> bool {
    server_outbound_pending
        || coalescer_pending
        || active_recv_queue != 0
        || active_send_queue != 0
        || matches!(
            active_state,
            TcpState::SynReceived
                | TcpState::CloseWait
                | TcpState::FinWait1
                | TcpState::FinWait2
                | TcpState::Closing
                | TcpState::LastAck
        )
        || !matches!(disconnect_phase, ConsoleDisconnectPhase::Idle)
        || console_standby_pending_state(standby_state)
}

const fn self_test_enabled_for_backend(backend: NetBackend) -> bool {
    cfg!(feature = "net-selftest")
        || backend.uses_dev_virt_defaults()
        || matches!(backend, NetBackend::BcmGenet)
}

const fn udp_beacon_send_failure_log_severity(
    buffer_full: bool,
    log_count: u32,
) -> SelfTestLogSeverity {
    if buffer_full {
        if log_count <= 1 {
            SelfTestLogSeverity::Debug
        } else {
            SelfTestLogSeverity::Trace
        }
    } else if log_count <= 1 {
        SelfTestLogSeverity::Warn
    } else {
        SelfTestLogSeverity::Debug
    }
}

fn dhcp_phase_for_bringup_status(status: &'static str) -> &'static str {
    match status.as_bytes() {
        b"wifi-host-eapol-pending" => "host-eapol-pending",
        b"wifi-host-eapol-required" => "host-eapol-required",
        b"wifi-data-handoff-pending" => "data-handoff-pending",
        b"wifi-association-failed" => "failed",
        b"wifi-link-down" => "link-down",
        _ => "associating",
    }
}

fn dhcp_restart_required_after_mac_sync(
    mode: NetMode,
    ip: Ipv4Address,
    dhcp_started: bool,
) -> bool {
    dhcp_started && matches!(mode, NetMode::Dhcp) && ip == Ipv4Address::UNSPECIFIED
}

fn budgeted_dhcp_service_required(mode: NetMode, ip: Ipv4Address, dhcp_socket_ready: bool) -> bool {
    dhcp_socket_ready && matches!(mode, NetMode::Dhcp) && ip == Ipv4Address::UNSPECIFIED
}

#[cfg(feature = "kernel")]
fn wifi_connection_generation_for<D: NetDevice>() -> u32 {
    if D::driver_task_contract() == CYW43_WIFI_DRIVER_TASK_CONTRACT {
        crate::drivers::driver_task_net::cyw43_connection_generation()
    } else {
        0
    }
}

const fn cyw43_pre_poll_generation_fence_required(cached: u32, observed: u32) -> bool {
    cached != observed
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Cyw43GenerationProofBaseline {
    generation: u32,
    accepts: u64,
    authenticated_sessions: u64,
    rx_bytes: u64,
    root_rx_drops: u64,
    runtime_rx_overflow_episodes: u64,
    data_trace_faults: u64,
    data_trace_tx_retries: u64,
    tx_submit: u64,
    tx_complete: u64,
}

impl Cyw43GenerationProofBaseline {
    fn capture(generation: u32, counters: NetCounters) -> Self {
        Self {
            generation,
            accepts: counters.tcp_accepts,
            authenticated_sessions: counters.tcp_auth_sessions,
            rx_bytes: counters.tcp_rx_bytes,
            root_rx_drops: counters.wifi_rx_pending_drops,
            runtime_rx_overflow_episodes: counters.wifi_rx_runtime_overflow_episodes,
            data_trace_faults: counters.wifi_data_trace_faults,
            data_trace_tx_retries: counters.wifi_data_trace_tx_retries,
            tx_submit: counters.tx_submit,
            tx_complete: counters.tx_complete,
        }
    }

    fn project(self, observed_generation: u32, mut counters: NetCounters) -> NetCounters {
        if self.generation != observed_generation {
            counters.tcp_accepts = 0;
            counters.tcp_auth_sessions = 0;
            counters.tcp_rx_bytes = 0;
            counters.wifi_rx_pending_drops = 0;
            counters.wifi_rx_runtime_queue_overflow_seen = 0;
            counters.wifi_rx_runtime_overflow_episodes = 0;
            counters.wifi_data_trace_faults = 0;
            counters.wifi_data_trace_tx_retries = 0;
            counters.tx_submit = 0;
            counters.tx_complete = 0;
            return counters;
        }
        counters.tcp_accepts = counters.tcp_accepts.saturating_sub(self.accepts);
        counters.tcp_auth_sessions = counters
            .tcp_auth_sessions
            .saturating_sub(self.authenticated_sessions);
        counters.tcp_rx_bytes = counters.tcp_rx_bytes.saturating_sub(self.rx_bytes);
        counters.wifi_rx_pending_drops = counters
            .wifi_rx_pending_drops
            .saturating_sub(self.root_rx_drops);
        counters.wifi_rx_runtime_overflow_episodes = counters
            .wifi_rx_runtime_overflow_episodes
            .saturating_sub(self.runtime_rx_overflow_episodes);
        counters.wifi_rx_runtime_queue_overflow_seen =
            u64::from(counters.wifi_rx_runtime_overflow_episodes != 0);
        counters.wifi_data_trace_faults = counters
            .wifi_data_trace_faults
            .saturating_sub(self.data_trace_faults);
        counters.wifi_data_trace_tx_retries = counters
            .wifi_data_trace_tx_retries
            .saturating_sub(self.data_trace_tx_retries);
        counters.tx_submit = counters.tx_submit.saturating_sub(self.tx_submit);
        counters.tx_complete = counters.tx_complete.saturating_sub(self.tx_complete);
        counters
    }
}

#[cfg(not(feature = "kernel"))]
fn wifi_connection_generation_for<D: NetDevice>() -> u32 {
    let _ = core::marker::PhantomData::<D>;
    0
}

macro_rules! set_primary_ipv4_addr {
    ($addrs:expr, $cidr:expr) => {{
        let cidr = $cidr;
        if $addrs.is_empty() {
            let _ = $addrs.push(cidr);
        } else {
            $addrs[0] = cidr;
            while $addrs.len() > 1 {
                let _ = $addrs.pop();
            }
        }
    }};
}

fn console_listener_defer_reason_for(
    mode: NetMode,
    ip: Ipv4Address,
    bringup_status: Option<&'static str>,
) -> Option<&'static str> {
    if let Some(status) = bringup_status {
        return Some(status);
    }
    match mode {
        NetMode::Off => Some("policy-off"),
        NetMode::Static if ip == Ipv4Address::UNSPECIFIED => Some("ip-unconfigured"),
        NetMode::Dhcp if ip == Ipv4Address::UNSPECIFIED => Some("dhcp-pending"),
        NetMode::Static | NetMode::Dhcp => None,
    }
}

fn timebase_stall_warning_suppressed(bringup_status: Option<&'static str>) -> bool {
    matches!(bringup_status, Some("wifi-host-eapol-pending"))
}

fn timebase_stall_warning_due(
    same_tick_poll_count: u16,
    already_warned: bool,
    bringup_status: Option<&'static str>,
) -> bool {
    same_tick_poll_count >= SAME_TICK_STALL_WARN_POLLS
        && !already_warned
        && !timebase_stall_warning_suppressed(bringup_status)
}

fn wifi_host_eapol_blocks_data_path(bringup_status: Option<&'static str>) -> bool {
    matches!(
        bringup_status,
        Some("wifi-host-eapol-pending" | "wifi-host-eapol-required")
    )
}

fn wifi_host_eapol_blocks_driver_task_pre_poll(bringup_status: Option<&'static str>) -> bool {
    wifi_host_eapol_blocks_data_path(bringup_status)
}

fn wifi_driver_task_pre_poll_due(
    bringup_status: Option<&'static str>,
    retained_net_data_continuation: bool,
    runtime_pre_poll_allowed: bool,
) -> bool {
    retained_net_data_continuation
        || (runtime_pre_poll_allowed
            && !wifi_host_eapol_blocks_driver_task_pre_poll(bringup_status))
}

fn wifi_host_eapol_stack_service_polls(bringup_status: Option<&'static str>) -> usize {
    let _ = bringup_status;
    1
}

fn dhcp_start_defer_reason_for(bringup_status: Option<&'static str>) -> Option<&'static str> {
    match bringup_status {
        Some("dhcp-pending" | "dhcp-failed") | None => None,
        Some(status) => Some(status),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Cyw43StatusBlocker {
    address_source: &'static str,
    dhcp_phase: &'static str,
}

fn cyw43_tcp_data_path_proven(
    active_driver: &'static str,
    active_interface: &'static str,
    counters: NetCounters,
) -> bool {
    active_driver == "cyw43"
        && active_interface == "wifi"
        && (counters.tcp_accepts != 0 || counters.tcp_auth_sessions != 0)
}

fn physical_driver_tcp_data_path_required(
    active_driver: &'static str,
    active_interface: &'static str,
) -> bool {
    matches!(
        (active_driver, active_interface),
        ("cyw43", "wifi") | ("bcmgenet-v5", "wired")
    )
}

fn physical_driver_tcp_data_path_proven(
    active_driver: &'static str,
    active_interface: &'static str,
    counters: NetCounters,
) -> bool {
    physical_driver_tcp_data_path_required(active_driver, active_interface)
        && (counters.tcp_accepts != 0 || counters.tcp_auth_sessions != 0)
}

fn net_status_tcp_ready(
    listener_ready: bool,
    active_driver: &'static str,
    active_interface: &'static str,
    counters: NetCounters,
) -> bool {
    listener_ready
        && (!physical_driver_tcp_data_path_required(active_driver, active_interface)
            || physical_driver_tcp_data_path_proven(active_driver, active_interface, counters))
}

fn net_console_listener_ready(
    allow_tcp: bool,
    listener_announced: bool,
    listener_deferred: bool,
    wifi_rx_admission_blocked: bool,
) -> bool {
    allow_tcp && listener_announced && !listener_deferred && !wifi_rx_admission_blocked
}

fn cyw43_status_blocker_for(
    active_driver: &'static str,
    active_interface: &'static str,
    counters: NetCounters,
) -> Option<Cyw43StatusBlocker> {
    if active_driver != "cyw43" || active_interface != "wifi" {
        return None;
    }
    if cyw43_tcp_data_path_proven(active_driver, active_interface, counters) {
        return None;
    }
    if counters.wifi_rx_runtime_overflow_episodes != 0 || counters.wifi_rx_pending_drops != 0 {
        return Some(Cyw43StatusBlocker {
            address_source: "wifi-rx-overflow",
            dhcp_phase: "rx-overflow",
        });
    }
    if counters.wifi_host_eapol_m1 != 0
        && counters.wifi_host_eapol_m2 != 0
        && counters.wifi_host_eapol_m3 == 0
        && counters.wifi_host_eapol_secure == 0
    {
        return Some(Cyw43StatusBlocker {
            address_source: "host-eapol-m3-missing",
            dhcp_phase: "host-eapol-m3-missing",
        });
    }
    // Root submit/complete/free counters describe logical ownership, not the
    // runtime's SDPCM credit window. An ordinary retained or in-flight owner is
    // not a fault; only a typed data-path terminal fault may replace status.
    if counters.wifi_data_trace_faults != 0 {
        return Some(Cyw43StatusBlocker {
            address_source: "wifi-tx-terminal-fault",
            dhcp_phase: "tx-terminal-fault",
        });
    }
    None
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Cyw43DhcpPostSecureEapolSettle {
    ready: bool,
    changed: bool,
    quiet_ms: u64,
    remaining_ms: u64,
    next_ready_ms: Option<u64>,
}

fn cyw43_dhcp_post_secure_eapol_settle(
    now_ms: u64,
    eapol_secure: u64,
    eapol_rx: u64,
    last_eapol_rx: &mut u64,
    quiet_since_ms: &mut Option<u64>,
) -> Cyw43DhcpPostSecureEapolSettle {
    if eapol_secure == 0 || eapol_rx == 0 {
        *last_eapol_rx = eapol_rx;
        *quiet_since_ms = None;
        return Cyw43DhcpPostSecureEapolSettle {
            ready: false,
            changed: false,
            quiet_ms: 0,
            remaining_ms: CYW43_DHCP_POST_SECURE_EAPOL_QUIET_MS,
            next_ready_ms: None,
        };
    }
    if CYW43_DHCP_POST_SECURE_EAPOL_QUIET_MS == 0 {
        let changed = eapol_rx != *last_eapol_rx;
        *last_eapol_rx = eapol_rx;
        *quiet_since_ms = Some(now_ms);
        return Cyw43DhcpPostSecureEapolSettle {
            ready: true,
            changed,
            quiet_ms: 0,
            remaining_ms: 0,
            next_ready_ms: Some(now_ms),
        };
    }
    if eapol_rx != *last_eapol_rx {
        *last_eapol_rx = eapol_rx;
        *quiet_since_ms = Some(now_ms);
        return Cyw43DhcpPostSecureEapolSettle {
            ready: false,
            changed: true,
            quiet_ms: 0,
            remaining_ms: CYW43_DHCP_POST_SECURE_EAPOL_QUIET_MS,
            next_ready_ms: Some(now_ms.saturating_add(CYW43_DHCP_POST_SECURE_EAPOL_QUIET_MS)),
        };
    }
    let since_ms = quiet_since_ms.get_or_insert(now_ms);
    let quiet_ms = now_ms.saturating_sub(*since_ms);
    let ready = quiet_ms >= CYW43_DHCP_POST_SECURE_EAPOL_QUIET_MS;
    Cyw43DhcpPostSecureEapolSettle {
        ready,
        changed: false,
        quiet_ms,
        remaining_ms: CYW43_DHCP_POST_SECURE_EAPOL_QUIET_MS.saturating_sub(quiet_ms),
        next_ready_ms: Some(since_ms.saturating_add(CYW43_DHCP_POST_SECURE_EAPOL_QUIET_MS)),
    }
}

#[cfg(feature = "net-backend-virtio")]
type DefaultNetDevice = VirtioNetStatic;
#[cfg(not(feature = "net-backend-virtio"))]
type DefaultNetDevice = Rtl8139Device;

#[derive(Debug)]
pub enum DefaultDriverError {
    Rtl8139(Rtl8139DriverError),
    DriverTaskNet(DriverTaskNetError),
    #[cfg(feature = "net-backend-virtio")]
    Virtio(VirtioDriverError),
}

impl fmt::Display for DefaultDriverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rtl8139(err) => write!(f, "{err}"),
            Self::DriverTaskNet(err) => write!(f, "{err}"),
            #[cfg(feature = "net-backend-virtio")]
            Self::Virtio(err) => write!(f, "{err}"),
        }
    }
}

impl NetDriverError for DefaultDriverError {
    fn is_absent(&self) -> bool {
        match self {
            Self::Rtl8139(err) => err.is_absent(),
            Self::DriverTaskNet(err) => err.is_absent(),
            #[cfg(feature = "net-backend-virtio")]
            Self::Virtio(err) => err.is_absent(),
        }
    }
}

impl From<Rtl8139DriverError> for DefaultDriverError {
    fn from(value: Rtl8139DriverError) -> Self {
        Self::Rtl8139(value)
    }
}

impl From<DriverTaskNetError> for DefaultDriverError {
    fn from(value: DriverTaskNetError) -> Self {
        Self::DriverTaskNet(value)
    }
}

#[cfg(feature = "net-backend-virtio")]
impl From<VirtioDriverError> for DefaultDriverError {
    fn from(value: VirtioDriverError) -> Self {
        Self::Virtio(value)
    }
}

pub enum DefaultNetStack {
    Rtl8139(Box<NetStack<Rtl8139Device>>),
    GenetDriverTask(Box<NetStack<GenetDriverTaskDevice>>),
    Cyw43DriverTask(Box<NetStack<Cyw43DriverTaskDevice>>),
    #[cfg(feature = "net-backend-virtio")]
    Virtio(Box<NetStack<VirtioNetStatic>>),
}

pub type DefaultNetStackError = NetStackError<DefaultDriverError>;
pub type DefaultNetConsoleError = NetConsoleError<DefaultDriverError>;

#[derive(Debug)]
pub enum NetStackError<DE> {
    Driver(DE),
    AlreadyInitialisingOrOnline,
    BootInfoCanary(&'static str),
    SocketStorageInUse,
    SocketStoragePoisoned,
    TcpRxStorageInUse,
    TcpTxStorageInUse,
    TcpStandbyRxStorageInUse,
    TcpStandbyTxStorageInUse,
    TcpSmokeRxStorageInUse,
    TcpSmokeTxStorageInUse,
    IcmpEchoStorageInUse,
    UdpBeaconStorageInUse,
    UdpEchoStorageInUse,
    DhcpStorageInUse,
    DhcpSocketBind(UdpBindError),
    TcpProbeRxStorageInUse,
    TcpProbeTxStorageInUse,
    DriverTaskContract(&'static str),
}

impl<DE: fmt::Display> fmt::Display for NetStackError<DE> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Driver(err) => write!(f, "{err}"),
            Self::AlreadyInitialisingOrOnline => {
                f.write_str("network stack already initialising or online")
            }
            Self::BootInfoCanary(mark) => write!(f, "bootinfo canary diverged at {mark}"),
            Self::SocketStorageInUse => f.write_str("socket storage already in use"),
            Self::SocketStoragePoisoned => f.write_str("socket storage poisoned"),
            Self::TcpRxStorageInUse => f.write_str("TCP RX storage already in use"),
            Self::TcpTxStorageInUse => f.write_str("TCP TX storage already in use"),
            Self::TcpStandbyRxStorageInUse => f.write_str("TCP standby RX storage already in use"),
            Self::TcpStandbyTxStorageInUse => f.write_str("TCP standby TX storage already in use"),
            Self::TcpSmokeRxStorageInUse => f.write_str("TCP smoke test RX storage already in use"),
            Self::TcpSmokeTxStorageInUse => f.write_str("TCP smoke test TX storage already in use"),
            Self::IcmpEchoStorageInUse => f.write_str("ICMP echo storage already in use"),
            Self::UdpBeaconStorageInUse => f.write_str("UDP beacon storage already in use"),
            Self::UdpEchoStorageInUse => f.write_str("UDP echo storage already in use"),
            Self::DhcpStorageInUse => f.write_str("DHCP socket storage already in use"),
            Self::DhcpSocketBind(err) => write!(f, "DHCP socket bind failed: {err:?}"),
            Self::TcpProbeRxStorageInUse => f.write_str("TCP probe RX storage already in use"),
            Self::TcpProbeTxStorageInUse => f.write_str("TCP probe TX storage already in use"),
            Self::DriverTaskContract(reason) => {
                write!(f, "driver task contract rejected: {reason}")
            }
        }
    }
}

impl<DE> From<DE> for NetStackError<DE> {
    fn from(value: DE) -> Self {
        Self::Driver(value)
    }
}

/// High-level errors surfaced while initialising the TCP console stack.
#[derive(Debug)]
pub enum NetConsoleError<DE> {
    /// No network device was found on the selected backend.
    NoDevice,
    /// Provided network configuration was unusable.
    InvalidConfig(&'static str),
    /// An error occurred during stack bring-up.
    Init(NetStackError<DE>),
}

impl<DE: fmt::Display> fmt::Display for NetConsoleError<DE> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDevice => f.write_str("network device not present"),
            Self::InvalidConfig(reason) => write!(f, "invalid net config: {reason}"),
            Self::Init(err) => write!(f, "{err}"),
        }
    }
}

impl<DE: NetDriverError> From<NetStackError<DE>> for NetConsoleError<DE> {
    fn from(err: NetStackError<DE>) -> Self {
        match err {
            NetStackError::Driver(driver_err) if driver_err.is_absent() => Self::NoDevice,
            other => Self::Init(other),
        }
    }
}

const NET_STATE_NEVER: u8 = 0;
const NET_STATE_INITIALISING: u8 = 1;
const NET_STATE_ONLINE: u8 = 2;
const NET_STATE_FAILED: u8 = 3;

static NETSTACK_STATE: AtomicU8 = AtomicU8::new(NET_STATE_NEVER);
static NET_INIT_BOOT_COUNTER: AtomicU32 = AtomicU32::new(1);
static NET_INIT_ATTEMPT_COUNTER: AtomicU32 = AtomicU32::new(0);

#[derive(Debug, Clone, Copy)]
struct NetInitAttempt {
    boot: u32,
    attempt: u32,
    id: u64,
    tag: &'static str,
}

impl NetInitAttempt {
    fn new(tag: &'static str) -> Self {
        let boot = NET_INIT_BOOT_COUNTER.load(Ordering::Relaxed);
        let attempt = NET_INIT_ATTEMPT_COUNTER
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1);
        let id = ((boot as u64) << 32) | u64::from(attempt);
        Self {
            boot,
            attempt,
            id,
            tag,
        }
    }

    fn owner_id(&self) -> u64 {
        self.id
    }
}

#[derive(Debug)]
struct NetStackInitGuard {
    attempt: NetInitAttempt,
    committed: bool,
}

impl NetStackInitGuard {
    fn begin<DE>(tag: &'static str) -> Result<Self, NetStackError<DE>> {
        let attempt = NetInitAttempt::new(tag);
        let mut state = NETSTACK_STATE.load(Ordering::Acquire);
        loop {
            match state {
                NET_STATE_NEVER | NET_STATE_FAILED => match NETSTACK_STATE.compare_exchange(
                    state,
                    NET_STATE_INITIALISING,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ) {
                    Ok(_) => {
                        info!(
                            "[net-init] attempt_id=0x{:016x} state={state}->{} tag={tag}",
                            attempt.id, NET_STATE_INITIALISING
                        );
                        return Ok(Self {
                            attempt,
                            committed: false,
                        });
                    }
                    Err(next) => state = next,
                },
                NET_STATE_INITIALISING | NET_STATE_ONLINE => {
                    warn!(
                        "[net-init] concurrent attempt blocked state={} attempt_id=0x{:016x} tag={tag}",
                        state,
                        attempt.id
                    );
                    return Err(NetStackError::AlreadyInitialisingOrOnline);
                }
                other => {
                    warn!(
                        "[net-init] unexpected state={} while starting attempt_id=0x{:016x}",
                        other, attempt.id
                    );
                    NETSTACK_STATE.store(NET_STATE_FAILED, Ordering::Release);
                    state = NET_STATE_FAILED;
                }
            }
        }
    }

    fn attempt(&self) -> &NetInitAttempt {
        &self.attempt
    }

    fn commit_online(mut self) {
        NETSTACK_STATE.store(NET_STATE_ONLINE, Ordering::Release);
        self.committed = true;
        info!(
            "[net-init] attempt_id=0x{:016x} transitioned to online",
            self.attempt.id
        );
    }
}

impl Drop for NetStackInitGuard {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        NETSTACK_STATE.store(NET_STATE_FAILED, Ordering::Release);
        warn!(
            "[net-init] attempt_id=0x{:016x} marked failed",
            self.attempt.id
        );
    }
}

#[derive(Clone, Copy)]
struct StorageTag {
    id: u32,
    label: &'static str,
}

impl StorageTag {
    fn new(label: &'static str) -> Self {
        const OFFSET: u32 = 0x811c_9dc5;
        const PRIME: u32 = 0x0100_0193;
        let mut hash = OFFSET;
        for byte in label.as_bytes() {
            hash ^= u32::from(*byte);
            hash = hash.wrapping_mul(PRIME);
        }
        Self {
            id: hash.max(1),
            label,
        }
    }
}

#[derive(Clone, Copy)]
struct StorageMetadata {
    flag: &'static AtomicBool,
    owner: &'static AtomicU64,
    tag_id: &'static AtomicU32,
    tag_label: &'static Mutex<Option<&'static str>>,
    label: &'static str,
}

struct StorageLease {
    metadata: StorageMetadata,
}

impl StorageLease {
    fn new(metadata: StorageMetadata) -> Self {
        Self { metadata }
    }
}

impl Drop for StorageLease {
    fn drop(&mut self) {
        self.metadata.tag_id.store(0, Ordering::Release);
        if let Some(mut guard) = self.metadata.tag_label.try_lock() {
            *guard = None;
        }
        self.metadata.flag.store(false, Ordering::Release);
        self.metadata.owner.store(0, Ordering::Release);
    }
}

#[track_caller]
fn reserve_storage<DE>(
    metadata: StorageMetadata,
    owner_id: u64,
    tag: StorageTag,
    busy_error: NetStackError<DE>,
    poisoned_error: Option<NetStackError<DE>>,
) -> Result<StorageLease, NetStackError<DE>> {
    let caller = core::panic::Location::caller();
    match metadata
        .flag
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
    {
        Ok(_) => {
            metadata.owner.store(owner_id, Ordering::Release);
            metadata.tag_id.store(tag.id, Ordering::Release);
            if let Some(mut tag_guard) = metadata.tag_label.try_lock() {
                *tag_guard = Some(tag.label);
            }
            metadata.flag.store(true, Ordering::Release);
            Ok(StorageLease::new(metadata))
        }
        Err(_) => {
            let active_owner = metadata.owner.load(Ordering::Acquire);
            let active_tag_id = metadata.tag_id.load(Ordering::Acquire);
            let active_tag_label = metadata
                .tag_label
                .try_lock()
                .and_then(|guard| *guard)
                .unwrap_or("(unknown)");
            let poisoned = active_owner == 0;
            if poisoned {
                warn!(
                    "[net-storage] poisoned {} reservation detected at {}:{} in_use={} active_owner=0x{active_owner:016x} active_tag=0x{active_tag_id:08x} active_tag_label={active_tag_label} attempt_owner=0x{owner_id:016x} attempt_tag={attempt_tag}",
                    metadata.label,
                    caller.file(),
                    caller.line(),
                    metadata.flag.load(Ordering::Acquire),
                    attempt_tag = tag.label,
                );
                if let Some(poisoned_error) = poisoned_error {
                    return Err(poisoned_error);
                }
            }
            warn!(
                "[net-storage] guard={} busy attempt_owner=0x{owner_id:016x} attempt_tag={} in_use={} active_owner=0x{active_owner:016x} active_tag=0x{active_tag_id:08x} active_tag_label={active_tag_label} poisoned={}",
                metadata.label,
                tag.label,
                metadata.flag.load(Ordering::Acquire),
                poisoned,
            );
            Err(busy_error)
        }
    }
}

#[track_caller]
fn reserve_socket_storage<DE>(
    owner_id: u64,
    tag: StorageTag,
) -> Result<StorageLease, NetStackError<DE>> {
    reserve_storage(
        StorageMetadata {
            flag: &SOCKET_STORAGE_IN_USE,
            owner: &SOCKET_STORAGE_OWNER,
            tag_id: &SOCKET_STORAGE_TAG_ID,
            tag_label: &SOCKET_STORAGE_TAG_LABEL,
            label: "socket",
        },
        owner_id,
        tag,
        NetStackError::SocketStorageInUse,
        Some(NetStackError::SocketStoragePoisoned),
    )
}

fn reserve_tcp_rx_storage<DE>(
    owner_id: u64,
    tag: StorageTag,
) -> Result<StorageLease, NetStackError<DE>> {
    reserve_storage(
        StorageMetadata {
            flag: &TCP_RX_STORAGE_IN_USE,
            owner: &TCP_RX_STORAGE_OWNER,
            tag_id: &TCP_RX_STORAGE_TAG_ID,
            tag_label: &TCP_RX_STORAGE_TAG_LABEL,
            label: "tcp-rx",
        },
        owner_id,
        tag,
        NetStackError::TcpRxStorageInUse,
        None,
    )
}

fn reserve_tcp_tx_storage<DE>(
    owner_id: u64,
    tag: StorageTag,
) -> Result<StorageLease, NetStackError<DE>> {
    reserve_storage(
        StorageMetadata {
            flag: &TCP_TX_STORAGE_IN_USE,
            owner: &TCP_TX_STORAGE_OWNER,
            tag_id: &TCP_TX_STORAGE_TAG_ID,
            tag_label: &TCP_TX_STORAGE_TAG_LABEL,
            label: "tcp-tx",
        },
        owner_id,
        tag,
        NetStackError::TcpTxStorageInUse,
        None,
    )
}

fn reserve_tcp_standby_rx_storage<DE>(
    owner_id: u64,
    tag: StorageTag,
) -> Result<StorageLease, NetStackError<DE>> {
    reserve_storage(
        StorageMetadata {
            flag: &TCP_STANDBY_RX_STORAGE_IN_USE,
            owner: &TCP_STANDBY_RX_STORAGE_OWNER,
            tag_id: &TCP_STANDBY_RX_STORAGE_TAG_ID,
            tag_label: &TCP_STANDBY_RX_STORAGE_TAG_LABEL,
            label: "tcp-standby-rx",
        },
        owner_id,
        tag,
        NetStackError::TcpStandbyRxStorageInUse,
        None,
    )
}

fn reserve_tcp_standby_tx_storage<DE>(
    owner_id: u64,
    tag: StorageTag,
) -> Result<StorageLease, NetStackError<DE>> {
    reserve_storage(
        StorageMetadata {
            flag: &TCP_STANDBY_TX_STORAGE_IN_USE,
            owner: &TCP_STANDBY_TX_STORAGE_OWNER,
            tag_id: &TCP_STANDBY_TX_STORAGE_TAG_ID,
            tag_label: &TCP_STANDBY_TX_STORAGE_TAG_LABEL,
            label: "tcp-standby-tx",
        },
        owner_id,
        tag,
        NetStackError::TcpStandbyTxStorageInUse,
        None,
    )
}

fn reserve_tcp_smoke_rx_storage<DE>(
    owner_id: u64,
    tag: StorageTag,
) -> Result<StorageLease, NetStackError<DE>> {
    reserve_storage(
        StorageMetadata {
            flag: &TCP_SMOKE_RX_STORAGE_IN_USE,
            owner: &TCP_SMOKE_RX_STORAGE_OWNER,
            tag_id: &TCP_SMOKE_RX_STORAGE_TAG_ID,
            tag_label: &TCP_SMOKE_RX_STORAGE_TAG_LABEL,
            label: "tcp-smoke-rx",
        },
        owner_id,
        tag,
        NetStackError::TcpSmokeRxStorageInUse,
        None,
    )
}

fn reserve_tcp_smoke_tx_storage<DE>(
    owner_id: u64,
    tag: StorageTag,
) -> Result<StorageLease, NetStackError<DE>> {
    reserve_storage(
        StorageMetadata {
            flag: &TCP_SMOKE_TX_STORAGE_IN_USE,
            owner: &TCP_SMOKE_TX_STORAGE_OWNER,
            tag_id: &TCP_SMOKE_TX_STORAGE_TAG_ID,
            tag_label: &TCP_SMOKE_TX_STORAGE_TAG_LABEL,
            label: "tcp-smoke-tx",
        },
        owner_id,
        tag,
        NetStackError::TcpSmokeTxStorageInUse,
        None,
    )
}

fn reserve_tcp_smoke_out_rx_storage<DE>(
    owner_id: u64,
    tag: StorageTag,
) -> Result<StorageLease, NetStackError<DE>> {
    reserve_storage(
        StorageMetadata {
            flag: &TCP_SMOKE_OUT_RX_STORAGE_IN_USE,
            owner: &TCP_SMOKE_OUT_RX_STORAGE_OWNER,
            tag_id: &TCP_SMOKE_OUT_RX_STORAGE_TAG_ID,
            tag_label: &TCP_SMOKE_OUT_RX_STORAGE_TAG_LABEL,
            label: "tcp-smoke-out-rx",
        },
        owner_id,
        tag,
        NetStackError::TcpSmokeRxStorageInUse,
        None,
    )
}

fn reserve_tcp_smoke_out_tx_storage<DE>(
    owner_id: u64,
    tag: StorageTag,
) -> Result<StorageLease, NetStackError<DE>> {
    reserve_storage(
        StorageMetadata {
            flag: &TCP_SMOKE_OUT_TX_STORAGE_IN_USE,
            owner: &TCP_SMOKE_OUT_TX_STORAGE_OWNER,
            tag_id: &TCP_SMOKE_OUT_TX_STORAGE_TAG_ID,
            tag_label: &TCP_SMOKE_OUT_TX_STORAGE_TAG_LABEL,
            label: "tcp-smoke-out-tx",
        },
        owner_id,
        tag,
        NetStackError::TcpSmokeTxStorageInUse,
        None,
    )
}

fn reserve_icmp_echo_storage<DE>(
    owner_id: u64,
    tag: StorageTag,
) -> Result<StorageLease, NetStackError<DE>> {
    reserve_storage(
        StorageMetadata {
            flag: &ICMP_ECHO_STORAGE_IN_USE,
            owner: &ICMP_ECHO_STORAGE_OWNER,
            tag_id: &ICMP_ECHO_STORAGE_TAG_ID,
            tag_label: &ICMP_ECHO_STORAGE_TAG_LABEL,
            label: "icmp-echo",
        },
        owner_id,
        tag,
        NetStackError::IcmpEchoStorageInUse,
        None,
    )
}

fn reserve_udp_beacon_storage<DE>(
    owner_id: u64,
    tag: StorageTag,
) -> Result<StorageLease, NetStackError<DE>> {
    reserve_storage(
        StorageMetadata {
            flag: &UDP_BEACON_STORAGE_IN_USE,
            owner: &UDP_BEACON_STORAGE_OWNER,
            tag_id: &UDP_BEACON_STORAGE_TAG_ID,
            tag_label: &UDP_BEACON_STORAGE_TAG_LABEL,
            label: "udp-beacon",
        },
        owner_id,
        tag,
        NetStackError::UdpBeaconStorageInUse,
        None,
    )
}

fn reserve_udp_echo_storage<DE>(
    owner_id: u64,
    tag: StorageTag,
) -> Result<StorageLease, NetStackError<DE>> {
    reserve_storage(
        StorageMetadata {
            flag: &UDP_ECHO_STORAGE_IN_USE,
            owner: &UDP_ECHO_STORAGE_OWNER,
            tag_id: &UDP_ECHO_STORAGE_TAG_ID,
            tag_label: &UDP_ECHO_STORAGE_TAG_LABEL,
            label: "udp-echo",
        },
        owner_id,
        tag,
        NetStackError::UdpEchoStorageInUse,
        None,
    )
}

fn reserve_dhcp_storage<DE>(
    owner_id: u64,
    tag: StorageTag,
) -> Result<StorageLease, NetStackError<DE>> {
    reserve_storage(
        StorageMetadata {
            flag: &DHCP_STORAGE_IN_USE,
            owner: &DHCP_STORAGE_OWNER,
            tag_id: &DHCP_STORAGE_TAG_ID,
            tag_label: &DHCP_STORAGE_TAG_LABEL,
            label: "dhcp",
        },
        owner_id,
        tag,
        NetStackError::DhcpStorageInUse,
        None,
    )
}

#[cfg(feature = "net-outbound-probe")]
fn reserve_tcp_probe_rx_storage<DE>(
    owner_id: u64,
    tag: StorageTag,
) -> Result<StorageLease, NetStackError<DE>> {
    reserve_storage(
        StorageMetadata {
            flag: &TCP_PROBE_RX_STORAGE_IN_USE,
            owner: &TCP_PROBE_RX_STORAGE_OWNER,
            tag_id: &TCP_PROBE_RX_STORAGE_TAG_ID,
            tag_label: &TCP_PROBE_RX_STORAGE_TAG_LABEL,
            label: "tcp-probe-rx",
        },
        owner_id,
        tag,
        NetStackError::TcpProbeRxStorageInUse,
        None,
    )
}

#[cfg(feature = "net-outbound-probe")]
fn reserve_tcp_probe_tx_storage<DE>(
    owner_id: u64,
    tag: StorageTag,
) -> Result<StorageLease, NetStackError<DE>> {
    reserve_storage(
        StorageMetadata {
            flag: &TCP_PROBE_TX_STORAGE_IN_USE,
            owner: &TCP_PROBE_TX_STORAGE_OWNER,
            tag_id: &TCP_PROBE_TX_STORAGE_TAG_ID,
            tag_label: &TCP_PROBE_TX_STORAGE_TAG_LABEL,
            label: "tcp-probe-tx",
        },
        owner_id,
        tag,
        NetStackError::TcpProbeTxStorageInUse,
        None,
    )
}

struct StorageReservation {
    socket: StorageLease,
    tcp_rx: StorageLease,
    tcp_tx: StorageLease,
    tcp_standby_rx: StorageLease,
    tcp_standby_tx: StorageLease,
    dhcp: Option<StorageLease>,
    tcp_smoke_rx: Option<StorageLease>,
    tcp_smoke_tx: Option<StorageLease>,
    tcp_smoke_out_rx: Option<StorageLease>,
    tcp_smoke_out_tx: Option<StorageLease>,
    icmp_echo: StorageLease,
    udp_beacon: Option<StorageLease>,
    udp_echo: Option<StorageLease>,
    #[cfg(feature = "net-outbound-probe")]
    tcp_probe_rx: StorageLease,
    #[cfg(feature = "net-outbound-probe")]
    tcp_probe_tx: StorageLease,
}

impl StorageReservation {
    fn acquire<DE>(
        self_test_enabled: bool,
        dhcp_enabled: bool,
        owner: &NetInitAttempt,
        tag: &'static str,
    ) -> Result<Self, NetStackError<DE>> {
        let reservation_tag = StorageTag::new(tag);
        let socket = reserve_socket_storage(owner.owner_id(), reservation_tag)?;
        let tcp_rx = reserve_tcp_rx_storage(owner.owner_id(), reservation_tag)?;
        let tcp_tx = reserve_tcp_tx_storage(owner.owner_id(), reservation_tag)?;
        let tcp_standby_rx = reserve_tcp_standby_rx_storage(owner.owner_id(), reservation_tag)?;
        let tcp_standby_tx = reserve_tcp_standby_tx_storage(owner.owner_id(), reservation_tag)?;
        let dhcp = if dhcp_enabled {
            Some(reserve_dhcp_storage(owner.owner_id(), reservation_tag)?)
        } else {
            None
        };
        let tcp_smoke_rx = if self_test_enabled {
            Some(reserve_tcp_smoke_rx_storage(
                owner.owner_id(),
                reservation_tag,
            )?)
        } else {
            None
        };
        let tcp_smoke_tx = if self_test_enabled {
            Some(reserve_tcp_smoke_tx_storage(
                owner.owner_id(),
                reservation_tag,
            )?)
        } else {
            None
        };
        let tcp_smoke_out_rx = if self_test_enabled {
            Some(reserve_tcp_smoke_out_rx_storage(
                owner.owner_id(),
                reservation_tag,
            )?)
        } else {
            None
        };
        let tcp_smoke_out_tx = if self_test_enabled {
            Some(reserve_tcp_smoke_out_tx_storage(
                owner.owner_id(),
                reservation_tag,
            )?)
        } else {
            None
        };
        let icmp_echo = reserve_icmp_echo_storage(owner.owner_id(), reservation_tag)?;
        let udp_beacon = if self_test_enabled {
            Some(reserve_udp_beacon_storage(
                owner.owner_id(),
                reservation_tag,
            )?)
        } else {
            None
        };
        let udp_echo = if self_test_enabled {
            Some(reserve_udp_echo_storage(owner.owner_id(), reservation_tag)?)
        } else {
            None
        };
        #[cfg(feature = "net-outbound-probe")]
        let tcp_probe_rx = reserve_tcp_probe_rx_storage(owner.owner_id(), reservation_tag)?;
        #[cfg(feature = "net-outbound-probe")]
        let tcp_probe_tx = reserve_tcp_probe_tx_storage(owner.owner_id(), reservation_tag)?;

        Ok(Self {
            socket,
            tcp_rx,
            tcp_tx,
            tcp_standby_rx,
            tcp_standby_tx,
            dhcp,
            tcp_smoke_rx,
            tcp_smoke_tx,
            tcp_smoke_out_rx,
            tcp_smoke_out_tx,
            icmp_echo,
            udp_beacon,
            udp_echo,
            #[cfg(feature = "net-outbound-probe")]
            tcp_probe_rx,
            #[cfg(feature = "net-outbound-probe")]
            tcp_probe_tx,
        })
    }
}

fn log_bootinfo_mark<DE>(
    mark: &'static str,
    attempt: &NetInitAttempt,
) -> Result<(), NetStackError<DE>> {
    if let Some(state) = BootInfoState::get() {
        if let Err(err) = state.verify("net.init", mark) {
            match err {
                BootInfoCanaryError::Canary { .. }
                | BootInfoCanaryError::Snapshot { .. }
                | BootInfoCanaryError::Diverged { .. } => {
                    log::error!(
                        "[bootinfo:net] canary divergence mark={mark} attempt_id=0x{:016x} err={err:?}",
                        attempt.id
                    );
                    return Err(NetStackError::BootInfoCanary(mark));
                }
            }
        }
        debug_assert!(bootinfo_net_mark_ok_log_fits_uart_frame(mark));
        info!(
            "[bootinfo:net] attempt_id=0x{:016x} mark={mark} status=ok",
            attempt.id,
        );
    }

    Ok(())
}

fn bootinfo_net_mark_ok_log_fits_uart_frame(mark: &str) -> bool {
    let message_len = "[bootinfo:net] attempt_id=0x".len()
        + 16
        + " mark=".len()
        + mark.len()
        + " status=ok".len();
    message_len
        .saturating_add(BOOTINFO_NET_LOGGER_PREFIX_BUDGET)
        .saturating_add(2)
        <= BOOTINFO_NET_LOGGER_FRAME_LIMIT
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthState {
    Start,
    WaitingVersion,
    AuthRequested,
    AuthOk,
    AttachRequested,
    Attached,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CohshBlockedSnapshot {
    tcp_state: TcpState,
    auth_state: AuthState,
    queued: bool,
}

#[derive(Debug, Default)]
struct SessionState {
    last_state: Option<TcpState>,
    close_logged: bool,
    logged_accept: bool,
    logged_first_recv: bool,
    connect_reported: bool,
    logged_first_send: bool,
    not_ready_logged: bool,
    last_flush_state: Option<TcpState>,
    last_flush_auth_state: Option<AuthState>,
    last_flush_log_ms: u64,
    flush_blocked_since: Option<u64>,
    last_blocked_snapshot: Option<CohshBlockedSnapshot>,
    flush_blocked_logged_preconnect: bool,
}

static SOCKET_STORAGE_IN_USE: AtomicBool = AtomicBool::new(false);
static SOCKET_STORAGE_OWNER: AtomicU64 = AtomicU64::new(0);
static SOCKET_STORAGE_TAG_ID: AtomicU32 = AtomicU32::new(0);
static SOCKET_STORAGE_TAG_LABEL: Mutex<Option<&'static str>> = Mutex::new(None);
static mut SOCKET_STORAGE: [SocketStorage<'static>; SOCKET_CAPACITY] =
    [SocketStorage::EMPTY; SOCKET_CAPACITY];
static TCP_RX_STORAGE_IN_USE: AtomicBool = AtomicBool::new(false);
static TCP_RX_STORAGE_OWNER: AtomicU64 = AtomicU64::new(0);
static TCP_RX_STORAGE_TAG_ID: AtomicU32 = AtomicU32::new(0);
static TCP_RX_STORAGE_TAG_LABEL: Mutex<Option<&'static str>> = Mutex::new(None);
static mut TCP_RX_STORAGE: [u8; TCP_RX_BUFFER] = [0u8; TCP_RX_BUFFER];
static TCP_TX_STORAGE_IN_USE: AtomicBool = AtomicBool::new(false);
static TCP_TX_STORAGE_OWNER: AtomicU64 = AtomicU64::new(0);
static TCP_TX_STORAGE_TAG_ID: AtomicU32 = AtomicU32::new(0);
static TCP_TX_STORAGE_TAG_LABEL: Mutex<Option<&'static str>> = Mutex::new(None);
static mut TCP_TX_STORAGE: [u8; TCP_TX_BUFFER] = [0u8; TCP_TX_BUFFER];
static TCP_STANDBY_RX_STORAGE_IN_USE: AtomicBool = AtomicBool::new(false);
static TCP_STANDBY_RX_STORAGE_OWNER: AtomicU64 = AtomicU64::new(0);
static TCP_STANDBY_RX_STORAGE_TAG_ID: AtomicU32 = AtomicU32::new(0);
static TCP_STANDBY_RX_STORAGE_TAG_LABEL: Mutex<Option<&'static str>> = Mutex::new(None);
static mut TCP_STANDBY_RX_STORAGE: [u8; TCP_RX_BUFFER] = [0u8; TCP_RX_BUFFER];
static TCP_STANDBY_TX_STORAGE_IN_USE: AtomicBool = AtomicBool::new(false);
static TCP_STANDBY_TX_STORAGE_OWNER: AtomicU64 = AtomicU64::new(0);
static TCP_STANDBY_TX_STORAGE_TAG_ID: AtomicU32 = AtomicU32::new(0);
static TCP_STANDBY_TX_STORAGE_TAG_LABEL: Mutex<Option<&'static str>> = Mutex::new(None);
static mut TCP_STANDBY_TX_STORAGE: [u8; TCP_TX_BUFFER] = [0u8; TCP_TX_BUFFER];
static TCP_SMOKE_RX_STORAGE_IN_USE: AtomicBool = AtomicBool::new(false);
static TCP_SMOKE_RX_STORAGE_OWNER: AtomicU64 = AtomicU64::new(0);
static TCP_SMOKE_RX_STORAGE_TAG_ID: AtomicU32 = AtomicU32::new(0);
static TCP_SMOKE_RX_STORAGE_TAG_LABEL: Mutex<Option<&'static str>> = Mutex::new(None);
static mut TCP_SMOKE_RX_STORAGE: [u8; TCP_SMOKE_RX_BUFFER] = [0u8; TCP_SMOKE_RX_BUFFER];
static TCP_SMOKE_TX_STORAGE_IN_USE: AtomicBool = AtomicBool::new(false);
static TCP_SMOKE_TX_STORAGE_OWNER: AtomicU64 = AtomicU64::new(0);
static TCP_SMOKE_TX_STORAGE_TAG_ID: AtomicU32 = AtomicU32::new(0);
static TCP_SMOKE_TX_STORAGE_TAG_LABEL: Mutex<Option<&'static str>> = Mutex::new(None);
static mut TCP_SMOKE_TX_STORAGE: [u8; TCP_SMOKE_TX_BUFFER] = [0u8; TCP_SMOKE_TX_BUFFER];
static TCP_SMOKE_OUT_RX_STORAGE_IN_USE: AtomicBool = AtomicBool::new(false);
static TCP_SMOKE_OUT_RX_STORAGE_OWNER: AtomicU64 = AtomicU64::new(0);
static TCP_SMOKE_OUT_RX_STORAGE_TAG_ID: AtomicU32 = AtomicU32::new(0);
static TCP_SMOKE_OUT_RX_STORAGE_TAG_LABEL: Mutex<Option<&'static str>> = Mutex::new(None);
static mut TCP_SMOKE_OUT_RX_STORAGE: [u8; TCP_SMOKE_RX_BUFFER] = [0u8; TCP_SMOKE_RX_BUFFER];
static TCP_SMOKE_OUT_TX_STORAGE_IN_USE: AtomicBool = AtomicBool::new(false);
static TCP_SMOKE_OUT_TX_STORAGE_OWNER: AtomicU64 = AtomicU64::new(0);
static TCP_SMOKE_OUT_TX_STORAGE_TAG_ID: AtomicU32 = AtomicU32::new(0);
static TCP_SMOKE_OUT_TX_STORAGE_TAG_LABEL: Mutex<Option<&'static str>> = Mutex::new(None);
static mut TCP_SMOKE_OUT_TX_STORAGE: [u8; TCP_SMOKE_TX_BUFFER] = [0u8; TCP_SMOKE_TX_BUFFER];
#[cfg(feature = "net-outbound-probe")]
static TCP_PROBE_RX_STORAGE_IN_USE: AtomicBool = AtomicBool::new(false);
#[cfg(feature = "net-outbound-probe")]
static TCP_PROBE_RX_STORAGE_OWNER: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "net-outbound-probe")]
static TCP_PROBE_RX_STORAGE_TAG_ID: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "net-outbound-probe")]
static TCP_PROBE_RX_STORAGE_TAG_LABEL: Mutex<Option<&'static str>> = Mutex::new(None);
#[cfg(feature = "net-outbound-probe")]
static mut TCP_PROBE_RX_STORAGE: [u8; TCP_PROBE_BUFFER] = [0u8; TCP_PROBE_BUFFER];
#[cfg(feature = "net-outbound-probe")]
static TCP_PROBE_TX_STORAGE_IN_USE: AtomicBool = AtomicBool::new(false);
#[cfg(feature = "net-outbound-probe")]
static TCP_PROBE_TX_STORAGE_OWNER: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "net-outbound-probe")]
static TCP_PROBE_TX_STORAGE_TAG_ID: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "net-outbound-probe")]
static TCP_PROBE_TX_STORAGE_TAG_LABEL: Mutex<Option<&'static str>> = Mutex::new(None);
#[cfg(feature = "net-outbound-probe")]
static mut TCP_PROBE_TX_STORAGE: [u8; TCP_PROBE_BUFFER] = [0u8; TCP_PROBE_BUFFER];
static ICMP_ECHO_STORAGE_IN_USE: AtomicBool = AtomicBool::new(false);
static ICMP_ECHO_STORAGE_OWNER: AtomicU64 = AtomicU64::new(0);
static ICMP_ECHO_STORAGE_TAG_ID: AtomicU32 = AtomicU32::new(0);
static ICMP_ECHO_STORAGE_TAG_LABEL: Mutex<Option<&'static str>> = Mutex::new(None);
static UDP_BEACON_STORAGE_IN_USE: AtomicBool = AtomicBool::new(false);
static UDP_BEACON_STORAGE_OWNER: AtomicU64 = AtomicU64::new(0);
static UDP_BEACON_STORAGE_TAG_ID: AtomicU32 = AtomicU32::new(0);
static UDP_BEACON_STORAGE_TAG_LABEL: Mutex<Option<&'static str>> = Mutex::new(None);
static UDP_ECHO_STORAGE_IN_USE: AtomicBool = AtomicBool::new(false);
static UDP_ECHO_STORAGE_OWNER: AtomicU64 = AtomicU64::new(0);
static UDP_ECHO_STORAGE_TAG_ID: AtomicU32 = AtomicU32::new(0);
static UDP_ECHO_STORAGE_TAG_LABEL: Mutex<Option<&'static str>> = Mutex::new(None);
static DHCP_STORAGE_IN_USE: AtomicBool = AtomicBool::new(false);
static DHCP_STORAGE_OWNER: AtomicU64 = AtomicU64::new(0);
static DHCP_STORAGE_TAG_ID: AtomicU32 = AtomicU32::new(0);
static DHCP_STORAGE_TAG_LABEL: Mutex<Option<&'static str>> = Mutex::new(None);
static mut UDP_BEACON_RX_METADATA: [UdpPacketMetadata; UDP_METADATA_CAPACITY] =
    [UdpPacketMetadata::EMPTY; UDP_METADATA_CAPACITY];
static mut UDP_BEACON_TX_METADATA: [UdpPacketMetadata; UDP_METADATA_CAPACITY] =
    [UdpPacketMetadata::EMPTY; UDP_METADATA_CAPACITY];
static mut UDP_ECHO_RX_METADATA: [UdpPacketMetadata; UDP_METADATA_CAPACITY] =
    [UdpPacketMetadata::EMPTY; UDP_METADATA_CAPACITY];
static mut UDP_ECHO_TX_METADATA: [UdpPacketMetadata; UDP_METADATA_CAPACITY] =
    [UdpPacketMetadata::EMPTY; UDP_METADATA_CAPACITY];
static mut DHCP_RX_METADATA: [UdpPacketMetadata; DHCP_METADATA_CAPACITY] =
    [UdpPacketMetadata::EMPTY; DHCP_METADATA_CAPACITY];
static mut DHCP_TX_METADATA: [UdpPacketMetadata; DHCP_METADATA_CAPACITY] =
    [UdpPacketMetadata::EMPTY; DHCP_METADATA_CAPACITY];
static mut ICMP_ECHO_RX_METADATA: [RawPacketMetadata; ICMP_ECHO_RX_METADATA_CAPACITY] =
    [RawPacketMetadata::EMPTY; ICMP_ECHO_RX_METADATA_CAPACITY];
static mut ICMP_ECHO_TX_METADATA: [RawPacketMetadata; ICMP_ECHO_TX_METADATA_CAPACITY] =
    [RawPacketMetadata::EMPTY; ICMP_ECHO_TX_METADATA_CAPACITY];
static mut ICMP_ECHO_RX_STORAGE: [u8; ICMP_ECHO_RX_PAYLOAD_CAPACITY] =
    [0u8; ICMP_ECHO_RX_PAYLOAD_CAPACITY];
static mut ICMP_ECHO_TX_STORAGE: [u8; ICMP_ECHO_TX_PAYLOAD_CAPACITY] =
    [0u8; ICMP_ECHO_TX_PAYLOAD_CAPACITY];
static mut UDP_BEACON_RX_STORAGE: [u8; UDP_PAYLOAD_CAPACITY] = [0u8; UDP_PAYLOAD_CAPACITY];
static mut UDP_BEACON_TX_STORAGE: [u8; UDP_PAYLOAD_CAPACITY] = [0u8; UDP_PAYLOAD_CAPACITY];
static mut UDP_ECHO_RX_STORAGE: [u8; UDP_PAYLOAD_CAPACITY] = [0u8; UDP_PAYLOAD_CAPACITY];
static mut UDP_ECHO_TX_STORAGE: [u8; UDP_PAYLOAD_CAPACITY] = [0u8; UDP_PAYLOAD_CAPACITY];
static mut DHCP_RX_STORAGE: [u8; DHCP_PAYLOAD_CAPACITY] = [0u8; DHCP_PAYLOAD_CAPACITY];
static mut DHCP_TX_STORAGE: [u8; DHCP_PAYLOAD_CAPACITY] = [0u8; DHCP_PAYLOAD_CAPACITY];

fn new_console_tcp_socket(
    rx_storage: &'static mut [u8],
    tx_storage: &'static mut [u8],
) -> TcpSocket<'static> {
    let mut socket = TcpSocket::new(
        TcpSocketBuffer::new(rx_storage),
        TcpSocketBuffer::new(tx_storage),
    );
    // Console traffic is interactive and CYW43 preserves an RX-coupled TX
    // permit only for the current stack poll. Emit ACKs in that same poll
    // instead of moving a header-only response onto the later generic lane.
    socket.set_ack_delay(None);
    socket
}

/// Shared monotonic clock for the interface.
#[derive(Debug, Default)]
pub struct NetworkClock;

impl NetworkClock {
    /// Creates a monotonic clock initialised to zero milliseconds.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Advances the clock by `delta_ms` and returns the resulting [`Instant`].
    pub fn advance(&self, delta_ms: u32) -> Instant {
        let _ = delta_ms;
        self.now()
    }

    /// Reads the current [`Instant`] without modifying the clock value.
    #[must_use]
    pub fn now(&self) -> Instant {
        let millis = i64::try_from(crate::hal::timebase().now_ms()).unwrap_or(i64::MAX);
        Instant::from_millis(millis)
    }
}

/// Smoltcp-backed network stack that bridges the selected network device into the root task.
pub struct NetStack<D: NetDevice> {
    clock: NetworkClock,
    device: Box<D>,
    interface: Interface,
    sockets: SocketSet<'static>,
    _reservation: StorageReservation,
    init_attempt: NetInitAttempt,
    icmp_echo_handle: SocketHandle,
    icmp_echo_pending_since_ms: Option<u64>,
    icmp_echo_next_poll_ms: Option<u64>,
    icmp_echo_arp_probe_sent: bool,
    icmp_echo_reply_constructed_this_turn: bool,
    tcp_handle: SocketHandle,
    tcp_standby_handle: SocketHandle,
    standby_listener_armed: bool,
    standby_pending_since_ms: Option<u64>,
    server: TcpConsoleServer,
    outbound: OutboundCoalescer,
    telemetry: NetTelemetry,
    backend: NetBackend,
    mode: NetMode,
    interface_policy: NetInterfacePolicy,
    wifi_credentials: Option<WifiCredentials>,
    wifi_connection_generation: u32,
    #[cfg(feature = "kernel")]
    wifi_association_supervisor: crate::drivers::driver_task_net::Cyw43AssociationSupervisor,
    wifi_static_address_pending: bool,
    ip: Ipv4Address,
    gateway: Option<Ipv4Address>,
    prefix_len: u8,
    listen_port: u16,
    session_active: bool,
    disconnect_phase: ConsoleDisconnectPhase,
    disconnect_phase_started_ms: Option<u64>,
    disconnect_reason: NetConsoleDisconnectReason,
    disconnect_forced_aborts: u64,
    listener_announced: bool,
    listener_defer_reason: Option<&'static str>,
    active_client_id: Option<u64>,
    client_counter: u64,
    auth_state: AuthState,
    session_state: SessionState,
    conn_bytes_read: u64,
    conn_bytes_written: u64,
    events: HeaplessVec<NetConsoleEvent, SOCKET_CAPACITY>,
    service_logged: bool,
    last_poll_snapshot: Option<PollSnapshot>,
    peer_endpoint: Option<(IpAddress, u16)>,
    dhcp_handle: Option<SocketHandle>,
    dhcp: Option<DhcpClient>,
    dhcp_started: bool,
    dhcp_restart_after_ms: Option<u64>,
    wifi_dhcp_last_eapol_rx: u64,
    wifi_dhcp_eapol_quiet_since_ms: Option<u64>,
    wifi_dhcp_eapol_settle_logged: bool,
    wifi_rx_admission_blocked: bool,
    wifi_rx_admission_next_retry_ms: u64,
    udp_beacon_handle: Option<SocketHandle>,
    udp_echo_handle: Option<SocketHandle>,
    tcp_smoke_handle: Option<SocketHandle>,
    tcp_smoke_out_handle: Option<SocketHandle>,
    #[cfg(feature = "net-outbound-probe")]
    tcp_probe_handle: Option<SocketHandle>,
    counters: NetCounters,
    cyw43_generation_proof_baseline: Cyw43GenerationProofBaseline,
    self_test: SelfTestState,
    stage_policy: NetStagePolicy,
    tx_only_sent: bool,
    tcp_smoke_outbound_sent: bool,
    tcp_smoke_outbound_connecting: bool,
    tcp_smoke_last_attempt_ms: u64,
    #[cfg(feature = "net-outbound-probe")]
    probe_sent: bool,
    #[cfg(feature = "net-outbound-probe")]
    probe_last_attempt_ms: u64,
    #[cfg(feature = "net-outbound-probe")]
    probe_fail_count: u32,
    #[cfg(feature = "net-outbound-probe")]
    probe_last_log_ms: u64,
    #[cfg(feature = "net-outbound-probe")]
    probe_warned_once: bool,
    #[cfg(feature = "net-outbound-probe")]
    probe_hint_logged: bool,
    last_now_ms: Option<u64>,
    same_tick_poll_count: u16,
    time_stall_warned: bool,
    budgeted_phase: BudgetedNetPhase,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct PollSnapshot {
    session_active: bool,
    auth_state: AuthState,
    listener_ready: bool,
    tcp_state: TcpState,
    can_recv: bool,
    can_send: bool,
    staged_events: usize,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum BudgetedNetPhase {
    Interface,
    Dhcp,
    Tcp,
    InterfaceFlush,
    SelfTest,
}

impl BudgetedNetPhase {
    const fn next(self) -> Self {
        match self {
            Self::Interface => Self::Dhcp,
            Self::Dhcp => Self::Tcp,
            Self::Tcp => Self::InterfaceFlush,
            Self::InterfaceFlush => Self::SelfTest,
            Self::SelfTest => Self::Interface,
        }
    }
}

#[cfg(feature = "kernel")]
fn budgeted_genet_tcp_fast_path_due(
    contract: crate::hal::driver_task::DriverTaskContract,
    stage_policy: NetStagePolicy,
    listener_defer_reason: Option<&str>,
) -> bool {
    contract == crate::hal::driver_task::GENET_DRIVER_TASK_CONTRACT
        && stage_policy.allow_tcp
        && listener_defer_reason.is_none()
}

#[cfg(feature = "kernel")]
fn budgeted_cyw43_tcp_fast_path_due(
    contract: crate::hal::driver_task::DriverTaskContract,
    stage_policy: NetStagePolicy,
    listener_defer_reason: Option<&str>,
) -> bool {
    contract == crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT
        && stage_policy.allow_tcp
        && listener_defer_reason.is_none()
}

#[cfg(feature = "kernel")]
const fn budgeted_cyw43_tcp_phase_borrow_allowed(phase: BudgetedNetPhase) -> bool {
    // Self-test stays available for health work; pre-poll drain plus TCP plus self-test
    // does not fit in one CYW43 turn.
    !matches!(phase, BudgetedNetPhase::Tcp | BudgetedNetPhase::SelfTest)
}

#[cfg(feature = "kernel")]
fn budgeted_cyw43_dhcp_service_preempts_phase(
    contract: crate::hal::driver_task::DriverTaskContract,
    phase: BudgetedNetPhase,
    dhcp_service_required: bool,
) -> bool {
    contract == crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT
        && dhcp_service_required
        && phase != BudgetedNetPhase::Dhcp
}

#[cfg(feature = "kernel")]
fn budgeted_cyw43_selftest_defers_to_tcp(
    contract: crate::hal::driver_task::DriverTaskContract,
    stage_policy: NetStagePolicy,
    listener_defer_reason: Option<&str>,
    phase: BudgetedNetPhase,
    session_active: bool,
    tcp_state: TcpState,
    tcp_pending_work: bool,
) -> bool {
    budgeted_cyw43_tcp_fast_path_due(contract, stage_policy, listener_defer_reason)
        && phase == BudgetedNetPhase::SelfTest
        && session_active
        && tcp_pending_work
        && matches!(
            tcp_state,
            TcpState::Established
                | TcpState::CloseWait
                | TcpState::FinWait1
                | TcpState::FinWait2
                | TcpState::LastAck
                | TcpState::TimeWait
        )
}

#[cfg(feature = "kernel")]
const fn budgeted_cyw43_smoltcp_poll_after_tcp_borrow(phase: BudgetedNetPhase) -> bool {
    matches!(
        phase,
        BudgetedNetPhase::Interface | BudgetedNetPhase::Dhcp | BudgetedNetPhase::InterfaceFlush
    )
}

#[cfg(feature = "kernel")]
const fn budgeted_genet_smoltcp_poll_after_tcp_borrow(phase: BudgetedNetPhase) -> bool {
    matches!(
        phase,
        BudgetedNetPhase::Interface | BudgetedNetPhase::InterfaceFlush
    )
}

const fn udp_beacon_bind_endpoint(port: u16) -> IpListenEndpoint {
    IpListenEndpoint { addr: None, port }
}

#[cfg(feature = "kernel")]
fn budgeted_outer_pre_poll_allowed(
    hot_path: crate::hal::driver_task::DriverTaskHotPath,
    cyw43_inner_pre_poll_owner: bool,
) -> bool {
    hot_path != crate::hal::driver_task::DriverTaskHotPath::Cyw43Wifi || !cyw43_inner_pre_poll_owner
}

#[cfg(feature = "kernel")]
fn cyw43_flush_pre_poll_data_ready_for(
    contract: crate::hal::driver_task::DriverTaskContract,
    active_interface: &'static str,
    mode: NetMode,
    ip: Ipv4Address,
    bringup_status: Option<&'static str>,
    dhcp_phase: Option<DhcpPhase>,
    dhcp_socket_ready: bool,
) -> bool {
    contract == crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT
        && active_interface == "wifi"
        && bringup_status.is_none()
        && match mode {
            NetMode::Dhcp => {
                matches!(
                    dhcp_phase,
                    Some(DhcpPhase::Selecting | DhcpPhase::Requesting | DhcpPhase::Bound)
                ) || (dhcp_socket_ready
                    && ip == Ipv4Address::UNSPECIFIED
                    && matches!(dhcp_phase, Some(DhcpPhase::Disabled)))
            }
            NetMode::Static => ip != Ipv4Address::UNSPECIFIED,
            NetMode::Off => false,
        }
}

#[cfg(feature = "kernel")]
fn cyw43_runtime_service_pre_poll_ready_for(
    contract: crate::hal::driver_task::DriverTaskContract,
    active_interface: &'static str,
    mode: NetMode,
    ip: Ipv4Address,
    bringup_status: Option<&'static str>,
    dhcp_phase: Option<DhcpPhase>,
    dhcp_socket_ready: bool,
) -> bool {
    contract == crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT
        && active_interface == "wifi"
        && !matches!(mode, NetMode::Off)
        && (cyw43_runtime_service_live_bringup_status(bringup_status)
            || cyw43_flush_pre_poll_data_ready_for(
                contract,
                active_interface,
                mode,
                ip,
                bringup_status,
                dhcp_phase,
                dhcp_socket_ready,
            ))
}

#[cfg(feature = "kernel")]
fn cyw43_runtime_service_live_bringup_status(bringup_status: Option<&'static str>) -> bool {
    matches!(
        bringup_status,
        Some(
            "wifi-associating"
                | "wifi-host-eapol-pending"
                | "wifi-host-eapol-required"
                | "wifi-link-down"
                | "wifi-data-handoff-pending"
                | "wifi-data-rx-admission-blocked"
        )
    )
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct SelfTestState {
    enabled: bool,
    running: bool,
    run_generation: u64,
    started_ms: u64,
    last_beacon_ms: u64,
    beacon_seq: u32,
    beacons_sent: u32,
    burst_remaining: u32,
    start_tx_complete: u64,
    udp_echo_ok: bool,
    tcp_ok: bool,
    tcp_accept_seen: bool,
    tx_invariant_failed: bool,
    console_probe_started_ms: u64,
    console_probe_established: bool,
    console_probe_auth_sent: bool,
    console_probe_banner_seen: bool,
    console_probe_done: bool,
    console_ok: bool,
    last_result: Option<NetSelfTestResult>,
    post_poll_flush_logs: u32,
    udp_beacon_blocked_logs: u32,
    udp_beacon_error_logs: u32,
    udp_rx_packets: u32,
    udp_reply_packets: u32,
    udp_last_peer: Option<IpEndpoint>,
}

#[derive(Debug, Clone, Copy)]
struct NetStagePolicy {
    allow_tcp: bool,
    allow_selftest: bool,
    allow_outbound_probe: bool,
    allow_console_io: bool,
    tx_only: bool,
}

struct HostCommandTarget {
    primary: HeaplessString<48>,
    direct: HeaplessString<48>,
    forwarded_hint: bool,
    loopback: HeaplessString<48>,
}

#[cfg(feature = "cohesix-dev")]
fn tcp_state_label(state: TcpState) -> &'static str {
    match state {
        TcpState::Closed => "Closed",
        TcpState::Listen => "Listen",
        TcpState::SynSent => "SynSent",
        TcpState::SynReceived => "SynReceived",
        TcpState::Established => "Established",
        TcpState::FinWait1 => "FinWait1",
        TcpState::FinWait2 => "FinWait2",
        TcpState::CloseWait => "CloseWait",
        TcpState::Closing => "Closing",
        TcpState::LastAck => "LastAck",
        TcpState::TimeWait => "TimeWait",
    }
}

impl SelfTestState {
    fn new(enabled: bool) -> Self {
        Self {
            enabled,
            ..Self::default()
        }
    }

    fn reset(&mut self, now_ms: u64, start_tx_complete: u64) {
        self.run_generation = self.run_generation.wrapping_add(1);
        if self.run_generation == 0 {
            self.run_generation = 1;
        }
        self.running = true;
        self.started_ms = now_ms;
        self.last_beacon_ms = now_ms.saturating_sub(SELF_TEST_BEACON_INTERVAL_MS);
        self.beacon_seq = 0;
        self.beacons_sent = 0;
        self.burst_remaining = SELF_TEST_TX_WRAP_BURST;
        self.start_tx_complete = start_tx_complete;
        self.udp_echo_ok = false;
        self.tcp_ok = false;
        self.tcp_accept_seen = false;
        self.tx_invariant_failed = false;
        self.console_probe_started_ms = 0;
        self.console_probe_established = false;
        self.console_probe_auth_sent = false;
        self.console_probe_banner_seen = false;
        self.console_probe_done = false;
        self.console_ok = false;
        self.last_result = None;
        self.post_poll_flush_logs = 0;
        self.udp_beacon_blocked_logs = 0;
        self.udp_beacon_error_logs = 0;
        self.udp_rx_packets = 0;
        self.udp_reply_packets = 0;
        self.udp_last_peer = None;
    }

    fn start(&mut self, now_ms: u64, start_tx_complete: u64) -> bool {
        if !self.enabled {
            return false;
        }
        self.reset(now_ms, start_tx_complete);
        true
    }

    fn reset_for_connection_generation(&mut self) {
        let enabled = self.enabled;
        let run_generation = self.run_generation;
        *self = Self {
            enabled,
            run_generation,
            ..Self::default()
        };
    }

    fn current_result(&self, counters: NetCounters) -> NetSelfTestResult {
        let mut result = NetSelfTestResult {
            tx_ok: counters.tx_complete > self.start_tx_complete,
            udp_echo_ok: self.udp_echo_ok,
            tcp_ok: self.tcp_ok,
            console_ok: self.console_ok,
            peer_assisted_ok: false,
        };
        result.peer_assisted_ok = self_test_peer_assisted_ok(result, counters);
        result
    }

    fn record_udp_echo_rx(&mut self, endpoint: IpEndpoint) {
        self.udp_echo_ok = true;
        self.udp_rx_packets = self.udp_rx_packets.saturating_add(1);
        self.udp_last_peer = Some(endpoint);
    }

    fn record_udp_echo_reply(&mut self, endpoint: IpEndpoint) {
        self.udp_echo_ok = true;
        self.udp_reply_packets = self.udp_reply_packets.saturating_add(1);
        self.udp_last_peer = Some(endpoint);
    }

    fn record_tcp_ok(&mut self) {
        self.tcp_ok = true;
    }

    fn record_console_ok(&mut self) {
        self.console_ok = true;
    }

    fn conclude_if_needed(
        &mut self,
        now_ms: u64,
        counters: NetCounters,
    ) -> Option<NetSelfTestResult> {
        if !self.running {
            return None;
        }
        let deadline_reached = now_ms.saturating_sub(self.started_ms) >= SELF_TEST_WINDOW_MS;
        let result = self.current_result(counters);
        let peer_assisted_ready = result.peer_assisted_ok
            && self.beacons_sent >= 8
            && now_ms.saturating_sub(self.started_ms) >= SELF_TEST_PEER_ASSISTED_MIN_MS;
        if (result.tx_ok && result.udp_echo_ok && result.tcp_ok && result.console_ok)
            || peer_assisted_ready
            || deadline_reached
        {
            self.last_result = Some(result);
            self.running = false;
            return Some(result);
        }
        None
    }

    fn report(&self) -> NetSelfTestReport {
        NetSelfTestReport {
            enabled: self.enabled,
            running: self.running,
            run_generation: self.run_generation,
            last_result: self.last_result,
            backend: "unknown",
            udp_target: HeaplessString::new(),
            tcp_target: HeaplessString::new(),
        }
    }
}

fn render_host_selftest_target(
    host_forward: Option<&str>,
    port: u16,
    guest_ip: Ipv4Address,
) -> HeaplessString<48> {
    let mut target = HeaplessString::new();
    if let Some(host) = host_forward {
        if host.contains(':') {
            let _ = write!(target, "{host}");
        } else {
            let _ = write!(target, "{host}:{port}");
        }
        return target;
    }

    let _ = write!(target, "{}:{}", guest_ip, port);
    target
}

fn self_test_peer_assisted_ok(result: NetSelfTestResult, counters: NetCounters) -> bool {
    result.tx_ok
        && counters.rx_packets > 0
        && (result.tcp_ok
            || result.console_ok
            || counters.tcp_auth_sessions > 0
            || counters.wifi_host_eapol_secure > 0)
        && (!result.udp_echo_ok || !result.tcp_ok || !result.console_ok)
}

fn self_test_failure_hint(
    result: NetSelfTestResult,
    counters: NetCounters,
) -> Option<&'static str> {
    if result.peer_assisted_ok {
        Some(
            "[net-selftest] hint: peer-assisted echo/smoke checks incomplete, but local TX/RX and authenticated link proof are present",
        )
    } else if !result.tx_ok {
        Some("[net-selftest] hint: TX never completed after self-test start -> queue notify / cache / descriptors / MAC")
    } else if !result.udp_echo_ok {
        if counters.rx_packets == 0 {
            Some(
                    "[net-selftest] hint: RX never reaches the driver -> buffers not posted / used ring not read / IRQ missing",
                )
        } else if counters.udp_rx == 0
            && counters.tcp_accepts == 0
            && counters.tcp_smoke_outbound_failures > 0
        {
            Some(
                    "[net-selftest] hint: driver RX works and the peer is refusing TCP smoke while no UDP echo arrived -> run the logged host-side commands on the peer (31338/31339)",
                )
        } else if counters.udp_rx == 0 && counters.tcp_accepts == 0 {
            Some(
                    "[net-selftest] hint: driver RX works, but no peer UDP/TCP reached self-test sockets -> run the logged host-side commands on the peer and verify IP/ARP/route",
                )
        } else {
            Some(
                "[net-selftest] hint: UDP echo path missing while other RX exists -> verify echo port/path",
            )
        }
    } else if !result.tcp_ok {
        if counters.tcp_accepts > 0 && counters.tcp_rx_bytes == 0 {
            Some("[net-selftest] hint: TCP accepts but no bytes -> poll loop scheduling / RX path")
        } else {
            Some(
                "[net-selftest] hint: UDP path works, but outbound TCP smoke did not complete -> verify peer listener/routing",
            )
        }
    } else if !result.console_ok {
        Some("[net-selftest] hint: TCP console banner missing or listener not recycling")
    } else {
        None
    }
}

fn prefix_to_netmask(prefix_len: u8) -> Ipv4Address {
    let prefix = core::cmp::min(prefix_len, 32);
    let mask = if prefix == 0 {
        0
    } else {
        let shift = 32 - u32::from(prefix);
        u32::MAX.checked_shl(shift).unwrap_or(u32::MAX)
    };
    Ipv4Address::from_bits(mask)
}

#[derive(Debug, Clone, Copy)]
struct StorageAddressSnapshot {
    label: &'static str,
    flag: usize,
    owner: usize,
    tag: usize,
    storage: usize,
    storage_len: usize,
}

impl StorageAddressSnapshot {
    fn new<T>(
        label: &'static str,
        flag: &AtomicBool,
        owner: &AtomicU64,
        tag: &AtomicU32,
        storage: *const T,
        storage_len: usize,
    ) -> Self {
        Self {
            label,
            flag: flag as *const _ as usize,
            owner: owner as *const _ as usize,
            tag: tag as *const _ as usize,
            storage: storage as usize,
            storage_len,
        }
    }
}

fn log_storage_addresses_once(marker: &'static str) {
    if STORAGE_ADDRESS_LOGGED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    let storage_snapshots = [
        StorageAddressSnapshot::new(
            "socket",
            &SOCKET_STORAGE_IN_USE,
            &SOCKET_STORAGE_OWNER,
            &SOCKET_STORAGE_TAG_ID,
            unsafe { SOCKET_STORAGE.as_ptr() },
            SOCKET_CAPACITY * mem::size_of::<SocketStorage<'static>>(),
        ),
        StorageAddressSnapshot::new(
            "tcp-rx",
            &TCP_RX_STORAGE_IN_USE,
            &TCP_RX_STORAGE_OWNER,
            &TCP_RX_STORAGE_TAG_ID,
            unsafe { TCP_RX_STORAGE.as_ptr() },
            TCP_RX_BUFFER,
        ),
        StorageAddressSnapshot::new(
            "tcp-tx",
            &TCP_TX_STORAGE_IN_USE,
            &TCP_TX_STORAGE_OWNER,
            &TCP_TX_STORAGE_TAG_ID,
            unsafe { TCP_TX_STORAGE.as_ptr() },
            TCP_TX_BUFFER,
        ),
        StorageAddressSnapshot::new(
            "tcp-standby-rx",
            &TCP_STANDBY_RX_STORAGE_IN_USE,
            &TCP_STANDBY_RX_STORAGE_OWNER,
            &TCP_STANDBY_RX_STORAGE_TAG_ID,
            core::ptr::addr_of!(TCP_STANDBY_RX_STORAGE).cast::<u8>(),
            TCP_RX_BUFFER,
        ),
        StorageAddressSnapshot::new(
            "tcp-standby-tx",
            &TCP_STANDBY_TX_STORAGE_IN_USE,
            &TCP_STANDBY_TX_STORAGE_OWNER,
            &TCP_STANDBY_TX_STORAGE_TAG_ID,
            core::ptr::addr_of!(TCP_STANDBY_TX_STORAGE).cast::<u8>(),
            TCP_TX_BUFFER,
        ),
        StorageAddressSnapshot::new(
            "tcp-smoke-rx",
            &TCP_SMOKE_RX_STORAGE_IN_USE,
            &TCP_SMOKE_RX_STORAGE_OWNER,
            &TCP_SMOKE_RX_STORAGE_TAG_ID,
            unsafe { TCP_SMOKE_RX_STORAGE.as_ptr() },
            TCP_SMOKE_RX_BUFFER,
        ),
        StorageAddressSnapshot::new(
            "tcp-smoke-tx",
            &TCP_SMOKE_TX_STORAGE_IN_USE,
            &TCP_SMOKE_TX_STORAGE_OWNER,
            &TCP_SMOKE_TX_STORAGE_TAG_ID,
            unsafe { TCP_SMOKE_TX_STORAGE.as_ptr() },
            TCP_SMOKE_TX_BUFFER,
        ),
        StorageAddressSnapshot::new(
            "tcp-smoke-out-rx",
            &TCP_SMOKE_OUT_RX_STORAGE_IN_USE,
            &TCP_SMOKE_OUT_RX_STORAGE_OWNER,
            &TCP_SMOKE_OUT_RX_STORAGE_TAG_ID,
            unsafe { TCP_SMOKE_OUT_RX_STORAGE.as_ptr() },
            TCP_SMOKE_RX_BUFFER,
        ),
        StorageAddressSnapshot::new(
            "tcp-smoke-out-tx",
            &TCP_SMOKE_OUT_TX_STORAGE_IN_USE,
            &TCP_SMOKE_OUT_TX_STORAGE_OWNER,
            &TCP_SMOKE_OUT_TX_STORAGE_TAG_ID,
            unsafe { TCP_SMOKE_OUT_TX_STORAGE.as_ptr() },
            TCP_SMOKE_TX_BUFFER,
        ),
        StorageAddressSnapshot::new(
            "icmp-echo-rx",
            &ICMP_ECHO_STORAGE_IN_USE,
            &ICMP_ECHO_STORAGE_OWNER,
            &ICMP_ECHO_STORAGE_TAG_ID,
            unsafe { ICMP_ECHO_RX_STORAGE.as_ptr() },
            ICMP_ECHO_RX_PAYLOAD_CAPACITY,
        ),
        StorageAddressSnapshot::new(
            "icmp-echo-tx",
            &ICMP_ECHO_STORAGE_IN_USE,
            &ICMP_ECHO_STORAGE_OWNER,
            &ICMP_ECHO_STORAGE_TAG_ID,
            unsafe { ICMP_ECHO_TX_STORAGE.as_ptr() },
            ICMP_ECHO_TX_PAYLOAD_CAPACITY,
        ),
        StorageAddressSnapshot::new(
            "udp-beacon",
            &UDP_BEACON_STORAGE_IN_USE,
            &UDP_BEACON_STORAGE_OWNER,
            &UDP_BEACON_STORAGE_TAG_ID,
            unsafe { UDP_BEACON_RX_STORAGE.as_ptr() },
            UDP_PAYLOAD_CAPACITY,
        ),
        StorageAddressSnapshot::new(
            "udp-echo",
            &UDP_ECHO_STORAGE_IN_USE,
            &UDP_ECHO_STORAGE_OWNER,
            &UDP_ECHO_STORAGE_TAG_ID,
            unsafe { UDP_ECHO_RX_STORAGE.as_ptr() },
            UDP_PAYLOAD_CAPACITY,
        ),
    ];

    for snapshot in storage_snapshots {
        let paddr = crate::sel4::user_image_vaddr_to_paddr(snapshot.storage);
        let pend = paddr
            .map(|addr| addr.saturating_add(snapshot.storage_len))
            .unwrap_or(0);
        let paddr_val = paddr.unwrap_or(0);
        let paddr_known = paddr.is_some();
        info!(
            target: "net-storage",
            "[net-storage] addr marker={marker} label={} flag=0x{flag:016x} owner=0x{owner:016x} tag=0x{tag:016x} storage=0x{storage:016x} len=0x{len:08x} paddr=0x{paddr:016x}..0x{pend:016x} paddr_known={known}",
            snapshot.label,
            flag = snapshot.flag,
            owner = snapshot.owner,
            tag = snapshot.tag,
            storage = snapshot.storage,
            len = snapshot.storage_len,
            paddr = paddr_val,
            pend = pend,
            known = paddr_known,
        );
    }

    #[cfg(feature = "net-outbound-probe")]
    {
        let probe_snapshots = [
            StorageAddressSnapshot::new(
                "tcp-probe-rx",
                &TCP_PROBE_RX_STORAGE_IN_USE,
                &TCP_PROBE_RX_STORAGE_OWNER,
                &TCP_PROBE_RX_STORAGE_TAG_ID,
                unsafe { TCP_PROBE_RX_STORAGE.as_ptr() },
                TCP_PROBE_BUFFER,
            ),
            StorageAddressSnapshot::new(
                "tcp-probe-tx",
                &TCP_PROBE_TX_STORAGE_IN_USE,
                &TCP_PROBE_TX_STORAGE_OWNER,
                &TCP_PROBE_TX_STORAGE_TAG_ID,
                unsafe { TCP_PROBE_TX_STORAGE.as_ptr() },
                TCP_PROBE_BUFFER,
            ),
        ];

        for snapshot in probe_snapshots {
            let paddr = crate::sel4::user_image_vaddr_to_paddr(snapshot.storage);
            let pend = paddr
                .map(|addr| addr.saturating_add(snapshot.storage_len))
                .unwrap_or(0);
            let paddr_val = paddr.unwrap_or(0);
            let paddr_known = paddr.is_some();
            info!(
                target: "net-storage",
                "[net-storage] addr marker={marker} label={} flag=0x{flag:016x} owner=0x{owner:016x} tag=0x{tag:016x} storage=0x{storage:016x} len=0x{len:08x} paddr=0x{paddr:016x}..0x{pend:016x} paddr_known={known}",
                snapshot.label,
                flag = snapshot.flag,
                owner = snapshot.owner,
                tag = snapshot.tag,
                storage = snapshot.storage,
                len = snapshot.storage_len,
                paddr = paddr_val,
                pend = pend,
                known = paddr_known,
            );
        }
    }
}

fn log_net_watch_targets(marker: &'static str) {
    if NET_WATCH_LOGGED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    if let Some((ptr, len)) = BOOTINFO_WINDOW_GUARD.watched_region() {
        info!(
            target: "bootinfo.window",
            "[bootinfo.window] addr marker={marker} ptr=0x{ptr:016x} len=0x{len:08x}",
            ptr = ptr as usize,
            len = len,
        );
    } else {
        info!(
            target: "bootinfo.window",
            "[bootinfo.window] addr marker={marker} state=unavailable"
        );
    }
    log_storage_addresses_once(marker);
}

fn tag_label_snapshot(tag_label: &Mutex<Option<&'static str>>) -> &'static str {
    tag_label
        .try_lock()
        .and_then(|guard| *guard)
        .unwrap_or("(unknown)")
}

fn log_socket_tripwire(marker: &'static str) {
    let in_use = SOCKET_STORAGE_IN_USE.load(Ordering::Acquire);
    let owner = SOCKET_STORAGE_OWNER.load(Ordering::Acquire);
    let tag = SOCKET_STORAGE_TAG_ID.load(Ordering::Acquire);
    let tag_label = tag_label_snapshot(&SOCKET_STORAGE_TAG_LABEL);

    let addresses = StorageAddressSnapshot::new(
        "socket",
        &SOCKET_STORAGE_IN_USE,
        &SOCKET_STORAGE_OWNER,
        &SOCKET_STORAGE_TAG_ID,
        unsafe { SOCKET_STORAGE.as_ptr() },
        SOCKET_CAPACITY * mem::size_of::<SocketStorage<'static>>(),
    );

    info!(
        target: "net-storage",
        "[net-storage] preinit marker={marker} in_use={} owner=0x{owner:016x} tag=0x{tag:08x} tag_label={tag_label} flag_addr=0x{flag:016x} owner_addr=0x{owner_addr:016x} tag_addr=0x{tag_addr:016x} storage_addr=0x{storage:016x} len=0x{len:08x}",
        in_use,
        owner = owner,
        tag = tag,
        flag = addresses.flag,
        owner_addr = addresses.owner,
        tag_addr = addresses.tag,
        storage = addresses.storage,
        len = addresses.storage_len,
    );

    if in_use && owner == 0 {
        warn!(
            target: "net-storage",
            "[net-storage] POISONED BEFORE NET INIT marker={marker} in_use={} owner=0x{owner:016x} tag=0x{tag:08x} tag_label={tag_label}",
            in_use,
            owner = owner,
            tag = tag,
        );
    }

    log_storage_addresses_once(marker);
}

#[cfg(debug_assertions)]
fn debug_validate_socket_storage(marker: &'static str) {
    let metadata = StorageMetadata {
        flag: &SOCKET_STORAGE_IN_USE,
        owner: &SOCKET_STORAGE_OWNER,
        tag_id: &SOCKET_STORAGE_TAG_ID,
        tag_label: &SOCKET_STORAGE_TAG_LABEL,
        label: "socket",
    };
    let in_use = metadata.flag.load(Ordering::Acquire);
    if !in_use {
        return;
    }

    let owner = metadata.owner.load(Ordering::Acquire);
    let tag = metadata.tag_id.load(Ordering::Acquire);
    let tag_label = metadata
        .tag_label
        .try_lock()
        .and_then(|guard| *guard)
        .unwrap_or("(unknown)");
    if owner == 0 {
        warn!(
            "[net-storage] poisoned socket flag observed at {marker} in_use={in_use} owner=0x{owner:016x} tag=0x{tag:08x} tag_label={tag_label}",
        );
        debug_assert_ne!(owner, 0, "socket storage poisoned at {marker}");
    }
}

#[cfg(not(debug_assertions))]
fn debug_validate_socket_storage(_: &'static str) {}

fn check_bootinfo_wrap(mark: &'static str) -> Result<(), DefaultNetConsoleError> {
    let Some(state) = BootInfoState::get() else {
        return Ok(());
    };

    if let Err(err) = state.verify("net.init.wrap", mark) {
        error!("[bootinfo:net-wrap] mark={mark} err={err:?}");
        return Err(NetConsoleError::Init(NetStackError::BootInfoCanary(mark)));
    }
    info!("[bootinfo:net-wrap] mark={mark} status=ok");
    Ok(())
}

fn init_genet_driver_task_console<H>(
    hal: &mut H,
    config: ConsoleNetConfig,
    backend: NetBackend,
    mark: &'static str,
) -> Result<DefaultNetStack, DefaultNetConsoleError>
where
    H: Hardware<Error = HalError>,
{
    crate::drivers::driver_task_net::init_genet_runtime(hal, &config, NET_STAGE)
        .map_err(convert_driver_error::<DriverTaskNetError>)?;
    crate::hal::driver_task::register_driver_task_pointer_free_ring_service(
        crate::hal::driver_task::GENET_DRIVER_TASK_CONTRACT,
        crate::hal::driver_task::DriverTaskHotPath::GenetNic.as_u32() as usize,
        crate::drivers::driver_task_net::runtime_ring_service,
    );
    let stack = NetStack::<GenetDriverTaskDevice>::new(hal, config, backend)
        .map_err(convert_console_error::<DriverTaskNetError>)?;
    check_bootinfo_wrap(mark)?;
    Ok(DefaultNetStack::GenetDriverTask(stack))
}

fn init_cyw43_driver_task_console<H>(
    hal: &mut H,
    config: ConsoleNetConfig,
    backend: NetBackend,
    mark: &'static str,
    progress: &mut dyn crate::drivers::driver_task_net::Cyw43BootstrapProgress,
) -> Result<DefaultNetStack, DefaultNetConsoleError>
where
    H: Hardware<Error = HalError>,
{
    crate::drivers::driver_task_net::init_cyw43_runtime_with_progress(
        hal, &config, NET_STAGE, progress,
    )
    .map_err(convert_driver_error::<DriverTaskNetError>)?;
    crate::hal::driver_task::register_driver_task_pointer_free_ring_service(
        crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT,
        crate::hal::driver_task::DriverTaskHotPath::Cyw43Wifi.as_u32() as usize,
        crate::drivers::driver_task_net::runtime_ring_service,
    );
    let stack = NetStack::<Cyw43DriverTaskDevice>::new(hal, config, backend)
        .map_err(convert_console_error::<DriverTaskNetError>)?;
    check_bootinfo_wrap(mark)?;
    Ok(DefaultNetStack::Cyw43DriverTask(stack))
}

/// Construct the root-side smoltcp shell after the retained CYW43 supervisor
/// has published a ready linked-runtime generation.
///
/// This function deliberately does not invoke the legacy bootstrap entrypoint
/// or register/replay a child runtime. `Cyw43BootstrapSupervisor` already did
/// that work one operation per outer event turn; construction below is local
/// stack bookkeeping and the driver-task device shell ignores the HAL value.
#[cfg(feature = "kernel")]
pub fn finish_cyw43_net_console_after_bootstrap<H>(
    hal: &mut H,
    config: ConsoleNetConfig,
) -> Result<DefaultNetStack, DefaultNetConsoleError>
where
    H: Hardware<Error = HalError>,
{
    let config = prepare_cyw43_net_console_config(config)?;
    let backend = config.backend;
    let mark = if matches!(config.policy.interface, NetInterfacePolicy::Auto) {
        "net.init.wrap.after-retained.cyw43-driver-task-auto"
    } else {
        "net.init.wrap.after-retained.cyw43-driver-task"
    };
    let stack = NetStack::<Cyw43DriverTaskDevice>::new(hal, config, backend)
        .map_err(convert_console_error::<DriverTaskNetError>)?;
    check_bootinfo_wrap(mark)?;
    Ok(DefaultNetStack::Cyw43DriverTask(stack))
}

/// Map a retained supervisor failure into the public network-console error
/// grammar without re-entering runtime bootstrap.
#[cfg(feature = "kernel")]
pub fn map_cyw43_bootstrap_error(error: DriverTaskNetError) -> DefaultNetConsoleError {
    convert_driver_error(error)
}

fn active_driver_label_for(profile_backend: NetBackend, active_interface: &str) -> &'static str {
    match (profile_backend, active_interface) {
        (NetBackend::BcmGenet, "wifi") => "cyw43",
        (NetBackend::BcmGenet, _) => "bcmgenet-v5",
        (NetBackend::Rtl8139, _) => "rtl8139",
        #[cfg(feature = "net-backend-virtio")]
        (NetBackend::Virtio, _) => "virtio-net",
    }
}

fn configured_active_driver_label(config: &ConsoleNetConfig) -> &'static str {
    let active_interface = match config.policy.interface {
        NetInterfacePolicy::Wifi => "wifi",
        NetInterfacePolicy::Auto if config.wifi_credentials.is_some() => "wifi",
        NetInterfacePolicy::Auto | NetInterfacePolicy::Wired => "wired",
    };
    active_driver_label_for(config.backend, active_interface)
}

fn validate_net_console_config(
    config: ConsoleNetConfig,
) -> Result<ConsoleNetConfig, DefaultNetConsoleError> {
    let config = config.with_profile_defaults();
    let iface_ip = config.address.ip;
    if config.listen_port == 0
        || (matches!(config.policy.mode, NetMode::Static) && iface_ip == [0, 0, 0, 0])
    {
        log::error!(
            "[net-console] invalid configuration: backend={} mode={} interface={} listen_port={} iface_ip={:?}; disabling net-console",
            config.backend.label(),
            config.policy.mode.as_str(),
            config.policy.interface.as_str(),
            config.listen_port,
            config.address.ip
        );
        return Err(NetConsoleError::InvalidConfig(
            "listen_port/ip must be configured for static mode",
        ));
    }
    if !config
        .backend
        .supports_interface_policy(config.policy.interface)
    {
        log::error!(
            "[net-console] invalid configuration: backend={} interface={} is not supported by the current runtime",
            config.backend.label(),
            config.policy.interface.as_str(),
        );
        return Err(NetConsoleError::InvalidConfig(
            "selected interface policy is not available in the current runtime",
        ));
    }
    if let Err(reason) = super::validate_console_auth_token(config.auth_token) {
        log::error!(
            "[net-console] invalid configuration: token rejected (reason={reason}); disabling net-console"
        );
        return Err(NetConsoleError::InvalidConfig(reason));
    }
    Ok(config)
}

/// Validate and canonicalise a deferred physical-CYW43 console configuration
/// before any supervisor turn is allowed to touch the linked runtimes.
#[cfg(feature = "kernel")]
pub fn prepare_cyw43_net_console_config(
    config: ConsoleNetConfig,
) -> Result<ConsoleNetConfig, DefaultNetConsoleError> {
    let config = validate_net_console_config(config)?;
    let selects_wifi = matches!(config.backend, NetBackend::BcmGenet)
        && (matches!(config.policy.interface, NetInterfacePolicy::Wifi)
            || (matches!(config.policy.interface, NetInterfacePolicy::Auto)
                && config.wifi_credentials.is_some()));
    if !selects_wifi {
        return Err(NetConsoleError::InvalidConfig(
            "deferred CYW43 supervisor requires a Wi-Fi interface policy",
        ));
    }
    if !crate::hal::pi4_wifi::wifi_sdio_pinctrl_ready_for_bootstrap() {
        return Err(NetConsoleError::InvalidConfig(
            "Pi 4 Wi-Fi SDIO pinctrl readback is not ready",
        ));
    }
    Ok(config)
}

/// Initialise the network console stack, translating low-level errors into
/// user-facing diagnostics.
pub fn init_net_console<H>(
    hal: &mut H,
    config: ConsoleNetConfig,
) -> Result<DefaultNetStack, DefaultNetConsoleError>
where
    H: Hardware<Error = HalError>,
{
    let mut progress = |_: &'static str| {};
    init_net_console_with_cyw43_progress(hal, config, &mut progress)
}

fn init_net_console_with_cyw43_progress<H>(
    hal: &mut H,
    config: ConsoleNetConfig,
    progress: &mut dyn crate::drivers::driver_task_net::Cyw43BootstrapProgress,
) -> Result<DefaultNetStack, DefaultNetConsoleError>
where
    H: Hardware<Error = HalError>,
{
    log_socket_tripwire(concat!(file!(), ":", line!()));

    let config = validate_net_console_config(config)?;
    let iface_ip = config.address.ip;

    debug_validate_socket_storage(concat!(file!(), ":", line!()));

    let gateway_label = config
        .address
        .gateway
        .map(|gateway| Ipv4Address::new(gateway[0], gateway[1], gateway[2], gateway[3]))
        .unwrap_or(Ipv4Address::UNSPECIFIED);
    let iface_ip = Ipv4Address::new(iface_ip[0], iface_ip[1], iface_ip[2], iface_ip[3]);
    log::info!(
        "[net-console] config: profile_backend={} active_driver={} mode={} interface={} static_default_ip={}/{} static_default_gateway={} listen_port={} udp_echo_port={} tcp_smoke_port={} dhcp(discover_ms={} request_ms={} retries={})",
        config.backend.label(),
        configured_active_driver_label(&config),
        config.policy.mode.as_str(),
        config.policy.interface.as_str(),
        iface_ip,
        config.address.prefix_len,
        gateway_label,
        config.listen_port,
        UDP_ECHO_PORT,
        TCP_SMOKE_PORT,
        config.policy.dhcp.discover_timeout_ms,
        config.policy.dhcp.request_timeout_ms,
        config.policy.dhcp.max_retries
    );
    info!(
        "[net-console] layout sizes: stack.rtl8139={} stack.genet_dt={} stack.cyw43_dt={} dev.genet_dt={} dev.cyw43_dt={} enum.default={}",
        mem::size_of::<NetStack<Rtl8139Device>>(),
        mem::size_of::<NetStack<GenetDriverTaskDevice>>(),
        mem::size_of::<NetStack<Cyw43DriverTaskDevice>>(),
        mem::size_of::<GenetDriverTaskDevice>(),
        mem::size_of::<Cyw43DriverTaskDevice>(),
        mem::size_of::<DefaultNetStack>(),
    );
    #[cfg(feature = "net-backend-virtio")]
    info!(
        "[net-console] layout sizes: stack.virtio={} dev.virtio={}",
        mem::size_of::<NetStack<VirtioNetStatic>>(),
        mem::size_of::<VirtioNetStatic>(),
    );

    let backend = config.backend;
    match backend {
        NetBackend::Rtl8139 => {
            let stack = NetStack::<Rtl8139Device>::new(hal, config, backend)
                .map_err(convert_console_error::<Rtl8139DriverError>)?;
            check_bootinfo_wrap("net.init.wrap.after-new.rtl8139")?;
            Ok(DefaultNetStack::Rtl8139(stack))
        }
        NetBackend::BcmGenet => match config.policy.interface {
            NetInterfacePolicy::Wired => init_genet_driver_task_console(
                hal,
                config,
                backend,
                "net.init.wrap.after-new.genet-driver-task",
            ),
            NetInterfacePolicy::Wifi => init_cyw43_driver_task_console(
                hal,
                config,
                backend,
                "net.init.wrap.after-new.cyw43-driver-task",
                progress,
            ),
            NetInterfacePolicy::Auto => {
                if config.wifi_credentials.is_some() {
                    init_cyw43_driver_task_console(
                        hal,
                        config,
                        backend,
                        "net.init.wrap.after-new.cyw43-driver-task-auto",
                        progress,
                    )
                } else {
                    info!(
                        "[net-console] auto policy missing Wi-Fi credentials; selecting wired driver-task backend"
                    );
                    init_genet_driver_task_console(
                        hal,
                        config,
                        backend,
                        "net.init.wrap.after-new.genet-driver-task-auto",
                    )
                }
            }
        },
        #[cfg(feature = "net-backend-virtio")]
        NetBackend::Virtio => {
            let stack = NetStack::<VirtioNetStatic>::new(hal, config, backend)
                .map_err(convert_console_error::<VirtioDriverError>)?;
            check_bootinfo_wrap("net.init.wrap.after-new.virtio")?;
            Ok(DefaultNetStack::Virtio(stack))
        }
    }
}

/// Return whether a physical-Pi CYW43 bootstrap failure is transient.
///
/// Transient pre-root failures may defer into the sole post-prompt production
/// boot episode. The classification does not authorize an automatic second
/// episode. Missing devices, invalid policy, corrupted root storage/bootinfo,
/// and immutable build-input defects remain permanent.
#[cfg(feature = "kernel")]
#[must_use]
pub fn cyw43_net_console_bootstrap_error_is_transient(error: &DefaultNetConsoleError) -> bool {
    matches!(
        error,
        NetConsoleError::Init(NetStackError::Driver(DefaultDriverError::DriverTaskNet(
            driver_error
        ))) if driver_error.cyw43_bootstrap_failure_is_transient()
    )
}

fn convert_console_error<E>(err: NetStackError<E>) -> DefaultNetConsoleError
where
    E: NetDriverError + Into<DefaultDriverError>,
{
    match err {
        NetStackError::Driver(driver_err) => {
            let driver_err = driver_err.into();
            if driver_err.is_absent() {
                NetConsoleError::NoDevice
            } else {
                NetConsoleError::Init(NetStackError::Driver(driver_err))
            }
        }
        NetStackError::AlreadyInitialisingOrOnline => {
            NetConsoleError::Init(NetStackError::AlreadyInitialisingOrOnline)
        }
        NetStackError::BootInfoCanary(mark) => {
            NetConsoleError::Init(NetStackError::BootInfoCanary(mark))
        }
        NetStackError::SocketStorageInUse => {
            NetConsoleError::Init(NetStackError::SocketStorageInUse)
        }
        NetStackError::SocketStoragePoisoned => {
            NetConsoleError::Init(NetStackError::SocketStoragePoisoned)
        }
        NetStackError::TcpRxStorageInUse => NetConsoleError::Init(NetStackError::TcpRxStorageInUse),
        NetStackError::TcpTxStorageInUse => NetConsoleError::Init(NetStackError::TcpTxStorageInUse),
        NetStackError::TcpStandbyRxStorageInUse => {
            NetConsoleError::Init(NetStackError::TcpStandbyRxStorageInUse)
        }
        NetStackError::TcpStandbyTxStorageInUse => {
            NetConsoleError::Init(NetStackError::TcpStandbyTxStorageInUse)
        }
        NetStackError::TcpSmokeRxStorageInUse => {
            NetConsoleError::Init(NetStackError::TcpSmokeRxStorageInUse)
        }
        NetStackError::TcpSmokeTxStorageInUse => {
            NetConsoleError::Init(NetStackError::TcpSmokeTxStorageInUse)
        }
        NetStackError::IcmpEchoStorageInUse => {
            NetConsoleError::Init(NetStackError::IcmpEchoStorageInUse)
        }
        NetStackError::UdpBeaconStorageInUse => {
            NetConsoleError::Init(NetStackError::UdpBeaconStorageInUse)
        }
        NetStackError::UdpEchoStorageInUse => {
            NetConsoleError::Init(NetStackError::UdpEchoStorageInUse)
        }
        NetStackError::DhcpStorageInUse => NetConsoleError::Init(NetStackError::DhcpStorageInUse),
        NetStackError::DhcpSocketBind(err) => {
            NetConsoleError::Init(NetStackError::DhcpSocketBind(err))
        }
        NetStackError::TcpProbeRxStorageInUse => {
            NetConsoleError::Init(NetStackError::TcpProbeRxStorageInUse)
        }
        NetStackError::TcpProbeTxStorageInUse => {
            NetConsoleError::Init(NetStackError::TcpProbeTxStorageInUse)
        }
        NetStackError::DriverTaskContract(reason) => {
            NetConsoleError::Init(NetStackError::DriverTaskContract(reason))
        }
    }
}

fn convert_driver_error<E>(err: E) -> DefaultNetConsoleError
where
    E: NetDriverError + Into<DefaultDriverError>,
{
    if err.is_absent() {
        NetConsoleError::NoDevice
    } else {
        NetConsoleError::Init(NetStackError::Driver(err.into()))
    }
}

impl<D: NetDevice> NetStack<D> {
    #[inline]
    fn console_socket_capacity_ok(rx_capacity: usize, tx_capacity: usize) -> bool {
        rx_capacity == TCP_RX_BUFFER && tx_capacity == TCP_TX_BUFFER
    }

    fn active_driver_label(&self) -> &'static str {
        active_driver_label_for(self.backend, self.device.interface_label())
    }

    fn console_listener_defer_reason(&self) -> Option<&'static str> {
        if self.wifi_rx_admission_blocked {
            return Some("wifi-data-rx-admission-blocked");
        }
        console_listener_defer_reason_for(self.mode, self.ip, self.device.bringup_status_label())
    }

    fn rebuild_console_sockets(
        &mut self,
        now_ms: u64,
        active: (usize, usize, usize, usize),
        standby: (usize, usize, usize, usize),
    ) -> bool {
        error!(
            "[net-console] console socket buffer corruption detected active(rx_capacity={}, tx_capacity={}, rx_queue={}, tx_queue={}) standby(rx_capacity={}, tx_capacity={}, rx_queue={}, tx_queue={}); rebuilding both sockets",
            active.0,
            active.1,
            active.2,
            active.3,
            standby.0,
            standby.1,
            standby.2,
            standby.3,
        );
        self.outbound.reset();
        self.server.end_session();
        self.session_active = false;
        self.active_client_id = None;
        self.peer_endpoint = None;
        self.listener_announced = false;
        self.standby_listener_armed = false;
        self.standby_pending_since_ms = None;
        self.reset_session_state_with(None);
        let defer_reason = self.console_listener_defer_reason();
        let _ = self.sockets.remove(self.tcp_handle);
        let _ = self.sockets.remove(self.tcp_standby_handle);
        // SAFETY: This NetStack holds every console storage lease and both old
        // sockets were removed above, so neither static buffer pair has a live
        // TcpSocket alias while the replacement pair is constructed.
        let (mut tcp_socket, standby_socket) = unsafe {
            (
                new_console_tcp_socket(&mut TCP_RX_STORAGE[..], &mut TCP_TX_STORAGE[..]),
                new_console_tcp_socket(
                    &mut TCP_STANDBY_RX_STORAGE[..],
                    &mut TCP_STANDBY_TX_STORAGE[..],
                ),
            )
        };
        match defer_reason {
            Some(reason) => {
                info!(
                    "[net-console] console socket rebuilt; listener deferred reason={} iface_ip={} now_ms={}",
                    reason, self.ip, now_ms
                );
            }
            None => {
                if let Err(err) = tcp_socket.listen(IpListenEndpoint::from(self.listen_port)) {
                    warn!(
                        "[net-console] console socket relisten failed port={} err={err:?}",
                        self.listen_port
                    );
                } else {
                    NET_DIAG.record_listener_bound();
                    self.listener_announced = true;
                    self.listener_defer_reason = None;
                    info!(
                        "[net-console] console socket rebuilt at now_ms={} port={}",
                        now_ms, self.listen_port
                    );
                }
            }
        }
        self.tcp_handle = self.sockets.add(tcp_socket);
        self.tcp_standby_handle = self.sockets.add(standby_socket);
        true
    }

    fn validate_console_socket(&mut self, now_ms: u64) -> bool {
        let active = {
            let socket = self.sockets.get::<TcpSocket>(self.tcp_handle);
            (
                socket.recv_capacity(),
                socket.send_capacity(),
                socket.recv_queue(),
                socket.send_queue(),
            )
        };
        let standby = {
            let socket = self.sockets.get::<TcpSocket>(self.tcp_standby_handle);
            (
                socket.recv_capacity(),
                socket.send_capacity(),
                socket.recv_queue(),
                socket.send_queue(),
            )
        };
        let active_ok = Self::console_socket_capacity_ok(active.0, active.1)
            && active.2 <= active.0
            && active.3 <= active.1;
        let standby_ok = Self::console_socket_capacity_ok(standby.0, standby.1)
            && standby.2 <= standby.0
            && standby.3 <= standby.1;
        if active_ok && standby_ok {
            return true;
        }
        self.rebuild_console_sockets(now_ms, active, standby);
        false
    }

    fn abort_console_standby(&mut self, reason: &'static str) -> bool {
        let (state, pending_bytes) = {
            let socket = self.sockets.get::<TcpSocket>(self.tcp_standby_handle);
            (socket.state(), socket.recv_queue())
        };
        let was_active = self.standby_listener_armed
            || self.standby_pending_since_ms.is_some()
            || state != TcpState::Closed;
        if state != TcpState::Closed {
            self.sockets
                .get_mut::<TcpSocket>(self.tcp_standby_handle)
                .abort();
        }
        self.standby_listener_armed = false;
        self.standby_pending_since_ms = None;
        if was_active {
            debug!(
                "[net-console] standby handoff cleared reason={} state={:?} pending_bytes={}",
                reason, state, pending_bytes
            );
        }
        was_active
    }

    fn abort_console_socket_pair(&mut self, reason: &'static str) {
        #[cfg(feature = "kernel")]
        Self::clear_cyw43_authenticated_console_peer();
        let active_state = self.sockets.get::<TcpSocket>(self.tcp_handle).state();
        if active_state != TcpState::Closed {
            self.sockets.get_mut::<TcpSocket>(self.tcp_handle).abort();
        }
        let _ = self.abort_console_standby(reason);
    }

    fn arm_console_standby_listener(&mut self, now_ms: u64) -> bool {
        let state = self
            .sockets
            .get::<TcpSocket>(self.tcp_standby_handle)
            .state();
        if state != TcpState::Closed && state != TcpState::TimeWait {
            self.sockets
                .get_mut::<TcpSocket>(self.tcp_standby_handle)
                .abort();
        }
        let socket = self.sockets.get_mut::<TcpSocket>(self.tcp_standby_handle);
        if let Err(err) = socket.listen(IpListenEndpoint::from(self.listen_port)) {
            self.standby_listener_armed = false;
            self.standby_pending_since_ms = None;
            warn!(
                "[net-console] standby handoff listen failed port={} state={:?} err={err:?}",
                self.listen_port,
                socket.state()
            );
            return false;
        }
        self.standby_listener_armed = true;
        self.standby_pending_since_ms = None;
        self.listener_announced = true;
        NET_DIAG.record_listener_bound();
        debug!(
            "[net-console] standby handoff armed port={} active_conn={} phase={:?} now_ms={}",
            self.listen_port,
            self.active_client_id.unwrap_or(0),
            self.disconnect_phase,
            now_ms
        );
        true
    }

    fn service_console_standby(&mut self, now_ms: u64) -> bool {
        if !console_standby_should_arm(self.disconnect_phase) {
            return false;
        }

        if !self.standby_listener_armed
            || !self
                .sockets
                .get::<TcpSocket>(self.tcp_standby_handle)
                .is_open()
        {
            return self.arm_console_standby_listener(now_ms);
        }

        let state = self
            .sockets
            .get::<TcpSocket>(self.tcp_standby_handle)
            .state();
        if console_standby_pending_state(state) {
            if self.standby_pending_since_ms.is_none() {
                self.standby_pending_since_ms = Some(now_ms);
                let pending_bytes = self
                    .sockets
                    .get::<TcpSocket>(self.tcp_standby_handle)
                    .recv_queue();
                debug!(
                    "[net-console] standby handoff pending state={:?} pending_bytes={} deadline_ms={} now_ms={}",
                    state,
                    pending_bytes,
                    now_ms.saturating_add(CONSOLE_HANDOFF_PENDING_DEADLINE_MS),
                    now_ms
                );
                return true;
            }
            if console_standby_pending_expired(self.standby_pending_since_ms, now_ms) {
                warn!(
                    "[net-console] standby handoff expired state={:?} pending_bytes={} now_ms={}",
                    state,
                    self.sockets
                        .get::<TcpSocket>(self.tcp_standby_handle)
                        .recv_queue(),
                    now_ms
                );
                let cleared = self.abort_console_standby("pending-deadline");
                return self.arm_console_standby_listener(now_ms) || cleared;
            }
            return false;
        }

        if state == TcpState::Listen {
            self.standby_pending_since_ms = None;
            return false;
        }

        debug!(
            "[net-console] standby handoff rejected state={:?} reason=non-promotable",
            state
        );
        let cleared = self.abort_console_standby("non-promotable");
        self.arm_console_standby_listener(now_ms) || cleared
    }

    fn console_authority_cleared_for_handoff(&self) -> bool {
        console_handoff_authority_cleared(
            self.disconnect_phase,
            self.session_active,
            self.active_client_id.is_some(),
            self.server.is_authenticated(),
            self.auth_state,
            self.peer_endpoint.is_some(),
            self.server.ingest_snapshot().queued,
            self.outbound.has_pending(),
        )
    }

    fn promote_console_standby(&mut self, now_ms: u64) -> bool {
        if !self.standby_listener_armed {
            return false;
        }
        let active_state = self.sockets.get::<TcpSocket>(self.tcp_handle).state();
        if !console_active_terminal_state(active_state) {
            return false;
        }
        let standby_state = self
            .sockets
            .get::<TcpSocket>(self.tcp_standby_handle)
            .state();
        if console_standby_pending_expired(self.standby_pending_since_ms, now_ms) {
            warn!(
                "[net-console] standby handoff promotion rejected reason=pending-deadline state={:?}",
                standby_state
            );
            return self.abort_console_standby("promotion-deadline");
        }
        if !console_standby_promotable_state(standby_state) {
            warn!(
                "[net-console] standby handoff promotion rejected reason=state state={:?}",
                standby_state
            );
            return self.abort_console_standby("promotion-state");
        }
        if !self.console_authority_cleared_for_handoff() {
            error!(
                "[net-console] standby handoff promotion rejected reason=authority-not-cleared state={:?}",
                standby_state
            );
            return self.abort_console_standby("authority-not-cleared");
        }

        let pending_bytes = self
            .sockets
            .get::<TcpSocket>(self.tcp_standby_handle)
            .recv_queue();
        mem::swap(&mut self.tcp_handle, &mut self.tcp_standby_handle);
        self.standby_listener_armed = false;
        self.standby_pending_since_ms = None;
        self.listener_announced = true;
        self.listener_defer_reason = None;
        self.last_poll_snapshot = None;
        info!(
            "[net-console] standby handoff promoted state={:?} pending_bytes={} now_ms={}",
            standby_state, pending_bytes, now_ms
        );
        true
    }

    #[cfg(feature = "kernel")]
    fn clear_cyw43_authenticated_console_peer() {
        if D::driver_task_contract() == CYW43_WIFI_DRIVER_TASK_CONTRACT {
            crate::drivers::driver_task_net::clear_cyw43_authenticated_console_peer();
        }
    }

    #[cfg(feature = "kernel")]
    fn publish_cyw43_authenticated_console_peer(
        authenticated: bool,
        generation: u32,
        active_client_id: Option<u64>,
        listen_port: u16,
        peer_endpoint: Option<(IpAddress, u16)>,
        socket: &TcpSocket,
    ) -> bool {
        if D::driver_task_contract() != CYW43_WIFI_DRIVER_TASK_CONTRACT {
            return true;
        }
        let Some(connection_id) = active_client_id.filter(|_| authenticated) else {
            Self::clear_cyw43_authenticated_console_peer();
            return false;
        };
        let local_port = socket
            .local_endpoint()
            .map(|endpoint| endpoint.port)
            .unwrap_or(listen_port);
        let Some((IpAddress::Ipv4(remote_ipv4), remote_port)) = peer_endpoint.or_else(|| {
            socket
                .remote_endpoint()
                .map(|endpoint| (endpoint.addr, endpoint.port))
        }) else {
            Self::clear_cyw43_authenticated_console_peer();
            return false;
        };
        crate::drivers::driver_task_net::publish_cyw43_authenticated_console_peer(
            generation,
            connection_id,
            local_port,
            remote_ipv4.octets(),
            remote_port,
        )
    }

    fn set_auth_state(auth_state: &mut AuthState, active_client_id: Option<u64>, next: AuthState) {
        if next != *auth_state {
            // Authentication state is the authority boundary for standalone
            // CYW43 response leases. Revoke the prior identity first; the
            // exact replacement is published only by `Authenticated` below.
            #[cfg(feature = "kernel")]
            Self::clear_cyw43_authenticated_console_peer();
            let conn_id = active_client_id.unwrap_or(0);
            info!(
                "[cohsh-net][auth] state: {:?} -> {:?} (conn_id={})",
                auth_state, next, conn_id
            );
            trace!(
                "[net-auth][conn={}] {:?} -> {:?}",
                conn_id,
                auth_state,
                next
            );
            *auth_state = next;
        }
    }

    fn reset_session_state(&mut self) {
        #[cfg(feature = "kernel")]
        Self::clear_cyw43_authenticated_console_peer();
        self.auth_state = AuthState::Start;
        let preconnect_logged = self.session_state.flush_blocked_logged_preconnect;
        self.session_state = SessionState::default();
        self.session_state.flush_blocked_logged_preconnect = preconnect_logged;
        self.conn_bytes_read = 0;
        self.conn_bytes_written = 0;
        self.disconnect_phase = ConsoleDisconnectPhase::Idle;
        self.disconnect_phase_started_ms = None;
        self.disconnect_reason = NetConsoleDisconnectReason::Quit;
    }

    fn reset_session_state_with(&mut self, tcp_state: Option<TcpState>) {
        self.reset_session_state();
        if let Some(state) = tcp_state {
            self.session_state.last_state = Some(state);
        }
    }

    fn guarded_connect(
        socket: &mut TcpSocket,
        cx: &mut smoltcp::iface::Context,
        dest: IpEndpoint,
        local_endpoint: IpListenEndpoint,
        role: &'static str,
    ) -> Result<(), TcpConnectError> {
        let state = socket.state();
        if state != TcpState::Closed {
            warn!(
                target: "root_task::net",
                "[tcp] connect.guard blocked role={} state={:?}",
                role,
                state
            );
            return Err(TcpConnectError::InvalidState);
        }
        debug_assert_eq!(state, TcpState::Closed);
        socket.connect(cx, dest, local_endpoint)
    }

    fn log_buffer_addresses_once(&mut self, marker: &'static str) {
        self.outbound.log_buffer_addresses_once(marker);
        self.server.log_buffer_addresses_once(marker);
    }

    fn watched_bootinfo_range() -> Option<Range<usize>> {
        if let Some(state) = BootInfoState::get() {
            return Some(state.snapshot_region());
        }
        BOOTINFO_WINDOW_GUARD
            .watched_region()
            .map(|(ptr, len)| ptr as usize..ptr as usize + len)
    }

    fn assert_range_disjoint(range: &Range<usize>, boot_range: &Range<usize>, label: &'static str) {
        if range.start < boot_range.end && boot_range.start < range.end {
            error!(
                target: "bootinfo.window",
                "[bootinfo.window] overlap label={label} range=[0x{start:016x}..0x{end:016x}) bootinfo=[0x{boot_start:016x}..0x{boot_end:016x})",
                start = range.start,
                end = range.end,
                boot_start = boot_range.start,
                boot_end = boot_range.end,
            );
            panic!(
                "bootinfo.window overlap: {label} range=[0x{start:016x}..0x{end:016x}) bootinfo=[0x{boot_start:016x}..0x{boot_end:016x})",
                start = range.start,
                end = range.end,
                boot_start = boot_range.start,
                boot_end = boot_range.end,
            );
        }
    }

    fn assert_bootinfo_overlaps(&self) {
        let Some(boot_range) = Self::watched_bootinfo_range() else {
            return;
        };
        let console_range = self.server.line_buffer_range();
        Self::assert_range_disjoint(&console_range, &boot_range, "net.console.line_buffer");
        if let Some(queue_range) = self.device.buffer_bounds() {
            Self::assert_range_disjoint(&queue_range, &boot_range, "net.device.queue");
        }
    }

    fn log_init_canary(&self, mark: &'static str) -> Result<(), NetStackError<D::Error>> {
        log_bootinfo_mark(mark, &self.init_attempt)
    }

    fn record_peer_endpoint(
        peer_endpoint: &mut Option<(IpAddress, u16)>,
        endpoint: Option<IpEndpoint>,
    ) -> bool {
        let updated = endpoint.map(|endpoint| (endpoint.addr, endpoint.port));
        let changed = *peer_endpoint != updated;
        *peer_endpoint = updated;
        changed
    }

    fn host_forward_override(&self) -> Option<&'static str> {
        option_env!("COHESIX_NET_HOSTFWD")
    }

    fn selftest_host_target(&self, port: u16) -> HostCommandTarget {
        let forward = self.host_forward_override();
        let direct = render_host_selftest_target(None, port, self.ip);
        let loopback = render_host_selftest_target(Some("127.0.0.1"), port, self.ip);
        let primary = match forward {
            Some(host) => render_host_selftest_target(Some(host), port, self.ip),
            None if self.backend.uses_dev_virt_defaults() => loopback.clone(),
            None => direct.clone(),
        };
        let forwarded_hint = forward.is_some();

        HostCommandTarget {
            primary,
            direct,
            forwarded_hint,
            loopback,
        }
    }

    fn selftest_gateway_target(&self) -> Ipv4Address {
        self.gateway.unwrap_or_else(|| {
            if self.backend.uses_dev_virt_defaults() {
                Ipv4Address::from(DEV_VIRT_GATEWAY)
            } else {
                self.ip
            }
        })
    }

    fn selftest_console_loopback_enabled(&self) -> bool {
        self.backend.uses_dev_virt_defaults()
    }

    fn selftest_outbound_peer_probe_enabled(&self) -> bool {
        self.backend.uses_dev_virt_defaults() || self.host_forward_override().is_some()
    }

    fn peer_parts(
        peer_endpoint: Option<(IpAddress, u16)>,
        socket: &TcpSocket,
    ) -> (HeaplessString<64>, u16) {
        let (addr, port) = peer_endpoint
            .or_else(|| {
                socket
                    .remote_endpoint()
                    .map(|endpoint| (endpoint.addr, endpoint.port))
            })
            .unwrap_or((IpAddress::Ipv4(Ipv4Address::UNSPECIFIED), 0));
        let mut label = HeaplessString::<64>::new();
        let _ = write!(&mut label, "{addr}");
        (label, port)
    }

    fn trace_conn_new(
        peer_endpoint: Option<(IpAddress, u16)>,
        ip: IpAddress,
        conn_id: u64,
        socket: &TcpSocket,
        listen_port: u16,
    ) {
        let (peer, port) = Self::peer_parts(peer_endpoint, socket);
        let local_port = socket
            .local_endpoint()
            .map(|endpoint| endpoint.port)
            .unwrap_or(listen_port);
        log::info!(
            "[cohsh-net] conn new id={} local={}:{} remote={}:{}",
            conn_id,
            ip,
            local_port,
            peer,
            port,
        );
    }

    fn note_close_reason(
        target: &mut Option<(u64, NetConsoleDisconnectReason)>,
        conn_id: u64,
        reason: NetConsoleDisconnectReason,
    ) {
        match target {
            None => {
                *target = Some((conn_id, reason));
            }
            Some((_existing_id, existing_reason)) => {
                if *existing_reason != NetConsoleDisconnectReason::Quit
                    && reason == NetConsoleDisconnectReason::Quit
                {
                    *target = Some((conn_id, reason));
                }
            }
        }
    }

    fn audit_conn_open(conn_id: u64, peer: &str, port: u16) {
        #[cfg(feature = "cohesix-dev")]
        {
            let mut message = heapless::String::<128>::new();
            let _ = core::fmt::write(
                &mut message,
                format_args!(
                    "audit tcp.conn.open conn_id={} peer={}:{}",
                    conn_id, peer, port
                ),
            );
            crate::debug_uart::debug_uart_line(message.as_str());
        }
        #[cfg(not(feature = "cohesix-dev"))]
        {
            let _ = conn_id;
            let _ = peer;
            let _ = port;
        }
    }

    fn audit_conn_close(conn_id: u64, reason: NetConsoleDisconnectReason) {
        #[cfg(feature = "cohesix-dev")]
        {
            let mut message = heapless::String::<96>::new();
            let _ = core::fmt::write(
                &mut message,
                format_args!(
                    "audit tcp.conn.close conn_id={} reason={}",
                    conn_id,
                    reason.as_str()
                ),
            );
            crate::debug_uart::debug_uart_line(message.as_str());
        }
        #[cfg(not(feature = "cohesix-dev"))]
        {
            let _ = conn_id;
            let _ = reason;
        }
    }

    #[inline]
    fn trace_conn_prefix(payload: &[u8], disclose_payload: bool) -> ([u8; 16], usize) {
        const PREFIX_CAP: usize = 16;
        let mut prefix = [0u8; PREFIX_CAP];
        if !disclose_payload {
            return (prefix, 0);
        }
        let prefix_len = payload.len().min(PREFIX_CAP);
        if prefix_len > 0 {
            let _ = maybe_report_str_write(
                prefix.as_mut_ptr(),
                prefix_len,
                payload.as_ptr(),
                prefix_len,
                "trace_conn_prefix",
            );
            prefix[..prefix_len].copy_from_slice(&payload[..prefix_len]);
        }
        debug_assert!(prefix_len <= PREFIX_CAP);
        (prefix, prefix_len)
    }

    fn trace_conn_recv(conn_id: u64, payload: &[u8], disclose_payload: bool) {
        if !log::log_enabled!(log::Level::Debug) {
            return;
        }
        if !disclose_payload {
            log::debug!(
                "[cohsh-net] conn id={} recv bytes={} payload=redacted auth=pending",
                conn_id,
                payload.len(),
            );
            return;
        }
        let (prefix, prefix_len) = Self::trace_conn_prefix(payload, true);
        log::debug!(
            "[cohsh-net] conn id={} recv bytes={} first16={:02x?}",
            conn_id,
            payload.len(),
            &prefix[..prefix_len]
        );
    }

    fn trace_conn_send(conn_id: u64, payload: &[u8], disclose_payload: bool) {
        if !log::log_enabled!(log::Level::Debug) {
            return;
        }
        if !disclose_payload {
            log::debug!(
                "[cohsh-net] conn id={} send bytes={} payload=redacted auth=pending",
                conn_id,
                payload.len(),
            );
            return;
        }
        let (prefix, prefix_len) = Self::trace_conn_prefix(payload, true);
        log::debug!(
            "[cohsh-net] conn id={} send bytes={} first16={:02x?}",
            conn_id,
            payload.len(),
            &prefix[..prefix_len]
        );
    }

    fn trace_conn_closed(conn_id: u64, reason: &str, bytes_in: u64, bytes_out: u64) {
        log::info!(
            "[cohsh-net] conn id={} closed reason={} bytes_in={} bytes_out={}",
            conn_id,
            reason,
            bytes_in,
            bytes_out
        );
    }

    fn log_poll_snapshot(&mut self, snapshot: PollSnapshot) {
        if self.last_poll_snapshot == Some(snapshot) {
            trace!(
                "[cohsh-net] poll state unchanged: state={:?} active={} auth={:?} recv={} send={}",
                snapshot.tcp_state,
                snapshot.session_active,
                snapshot.auth_state,
                snapshot.can_recv,
                snapshot.can_send,
            );
            return;
        }

        debug!(
            "[cohsh-net] poll state: tcp={:?} session_active={} auth_state={:?} listener_ready={} recv={} send={} staged_events={}",
            snapshot.tcp_state,
            snapshot.session_active,
            snapshot.auth_state,
            snapshot.listener_ready,
            snapshot.can_recv,
            snapshot.can_send,
            snapshot.staged_events,
        );
        self.last_poll_snapshot = Some(snapshot);
    }

    fn log_tcp_state_change(
        session_state: &mut SessionState,
        socket: &TcpSocket,
        peer_endpoint: Option<(IpAddress, u16)>,
        iface_ip: Ipv4Address,
    ) {
        let current = socket.state();
        let previous = session_state.last_state;
        let previous_state = previous.unwrap_or(TcpState::Closed);
        if Some(current) == previous {
            return;
        }
        log::info!(
            target: "cohsh-net",
            "[tcp] state transition: {:?} -> {:?} local={:?} peer={:?}",
            previous_state,
            current,
            socket.local_endpoint(),
            socket.remote_endpoint(),
        );
        let (peer, port) = Self::peer_parts(peer_endpoint, socket);

        match (previous_state, current) {
            (TcpState::Closed, TcpState::Listen) => {
                log::info!(
                    target: "cohsh-net",
                    "[tcp] listener active local={:?} peer={:?}",
                    socket.local_endpoint(),
                    socket.remote_endpoint(),
                );
            }
            (TcpState::Listen, TcpState::SynReceived) => {
                log::info!(
                    target: "cohsh-net",
                    "[tcp] syn-received local={:?} peer={:?}",
                    socket.local_endpoint(),
                    socket.remote_endpoint(),
                );
            }
            (TcpState::SynReceived, TcpState::Established) => {
                log::info!(
                    target: "cohsh-net",
                    "[tcp] established local={:?} peer={:?}",
                    socket.local_endpoint(),
                    socket.remote_endpoint(),
                );
            }
            (_, TcpState::SynReceived) => {
                info!(
                    target: "root_task::net",
                    "[tcp] connect.begin addr={peer} port={port} iface_ip={iface_ip}"
                );
            }
            (_, TcpState::Established) => {
                info!(
                    target: "root_task::net",
                    "[tcp] connect.ok addr={peer} port={port} iface_ip={iface_ip}"
                );
                session_state.connect_reported = true;
            }
            _ => {}
        }

        if !session_state.connect_reported
            && matches!(current, TcpState::CloseWait | TcpState::Closed)
            && !matches!(previous_state, TcpState::Established)
        {
            warn!(
                target: "root_task::net",
                "[tcp] connect.err addr={peer} port={port} iface_ip={iface_ip} err={:?}",
                current
            );
            session_state.connect_reported = true;
        }
        session_state.last_state = Some(current);
        if !session_state.logged_accept && current == TcpState::Established {
            session_state.logged_accept = true;
        }
    }

    fn log_session_closed(
        session_state: &mut SessionState,
        peer_endpoint: Option<(IpAddress, u16)>,
        socket: &TcpSocket,
    ) {
        if session_state.close_logged {
            return;
        }
        let (peer, port) = Self::peer_parts(peer_endpoint, socket);
        info!(
            target: "root_task::net",
            "[tcp] close addr={peer} port={port} state={:?}",
            socket.state()
        );
        session_state.close_logged = true;
    }

    /// Constructs a network stack bound to the provided hardware abstraction.
    pub fn new<H>(
        hal: &mut H,
        config: ConsoleNetConfig,
        backend: NetBackend,
    ) -> Result<Box<Self>, NetStackError<D::Error>>
    where
        H: Hardware<Error = HalError>,
    {
        let init_guard = NetStackInitGuard::begin::<D::Error>(NET_INIT_TAG)?;
        info!("[net-console] init: constructing smoltcp stack");
        debug_assert_ne!(config.listen_port, 0, "TCP console port must be non-zero");
        if cfg!(feature = "dev-virt") && backend.uses_dev_virt_defaults() {
            debug_assert_eq!(config.listen_port, super::COHESIX_TCP_CONSOLE_PORT);
            debug_assert_eq!(config.address.ip, DEV_VIRT_IP);
            debug_assert_eq!(config.address.prefix_len, DEV_VIRT_PREFIX);
            debug_assert_eq!(config.address.gateway, Some(DEV_VIRT_GATEWAY));
        }
        let (ip, prefix, gateway) = match config.policy.mode {
            NetMode::Dhcp => (Ipv4Address::UNSPECIFIED, 0, None),
            NetMode::Static | NetMode::Off => {
                let ip = Ipv4Address::new(
                    config.address.ip[0],
                    config.address.ip[1],
                    config.address.ip[2],
                    config.address.ip[3],
                );
                let gateway = config.address.gateway.map(|gateway| {
                    Ipv4Address::new(gateway[0], gateway[1], gateway[2], gateway[3])
                });
                (ip, config.address.prefix_len, gateway)
            }
        };
        log_bootinfo_mark("net.init.begin", init_guard.attempt())?;
        Self::with_ipv4(hal, ip, prefix, gateway, config, backend, init_guard)
    }

    fn with_ipv4(
        hal: &mut impl Hardware<Error = HalError>,
        ip: Ipv4Address,
        prefix: u8,
        gateway: Option<Ipv4Address>,
        console_config: ConsoleNetConfig,
        backend: NetBackend,
        init_guard: NetStackInitGuard,
    ) -> Result<Box<Self>, NetStackError<D::Error>> {
        let netmask = prefix_to_netmask(prefix);
        let gateway_label = gateway.unwrap_or(Ipv4Address::UNSPECIFIED);
        let backend_label = backend.label();
        info!(
            "[net-console] init: bringing up backend={} device={} mode={} interface={} ip={}/{} netmask={} gateway={}",
            backend_label,
            D::name(),
            console_config.policy.mode.as_str(),
            console_config.policy.interface.as_str(),
            ip,
            prefix,
            netmask,
            gateway_label
        );
        info!(
            "[net-console] init: creating device={} backend={} interface={} (listen_port={})",
            D::name(),
            backend_label,
            console_config.policy.interface.as_str(),
            console_config.listen_port
        );
        let stage = NET_STAGE;
        info!("[net-console] net.stage={}", stage.as_str());
        let stage_policy = match stage {
            NetStage::ProbeOnly
            | NetStage::QueueInitOnly
            | NetStage::RxOnly
            | NetStage::ArpOnly
            | NetStage::IcmpOnly => NetStagePolicy {
                allow_tcp: false,
                allow_selftest: false,
                allow_outbound_probe: false,
                allow_console_io: false,
                tx_only: false,
            },
            NetStage::TxOnly => NetStagePolicy {
                allow_tcp: false,
                allow_selftest: self_test_enabled_for_backend(backend),
                allow_outbound_probe: false,
                allow_console_io: false,
                tx_only: true,
            },
            NetStage::TcpHandshakeOnly => NetStagePolicy {
                allow_tcp: true,
                allow_selftest: false,
                allow_outbound_probe: false,
                allow_console_io: false,
                tx_only: false,
            },
            NetStage::Full => NetStagePolicy {
                allow_tcp: true,
                allow_selftest: self_test_enabled_for_backend(backend),
                allow_outbound_probe: true,
                allow_console_io: true,
                tx_only: false,
            },
        };
        log_net_watch_targets("net.init.begin");
        BOOTINFO_WINDOW_GUARD.check("net.init.device.pre");
        D::driver_task_contract()
            .validate()
            .map_err(|err| NetStackError::DriverTaskContract(err.reason()))?;
        let mut device = Box::new(D::create_with_stage(hal, &console_config, stage)?);
        BOOTINFO_WINDOW_GUARD.check("net.init.device.post");
        let mac = device.mac();
        let bringup_status = device.bringup_status_label().unwrap_or("ready");
        info!(
            "[net-console] {} device initialized: mac={} interface={} bringup_status={}",
            D::name(),
            mac,
            device.interface_label(),
            bringup_status
        );

        let attempt = *init_guard.attempt();
        log_bootinfo_mark("net.init.device", &attempt)?;
        let dhcp_enabled = matches!(console_config.policy.mode, NetMode::Dhcp);

        BOOTINFO_WINDOW_GUARD.check("net.init.storage.pre");
        log_storage_addresses_once("net.init.reservation");
        let reservation = StorageReservation::acquire::<D::Error>(
            stage_policy.allow_selftest,
            dhcp_enabled,
            &attempt,
            attempt.tag,
        )?;
        BOOTINFO_WINDOW_GUARD.check("net.init.storage.post");

        let init_now_ms = crate::hal::timebase().now_ms();
        debug!("[net-console] init: timebase.now_ms={init_now_ms}");

        let clock = NetworkClock::new();
        let mut iface_config = IfaceConfig::new(HardwareAddress::Ethernet(mac));
        iface_config.random_seed = RANDOM_SEED;

        let mut interface = Interface::new(iface_config, device.as_mut(), clock.now());
        info!(
            "[net-console] smoltcp interface created; assigning ip={}/{} netmask={}",
            ip, prefix, netmask
        );
        interface.update_ip_addrs(|addrs| {
            let cidr = IpCidr::new(IpAddress::from(ip), prefix);
            set_primary_ipv4_addr!(addrs, cidr);
        });
        match gateway {
            Some(gw) => {
                let _ = interface.routes_mut().add_default_ipv4_route(gw);
                info!("[net-console] default gateway set to {gw}");
            }
            None => {
                info!(
                    "[net-console] default gateway set to {}",
                    Ipv4Address::UNSPECIFIED
                );
            }
        }
        debug_assert!(
            NEIGHBOR_CACHE_SIZE > 0,
            "smoltcp neighbor cache must allow at least one entry"
        );
        info!(
            "[net-console] iface cfg ip={}/{} gateway={} neighbor_cache_entries={}",
            ip, prefix, gateway_label, NEIGHBOR_CACHE_SIZE,
        );
        log_bootinfo_mark("net.init.interface", &attempt)?;
        let sockets = SocketSet::new(unsafe { &mut SOCKET_STORAGE[..] });
        log_bootinfo_mark("net.init.socketset", &attempt)?;
        let wifi_connection_generation = wifi_connection_generation_for::<D>();
        #[cfg(feature = "kernel")]
        let wifi_association_supervisor =
            crate::drivers::driver_task_net::Cyw43AssociationSupervisor::new(
                D::driver_task_contract()
                    == crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT,
                console_config.wifi_credentials,
                wifi_connection_generation,
                init_now_ms,
            );

        let mut stack = Box::new(Self {
            clock,
            device,
            interface,
            sockets,
            _reservation: reservation,
            init_attempt: attempt,
            icmp_echo_handle: SocketHandle::default(),
            icmp_echo_pending_since_ms: None,
            icmp_echo_next_poll_ms: None,
            icmp_echo_arp_probe_sent: false,
            icmp_echo_reply_constructed_this_turn: false,
            tcp_handle: SocketHandle::default(),
            tcp_standby_handle: SocketHandle::default(),
            standby_listener_armed: false,
            standby_pending_since_ms: None,
            server: TcpConsoleServer::new(
                console_config.auth_token,
                console_config.idle_timeout_ms,
            ),
            outbound: OutboundCoalescer::new(),
            telemetry: NetTelemetry::default(),
            backend,
            mode: console_config.policy.mode,
            interface_policy: console_config.policy.interface,
            wifi_credentials: console_config.wifi_credentials,
            wifi_connection_generation,
            #[cfg(feature = "kernel")]
            wifi_association_supervisor,
            wifi_static_address_pending: false,
            ip,
            gateway,
            prefix_len: prefix,
            listen_port: console_config.listen_port,
            session_active: false,
            disconnect_phase: ConsoleDisconnectPhase::Idle,
            disconnect_phase_started_ms: None,
            disconnect_reason: NetConsoleDisconnectReason::Quit,
            disconnect_forced_aborts: 0,
            listener_announced: false,
            listener_defer_reason: None,
            active_client_id: None,
            client_counter: 0,
            auth_state: AuthState::Start,
            session_state: SessionState::default(),
            conn_bytes_read: 0,
            conn_bytes_written: 0,
            events: HeaplessVec::new(),
            service_logged: false,
            last_poll_snapshot: None,
            peer_endpoint: None,
            dhcp_handle: None,
            dhcp: dhcp_enabled.then(|| DhcpClient::new(console_config.policy.dhcp)),
            dhcp_started: false,
            dhcp_restart_after_ms: None,
            wifi_dhcp_last_eapol_rx: 0,
            wifi_dhcp_eapol_quiet_since_ms: None,
            wifi_dhcp_eapol_settle_logged: false,
            wifi_rx_admission_blocked: false,
            wifi_rx_admission_next_retry_ms: 0,
            udp_beacon_handle: None,
            udp_echo_handle: None,
            tcp_smoke_handle: None,
            tcp_smoke_out_handle: None,
            #[cfg(feature = "net-outbound-probe")]
            tcp_probe_handle: None,
            counters: NetCounters::default(),
            cyw43_generation_proof_baseline: Cyw43GenerationProofBaseline::capture(
                wifi_connection_generation,
                NetCounters::default(),
            ),
            self_test: SelfTestState::new(stage_policy.allow_selftest),
            stage_policy,
            tx_only_sent: false,
            tcp_smoke_outbound_sent: false,
            tcp_smoke_outbound_connecting: false,
            tcp_smoke_last_attempt_ms: 0,
            #[cfg(feature = "net-outbound-probe")]
            probe_sent: false,
            #[cfg(feature = "net-outbound-probe")]
            probe_last_attempt_ms: 0,
            #[cfg(feature = "net-outbound-probe")]
            probe_fail_count: 0,
            #[cfg(feature = "net-outbound-probe")]
            probe_last_log_ms: 0,
            #[cfg(feature = "net-outbound-probe")]
            probe_warned_once: false,
            #[cfg(feature = "net-outbound-probe")]
            probe_hint_logged: false,
            last_now_ms: None,
            same_tick_poll_count: 0,
            time_stall_warned: false,
            budgeted_phase: BudgetedNetPhase::Interface,
        });
        stack.assert_bootinfo_overlaps();
        stack.log_buffer_addresses_once("net.init.buffers");
        stack.initialise_icmp_echo_socket()?;
        if stage_policy.allow_tcp {
            stack.initialise_socket()?;
        }
        if dhcp_enabled {
            stack.initialise_dhcp_socket()?;
            let _ = stack.start_dhcp_if_ready(init_now_ms);
        }
        if stage_policy.allow_selftest {
            stack.initialise_self_test_sockets()?;
        }
        #[cfg(feature = "net-outbound-probe")]
        if stage_policy.allow_outbound_probe {
            stack.initialise_probe_socket()?;
        }
        if stage_policy.allow_tcp {
            info!(
                target: "net-console",
                "[net-console] init: TCP listener socket prepared (port={})",
                console_config.listen_port
            );
            info!(
                target: "net-console",
                "[net-console] init: success; tcp console wired (non-blocking, port={})",
                console_config.listen_port
            );
        } else {
            info!(
                target: "net-console",
                "[net-console] init: staged bring-up (tcp sockets disabled)"
            );
        }
        log_bootinfo_mark("net.init.post", &attempt)?;
        init_guard.commit_online();
        Ok(stack)
    }

    fn add_icmp_echo_socket(&mut self) {
        debug_assert!(ICMP_ECHO_STORAGE_IN_USE.load(Ordering::Acquire));
        // SAFETY: StorageReservation owns all four ICMP buffer arrays for this
        // NetStack lifetime. Callers remove the prior raw socket before
        // reconstructing it, so no live socket aliases these slices.
        let socket = unsafe {
            let rx_buffer = RawPacketBuffer::new(
                &mut ICMP_ECHO_RX_METADATA[..],
                &mut ICMP_ECHO_RX_STORAGE[..],
            );
            let tx_buffer = RawPacketBuffer::new(
                &mut ICMP_ECHO_TX_METADATA[..],
                &mut ICMP_ECHO_TX_STORAGE[..],
            );
            RawSocket::new(
                Some(IpVersion::Ipv4),
                Some(IpProtocol::Icmp),
                rx_buffer,
                tx_buffer,
            )
        };
        self.icmp_echo_handle = self.sockets.add(socket);
    }

    fn initialise_icmp_echo_socket(&mut self) -> Result<(), NetStackError<D::Error>> {
        self.add_icmp_echo_socket();
        self.log_init_canary("net.init.socket.icmp_echo")?;
        Ok(())
    }

    fn reset_icmp_echo_socket(&mut self, reason: &'static str, now_ms: u64) {
        let socket = self.sockets.get::<RawSocket>(self.icmp_echo_handle);
        let rx_bytes = socket.recv_queue();
        let tx_bytes = socket.send_queue();
        let _ = self.sockets.remove(self.icmp_echo_handle);
        self.add_icmp_echo_socket();
        self.icmp_echo_pending_since_ms = None;
        self.icmp_echo_next_poll_ms = None;
        self.icmp_echo_arp_probe_sent = false;
        if rx_bytes != 0 || tx_bytes != 0 {
            info!(
                "[icmp-echo] reset reason={reason} dropped_rx_bytes={rx_bytes} dropped_tx_bytes={tx_bytes} now_ms={now_ms}"
            );
        }
    }

    fn initialise_socket(&mut self) -> Result<(), NetStackError<D::Error>> {
        debug_assert!(SOCKET_STORAGE_IN_USE.load(Ordering::Acquire));
        debug_assert!(TCP_RX_STORAGE_IN_USE.load(Ordering::Acquire));
        debug_assert!(TCP_TX_STORAGE_IN_USE.load(Ordering::Acquire));
        debug_assert!(TCP_STANDBY_RX_STORAGE_IN_USE.load(Ordering::Acquire));
        debug_assert!(TCP_STANDBY_TX_STORAGE_IN_USE.load(Ordering::Acquire));
        // SAFETY: StorageReservation uniquely owns both console buffer pairs,
        // and initialization has not yet created a socket referencing either
        // pair. The two constructors receive disjoint static storage.
        let (tcp_socket, tcp_standby_socket) = unsafe {
            (
                new_console_tcp_socket(&mut TCP_RX_STORAGE[..], &mut TCP_TX_STORAGE[..]),
                new_console_tcp_socket(
                    &mut TCP_STANDBY_RX_STORAGE[..],
                    &mut TCP_STANDBY_TX_STORAGE[..],
                ),
            )
        };
        self.tcp_handle = self.sockets.add(tcp_socket);
        self.tcp_standby_handle = self.sockets.add(tcp_standby_socket);
        self.log_init_canary("net.init.socket.tcp")?;
        Ok(())
    }

    fn initialise_dhcp_socket(&mut self) -> Result<(), NetStackError<D::Error>> {
        if self.dhcp.is_none() {
            return Ok(());
        }
        debug_assert!(DHCP_STORAGE_IN_USE.load(Ordering::Acquire));
        let rx_buffer =
            unsafe { UdpPacketBuffer::new(&mut DHCP_RX_METADATA[..], &mut DHCP_RX_STORAGE[..]) };
        let tx_buffer =
            unsafe { UdpPacketBuffer::new(&mut DHCP_TX_METADATA[..], &mut DHCP_TX_STORAGE[..]) };
        let mut socket = UdpSocket::new(rx_buffer, tx_buffer);
        socket
            .bind(DHCP_CLIENT_PORT)
            .map_err(NetStackError::DhcpSocketBind)?;
        self.dhcp_handle = Some(self.sockets.add(socket));
        self.log_init_canary("net.init.socket.dhcp")?;
        info!(
            "[dhcp] socket ready local_port={} server_port={}",
            DHCP_CLIENT_PORT, DHCP_SERVER_PORT
        );
        Ok(())
    }

    fn initialise_self_test_sockets(&mut self) -> Result<(), NetStackError<D::Error>> {
        if !self.self_test.enabled {
            return Ok(());
        }

        unsafe {
            let rx_buffer = UdpPacketBuffer::new(
                &mut UDP_BEACON_RX_METADATA[..],
                &mut UDP_BEACON_RX_STORAGE[..],
            );
            let tx_buffer = UdpPacketBuffer::new(
                &mut UDP_BEACON_TX_METADATA[..],
                &mut UDP_BEACON_TX_STORAGE[..],
            );
            let mut beacon_socket = UdpSocket::new(rx_buffer, tx_buffer);
            let beacon_endpoint = udp_beacon_bind_endpoint(UDP_BEACON_PORT);
            if let Err(err) = beacon_socket.bind(beacon_endpoint) {
                warn!(
                    "[net-selftest] failed to bind UDP beacon socket port={}: {:?}",
                    UDP_BEACON_PORT, err
                );
            } else {
                self.udp_beacon_handle = Some(self.sockets.add(beacon_socket));
                self.log_init_canary("net.init.socket.udp_beacon")?;
            }
        }

        unsafe {
            let rx_buffer =
                UdpPacketBuffer::new(&mut UDP_ECHO_RX_METADATA[..], &mut UDP_ECHO_RX_STORAGE[..]);
            let tx_buffer =
                UdpPacketBuffer::new(&mut UDP_ECHO_TX_METADATA[..], &mut UDP_ECHO_TX_STORAGE[..]);
            let mut echo_socket = UdpSocket::new(rx_buffer, tx_buffer);
            let echo_endpoint = IpListenEndpoint {
                addr: Some(Ipv4Address::UNSPECIFIED.into()),
                port: UDP_ECHO_PORT,
            };
            match echo_socket.bind(echo_endpoint) {
                Ok(()) => {
                    info!(
                        "[net-selftest] udp-echo ready on 0.0.0.0:{} (beacon dst={}:{})",
                        UDP_ECHO_PORT,
                        self.selftest_gateway_target(),
                        UDP_ECHO_PORT
                    );
                    self.udp_echo_handle = Some(self.sockets.add(echo_socket));
                    self.log_init_canary("net.init.socket.udp_echo")?;
                }
                Err(UdpBindError::Unaddressable) => {
                    warn!(
                        "[net-selftest] failed to bind UDP echo port {}: unaddressable",
                        UDP_ECHO_PORT
                    );
                }
                Err(UdpBindError::InvalidState) => {
                    warn!(
                        "[net-selftest] failed to bind UDP echo port {}: invalid state",
                        UDP_ECHO_PORT
                    );
                }
            }
        }

        unsafe {
            let rx_buffer = TcpSocketBuffer::new(&mut TCP_SMOKE_RX_STORAGE[..]);
            let tx_buffer = TcpSocketBuffer::new(&mut TCP_SMOKE_TX_STORAGE[..]);
            let mut tcp_socket = TcpSocket::new(rx_buffer, tx_buffer);
            if let Err(err) = tcp_socket.listen(TCP_SMOKE_PORT) {
                warn!(
                    "[net-selftest] failed to start TCP smoke listener on port {}: {:?}",
                    TCP_SMOKE_PORT, err
                );
            } else {
                info!(
                    "[net-selftest] tcp-smoke listener ready on 0.0.0.0:{}",
                    TCP_SMOKE_PORT
                );
                self.tcp_smoke_handle = Some(self.sockets.add(tcp_socket));
                self.log_init_canary("net.init.socket.tcp_smoke")?;
            }
        }

        unsafe {
            let rx_buffer = TcpSocketBuffer::new(&mut TCP_SMOKE_OUT_RX_STORAGE[..]);
            let tx_buffer = TcpSocketBuffer::new(&mut TCP_SMOKE_OUT_TX_STORAGE[..]);
            let tcp_socket = TcpSocket::new(rx_buffer, tx_buffer);
            self.tcp_smoke_out_handle = Some(self.sockets.add(tcp_socket));
            self.log_init_canary("net.init.socket.tcp_smoke_out")?;
        }

        Ok(())
    }

    #[cfg(feature = "net-outbound-probe")]
    fn initialise_probe_socket(&mut self) -> Result<(), NetStackError<D::Error>> {
        unsafe {
            let rx_buffer = TcpSocketBuffer::new(&mut TCP_PROBE_RX_STORAGE[..]);
            let tx_buffer = TcpSocketBuffer::new(&mut TCP_PROBE_TX_STORAGE[..]);
            let tcp_socket = TcpSocket::new(rx_buffer, tx_buffer);
            self.tcp_probe_handle = Some(self.sockets.add(tcp_socket));
        }

        self.log_init_canary("net.init.socket.tcp_probe")?;
        Ok(())
    }

    fn sync_wifi_connection_generation(&mut self, now_ms: u64) -> bool {
        let generation = wifi_connection_generation_for::<D>();
        if !cyw43_pre_poll_generation_fence_required(self.wifi_connection_generation, generation) {
            return false;
        }
        let previous_generation = self.wifi_connection_generation;
        self.cyw43_generation_proof_baseline =
            Cyw43GenerationProofBaseline::capture(generation, self.current_counters_unprojected());
        self.wifi_connection_generation = generation;
        #[cfg(feature = "kernel")]
        self.wifi_association_supervisor
            .sync_generation(self.wifi_credentials, generation, now_ms);

        self.device.set_assigned_ipv4(Ipv4Address::UNSPECIFIED);
        if matches!(self.mode, NetMode::Dhcp) {
            self.wifi_static_address_pending = false;
            self.ip = Ipv4Address::UNSPECIFIED;
            self.gateway = None;
            self.prefix_len = 0;
            self.interface.update_ip_addrs(|addrs| {
                let cidr = IpCidr::new(IpAddress::from(Ipv4Address::UNSPECIFIED), 0);
                set_primary_ipv4_addr!(addrs, cidr);
            });
            let _ = self.interface.routes_mut().remove_default_ipv4_route();
            if let Some(client) = self.dhcp.as_mut() {
                client.reset();
            }
            self.dhcp_started = false;
            #[cfg(feature = "kernel")]
            if D::driver_task_contract() == CYW43_WIFI_DRIVER_TASK_CONTRACT {
                clear_cyw43_oldgood_dhcp_receipt();
            }
            self.dhcp_restart_after_ms = None;
            if let Some(handle) = self.dhcp_handle {
                let socket = self.sockets.get_mut::<UdpSocket>(handle);
                socket.close();
                if let Err(err) = socket.bind(DHCP_CLIENT_PORT) {
                    warn!(
                        "[dhcp] generation reset rebind failed generation={} err={err:?}",
                        generation
                    );
                }
            }
        } else {
            // Rewriting the existing address list through the public smoltcp
            // API flushes its private neighbor cache without discarding a
            // manifest-configured static address.
            self.interface.update_ip_addrs(|_| {});
            self.wifi_static_address_pending = true;
        }

        self.reset_icmp_echo_socket("wifi-generation-reset", now_ms);
        self.abort_console_socket_pair("wifi-generation-reset");
        if let Some(handle) = self.tcp_smoke_handle {
            self.sockets.get_mut::<TcpSocket>(handle).abort();
        }
        if let Some(handle) = self.tcp_smoke_out_handle {
            self.sockets.get_mut::<TcpSocket>(handle).abort();
        }
        #[cfg(feature = "net-outbound-probe")]
        if let Some(handle) = self.tcp_probe_handle {
            self.sockets.get_mut::<TcpSocket>(handle).abort();
        }

        self.server.end_session();
        self.outbound.reset();
        self.session_active = false;
        self.active_client_id = None;
        self.peer_endpoint = None;
        self.events.clear();
        self.reset_session_state();
        self.listener_announced = false;
        self.listener_defer_reason = Some("wifi-generation-reset");
        self.last_poll_snapshot = None;
        self.telemetry.link_up = false;
        self.wifi_dhcp_last_eapol_rx = 0;
        self.wifi_dhcp_eapol_quiet_since_ms = None;
        self.wifi_dhcp_eapol_settle_logged = false;
        self.wifi_rx_admission_blocked = false;
        self.wifi_rx_admission_next_retry_ms = 0;
        self.tx_only_sent = false;
        self.tcp_smoke_outbound_sent = false;
        self.tcp_smoke_outbound_connecting = false;
        self.tcp_smoke_last_attempt_ms = 0;
        self.self_test.reset_for_connection_generation();
        self.budgeted_phase = BudgetedNetPhase::Interface;
        #[cfg(feature = "net-outbound-probe")]
        {
            self.probe_sent = false;
            self.probe_last_attempt_ms = 0;
            self.probe_fail_count = 0;
            self.probe_last_log_ms = 0;
            self.probe_hint_logged = false;
        }
        info!(
            "[net-console] wifi generation reset previous={} current={} address={} arp_cache=flushed tcp=closed dhcp={} now_ms={}",
            previous_generation,
            generation,
            self.ip,
            if matches!(self.mode, NetMode::Dhcp) {
                "reset"
            } else {
                "static-preserved"
            },
            now_ms,
        );
        true
    }

    fn restore_static_wifi_generation_address_if_ready(&mut self) -> bool {
        if !self.wifi_static_address_pending || self.device.bringup_status_label().is_some() {
            return false;
        }
        self.device.set_assigned_ipv4(self.ip);
        self.wifi_static_address_pending = false;
        info!(
            "[net-console] wifi static address restored generation={} ip={}/{} gateway={}",
            self.wifi_connection_generation,
            self.ip,
            self.prefix_len,
            self.gateway.unwrap_or(Ipv4Address::UNSPECIFIED),
        );
        true
    }

    fn begin_poll_turn(&mut self, now_ms: u64) -> (Instant, bool) {
        self.icmp_echo_reply_constructed_this_turn = false;
        let wifi_generation_changed = self.sync_wifi_connection_generation(now_ms);
        let _ = self.restore_static_wifi_generation_address_if_ready();
        if !self.service_logged {
            info!("[net-console] service loop running");
            self.service_logged = true;
        }
        if let Some(previous) = self.last_now_ms {
            if now_ms < previous {
                warn!(
                    "[net-console] timebase regression detected: prev_now_ms={} now_ms={}",
                    previous, now_ms
                );
                self.same_tick_poll_count = 0;
                self.time_stall_warned = false;
            } else if now_ms == previous {
                self.same_tick_poll_count = self.same_tick_poll_count.saturating_add(1);
                if timebase_stall_warning_due(
                    self.same_tick_poll_count,
                    self.time_stall_warned,
                    self.device.bringup_status_label(),
                ) {
                    warn!(
                        "[net-console] timebase stalled: now_ms={} polls={} (no forward progress)",
                        now_ms, self.same_tick_poll_count
                    );
                    self.time_stall_warned = true;
                }
            } else if now_ms > previous {
                self.same_tick_poll_count = 0;
                self.time_stall_warned = false;
            }
        }
        self.last_now_ms = Some(now_ms);

        let last = self.telemetry.last_poll_ms;
        let delta = now_ms.saturating_sub(last);
        let delta_ms = core::cmp::min(delta, u64::from(u32::MAX)) as u32;
        let timestamp = if delta_ms == 0 {
            self.clock.now()
        } else {
            self.clock.advance(delta_ms)
        };

        if now_ms % 1000 == 0 {
            self.device.debug_snapshot();
        }

        (timestamp, wifi_generation_changed)
    }

    fn finish_poll_turn(&mut self, now_ms: u64, activity: bool) {
        self.telemetry.last_poll_ms = now_ms;
        if self.device.bringup_status_label().is_some() {
            self.telemetry.link_up = false;
        } else if activity {
            self.telemetry.link_up = true;
        }
        self.telemetry.tx_drops = self.device.tx_drop_count();
        self.sync_device_counters();
    }

    fn icmp_echo_service_due_at(&self, now_ms: u64) -> bool {
        let socket = self.sockets.get::<RawSocket>(self.icmp_echo_handle);
        if socket.recv_queue() != 0 {
            return true;
        }
        if socket.send_queue() == 0 {
            return false;
        }
        if self
            .icmp_echo_pending_since_ms
            .is_some_and(|started| now_ms.saturating_sub(started) >= ICMP_ECHO_REPLY_DEADLINE_MS)
        {
            return true;
        }
        self.icmp_echo_next_poll_ms
            .is_none_or(|next_poll| now_ms >= next_poll)
    }

    fn update_icmp_echo_dispatch_state(
        &mut self,
        now_ms: u64,
        tx_pending_before: bool,
        arp_tx_before: u64,
    ) {
        let tx_pending_after = self
            .sockets
            .get::<RawSocket>(self.icmp_echo_handle)
            .send_queue()
            != 0;
        if !tx_pending_after {
            self.icmp_echo_pending_since_ms = None;
            self.icmp_echo_next_poll_ms = None;
            self.icmp_echo_arp_probe_sent = false;
            return;
        }
        if !tx_pending_before {
            return;
        }
        if self.device.counters().arp_tx > arp_tx_before {
            self.icmp_echo_arp_probe_sent = true;
        }
        let retry_ms = if self.icmp_echo_arp_probe_sent {
            ICMP_ECHO_NEIGHBOR_RETRY_MS
        } else {
            ICMP_ECHO_TX_AVAILABILITY_RETRY_MS
        };
        let retry_at = now_ms.saturating_add(retry_ms);
        let deadline = self
            .icmp_echo_pending_since_ms
            .map(|started| started.saturating_add(ICMP_ECHO_REPLY_DEADLINE_MS))
            .unwrap_or(retry_at);
        self.icmp_echo_next_poll_ms = Some(core::cmp::min(retry_at, deadline));
    }

    fn expire_icmp_echo_if_due(&mut self, now_ms: u64) -> bool {
        let expired = self
            .icmp_echo_pending_since_ms
            .is_some_and(|started| now_ms.saturating_sub(started) >= ICMP_ECHO_REPLY_DEADLINE_MS)
            && self
                .sockets
                .get::<RawSocket>(self.icmp_echo_handle)
                .send_queue()
                != 0;
        if expired {
            self.reset_icmp_echo_socket("neighbor-resolution-timeout", now_ms);
        }
        expired
    }

    fn service_icmp_echo(&mut self, now_ms: u64) -> bool {
        let (rx_bytes, tx_bytes) = {
            let socket = self.sockets.get::<RawSocket>(self.icmp_echo_handle);
            (socket.recv_queue(), socket.send_queue())
        };
        if tx_bytes == 0 {
            self.icmp_echo_pending_since_ms = None;
            self.icmp_echo_next_poll_ms = None;
            self.icmp_echo_arp_probe_sent = false;
        } else if self
            .icmp_echo_pending_since_ms
            .is_some_and(|started| now_ms.saturating_sub(started) >= ICMP_ECHO_REPLY_DEADLINE_MS)
        {
            self.reset_icmp_echo_socket("neighbor-resolution-timeout", now_ms);
            return true;
        }
        if rx_bytes == 0 {
            return false;
        }
        if tx_bytes == 0 && self.icmp_echo_reply_constructed_this_turn {
            return false;
        }

        let mut packet = [0u8; MAX_FRAME_LEN];
        let recv_result = self
            .sockets
            .get_mut::<RawSocket>(self.icmp_echo_handle)
            .recv_slice(&mut packet);
        let packet_len = match recv_result {
            Ok(packet_len) => packet_len,
            Err(RawRecvError::Exhausted) => return false,
            Err(RawRecvError::Truncated) => {
                warn!("[icmp-echo] dropped oversized raw IPv4 packet");
                return true;
            }
        };

        if tx_bytes != 0 {
            debug!(
                "[icmp-echo] dropped request reason=reply-pending packet_len={packet_len} now_ms={now_ms}"
            );
            return true;
        }

        let request = match parse_icmp_echo_request(&packet[..packet_len], self.ip) {
            Ok(request) => request,
            Err(reason) => {
                trace!(
                    "[icmp-echo] ignored raw ICMP packet reason={reason:?} packet_len={packet_len}"
                );
                return true;
            }
        };
        let reply_len = request.reply_len();
        let emit_result = {
            let socket = self.sockets.get_mut::<RawSocket>(self.icmp_echo_handle);
            match socket.send(reply_len) {
                Ok(output) => request.emit_reply(self.ip, output),
                Err(RawSendError::BufferFull) => {
                    debug!(
                        "[icmp-echo] dropped request reason=reply-buffer-full packet_len={packet_len}"
                    );
                    return true;
                }
            }
        };
        if let Err(reason) = emit_result {
            warn!(
                "[icmp-echo] reply construction failed reason={reason:?} action=socket-reset now_ms={now_ms}"
            );
            self.reset_icmp_echo_socket("reply-construction-failed", now_ms);
            return true;
        }
        self.icmp_echo_pending_since_ms = Some(now_ms);
        self.icmp_echo_next_poll_ms = Some(now_ms);
        self.icmp_echo_arp_probe_sent = false;
        self.icmp_echo_reply_constructed_this_turn = true;
        debug!(
            "[icmp-echo] reply retained src={} bytes={} deadline_ms={}",
            request.source,
            reply_len,
            now_ms.saturating_add(ICMP_ECHO_REPLY_DEADLINE_MS)
        );
        true
    }

    fn poll_smoltcp_interface(&mut self, timestamp: Instant) -> PollResult {
        self.interface.poll_maintenance(timestamp);
        let mut result = PollResult::None;

        // Keep one exact copied RX and its immediate socket egress in the same
        // bounded device transaction. CYW43 may transfer the already-reserved
        // paired queue slot across this boundary; every other device uses the
        // default no-op hooks.
        for _ in 0..MAX_CONSOLE_FRAMES_PER_POLL {
            self.device.begin_smoltcp_rx_transaction();
            let ingress = self.interface.poll_ingress_single(
                timestamp,
                self.device.as_mut(),
                &mut self.sockets,
            );
            if ingress == PollIngressSingleResult::None {
                self.device.end_smoltcp_rx_transaction();
                break;
            }
            if ingress == PollIngressSingleResult::SocketStateChanged {
                result = PollResult::SocketStateChanged;
            }
            if self
                .interface
                .poll_egress(timestamp, self.device.as_mut(), &mut self.sockets)
                == PollResult::SocketStateChanged
            {
                result = PollResult::SocketStateChanged;
            }
            self.device.end_smoltcp_rx_transaction();
        }

        // No copied-RX authority survives into ordinary socket flushes. Keep
        // the former repeated-egress behavior, bounded by the existing frame
        // budget for one console poll.
        for _ in 0..MAX_CONSOLE_FRAMES_PER_POLL {
            match self
                .interface
                .poll_egress(timestamp, self.device.as_mut(), &mut self.sockets)
            {
                PollResult::None => break,
                PollResult::SocketStateChanged => result = PollResult::SocketStateChanged,
            }
        }

        result
    }

    fn poll_smoltcp_once(&mut self, timestamp: Instant, now_ms: u64, label: &'static str) -> bool {
        let mut activity = self.expire_icmp_echo_if_due(now_ms);
        let icmp_echo_tx_pending_before = self
            .sockets
            .get::<RawSocket>(self.icmp_echo_handle)
            .send_queue()
            != 0;
        let arp_tx_before = self.device.counters().arp_tx;
        self.bump_poll_counter();
        let poll_result = self.poll_smoltcp_interface(timestamp);
        activity |= poll_result != PollResult::None;
        self.update_icmp_echo_dispatch_state(now_ms, icmp_echo_tx_pending_before, arp_tx_before);
        activity |= self.service_icmp_echo(now_ms);
        if activity {
            log::debug!("[net] smoltcp: {label} poll now_ms={now_ms}");
        }
        activity
    }

    fn charge_tcp_budget(budget: &mut DriverServiceBudget) -> Result<(), DriverServiceBudgetError> {
        budget.charge_ops(64)?;
        budget.charge_frames(MAX_CONSOLE_FRAMES_PER_POLL as u16)?;
        budget.charge_bytes(TCP_SERVICE_BYTES_PER_TURN)
    }

    fn charge_interface_poll_budget(
        budget: &mut DriverServiceBudget,
    ) -> Result<(), DriverServiceBudgetError> {
        budget.charge_ops(2)?;
        budget.charge_frames(1)?;
        budget.charge_bytes(2048)
    }

    fn charge_dhcp_budget(
        budget: &mut DriverServiceBudget,
    ) -> Result<(), DriverServiceBudgetError> {
        budget.charge_ops(16)?;
        budget.charge_frames(MAX_DHCP_RX_PACKETS_PER_POLL as u16 + 1)?;
        budget.charge_bytes((MAX_DHCP_RX_PACKETS_PER_POLL as u32 + 1) * 1024)
    }

    fn charge_wifi_host_eapol_budget(
        budget: &mut DriverServiceBudget,
    ) -> Result<(), DriverServiceBudgetError> {
        budget.charge_ops(CYW43_HOST_EAPOL_BUDGETED_SERVICE_POLLS as u16)?;
        budget.charge_frames(8)?;
        budget.charge_bytes(8 * 1024)
    }

    fn budgeted_dhcp_service_required(&self) -> bool {
        budgeted_dhcp_service_required(self.mode, self.ip, self.dhcp_handle.is_some())
    }

    fn budgeted_genet_tcp_fast_path_due(&self) -> bool {
        #[cfg(feature = "kernel")]
        {
            budgeted_genet_tcp_fast_path_due(
                D::driver_task_contract(),
                self.stage_policy,
                self.console_listener_defer_reason(),
            )
        }
        #[cfg(not(feature = "kernel"))]
        {
            false
        }
    }

    fn budgeted_cyw43_tcp_fast_path_due(&self) -> bool {
        #[cfg(feature = "kernel")]
        {
            budgeted_cyw43_tcp_fast_path_due(
                D::driver_task_contract(),
                self.stage_policy,
                self.console_listener_defer_reason(),
            )
        }
        #[cfg(not(feature = "kernel"))]
        {
            false
        }
    }

    #[cfg(feature = "kernel")]
    fn budgeted_cyw43_selftest_defers_to_tcp(&self, phase: BudgetedNetPhase) -> bool {
        let socket = self.sockets.get::<TcpSocket>(self.tcp_handle);
        let tcp_pending_work = socket.can_recv()
            || self.server.has_outbound()
            || self.outbound.has_pending()
            || self.disconnect_phase != ConsoleDisconnectPhase::Idle;
        budgeted_cyw43_selftest_defers_to_tcp(
            D::driver_task_contract(),
            self.stage_policy,
            self.console_listener_defer_reason(),
            phase,
            self.session_active,
            socket.state(),
            tcp_pending_work,
        )
    }

    #[cfg(feature = "kernel")]
    fn cyw43_flush_pre_poll_data_ready(&self) -> bool {
        let dhcp_phase = self.dhcp.as_ref().map(|client| client.status().phase);
        cyw43_flush_pre_poll_data_ready_for(
            D::driver_task_contract(),
            self.device.interface_label(),
            self.mode,
            self.ip,
            self.device.bringup_status_label(),
            dhcp_phase,
            self.dhcp_handle.is_some(),
        )
    }

    #[cfg(feature = "kernel")]
    fn cyw43_runtime_service_pre_poll_ready(&self) -> bool {
        let dhcp_phase = self.dhcp.as_ref().map(|client| client.status().phase);
        cyw43_runtime_service_pre_poll_ready_for(
            D::driver_task_contract(),
            self.device.interface_label(),
            self.mode,
            self.ip,
            self.device.bringup_status_label(),
            dhcp_phase,
            self.dhcp_handle.is_some(),
        )
    }

    #[cfg(feature = "kernel")]
    fn reassert_cyw43_dhcp_rx_admission(&mut self, reason: &'static str, now_ms: u64) -> bool {
        if D::driver_task_contract() != crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT {
            self.wifi_rx_admission_blocked = false;
            self.wifi_rx_admission_next_retry_ms = 0;
            return true;
        }
        if self.wifi_rx_admission_blocked && now_ms < self.wifi_rx_admission_next_retry_ms {
            return false;
        }
        let reasserted = crate::drivers::driver_task_net::reassert_cyw43_post_secure_data_rx(
            D::driver_task_contract(),
        );
        if reasserted {
            self.wifi_rx_admission_blocked = false;
            self.wifi_rx_admission_next_retry_ms = 0;
            info!(
                "[dhcp] wifi data admission reasserted reason={} interface={} now_ms={}",
                reason,
                self.device.interface_label(),
                now_ms
            );
        } else {
            self.wifi_rx_admission_blocked = true;
            self.wifi_rx_admission_next_retry_ms =
                now_ms.saturating_add(CYW43_DHCP_RX_ADMISSION_RETRY_MS);
            warn!(
                "[dhcp] wifi data admission blocked reason={} interface={} now_ms={} retry_after_ms={} action=defer-tcp-listener",
                reason,
                self.device.interface_label(),
                now_ms,
                self.wifi_rx_admission_next_retry_ms
            );
        }
        reasserted
    }

    #[cfg(feature = "kernel")]
    fn retry_blocked_cyw43_rx_admission(&mut self, now_ms: u64) -> bool {
        if !self.wifi_rx_admission_blocked {
            return false;
        }
        self.reassert_cyw43_dhcp_rx_admission("post-bind-retry", now_ms)
    }

    #[cfg(not(feature = "kernel"))]
    fn retry_blocked_cyw43_rx_admission(&mut self, _now_ms: u64) -> bool {
        false
    }

    #[cfg(feature = "kernel")]
    fn service_cyw43_data_pre_poll_burst(
        &self,
        contract: crate::hal::driver_task::DriverTaskContract,
    ) -> bool {
        if net_driver_task_hot_path(contract)
            != Some(crate::hal::driver_task::DriverTaskHotPath::Cyw43Wifi)
            || !self.cyw43_runtime_service_pre_poll_ready()
            || wifi_host_eapol_blocks_driver_task_pre_poll(self.device.bringup_status_label())
            || !crate::drivers::driver_task_net::driver_task_runtime_pre_poll_allowed(contract)
        {
            return false;
        }
        service_driver_task_pre_poll_burst(
            contract,
            crate::hal::driver_task::DriverTaskHotPath::Cyw43Wifi,
            0,
        )
    }

    #[cfg(feature = "kernel")]
    fn service_cyw43_data_pre_poll_burst_budgeted(
        &self,
        contract: crate::hal::driver_task::DriverTaskContract,
        budget: &mut DriverServiceBudget,
    ) -> bool {
        if net_driver_task_hot_path(contract)
            != Some(crate::hal::driver_task::DriverTaskHotPath::Cyw43Wifi)
            || !self.cyw43_runtime_service_pre_poll_ready()
            || wifi_host_eapol_blocks_driver_task_pre_poll(self.device.bringup_status_label())
            || !crate::drivers::driver_task_net::driver_task_runtime_pre_poll_allowed(contract)
        {
            return false;
        }
        service_driver_task_pre_poll_burst_budgeted(
            contract,
            crate::hal::driver_task::DriverTaskHotPath::Cyw43Wifi,
            NET_RING_FLAG_BUDGETED,
            budget,
        )
    }

    #[cfg(feature = "kernel")]
    fn service_genet_tcp_flush_pre_poll_burst_budgeted(
        &self,
        contract: crate::hal::driver_task::DriverTaskContract,
        hot_path: crate::hal::driver_task::DriverTaskHotPath,
        budget: &mut DriverServiceBudget,
    ) -> bool {
        if !genet_tcp_flush_pre_poll_enabled(hot_path)
            || !crate::drivers::driver_task_net::driver_task_runtime_pre_poll_allowed(contract)
        {
            return false;
        }
        service_driver_task_pre_poll_burst_budgeted(
            contract,
            hot_path,
            NET_RING_FLAG_BUDGETED,
            budget,
        )
    }

    #[cfg(feature = "kernel")]
    fn drain_cyw43_pre_poll_activity(
        &mut self,
        timestamp: Instant,
        now_ms: u64,
        budget: &mut DriverServiceBudget,
        pre_poll_activity: bool,
    ) -> bool {
        if !pre_poll_activity {
            return false;
        }
        if Self::charge_interface_poll_budget(budget).is_err() {
            return false;
        }
        self.poll_smoltcp_once(timestamp, now_ms, "budgeted-cyw43-pre-poll-drain")
    }

    fn service_budgeted_dhcp_turn(&mut self, timestamp: Instant, now_ms: u64) -> bool {
        let mut activity = self.poll_smoltcp_once(timestamp, now_ms, "budgeted-pre-dhcp");
        activity |= self.start_dhcp_if_ready(now_ms);
        let dhcp_activity = self.service_dhcp(now_ms);
        activity |= dhcp_activity;
        if dhcp_activity {
            activity |= self.poll_smoltcp_once(timestamp, now_ms, "budgeted-post-dhcp");
        }
        activity
    }

    fn service_budgeted_tcp_turn(
        &mut self,
        timestamp: Instant,
        now_ms: u64,
        pre_label: &'static str,
        post_label: &'static str,
    ) -> bool {
        let pre_activity = self.poll_smoltcp_once(timestamp, now_ms, pre_label);
        let mut activity = pre_activity;
        let tcp_activity = self.stage_policy.allow_tcp && self.process_tcp(now_ms);
        activity |= tcp_activity;
        if pre_activity || tcp_activity {
            activity |= self.poll_smoltcp_once(timestamp, now_ms, post_label);
        }
        activity
    }

    fn flush_budgeted_tcp_with_time(
        &mut self,
        now_ms: u64,
        budget: &mut DriverServiceBudget,
        cyw43_pre_poll_activity: bool,
    ) -> bool {
        if !self.stage_policy.allow_tcp {
            return false;
        }
        if Self::charge_tcp_budget(budget).is_err() {
            return false;
        }
        let (timestamp, wifi_generation_changed) = self.begin_poll_turn(now_ms);
        if wifi_generation_changed {
            // An outer CYW43 pre-poll can observe disconnect before this
            // flush turn starts. Never drain its preserved frames or touch a
            // TCP socket under the invalidated generation.
            self.finish_poll_turn(now_ms, true);
            return true;
        }
        if !self.validate_console_socket(now_ms) {
            self.finish_poll_turn(now_ms, false);
            return true;
        }

        let mut activity =
            self.drain_cyw43_pre_poll_activity(timestamp, now_ms, budget, cyw43_pre_poll_activity);
        activity |= self.service_budgeted_tcp_turn(
            timestamp,
            now_ms,
            "budgeted-flush-tcp-pre",
            "budgeted-flush-tcp-post",
        );
        if self.budgeted_genet_tcp_fast_path_due() && activity {
            for _ in 0..GENET_TCP_POST_DISPATCH_EXTRA_TURNS {
                if Self::charge_tcp_budget(budget).is_err() {
                    break;
                }
                let turn_activity = self.service_budgeted_tcp_turn(
                    timestamp,
                    now_ms,
                    "budgeted-genet-dispatch-tcp",
                    "budgeted-genet-dispatch-flush",
                );
                if !turn_activity {
                    break;
                }
                activity = true;
            }
        }

        self.finish_poll_turn(now_ms, activity);
        activity
    }

    fn sync_interface_hardware_addr_value(
        interface: &mut Interface,
        device_mac: EthernetAddress,
    ) -> Option<HardwareAddress> {
        let current = interface.hardware_addr();
        let target = HardwareAddress::Ethernet(device_mac);
        if current == target {
            return None;
        }
        interface.set_hardware_addr(target);
        Some(current)
    }

    fn sync_interface_hardware_addr(&mut self, now_ms: u64) -> bool {
        let device_mac = self.device.mac();
        let previous = Self::sync_interface_hardware_addr_value(&mut self.interface, device_mac);
        let Some(previous) = previous else {
            return false;
        };
        info!(
            "[net-console] hardware address sync interface={} old={} new={} now_ms={}",
            self.device.interface_label(),
            previous,
            device_mac,
            now_ms
        );
        if dhcp_restart_required_after_mac_sync(self.mode, self.ip, self.dhcp_started) {
            if let Some(client) = self.dhcp.as_mut() {
                client.start(device_mac.0, now_ms, self.wifi_connection_generation);
                #[cfg(feature = "kernel")]
                if D::driver_task_contract() == CYW43_WIFI_DRIVER_TASK_CONTRACT {
                    record_cyw43_oldgood_dhcp_start(
                        self.wifi_connection_generation,
                        client.transaction_id(),
                        now_ms,
                    );
                }
                self.dhcp_restart_after_ms = None;
                info!(
                    "[dhcp] restart reason=hardware-address-sync interface={} mac={} generation={} xid=0x{:08x} now_ms={}",
                    self.device.interface_label(),
                    device_mac,
                    self.wifi_connection_generation,
                    client.transaction_id(),
                    now_ms
                );
            }
        }
        true
    }

    /// Polls the network stack using a host-supplied monotonic timestamp in milliseconds.
    pub fn poll_with_time(&mut self, now_ms: u64) -> bool {
        let (timestamp, wifi_generation_changed) = self.begin_poll_turn(now_ms);
        if wifi_generation_changed {
            // The caller may have serviced a linked-runtime burst before
            // entering the stack. End the turn immediately after generation
            // state is reset and before smoltcp, DHCP, or TCP work.
            self.finish_poll_turn(now_ms, true);
            return true;
        }

        if self.stage_policy.allow_tcp && !self.validate_console_socket(now_ms) {
            self.finish_poll_turn(now_ms, false);
            return true;
        }

        let mut activity = self.sync_interface_hardware_addr(now_ms);
        if self.wifi_association_claims_runtime_turn(now_ms) {
            activity |= self.service_wifi_host_eapol_slice_with_limit(now_ms, 1);
            activity |= self.sync_interface_hardware_addr(now_ms);
            self.finish_poll_turn(now_ms, activity);
            return activity;
        }
        if self.stage_policy.tx_only && !self.tx_only_sent {
            activity |= self.send_udp_beacon();
            if activity {
                let _ = self.poll_smoltcp_once(timestamp, now_ms, "tx-only");
            }
            self.tx_only_sent = true;
            self.finish_poll_turn(now_ms, activity);
            return activity;
        }

        activity |= self.service_wifi_host_eapol_slice(now_ms);
        activity |= self.sync_interface_hardware_addr(now_ms);
        let host_eapol_blocks_data =
            wifi_host_eapol_blocks_data_path(self.device.bringup_status_label());
        let canonical_net_data = matches!(
            crate::drivers::driver_task_net::cyw43_canonical_policy_owner(),
            Some(crate::drivers::driver_task_net::Cyw43CanonicalPolicyOwner::NetData)
        );
        if host_eapol_blocks_data && !canonical_net_data {
            self.finish_poll_turn(now_ms, activity);
            return activity;
        }
        #[cfg(feature = "kernel")]
        {
            let cyw43_pre_poll_activity =
                self.service_cyw43_data_pre_poll_burst(D::driver_task_contract());
            activity |= cyw43_pre_poll_activity;
            if self.sync_wifi_connection_generation(now_ms) {
                // The pre-poll burst can preserve a disconnect event and
                // advance the CYW43 epoch. End this turn before smoltcp, DHCP,
                // or TCP can consume queued bytes under the old generation.
                activity = true;
                self.finish_poll_turn(now_ms, activity);
                return activity;
            }
            if cyw43_pre_poll_activity && !host_eapol_blocks_data {
                activity |= self.poll_smoltcp_once(timestamp, now_ms, "cyw43-pre-poll-drain");
            }
        }
        if host_eapol_blocks_data {
            // A recovery-fenced NetData terminal reached its canonical prompt
            // owner above. Keep DHCP/TCP closed until pair recovery completes;
            // this exception exists only to retire the exact old parent.
            self.finish_poll_turn(now_ms, activity);
            return activity;
        }
        activity |= self.retry_blocked_cyw43_rx_admission(now_ms);
        if self.wifi_rx_admission_blocked {
            self.finish_poll_turn(now_ms, activity);
            return activity;
        }
        activity |= self.poll_smoltcp_once(timestamp, now_ms, "main");
        let dhcp_start_activity = self.start_dhcp_if_ready(now_ms);
        activity |= dhcp_start_activity;
        let dhcp_activity = self.service_dhcp(now_ms);
        activity |= dhcp_activity;
        let tcp_activity = if self.stage_policy.allow_tcp {
            self.process_tcp(now_ms)
        } else {
            false
        };
        activity |= tcp_activity;

        // Run a second poll pass when TCP work was observed so any queued
        // responses (including AUTH acknowledgements) are flushed to the wire
        // without waiting for the next timer tick.
        if tcp_activity || dhcp_activity {
            if self.stage_policy.allow_tcp && !self.validate_console_socket(now_ms) {
                self.finish_poll_turn(now_ms, activity);
                return true;
            }
            activity |= self.poll_smoltcp_once(timestamp, now_ms, "post-tcp");
        }

        activity |= self.stage_policy.allow_selftest && self.service_self_test(now_ms, timestamp);

        #[cfg(feature = "net-outbound-probe")]
        {
            activity |= self.stage_policy.allow_outbound_probe
                && self.service_outbound_probe(now_ms, timestamp);
        }

        self.finish_poll_turn(now_ms, activity);
        activity
    }

    fn poll_budgeted_with_time(
        &mut self,
        now_ms: u64,
        budget: &mut DriverServiceBudget,
    ) -> Result<bool, DriverServiceBudgetError> {
        budget.charge_ops(1)?;

        if self.wifi_association_claims_runtime_turn(now_ms) {
            Self::charge_wifi_host_eapol_budget(budget)?;
            let (_, wifi_generation_changed) = self.begin_poll_turn(now_ms);
            if wifi_generation_changed {
                self.finish_poll_turn(now_ms, true);
                return Ok(true);
            }
            if self.stage_policy.allow_tcp && !self.validate_console_socket(now_ms) {
                self.finish_poll_turn(now_ms, false);
                return Ok(true);
            }
            let mut activity = self.sync_interface_hardware_addr(now_ms);
            activity |= self.service_wifi_host_eapol_slice_with_limit(now_ms, 1);
            activity |= self.sync_interface_hardware_addr(now_ms);
            self.finish_poll_turn(now_ms, activity);
            return Ok(activity);
        }

        if self.stage_policy.tx_only && !self.tx_only_sent {
            budget.charge_ops(2)?;
            budget.charge_frames(1)?;
            budget.charge_bytes(256)?;

            let (timestamp, wifi_generation_changed) = self.begin_poll_turn(now_ms);
            if wifi_generation_changed {
                self.finish_poll_turn(now_ms, true);
                return Ok(true);
            }
            if self.stage_policy.allow_tcp && !self.validate_console_socket(now_ms) {
                self.finish_poll_turn(now_ms, false);
                return Ok(true);
            }

            let mut activity = self.sync_interface_hardware_addr(now_ms);
            activity |= self.send_udp_beacon();
            if activity {
                let _ = self.poll_smoltcp_once(timestamp, now_ms, "budgeted-tx-only");
            }
            self.tx_only_sent = true;
            self.budgeted_phase = self.budgeted_phase.next();
            self.finish_poll_turn(now_ms, activity);
            return Ok(activity);
        }

        if wifi_host_eapol_blocks_data_path(self.device.bringup_status_label()) {
            let (timestamp, wifi_generation_changed) = self.begin_poll_turn(now_ms);
            if wifi_generation_changed {
                self.finish_poll_turn(now_ms, true);
                return Ok(true);
            }
            if self.stage_policy.allow_tcp && !self.validate_console_socket(now_ms) {
                self.finish_poll_turn(now_ms, false);
                return Ok(true);
            }
            let mut activity = self.sync_interface_hardware_addr(now_ms);
            Self::charge_wifi_host_eapol_budget(budget)?;
            activity |= self.service_wifi_host_eapol_slice_with_limit(
                now_ms,
                CYW43_HOST_EAPOL_BUDGETED_SERVICE_POLLS,
            );
            activity |= self.sync_interface_hardware_addr(now_ms);
            activity |=
                self.service_cyw43_data_pre_poll_burst_budgeted(D::driver_task_contract(), budget);
            if self.sync_wifi_connection_generation(now_ms) {
                // Do not let an inner pre-poll disconnect cross into the DHCP
                // service below during the same budgeted turn.
                activity = true;
                self.finish_poll_turn(now_ms, activity);
                return Ok(activity);
            }
            activity |= self.retry_blocked_cyw43_rx_admission(now_ms);
            if !wifi_host_eapol_blocks_data_path(self.device.bringup_status_label()) {
                if self.budgeted_dhcp_service_required() {
                    Self::charge_dhcp_budget(budget)?;
                    activity |= self.service_budgeted_dhcp_turn(timestamp, now_ms);
                }
                self.budgeted_phase = BudgetedNetPhase::Dhcp;
            }
            self.finish_poll_turn(now_ms, activity);
            return Ok(activity);
        }

        let scheduled_phase = self.budgeted_phase;
        let dhcp_service_required = self.budgeted_dhcp_service_required();
        #[cfg(feature = "kernel")]
        let cyw43_dhcp_preempts_phase = budgeted_cyw43_dhcp_service_preempts_phase(
            D::driver_task_contract(),
            scheduled_phase,
            dhcp_service_required,
        );
        #[cfg(not(feature = "kernel"))]
        let cyw43_dhcp_preempts_phase = false;
        let phase = if cyw43_dhcp_preempts_phase {
            BudgetedNetPhase::Dhcp
        } else {
            scheduled_phase
        };
        let genet_tcp_fast_path = self.budgeted_genet_tcp_fast_path_due();
        let cyw43_tcp_fast_path = self.budgeted_cyw43_tcp_fast_path_due();
        let cyw43_tcp_phase_borrow =
            cyw43_tcp_fast_path && budgeted_cyw43_tcp_phase_borrow_allowed(phase);
        let tcp_phase_borrow = genet_tcp_fast_path || cyw43_tcp_phase_borrow;
        let genet_smoltcp_after_tcp_borrow =
            genet_tcp_fast_path && budgeted_genet_smoltcp_poll_after_tcp_borrow(phase);
        let cyw43_smoltcp_after_tcp_borrow =
            cyw43_tcp_phase_borrow && budgeted_cyw43_smoltcp_poll_after_tcp_borrow(phase);
        #[cfg(feature = "kernel")]
        let cyw43_selftest_tcp_defer = self.budgeted_cyw43_selftest_defers_to_tcp(phase);
        #[cfg(not(feature = "kernel"))]
        let cyw43_selftest_tcp_defer = false;
        match phase {
            BudgetedNetPhase::Interface | BudgetedNetPhase::InterfaceFlush => {
                Self::charge_interface_poll_budget(budget)?;
            }
            BudgetedNetPhase::Dhcp => {
                if cyw43_tcp_phase_borrow && !dhcp_service_required {
                    Self::charge_interface_poll_budget(budget)?;
                } else {
                    Self::charge_dhcp_budget(budget)?;
                }
            }
            BudgetedNetPhase::Tcp => {
                Self::charge_tcp_budget(budget)?;
            }
            BudgetedNetPhase::SelfTest if cyw43_selftest_tcp_defer => {
                Self::charge_tcp_budget(budget)?;
            }
            BudgetedNetPhase::SelfTest => {
                budget.charge_ops(16)?;
                budget.charge_frames(8)?;
                budget.charge_bytes(8 * 1024)?;
            }
        }
        if tcp_phase_borrow && phase != BudgetedNetPhase::Tcp {
            Self::charge_tcp_budget(budget)?;
        }

        let (timestamp, wifi_generation_changed) = self.begin_poll_turn(now_ms);
        if wifi_generation_changed {
            // Keep the recovery-selected Interface phase and do not enter any
            // budgeted smoltcp/DHCP/TCP phase with old-generation frames.
            self.finish_poll_turn(now_ms, true);
            return Ok(true);
        }
        if self.stage_policy.allow_tcp && !self.validate_console_socket(now_ms) {
            self.finish_poll_turn(now_ms, false);
            self.budgeted_phase = if cyw43_dhcp_preempts_phase {
                scheduled_phase
            } else {
                phase.next()
            };
            return Ok(true);
        }

        #[cfg(feature = "kernel")]
        let mut activity = {
            let pre_poll_activity =
                self.service_cyw43_data_pre_poll_burst_budgeted(D::driver_task_contract(), budget);
            let mut activity = pre_poll_activity;
            if self.sync_wifi_connection_generation(now_ms) {
                // sync_wifi_connection_generation resets the phase to
                // Interface. Return before drain/smoltcp and before the normal
                // phase advance can overwrite that recovery fence.
                activity = true;
                self.finish_poll_turn(now_ms, activity);
                return Ok(activity);
            }
            activity |=
                self.drain_cyw43_pre_poll_activity(timestamp, now_ms, budget, pre_poll_activity);
            activity
        };
        #[cfg(not(feature = "kernel"))]
        let mut activity = false;
        activity |= self.sync_interface_hardware_addr(now_ms);
        let host_eapol_blocked =
            wifi_host_eapol_blocks_data_path(self.device.bringup_status_label());
        if host_eapol_blocked {
            Self::charge_wifi_host_eapol_budget(budget)?;
        }
        let generation_before_host_eapol = self.wifi_connection_generation;
        activity |= self.service_wifi_host_eapol_slice_with_limit(
            now_ms,
            if host_eapol_blocked {
                CYW43_HOST_EAPOL_BUDGETED_SERVICE_POLLS
            } else {
                1
            },
        );
        if cyw43_pre_poll_generation_fence_required(
            generation_before_host_eapol,
            self.wifi_connection_generation,
        ) {
            // Host-EAPOL/event service can observe carrier loss after the
            // pre-poll fence above. Preserve sync's Interface phase and end
            // this turn before the scheduled smoltcp/DHCP/TCP phase.
            self.finish_poll_turn(now_ms, true);
            return Ok(true);
        }
        activity |= self.sync_interface_hardware_addr(now_ms);
        activity |= self.retry_blocked_cyw43_rx_admission(now_ms);
        if self.wifi_rx_admission_blocked {
            self.finish_poll_turn(now_ms, activity);
            self.budgeted_phase = if cyw43_dhcp_preempts_phase {
                scheduled_phase
            } else {
                phase.next()
            };
            return Ok(activity);
        }
        if tcp_phase_borrow && phase != BudgetedNetPhase::Tcp {
            activity |= self.service_budgeted_tcp_turn(
                timestamp,
                now_ms,
                if genet_tcp_fast_path {
                    "budgeted-genet-pre-tcp"
                } else {
                    "budgeted-cyw43-pre-tcp"
                },
                if genet_tcp_fast_path {
                    "budgeted-genet-post-tcp"
                } else {
                    "budgeted-cyw43-post-tcp"
                },
            );
        }
        activity |= match phase {
            BudgetedNetPhase::Interface if genet_smoltcp_after_tcp_borrow => {
                self.poll_smoltcp_once(timestamp, now_ms, "budgeted-genet-main-after-tcp-borrow")
            }
            BudgetedNetPhase::Interface if cyw43_smoltcp_after_tcp_borrow => {
                self.poll_smoltcp_once(timestamp, now_ms, "budgeted-cyw43-main-after-tcp-borrow")
            }
            BudgetedNetPhase::Interface if tcp_phase_borrow => false,
            BudgetedNetPhase::Interface => {
                self.poll_smoltcp_once(timestamp, now_ms, "budgeted-main")
            }
            BudgetedNetPhase::Dhcp if cyw43_smoltcp_after_tcp_borrow && !dhcp_service_required => {
                self.poll_smoltcp_once(timestamp, now_ms, "budgeted-cyw43-dhcp-after-tcp-borrow")
            }
            BudgetedNetPhase::Dhcp => self.service_budgeted_dhcp_turn(timestamp, now_ms),
            BudgetedNetPhase::Tcp => self.service_budgeted_tcp_turn(
                timestamp,
                now_ms,
                "budgeted-tcp-pre",
                "budgeted-post-tcp",
            ),
            BudgetedNetPhase::InterfaceFlush if genet_smoltcp_after_tcp_borrow => {
                self.poll_smoltcp_once(timestamp, now_ms, "budgeted-genet-flush-after-tcp-borrow")
            }
            BudgetedNetPhase::InterfaceFlush if cyw43_smoltcp_after_tcp_borrow => {
                self.poll_smoltcp_once(timestamp, now_ms, "budgeted-cyw43-flush-after-tcp-borrow")
            }
            BudgetedNetPhase::InterfaceFlush if tcp_phase_borrow => false,
            BudgetedNetPhase::InterfaceFlush => {
                self.poll_smoltcp_once(timestamp, now_ms, "budgeted-flush")
            }
            BudgetedNetPhase::SelfTest if cyw43_selftest_tcp_defer => self
                .service_budgeted_tcp_turn(
                    timestamp,
                    now_ms,
                    "budgeted-cyw43-selftest-defer-tcp-pre",
                    "budgeted-cyw43-selftest-defer-tcp-post",
                ),
            BudgetedNetPhase::SelfTest => {
                let selftest_activity =
                    self.stage_policy.allow_selftest && self.service_self_test(now_ms, timestamp);
                #[cfg(feature = "net-outbound-probe")]
                {
                    let outbound_activity = self.stage_policy.allow_outbound_probe
                        && self.service_outbound_probe(now_ms, timestamp);
                    selftest_activity || outbound_activity
                }
                #[cfg(not(feature = "net-outbound-probe"))]
                {
                    selftest_activity
                }
            }
        };
        if genet_tcp_fast_path && phase == BudgetedNetPhase::Tcp && activity {
            for _ in 0..GENET_TCP_FAST_PATH_EXTRA_TURNS {
                if Self::charge_tcp_budget(budget).is_err() {
                    break;
                }
                let turn_activity = self.service_budgeted_tcp_turn(
                    timestamp,
                    now_ms,
                    "budgeted-genet-extra-tcp",
                    "budgeted-genet-extra-flush",
                );
                if !turn_activity {
                    break;
                }
                activity = true;
            }
        }

        self.budgeted_phase = if cyw43_dhcp_preempts_phase {
            scheduled_phase
        } else {
            phase.next()
        };
        self.finish_poll_turn(now_ms, activity);
        Ok(activity)
    }

    fn service_wifi_host_eapol_slice(&mut self, now_ms: u64) -> bool {
        self.service_wifi_host_eapol_slice_with_limit(
            now_ms,
            wifi_host_eapol_stack_service_polls(self.device.bringup_status_label()),
        )
    }

    fn service_wifi_host_eapol_slice_with_limit(&mut self, now_ms: u64, poll_limit: usize) -> bool {
        #[cfg(feature = "kernel")]
        {
            if D::driver_task_contract() != crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT
            {
                return false;
            }
            let canonical_owner = crate::drivers::driver_task_net::cyw43_canonical_policy_owner();
            let defer_association = matches!(
                canonical_owner,
                Some(
                    crate::drivers::driver_task_net::Cyw43CanonicalPolicyOwner::HostPolicy
                        | crate::drivers::driver_task_net::Cyw43CanonicalPolicyOwner::NetData
                )
            );
            let association = (!defer_association).then(|| {
                self.wifi_association_supervisor
                    .service(self.wifi_credentials, now_ms)
            });
            let mut activity = association.is_some_and(|outcome| outcome.activity);
            let association_claimed =
                association.is_some_and(|outcome| outcome.claimed_runtime_turn);
            let host_policy_turn = matches!(
                canonical_owner,
                Some(crate::drivers::driver_task_net::Cyw43CanonicalPolicyOwner::HostPolicy)
            );
            let host_eapol_allowed =
                host_policy_turn || association.is_some_and(|outcome| outcome.host_eapol_allowed);
            if !association_claimed && host_eapol_allowed {
                if let Some(credentials) = self.wifi_credentials {
                    activity |= crate::drivers::driver_task_net::service_cyw43_host_eapol_slice(
                        credentials,
                        poll_limit,
                        now_ms,
                    );
                }
            }
            // Association recovery and carrier-loss events can advance the
            // CYW43 connection generation inside this service call.  Fence
            // DHCP, neighbor, and TCP state before the caller can finish the
            // same smoltcp turn with old-generation network state.
            activity |= self.sync_wifi_connection_generation(now_ms);
            return activity;
        }
        #[cfg(not(feature = "kernel"))]
        {
            false
        }
    }

    fn wifi_association_claims_runtime_turn(&self, now_ms: u64) -> bool {
        #[cfg(feature = "kernel")]
        {
            if D::driver_task_contract() != crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT
            {
                return false;
            }
            if matches!(
                crate::drivers::driver_task_net::cyw43_canonical_policy_owner(),
                Some(
                    crate::drivers::driver_task_net::Cyw43CanonicalPolicyOwner::HostPolicy
                        | crate::drivers::driver_task_net::Cyw43CanonicalPolicyOwner::NetData
                )
            ) {
                return false;
            }
            return self
                .wifi_association_supervisor
                .claims_runtime_turn(self.wifi_credentials, now_ms);
        }
        #[cfg(not(feature = "kernel"))]
        {
            let _ = now_ms;
            false
        }
    }

    fn bump_poll_counter(&mut self) {
        self.counters.smoltcp_polls = self.counters.smoltcp_polls.saturating_add(1);
        NET_DIAG.record_poll_call();
    }

    fn service_dhcp(&mut self, now_ms: u64) -> bool {
        let Some(handle) = self.dhcp_handle else {
            return false;
        };
        if !self.dhcp_started {
            return false;
        }
        let Some(mut client) = self.dhcp.take() else {
            return false;
        };
        let mut activity = false;
        let mac = self.device.mac().0;
        let mut rx_packet = [0u8; DHCP_PAYLOAD_CAPACITY];
        let mut packets = 0usize;

        loop {
            if packets >= MAX_DHCP_RX_PACKETS_PER_POLL {
                break;
            }
            let recv = {
                let socket = self.sockets.get_mut::<UdpSocket>(handle);
                match socket.recv() {
                    Ok((data, _endpoint)) => {
                        let len = core::cmp::min(data.len(), rx_packet.len());
                        rx_packet[..len].copy_from_slice(&data[..len]);
                        Some(len)
                    }
                    Err(UdpRecvError::Exhausted) => None,
                    Err(UdpRecvError::Truncated) => {
                        self.counters.udp_rx = self.counters.udp_rx.saturating_add(1);
                        client.handle_packet(mac, &[], now_ms);
                        None
                    }
                }
            };
            let Some(len) = recv else {
                break;
            };
            packets = packets.saturating_add(1);
            self.counters.udp_rx = self.counters.udp_rx.saturating_add(1);
            let before_status = client.status();
            let event = client.handle_packet(mac, &rx_packet[..len], now_ms);
            let after_status = client.status();
            match event {
                DhcpEvent::SendQueued => info!(
                    "[dhcp] rx transition from={} to={} action=send-queued len={} attempts={} rx_packets={} invalid={}",
                    before_status.phase.as_str(),
                    after_status.phase.as_str(),
                    len,
                    after_status.attempts,
                    after_status.metrics.rx_packets,
                    after_status.metrics.invalid_packets,
                ),
                DhcpEvent::LeaseAcquired(lease) => info!(
                    "[dhcp] rx ack ip={}.{}.{}.{} phase={} len={} rx_packets={}",
                    lease.ip[0],
                    lease.ip[1],
                    lease.ip[2],
                    lease.ip[3],
                    after_status.phase.as_str(),
                    len,
                    after_status.metrics.rx_packets,
                ),
                DhcpEvent::Failed(reason) => warn!(
                    "[dhcp] rx failed reason={} from={} to={} len={} rx_packets={}",
                    reason.as_str(),
                    before_status.phase.as_str(),
                    after_status.phase.as_str(),
                    len,
                    after_status.metrics.rx_packets,
                ),
                DhcpEvent::None
                    if after_status.metrics.invalid_packets
                        != before_status.metrics.invalid_packets =>
                {
                    warn!(
                        "[dhcp] rx ignored reason=invalid phase={} len={} invalid={}",
                        after_status.phase.as_str(),
                        len,
                        after_status.metrics.invalid_packets,
                    );
                }
                DhcpEvent::None => {}
            }
            activity |= self.apply_dhcp_event(event, now_ms);
        }

        let timer_event = client.on_timer(now_ms);
        if let DhcpEvent::Failed(reason) = timer_event {
            warn!("[dhcp] timer failure reason={}", reason.as_str());
        }
        activity |= self.apply_dhcp_event(timer_event, now_ms);

        if let Some(socket) = self.dhcp_handle {
            let mut tx_packet = [0u8; DHCP_PAYLOAD_CAPACITY];
            let can_send = self.sockets.get::<UdpSocket>(socket).can_send();
            if can_send {
                let before_status = client.status();
                match client.build_outbound(mac, &mut tx_packet, now_ms) {
                    Ok(Some(len)) => {
                        let endpoint =
                            IpEndpoint::new(Ipv4Address::BROADCAST.into(), DHCP_SERVER_PORT);
                        let send_result = self
                            .sockets
                            .get_mut::<UdpSocket>(socket)
                            .send_slice(&tx_packet[..len], endpoint);
                        match send_result {
                            Ok(()) => {
                                self.counters.udp_tx = self.counters.udp_tx.saturating_add(1);
                                let after_status = client.status();
                                let kind = match before_status.phase {
                                    DhcpPhase::Selecting => "discover",
                                    DhcpPhase::Requesting => "request",
                                    _ => "unknown",
                                };
                                info!(
                                    "[dhcp] tx queued kind={} from={} to={} len={} attempts={} tx_packets={}",
                                    kind,
                                    before_status.phase.as_str(),
                                    after_status.phase.as_str(),
                                    len,
                                    after_status.attempts,
                                    after_status.metrics.tx_packets,
                                );
                                activity = true;
                            }
                            Err(UdpSendError::BufferFull) => {}
                            Err(UdpSendError::Unaddressable) => {
                                warn!("[dhcp] send failed: unaddressable");
                            }
                        }
                    }
                    Ok(None) => {}
                    Err(reason) => {
                        warn!("[dhcp] encode failed reason={}", reason.as_str());
                        activity = true;
                    }
                }
            }
        }

        self.dhcp = Some(client);
        activity
    }

    fn cyw43_dhcp_start_defer_reason(&mut self, now_ms: u64) -> Option<&'static str> {
        #[cfg(feature = "kernel")]
        {
            if D::driver_task_contract() != crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT
            {
                return None;
            }
            let counters = self.device.counters();
            let settle = cyw43_dhcp_post_secure_eapol_settle(
                now_ms,
                counters.wifi_host_eapol_secure,
                counters.wifi_host_eapol_rx,
                &mut self.wifi_dhcp_last_eapol_rx,
                &mut self.wifi_dhcp_eapol_quiet_since_ms,
            );
            if settle.ready {
                if let Some(next_ready_ms) = settle.next_ready_ms {
                    let overshoot_ms = now_ms.saturating_sub(next_ready_ms);
                    if overshoot_ms >= CYW43_DHCP_POST_SECURE_EAPOL_OVERSHOOT_LOG_MS {
                        info!(
                            "[dhcp] start settle ready reason=wifi-post-secure-eapol-settle eapol_rx={} quiet_ms={} target_ms={} overshoot_ms={} now_ms={}",
                            counters.wifi_host_eapol_rx,
                            settle.quiet_ms,
                            CYW43_DHCP_POST_SECURE_EAPOL_QUIET_MS,
                            overshoot_ms,
                            now_ms
                        );
                    }
                }
                self.wifi_dhcp_eapol_settle_logged = false;
                return None;
            }
            if counters.wifi_host_eapol_secure != 0 && counters.wifi_host_eapol_rx != 0 {
                if settle.changed || !self.wifi_dhcp_eapol_settle_logged {
                    info!(
                        "[dhcp] start deferred reason=wifi-post-secure-eapol-settle eapol_secure={} eapol_rx={} changed={} quiet_ms={} target_ms={} remaining_ms={} next_ready_ms={} now_ms={}",
                        counters.wifi_host_eapol_secure,
                        counters.wifi_host_eapol_rx,
                        settle.changed,
                        settle.quiet_ms,
                        CYW43_DHCP_POST_SECURE_EAPOL_QUIET_MS,
                        settle.remaining_ms,
                        settle.next_ready_ms.unwrap_or(0),
                        now_ms
                    );
                    self.wifi_dhcp_eapol_settle_logged = true;
                }
                return Some("wifi-post-secure-eapol-settle");
            }
        }
        #[cfg(not(feature = "kernel"))]
        {
            let _ = now_ms;
        }
        None
    }

    fn start_dhcp_if_ready(&mut self, now_ms: u64) -> bool {
        if self.dhcp_started || self.dhcp_handle.is_none() {
            return false;
        }
        if let Some(restart_after_ms) = self.dhcp_restart_after_ms {
            if now_ms < restart_after_ms {
                return false;
            }
            self.dhcp_restart_after_ms = None;
        }
        if let Some(status) = dhcp_start_defer_reason_for(self.device.bringup_status_label()) {
            log::debug!(
                "[dhcp] start deferred reason=device-bringup status={} now_ms={}",
                status,
                now_ms
            );
            return false;
        }
        #[cfg(feature = "kernel")]
        if matches!(
            self.device.bringup_status_label(),
            Some("wifi-data-rx-admission-blocked")
        ) && !self.reassert_cyw43_dhcp_rx_admission("start", now_ms)
        {
            return false;
        }
        if let Some(status) = self.cyw43_dhcp_start_defer_reason(now_ms) {
            log::debug!(
                "[dhcp] start deferred reason=device-settle status={} now_ms={}",
                status,
                now_ms
            );
            return false;
        }
        #[cfg(feature = "kernel")]
        if !self.reassert_cyw43_dhcp_rx_admission("start", now_ms) {
            return false;
        }
        let Some(client) = self.dhcp.as_mut() else {
            return false;
        };
        client.start(self.device.mac().0, now_ms, self.wifi_connection_generation);
        self.dhcp_started = true;
        self.dhcp_restart_after_ms = None;
        self.wifi_dhcp_eapol_settle_logged = false;
        #[cfg(feature = "kernel")]
        if D::driver_task_contract() == CYW43_WIFI_DRIVER_TASK_CONTRACT {
            record_cyw43_oldgood_dhcp_start(
                self.wifi_connection_generation,
                client.transaction_id(),
                now_ms,
            );
        }
        info!(
            "[dhcp] start ready interface={} generation={} xid=0x{:08x} now_ms={}",
            self.device.interface_label(),
            self.wifi_connection_generation,
            client.transaction_id(),
            now_ms
        );
        true
    }

    fn apply_dhcp_event(&mut self, event: DhcpEvent, now_ms: u64) -> bool {
        match event {
            DhcpEvent::None => false,
            DhcpEvent::SendQueued => true,
            DhcpEvent::LeaseAcquired(lease) => {
                #[cfg(feature = "kernel")]
                let admission_ready = self.reassert_cyw43_dhcp_rx_admission("lease-bound", now_ms);
                self.apply_dhcp_lease(lease, now_ms);
                #[cfg(feature = "kernel")]
                if !admission_ready {
                    warn!(
                        "[dhcp] lease retained while TCP listener is deferred reason=wifi-data-rx-admission-blocked now_ms={}",
                        now_ms
                    );
                }
                true
            }
            DhcpEvent::Failed(reason) => {
                let restart_after_ms = now_ms.saturating_add(DHCP_RESTART_BACKOFF_MS);
                warn!(
                    "[dhcp] failed reason={} action=restart-armed restart_after_ms={}",
                    reason.as_str(),
                    restart_after_ms
                );
                self.dhcp_started = false;
                #[cfg(feature = "kernel")]
                if D::driver_task_contract() == CYW43_WIFI_DRIVER_TASK_CONTRACT {
                    clear_cyw43_oldgood_dhcp_receipt();
                }
                self.dhcp_restart_after_ms = Some(restart_after_ms);
                true
            }
        }
    }

    fn apply_dhcp_lease(&mut self, lease: DhcpLease, now_ms: u64) {
        self.dhcp_restart_after_ms = None;
        #[cfg(feature = "kernel")]
        if D::driver_task_contract() == CYW43_WIFI_DRIVER_TASK_CONTRACT {
            record_cyw43_oldgood_dhcp_bound(self.wifi_connection_generation, &lease);
        }
        let ip = Ipv4Address::new(lease.ip[0], lease.ip[1], lease.ip[2], lease.ip[3]);
        let gateway = lease
            .gateway
            .map(|value| Ipv4Address::new(value[0], value[1], value[2], value[3]));
        if self.ip != ip || self.prefix_len != lease.prefix_len || self.gateway != gateway {
            self.reset_icmp_echo_socket("dhcp-address-change", now_ms);
        }
        self.ip = ip;
        self.prefix_len = lease.prefix_len;
        self.gateway = gateway;
        self.device.set_assigned_ipv4(ip);
        self.interface.update_ip_addrs(|addrs| {
            let cidr = IpCidr::new(IpAddress::from(ip), lease.prefix_len);
            set_primary_ipv4_addr!(addrs, cidr);
        });
        let _ = self.interface.routes_mut().remove_default_ipv4_route();
        if let Some(gw) = gateway {
            let _ = self.interface.routes_mut().add_default_ipv4_route(gw);
        }
        info!(
            "[dhcp] lease bound generation={} ip={}/{} gateway={} server={}.{}.{}.{} lease_s={}",
            self.wifi_connection_generation,
            ip,
            lease.prefix_len,
            gateway.unwrap_or(Ipv4Address::UNSPECIFIED),
            lease.server_id[0],
            lease.server_id[1],
            lease.server_id[2],
            lease.server_id[3],
            lease.lease_seconds
        );
    }

    fn sync_device_counters(&mut self) {
        let device_counters = self.device.counters();
        self.counters.rx_packets = device_counters.rx_packets;
        self.counters.tx_packets = device_counters.tx_packets;
        self.counters.rx_used_advances = device_counters.rx_used_advances;
        self.counters.tx_used_advances = device_counters.tx_used_advances;
        self.counters.tx_submit = device_counters.tx_submit;
        self.counters.tx_complete = device_counters.tx_complete;
        self.counters.tx_free = device_counters.tx_free;
        self.counters.tx_in_flight = device_counters.tx_in_flight;
        self.counters.tx_double_submit = device_counters.tx_double_submit;
        self.counters.tx_zero_len_attempt = device_counters.tx_zero_len_attempt;
        self.counters.arp_rx = device_counters.arp_rx;
        self.counters.arp_tx = device_counters.arp_tx;
        self.counters.driver_rx_last_len = device_counters.driver_rx_last_len;
        self.counters.driver_rx_last_ethertype = device_counters.driver_rx_last_ethertype;
        self.counters.genet_rx_runtime_queue_count = device_counters.genet_rx_runtime_queue_count;
        self.counters.genet_rx_runtime_queue_high_water =
            device_counters.genet_rx_runtime_queue_high_water;
        self.counters.genet_rx_runtime_queue_overflow_seen =
            device_counters.genet_rx_runtime_queue_overflow_seen;
        self.counters.genet_rx_runtime_drain_budget_hit =
            device_counters.genet_rx_runtime_drain_budget_hit;
        self.counters.genet_rx_runtime_byte_budget_hit =
            device_counters.genet_rx_runtime_byte_budget_hit;
        self.counters.genet_rx_runtime_max_drained_per_turn =
            device_counters.genet_rx_runtime_max_drained_per_turn;
        self.counters.genet_rx_pending_queue_count = device_counters.genet_rx_pending_queue_count;
        self.counters.genet_rx_pending_queue_high_water =
            device_counters.genet_rx_pending_queue_high_water;
        self.counters.genet_rx_pending_drops = device_counters.genet_rx_pending_drops;
        self.counters.wifi_rx_pending_queue_count = device_counters.wifi_rx_pending_queue_count;
        self.counters.wifi_rx_pending_queue_high_water =
            device_counters.wifi_rx_pending_queue_high_water;
        self.counters.wifi_rx_pending_drops = device_counters.wifi_rx_pending_drops;
        self.counters.wifi_rx_pending_drops_boot = device_counters.wifi_rx_pending_drops;
        self.counters.wifi_rx_runtime_queue_count = device_counters.wifi_rx_runtime_queue_count;
        self.counters.wifi_rx_runtime_queue_high_water =
            device_counters.wifi_rx_runtime_queue_high_water;
        self.counters.wifi_rx_runtime_queue_overflow_seen =
            device_counters.wifi_rx_runtime_queue_overflow_seen;
        self.counters.wifi_rx_runtime_overflow_episodes =
            device_counters.wifi_rx_runtime_overflow_episodes;
        self.counters.wifi_rx_runtime_overflow_episodes_boot =
            device_counters.wifi_rx_runtime_overflow_episodes;
        self.counters.wifi_rx_runtime_drain_budget_hit =
            device_counters.wifi_rx_runtime_drain_budget_hit;
        self.counters.wifi_rx_runtime_max_drained_per_turn =
            device_counters.wifi_rx_runtime_max_drained_per_turn;
        self.counters.wifi_service_last_op = device_counters.wifi_service_last_op;
        self.counters.wifi_service_last_reason = device_counters.wifi_service_last_reason;
        self.counters.wifi_service_last_progress = device_counters.wifi_service_last_progress;
        self.counters.wifi_service_last_seq_window = device_counters.wifi_service_last_seq_window;
        self.counters.wifi_service_last_channel = device_counters.wifi_service_last_channel;
        self.counters.wifi_service_last_credit_observations =
            device_counters.wifi_service_last_credit_observations;
        self.counters.wifi_service_last_rframe_len = device_counters.wifi_service_last_rframe_len;
        self.counters.wifi_service_last_source_flags =
            device_counters.wifi_service_last_source_flags;
        self.counters.wifi_service_last_pre_source = device_counters.wifi_service_last_pre_source;
        self.counters.wifi_service_last_post_source = device_counters.wifi_service_last_post_source;
        self.counters.wifi_data_trace_faults = device_counters.wifi_data_trace_faults;
        self.counters.wifi_data_trace_tx_retries = device_counters.wifi_data_trace_tx_retries;
        self.counters.wifi_arp_target_hw_zeroed = device_counters.wifi_arp_target_hw_zeroed;
        self.counters.wifi_post_dhcp_rx_any = device_counters.wifi_post_dhcp_rx_any;
        self.counters.wifi_post_dhcp_rx_unicast = device_counters.wifi_post_dhcp_rx_unicast;
        self.counters.wifi_post_dhcp_rx_arp = device_counters.wifi_post_dhcp_rx_arp;
        self.counters.wifi_post_dhcp_rx_ipv4 = device_counters.wifi_post_dhcp_rx_ipv4;
        self.counters.wifi_post_dhcp_rx_icmp = device_counters.wifi_post_dhcp_rx_icmp;
        self.counters.wifi_post_dhcp_rx_tcp = device_counters.wifi_post_dhcp_rx_tcp;
        self.counters.wifi_post_dhcp_rx_last_len = device_counters.wifi_post_dhcp_rx_last_len;
        self.counters.wifi_post_dhcp_rx_last_ethertype =
            device_counters.wifi_post_dhcp_rx_last_ethertype;
        self.counters.dropped_zero_len_tx = device_counters.dropped_zero_len_tx;
        self.counters.wifi_assoc = device_counters.wifi_assoc;
        self.counters.wifi_connection_generation = u64::from(wifi_connection_generation_for::<D>());
        self.counters.wifi_link_up = device_counters.wifi_link_up;
        self.counters.wifi_host_eapol_rx = device_counters.wifi_host_eapol_rx;
        self.counters.wifi_host_eapol_start = device_counters.wifi_host_eapol_start;
        self.counters.wifi_host_eapol_secure = device_counters.wifi_host_eapol_secure;
        self.counters.wifi_host_eapol_m1 = device_counters.wifi_host_eapol_m1;
        self.counters.wifi_host_eapol_m2 = device_counters.wifi_host_eapol_m2;
        self.counters.wifi_host_eapol_m3 = device_counters.wifi_host_eapol_m3;
        self.counters.wifi_host_eapol_m4 = device_counters.wifi_host_eapol_m4;
        self.counters.wifi_host_eapol_ptk = device_counters.wifi_host_eapol_ptk;
        self.counters.wifi_host_eapol_gtk = device_counters.wifi_host_eapol_gtk;
    }

    fn current_counters_unprojected(&self) -> NetCounters {
        let device_counters = self.device.counters();
        let counters = NetCounters {
            rx_packets: device_counters.rx_packets,
            tx_packets: device_counters.tx_packets,
            rx_used_advances: device_counters.rx_used_advances,
            tx_used_advances: device_counters.tx_used_advances,
            smoltcp_polls: self.counters.smoltcp_polls,
            udp_rx: self.counters.udp_rx,
            udp_tx: self.counters.udp_tx,
            tcp_accepts: self.counters.tcp_accepts,
            tcp_auth_sessions: self.counters.tcp_auth_sessions,
            tcp_rx_bytes: self.counters.tcp_rx_bytes,
            tcp_console_recv_ready: self.counters.tcp_console_recv_ready,
            tcp_console_recv_budget_hits: self.counters.tcp_console_recv_budget_hits,
            tcp_tx_bytes: self.counters.tcp_tx_bytes,
            tcp_smoke_outbound: self.counters.tcp_smoke_outbound,
            tcp_smoke_outbound_failures: self.counters.tcp_smoke_outbound_failures,
            tx_submit: device_counters.tx_submit,
            tx_complete: device_counters.tx_complete,
            tx_free: device_counters.tx_free,
            tx_in_flight: device_counters.tx_in_flight,
            tx_double_submit: device_counters.tx_double_submit,
            tx_zero_len_attempt: device_counters.tx_zero_len_attempt,
            arp_rx: device_counters.arp_rx,
            arp_tx: device_counters.arp_tx,
            driver_rx_last_len: device_counters.driver_rx_last_len,
            driver_rx_last_ethertype: device_counters.driver_rx_last_ethertype,
            genet_rx_runtime_queue_count: device_counters.genet_rx_runtime_queue_count,
            genet_rx_runtime_queue_high_water: device_counters.genet_rx_runtime_queue_high_water,
            genet_rx_runtime_queue_overflow_seen: device_counters
                .genet_rx_runtime_queue_overflow_seen,
            genet_rx_runtime_drain_budget_hit: device_counters.genet_rx_runtime_drain_budget_hit,
            genet_rx_runtime_byte_budget_hit: device_counters.genet_rx_runtime_byte_budget_hit,
            genet_rx_runtime_max_drained_per_turn: device_counters
                .genet_rx_runtime_max_drained_per_turn,
            genet_rx_pending_queue_count: device_counters.genet_rx_pending_queue_count,
            genet_rx_pending_queue_high_water: device_counters.genet_rx_pending_queue_high_water,
            genet_rx_pending_drops: device_counters.genet_rx_pending_drops,
            wifi_rx_pending_queue_count: device_counters.wifi_rx_pending_queue_count,
            wifi_rx_pending_queue_high_water: device_counters.wifi_rx_pending_queue_high_water,
            wifi_rx_pending_drops: device_counters.wifi_rx_pending_drops,
            wifi_rx_pending_drops_boot: device_counters.wifi_rx_pending_drops,
            wifi_rx_runtime_queue_count: device_counters.wifi_rx_runtime_queue_count,
            wifi_rx_runtime_queue_high_water: device_counters.wifi_rx_runtime_queue_high_water,
            wifi_rx_runtime_queue_overflow_seen: device_counters
                .wifi_rx_runtime_queue_overflow_seen,
            wifi_rx_runtime_overflow_episodes: device_counters.wifi_rx_runtime_overflow_episodes,
            wifi_rx_runtime_overflow_episodes_boot: device_counters
                .wifi_rx_runtime_overflow_episodes,
            wifi_rx_runtime_drain_budget_hit: device_counters.wifi_rx_runtime_drain_budget_hit,
            wifi_rx_runtime_max_drained_per_turn: device_counters
                .wifi_rx_runtime_max_drained_per_turn,
            wifi_service_last_op: device_counters.wifi_service_last_op,
            wifi_service_last_reason: device_counters.wifi_service_last_reason,
            wifi_service_last_progress: device_counters.wifi_service_last_progress,
            wifi_service_last_seq_window: device_counters.wifi_service_last_seq_window,
            wifi_service_last_channel: device_counters.wifi_service_last_channel,
            wifi_service_last_credit_observations: device_counters
                .wifi_service_last_credit_observations,
            wifi_service_last_rframe_len: device_counters.wifi_service_last_rframe_len,
            wifi_service_last_source_flags: device_counters.wifi_service_last_source_flags,
            wifi_service_last_pre_source: device_counters.wifi_service_last_pre_source,
            wifi_service_last_post_source: device_counters.wifi_service_last_post_source,
            wifi_data_trace_faults: device_counters.wifi_data_trace_faults,
            wifi_data_trace_tx_retries: device_counters.wifi_data_trace_tx_retries,
            wifi_arp_target_hw_zeroed: device_counters.wifi_arp_target_hw_zeroed,
            wifi_post_dhcp_rx_any: device_counters.wifi_post_dhcp_rx_any,
            wifi_post_dhcp_rx_unicast: device_counters.wifi_post_dhcp_rx_unicast,
            wifi_post_dhcp_rx_arp: device_counters.wifi_post_dhcp_rx_arp,
            wifi_post_dhcp_rx_ipv4: device_counters.wifi_post_dhcp_rx_ipv4,
            wifi_post_dhcp_rx_icmp: device_counters.wifi_post_dhcp_rx_icmp,
            wifi_post_dhcp_rx_tcp: device_counters.wifi_post_dhcp_rx_tcp,
            wifi_post_dhcp_rx_last_len: device_counters.wifi_post_dhcp_rx_last_len,
            wifi_post_dhcp_rx_last_ethertype: device_counters.wifi_post_dhcp_rx_last_ethertype,
            dropped_zero_len_tx: device_counters.dropped_zero_len_tx,
            wifi_assoc: device_counters.wifi_assoc,
            wifi_connection_generation: u64::from(wifi_connection_generation_for::<D>()),
            wifi_link_up: device_counters.wifi_link_up,
            wifi_host_eapol_rx: device_counters.wifi_host_eapol_rx,
            wifi_host_eapol_start: device_counters.wifi_host_eapol_start,
            wifi_host_eapol_secure: device_counters.wifi_host_eapol_secure,
            wifi_host_eapol_m1: device_counters.wifi_host_eapol_m1,
            wifi_host_eapol_m2: device_counters.wifi_host_eapol_m2,
            wifi_host_eapol_m3: device_counters.wifi_host_eapol_m3,
            wifi_host_eapol_m4: device_counters.wifi_host_eapol_m4,
            wifi_host_eapol_ptk: device_counters.wifi_host_eapol_ptk,
            wifi_host_eapol_gtk: device_counters.wifi_host_eapol_gtk,
        };
        counters
    }

    fn current_counters(&self) -> NetCounters {
        self.cyw43_generation_proof_baseline.project(
            wifi_connection_generation_for::<D>(),
            self.current_counters_unprojected(),
        )
    }

    fn log_self_test_result(&self, result: NetSelfTestResult) {
        let counters = self.current_counters();
        info!(
            "[net-selftest] result generation={} run_generation={} tx_ok={} udp_echo_ok={} tcp_ok={} console_ok={} peer_assisted_ok={} result={}",
            self.wifi_connection_generation,
            self.self_test.run_generation,
            result.tx_ok,
            result.udp_echo_ok,
            result.tcp_ok,
            result.console_ok,
            result.peer_assisted_ok,
            result.verdict(),
        );
        if !result.udp_echo_ok {
            match self.self_test.udp_last_peer {
                Some(peer) if result.peer_assisted_ok => info!(
                    "[net-selftest] udp-echo peer-assisted summary rx_pkts={} reply_pkts={} last_peer={}:{}",
                    self.self_test.udp_rx_packets,
                    self.self_test.udp_reply_packets,
                    peer.addr,
                    peer.port
                ),
                Some(peer) => warn!(
                    "[net-selftest] udp-echo summary rx_pkts={} reply_pkts={} last_peer={}:{}",
                    self.self_test.udp_rx_packets,
                    self.self_test.udp_reply_packets,
                    peer.addr,
                    peer.port
                ),
                None if result.peer_assisted_ok => info!(
                    "[net-selftest] udp-echo peer-assisted summary rx_pkts={} reply_pkts={} last_peer=none",
                    self.self_test.udp_rx_packets, self.self_test.udp_reply_packets
                ),
                None => warn!(
                    "[net-selftest] udp-echo summary rx_pkts={} reply_pkts={} last_peer=none",
                    self.self_test.udp_rx_packets, self.self_test.udp_reply_packets
                ),
            }
        }
        if let Some(hint) = self_test_failure_hint(result, counters) {
            info!("{hint}");
        }
    }

    fn enforce_tx_invariants(&mut self) {
        if !self.self_test.running {
            return;
        }
        let counters = self.device.counters();
        if counters.tx_dup_publish_blocked == 0
            && counters.tx_invalid_used_state == 0
            && counters.tx_alloc_blocked_inflight == 0
        {
            return;
        }
        if !self.self_test.tx_invariant_failed {
            self.self_test.tx_invariant_failed = true;
            error!(
                "[net-selftest] tx invariant violation: dup_publish_blocked={} invalid_used_state={} alloc_blocked_inflight={} dup_used_ignored={}",
                counters.tx_dup_publish_blocked,
                counters.tx_invalid_used_state,
                counters.tx_alloc_blocked_inflight,
                counters.tx_dup_used_ignored,
            );
        }
        let result = NetSelfTestResult {
            tx_ok: false,
            udp_echo_ok: false,
            tcp_ok: false,
            console_ok: false,
            peer_assisted_ok: false,
        };
        self.self_test.udp_echo_ok = false;
        self.self_test.tcp_ok = false;
        self.self_test.console_ok = false;
        self.self_test.running = false;
        self.self_test.last_result = Some(result);
        self.log_self_test_result(result);
    }

    fn service_self_test(&mut self, now_ms: u64, timestamp: Instant) -> bool {
        if !self.self_test.enabled {
            return false;
        }

        let mut counters = self.current_counters();
        if let Some(result) = self.self_test.conclude_if_needed(now_ms, counters) {
            self.log_self_test_result(result);
        }

        let mut activity = false;
        if self.self_test.running
            && now_ms.saturating_sub(self.self_test.last_beacon_ms) >= SELF_TEST_BEACON_INTERVAL_MS
            && now_ms.saturating_sub(self.self_test.started_ms) <= SELF_TEST_BEACON_WINDOW_MS
        {
            activity |= self.send_udp_beacon();
            self.self_test.last_beacon_ms = now_ms;
        }
        if self.self_test.running && self.self_test.burst_remaining > 0 {
            let burst_goal = core::cmp::min(self.self_test.burst_remaining, 8);
            let burst_sent = self.send_udp_beacon_burst(burst_goal);
            if burst_sent > 0 {
                activity = true;
            }
        }

        activity |= self.poll_udp_echo();
        activity |= self.poll_tcp_smoke(now_ms);
        activity |= self.poll_tcp_smoke_outbound(now_ms);

        if activity {
            self.bump_poll_counter();
            let poll_result = self.poll_smoltcp_interface(timestamp);
            if poll_result != PollResult::None {
                self.self_test.post_poll_flush_logs =
                    self.self_test.post_poll_flush_logs.saturating_add(1);
                if self.self_test.post_poll_flush_logs == 1 {
                    info!("[net-selftest] post-selftest poll flushed pending work");
                } else {
                    debug!("[net-selftest] post-selftest poll flushed pending work");
                }
            }
        }

        counters = self.current_counters();
        if let Some(result) = self.self_test.conclude_if_needed(now_ms, counters) {
            self.log_self_test_result(result);
        }

        activity
    }

    #[cfg(feature = "net-outbound-probe")]
    fn log_probe_hint_once(&mut self, port: u16) {
        if self.probe_hint_logged {
            return;
        }
        info!(
            target: "net-probe",
            "[net-probe] host listener hint: nc -lv {port}",
        );
        self.probe_hint_logged = true;
    }

    #[cfg(feature = "net-outbound-probe")]
    fn service_outbound_probe(&mut self, now_ms: u64, timestamp: Instant) -> bool {
        let Some(handle) = self.tcp_probe_handle else {
            return false;
        };
        if !self.service_logged {
            return false;
        }
        if readiness::gate().is_some() {
            return false;
        }
        if self.ip == Ipv4Address::UNSPECIFIED {
            return false;
        }
        if !self.telemetry.link_up {
            return false;
        }
        let socket = self.sockets.get_mut::<TcpSocket>(handle);
        let dest = IpEndpoint::new(self.selftest_gateway_target().into(), TCP_PROBE_PORT);
        let mut activity = false;

        if self.probe_sent {
            if socket.state() != TcpState::Closed {
                socket.close();
                activity = true;
            }
            return activity;
        }

        if matches!(socket.state(), TcpState::Closed) {
            if self.probe_last_attempt_ms != 0
                && now_ms.saturating_sub(self.probe_last_attempt_ms) < TCP_PROBE_RETRY_MS
            {
                return false;
            }
            self.probe_last_attempt_ms = now_ms;
            let local_endpoint = IpListenEndpoint {
                addr: Some(self.ip.into()),
                port: 0,
            };
            drop(socket);
            self.log_probe_hint_once(dest.port);
            let connect_result = {
                let mut cx = self.interface.context();
                let socket = self.sockets.get_mut::<TcpSocket>(handle);
                Self::guarded_connect(socket, &mut cx, dest, local_endpoint, "outbound-probe")
            };
            match connect_result {
                Ok(()) => {
                    self.probe_fail_count = 0;
                    if !self.probe_warned_once {
                        log::info!(
                            target: "net-probe",
                            "[net-probe] outbound connect dest={}:{} now_ms={}",
                            dest.addr,
                            dest.port,
                            now_ms
                        );
                        self.probe_warned_once = true;
                    }
                    activity = true;
                }
                Err(err) => {
                    self.probe_fail_count = self.probe_fail_count.saturating_add(1);
                    let should_log = !self.probe_warned_once
                        || now_ms.saturating_sub(self.probe_last_log_ms) >= 5_000;
                    if should_log {
                        self.probe_last_log_ms = now_ms;
                        self.probe_warned_once = true;
                        log::warn!(
                            target: "net-probe",
                            "[net-probe] connect failed dest={}:{} err={:?} failures={}",
                            dest.addr,
                            dest.port,
                            err,
                            self.probe_fail_count,
                        );
                    }
                }
            }
            return activity;
        }

        if socket.state() == TcpState::Established && socket.can_send() {
            if !self.probe_sent {
                log::info!(
                    target: "net-probe",
                    "[net-probe] established dest={}:{}", dest.addr, dest.port
                );
            }
            match socket.send_slice(TCP_PROBE_PAYLOAD) {
                Ok(sent) => {
                    log::info!(
                        target: "net-probe",
                        "[net-probe] sent payload bytes={} dest={}:{}", sent, dest.addr, dest.port
                    );
                    self.probe_sent = true;
                    socket.close();
                    activity = true;
                }
                Err(err) => {
                    log::warn!(
                        target: "net-probe",
                        "[net-probe] send failed err={:?}",
                        err
                    );
                    socket.close();
                }
            }

            self.bump_poll_counter();
            let poll_result = self.poll_smoltcp_interface(timestamp);
            if poll_result != PollResult::None {
                activity = true;
            }
            return activity;
        }

        if matches!(
            socket.state(),
            TcpState::CloseWait | TcpState::TimeWait | TcpState::LastAck
        ) {
            socket.close();
            activity = true;
        }

        activity
    }

    fn send_udp_beacon(&mut self) -> bool {
        self.send_udp_beacon_internal(1, false) > 0
    }

    fn send_udp_beacon_burst(&mut self, max_packets: u32) -> u32 {
        self.send_udp_beacon_internal(max_packets, true)
    }

    fn send_udp_beacon_internal(&mut self, max_packets: u32, consume_burst: bool) -> u32 {
        let Some(handle) = self.udp_beacon_handle else {
            return 0;
        };
        let gateway_addr = self.selftest_gateway_target();
        let mut sent = 0;
        let mut request_tx_scan = false;
        {
            let socket = self.sockets.get_mut::<UdpSocket>(handle);
            while sent < max_packets {
                if consume_burst && self.self_test.burst_remaining == 0 {
                    break;
                }
                if !socket.can_send() {
                    break;
                }
                let mut payload = HeaplessString::<64>::new();
                let _ = write!(&mut payload, "COHESIX_NET_OK {}", self.self_test.beacon_seq);
                let endpoint = IpEndpoint::new(gateway_addr.into(), UDP_ECHO_PORT);
                match socket.send_slice(payload.as_bytes(), endpoint) {
                    Ok(()) => {
                        self.counters.udp_tx = self.counters.udp_tx.saturating_add(1);
                        self.self_test.beacon_seq = self.self_test.beacon_seq.wrapping_add(1);
                        self.self_test.beacons_sent = self.self_test.beacons_sent.saturating_add(1);
                        self.self_test.udp_beacon_blocked_logs = 0;
                        if consume_burst {
                            self.self_test.burst_remaining =
                                self.self_test.burst_remaining.saturating_sub(1);
                        }
                        if self.self_test.running
                            && self.self_test.beacons_sent >= 8
                            && (self.self_test.beacons_sent & 0xf) == 0
                        {
                            request_tx_scan = true;
                        }
                        if self.self_test.beacons_sent < 8 {
                            info!(
                                "[net-selftest] udp-beacon queued seq={} -> {}:{} payload='{}'",
                                self.self_test.beacon_seq.saturating_sub(1),
                                gateway_addr,
                                UDP_ECHO_PORT,
                                payload
                            );
                        } else {
                            debug!(
                                "[net-selftest] udp-beacon queued seq={} -> {}:{} payload='{}'",
                                self.self_test.beacon_seq.saturating_sub(1),
                                gateway_addr,
                                UDP_ECHO_PORT,
                                payload
                            );
                        }
                        sent = sent.saturating_add(1);
                    }
                    Err(err) => {
                        let buffer_full = matches!(err, UdpSendError::BufferFull);
                        let log_count = match err {
                            UdpSendError::BufferFull => {
                                self.self_test.udp_beacon_blocked_logs =
                                    self.self_test.udp_beacon_blocked_logs.saturating_add(1);
                                self.self_test.udp_beacon_blocked_logs
                            }
                            _ => {
                                self.self_test.udp_beacon_error_logs =
                                    self.self_test.udp_beacon_error_logs.saturating_add(1);
                                self.self_test.udp_beacon_error_logs
                            }
                        };
                        match udp_beacon_send_failure_log_severity(buffer_full, log_count) {
                            SelfTestLogSeverity::Warn => warn!(
                                "[net-selftest] udp-beacon send failed seq={} err={:?}",
                                self.self_test.beacon_seq, err
                            ),
                            SelfTestLogSeverity::Debug => debug!(
                                "[net-selftest] udp-beacon send failed seq={} err={:?}",
                                self.self_test.beacon_seq, err
                            ),
                            SelfTestLogSeverity::Trace => trace!(
                                "[net-selftest] udp-beacon send failed seq={} err={:?}",
                                self.self_test.beacon_seq,
                                err
                            ),
                        }
                        break;
                    }
                }
            }
        }
        if request_tx_scan {
            self.device.debug_scan_tx_avail_duplicates();
            self.enforce_tx_invariants();
        }
        sent
    }

    fn poll_udp_echo(&mut self) -> bool {
        let Some(handle) = self.udp_echo_handle else {
            return false;
        };
        let socket = self.sockets.get_mut::<UdpSocket>(handle);
        let mut activity = false;
        let mut packets = 0usize;
        loop {
            if packets >= MAX_UDP_ECHO_PACKETS_PER_POLL {
                break;
            }
            match socket.recv() {
                Ok((payload, meta)) => {
                    packets = packets.saturating_add(1);
                    let endpoint = meta.endpoint;
                    let mut reply = [0u8; UDP_PAYLOAD_CAPACITY];
                    let prefix = b"ECHO:";
                    let _ = maybe_report_str_write(
                        reply.as_mut_ptr(),
                        prefix.len(),
                        prefix.as_ptr(),
                        prefix.len(),
                        "udp_echo.prefix",
                    );
                    reply[..prefix.len()].copy_from_slice(prefix);
                    let copy_len =
                        core::cmp::min(payload.len(), reply.len().saturating_sub(prefix.len()));
                    let _ = maybe_report_str_write(
                        reply[prefix.len()..].as_mut_ptr(),
                        copy_len,
                        payload.as_ptr(),
                        copy_len,
                        "udp_echo.payload",
                    );
                    reply[prefix.len()..prefix.len() + copy_len]
                        .copy_from_slice(&payload[..copy_len]);
                    let reply_len = prefix.len() + copy_len;
                    self.counters.udp_rx = self.counters.udp_rx.saturating_add(1);
                    if self.self_test.running {
                        self.self_test.record_udp_echo_rx(endpoint);
                    }
                    info!(
                        "[net-selftest] udp-echo rx len={} from {}:{}",
                        payload.len(),
                        endpoint.addr,
                        endpoint.port
                    );
                    match socket.send_slice(&reply[..reply_len], endpoint) {
                        Ok(()) => {
                            self.counters.udp_tx = self.counters.udp_tx.saturating_add(1);
                            if self.self_test.running {
                                self.self_test.record_udp_echo_reply(endpoint);
                            }
                            info!(
                                "[net-selftest] udp-echo tx len={} to {}:{}",
                                reply_len, endpoint.addr, endpoint.port
                            );
                        }
                        Err(err) => {
                            warn!(
                                "[net-selftest] udp-echo send failed len={} err={:?}",
                                reply_len, err
                            );
                        }
                    }
                    activity = true;
                }
                Err(UdpRecvError::Exhausted) => break,
                Err(UdpRecvError::Truncated) => {
                    warn!("[net-selftest] udp-echo truncated packet dropped");
                    break;
                }
            }
        }

        activity
    }

    fn poll_tcp_smoke(&mut self, now_ms: u64) -> bool {
        let Some(handle) = self.tcp_smoke_handle else {
            return false;
        };
        let socket = self.sockets.get_mut::<TcpSocket>(handle);
        if !socket.is_open() {
            let _ = socket.listen(TCP_SMOKE_PORT);
            NET_DIAG.record_listener_bound();
            return false;
        }

        let mut activity = false;
        if socket.state() == TcpState::Established {
            NET_DIAG.record_accept_attempt();
            if !self.self_test.tcp_accept_seen {
                self.self_test.tcp_accept_seen = true;
                self.counters.tcp_accepts = self.counters.tcp_accepts.saturating_add(1);
                info!(
                    "[net-selftest] tcp-smoke accept peer={:?}",
                    socket.remote_endpoint()
                );
            }

            let mut copied = 0usize;
            let mut temp = [0u8; 64];
            let mut chunks = 0usize;
            while socket.can_recv() {
                if chunks >= MAX_TCP_SMOKE_RECV_CHUNKS_PER_POLL {
                    break;
                }
                let recv_result = socket.recv(|data| {
                    let copy_len = core::cmp::min(data.len(), temp.len());
                    let _ = maybe_report_str_write(
                        temp.as_mut_ptr(),
                        copy_len,
                        data.as_ptr(),
                        copy_len,
                        "tcp_smoke.payload",
                    );
                    temp[..copy_len].copy_from_slice(&data[..copy_len]);
                    copied = copy_len;
                    (copy_len, ())
                });
                if recv_result.is_err() || copied == 0 {
                    break;
                }
                self.counters.tcp_rx_bytes =
                    self.counters.tcp_rx_bytes.saturating_add(copied as u64);
                NET_DIAG.add_bytes_read(copied as u64);
                chunks = chunks.saturating_add(1);
                info!(
                    "[net-selftest] tcp-smoke recv bytes={} state={:?}",
                    copied,
                    socket.state()
                );
                activity = true;
            }

            if socket.can_send() && (copied > 0 || !socket.can_recv()) {
                match socket.send_slice(b"ok\n") {
                    Ok(sent) => {
                        self.counters.tcp_tx_bytes =
                            self.counters.tcp_tx_bytes.saturating_add(sent as u64);
                        NET_DIAG.add_bytes_written(sent as u64);
                        NET_DIAG.record_accept_success();
                        self.self_test.record_tcp_ok();
                        info!(
                            "[net-selftest] tcp-smoke reply sent bytes={} close_reason=active",
                            sent
                        );
                        socket.close();
                    }
                    Err(err) => {
                        warn!("[net-selftest] tcp-smoke send failed err={:?}", err);
                    }
                }
            } else if socket.state() == TcpState::CloseWait {
                info!("[net-selftest] tcp-smoke peer closed (now_ms={})", now_ms);
                socket.close();
            }
        }

        if matches!(socket.state(), TcpState::Closed) {
            let _ = socket.listen(TCP_SMOKE_PORT);
            NET_DIAG.record_listener_bound();
        }

        activity
    }

    fn poll_console_listener_selftest(&mut self, handle: SocketHandle, now_ms: u64) -> bool {
        if self.self_test.console_probe_done {
            return false;
        }
        if self.self_test.console_probe_started_ms == 0 {
            self.self_test.console_probe_started_ms = now_ms;
        }

        let socket = self.sockets.get_mut::<TcpSocket>(handle);
        let mut activity = false;
        let dest = IpEndpoint::new(self.ip.into(), self.listen_port);
        if matches!(socket.state(), TcpState::Closed) && !self.self_test.console_probe_banner_seen {
            if now_ms.saturating_sub(self.tcp_smoke_last_attempt_ms) >= CONSOLE_SELFTEST_RETRY_MS {
                self.tcp_smoke_last_attempt_ms = now_ms;
                let local_endpoint = IpListenEndpoint {
                    addr: Some(self.ip.into()),
                    port: TCP_CONSOLE_SELFTEST_LOCAL_PORT,
                };
                let mut cx = self.interface.context();
                match Self::guarded_connect(
                    socket,
                    &mut cx,
                    dest,
                    local_endpoint,
                    "console-selftest",
                ) {
                    Ok(()) => {
                        info!(
                            "[net-selftest] console listener selftest connect -> {}:{} (now_ms={})",
                            dest.addr, dest.port, now_ms
                        );
                        activity = true;
                    }
                    Err(err) => {
                        warn!(
                            "[net-selftest] console listener selftest connect failed err={:?}",
                            err
                        );
                    }
                }
            }
        }

        if socket.state() == TcpState::Established {
            if !self.self_test.console_probe_established {
                self.self_test.console_probe_established = true;
                info!("[net-selftest] console listener selftest established");
            }
            if !self.self_test.console_probe_auth_sent && socket.can_send() {
                let mut line: HeaplessString<DEFAULT_LINE_CAPACITY> = HeaplessString::new();
                if write!(line, "AUTH {}", self.server.auth_token()).is_ok() {
                    let total_len = line.len().saturating_add(4);
                    let mut frame: HeaplessVec<u8, { DEFAULT_LINE_CAPACITY + 4 }> =
                        HeaplessVec::new();
                    if frame
                        .extend_from_slice(&(total_len as u32).to_le_bytes())
                        .is_ok()
                        && frame.extend_from_slice(line.as_bytes()).is_ok()
                    {
                        match socket.send_slice(frame.as_slice()) {
                            Ok(sent) if sent == frame.len() => {
                                self.self_test.console_probe_auth_sent = true;
                                self.counters.tcp_tx_bytes =
                                    self.counters.tcp_tx_bytes.saturating_add(sent as u64);
                                NET_DIAG.add_bytes_written(sent as u64);
                                activity = true;
                            }
                            Ok(_) => {}
                            Err(_) => {}
                        }
                    }
                }
            }
            if socket.can_recv() {
                let mut copied = 0usize;
                let mut temp = [0u8; 64];
                let recv_result = socket.recv(|data| {
                    let len = core::cmp::min(data.len(), temp.len());
                    let _ = maybe_report_str_write(
                        temp.as_mut_ptr(),
                        len,
                        data.as_ptr(),
                        len,
                        "console_selftest.payload",
                    );
                    temp[..len].copy_from_slice(&data[..len]);
                    copied = len;
                    (len, ())
                });
                match recv_result {
                    Ok(()) if copied > 0 => {
                        self.self_test.console_probe_banner_seen = true;
                        if copied >= 4 {
                            let mut len_buf = [0u8; 4];
                            len_buf.copy_from_slice(&temp[..4]);
                            let frame_len = u32::from_le_bytes(len_buf) as usize;
                            if frame_len >= 4 && frame_len <= copied {
                                let payload = &temp[4..frame_len];
                                if payload.starts_with(b"OK AUTH") {
                                    info!("[net-selftest] console listener selftest auth OK");
                                } else if payload.starts_with(b"ERR AUTH") {
                                    warn!("[net-selftest] console listener selftest auth rejected");
                                }
                            }
                        }
                        let preview_len = core::cmp::min(copied, 16);
                        info!(
                            "[net-selftest] console listener banner bytes={} first={:02x?}",
                            copied,
                            &temp[..preview_len]
                        );
                        socket.close();
                        activity = true;
                    }
                    Ok(()) => {}
                    Err(err) => {
                        warn!(
                            "[net-selftest] console listener selftest recv failed err={:?}",
                            err
                        );
                    }
                }
            }
        }

        if self.self_test.console_probe_banner_seen
            && matches!(
                socket.state(),
                TcpState::CloseWait | TcpState::FinWait1 | TcpState::FinWait2 | TcpState::LastAck
            )
        {
            socket.close();
        }

        if self.self_test.console_probe_banner_seen
            && matches!(socket.state(), TcpState::Closed)
            && !self.session_active
            && matches!(self.session_state.last_state, Some(TcpState::Listen))
        {
            self.self_test.console_probe_done = true;
            self.self_test.record_console_ok();
            self.tcp_smoke_last_attempt_ms = now_ms;
            info!(
                "[net-selftest] console listener selftest recovered to listen (now_ms={})",
                now_ms
            );
        }

        let elapsed = now_ms.saturating_sub(self.self_test.console_probe_started_ms);
        if elapsed >= CONSOLE_SELFTEST_RECOVERY_DEADLINE_MS && !self.self_test.console_probe_done {
            warn!(
                "[net-selftest] console listener selftest timed out banner={} established={} state={:?}",
                self.self_test.console_probe_banner_seen,
                self.self_test.console_probe_established,
                socket.state()
            );
            if !matches!(socket.state(), TcpState::Closed) {
                socket.abort();
            }
            self.self_test.console_probe_done = true;
        }

        activity
    }

    fn poll_tcp_smoke_outbound(&mut self, now_ms: u64) -> bool {
        if !self.self_test.running {
            return false;
        }

        let Some(handle) = self.tcp_smoke_out_handle else {
            return false;
        };
        if !self.selftest_outbound_peer_probe_enabled() {
            return false;
        }
        if !self.self_test.console_probe_done {
            return self.poll_console_listener_selftest(handle, now_ms);
        }
        let dest_ip = self.selftest_gateway_target();
        let dest = IpEndpoint::new(dest_ip.into(), TCP_SMOKE_PORT);
        let socket = self.sockets.get_mut::<TcpSocket>(handle);
        let mut activity = false;

        if matches!(socket.state(), TcpState::Closed) {
            if self.tcp_smoke_outbound_connecting && !self.tcp_smoke_outbound_sent {
                self.counters.tcp_smoke_outbound_failures =
                    self.counters.tcp_smoke_outbound_failures.saturating_add(1);
                warn!(
                    "[net-selftest] tcp-smoke outbound closed/reset before establish dest={}:{}",
                    dest.addr, dest.port
                );
                self.tcp_smoke_outbound_connecting = false;
            }
            if now_ms.saturating_sub(self.tcp_smoke_last_attempt_ms) >= 1_000 {
                self.tcp_smoke_last_attempt_ms = now_ms;
                self.tcp_smoke_outbound_sent = false;
                let local_endpoint = IpListenEndpoint {
                    addr: Some(self.ip.into()),
                    port: TCP_SMOKE_OUT_LOCAL_PORT,
                };
                let mut cx = self.interface.context();
                match Self::guarded_connect(
                    socket,
                    &mut cx,
                    dest,
                    local_endpoint,
                    "tcp-smoke-outbound",
                ) {
                    Ok(()) => {
                        self.tcp_smoke_outbound_connecting = true;
                        info!(
                            "[net-selftest] tcp-smoke outbound connect -> {}:{} (now_ms={})",
                            dest.addr, dest.port, now_ms
                        );
                        activity = true;
                    }
                    Err(err) => {
                        self.tcp_smoke_outbound_connecting = false;
                        self.counters.tcp_smoke_outbound_failures =
                            self.counters.tcp_smoke_outbound_failures.saturating_add(1);
                        warn!(
                            "[net-selftest] tcp-smoke outbound connect failed dest={}:{} err={:?}",
                            dest.addr, dest.port, err
                        );
                    }
                }
            }
            return activity;
        }

        if socket.state() == TcpState::Established && !self.tcp_smoke_outbound_sent {
            if socket.can_send() {
                match socket.send_slice(b"hi\n") {
                    Ok(sent) => {
                        self.counters.tcp_tx_bytes =
                            self.counters.tcp_tx_bytes.saturating_add(sent as u64);
                        self.counters.tcp_smoke_outbound =
                            self.counters.tcp_smoke_outbound.saturating_add(1);
                        self.tcp_smoke_outbound_sent = true;
                        self.tcp_smoke_outbound_connecting = false;
                        self.self_test.record_tcp_ok();
                        info!(
                            "[net-selftest] tcp-smoke outbound sent bytes={} dest={}:{}",
                            sent, dest.addr, dest.port
                        );
                        socket.close();
                        activity = true;
                    }
                    Err(err) => {
                        warn!(
                            "[net-selftest] tcp-smoke outbound send failed err={:?}",
                            err
                        );
                    }
                }
            }
        }

        if matches!(
            socket.state(),
            TcpState::CloseWait | TcpState::TimeWait | TcpState::LastAck
        ) && !self.tcp_smoke_outbound_sent
        {
            self.counters.tcp_smoke_outbound_failures =
                self.counters.tcp_smoke_outbound_failures.saturating_add(1);
            self.tcp_smoke_outbound_connecting = false;
            warn!(
                "[net-selftest] tcp-smoke outbound closed without send state={:?}",
                socket.state()
            );
            socket.close();
            activity = true;
        }

        activity
    }

    fn process_tcp(&mut self, now_ms: u64) -> bool {
        let mut activity = false;
        let mut log_closed_conn: Option<(u64, NetConsoleDisconnectReason)> = None;
        let mut record_closed_conn: Option<(u64, NetConsoleDisconnectReason)> = None;
        let mut outbound_pending = self.server.has_outbound();
        let mut reset_session = false;
        let mut reset_tcp_state: Option<TcpState> = None;
        let last_tcp_state;
        let mut allow_flush = true;
        let mut disconnect_entered_this_turn = false;
        if !self.validate_console_socket(now_ms) {
            return true;
        }
        if let Some(reason) = self.console_listener_defer_reason() {
            if self.listener_defer_reason != Some(reason) {
                info!(
                    "[net-console] listener deferred reason={} iface={} mode={} ip={} now_ms={}",
                    reason,
                    self.device.interface_label(),
                    self.mode.as_str(),
                    self.ip,
                    now_ms
                );
                self.listener_defer_reason = Some(reason);
            }
            self.listener_announced = false;
            activity |= self.abort_console_standby("listener-deferred");
            let socket = self.sockets.get_mut::<TcpSocket>(self.tcp_handle);
            if !self.session_active && socket.is_open() {
                socket.abort();
            }
            return activity;
        }
        if let Some(reason) = self.listener_defer_reason.take() {
            info!(
                "[net-console] listener gate open previous_defer={} iface={} mode={} ip={} now_ms={}",
                reason,
                self.device.interface_label(),
                self.mode.as_str(),
                self.ip,
                now_ms
            );
        }
        activity |= self.service_console_standby(now_ms);

        let (snapshot, tcp_state) = {
            let socket = self.sockets.get_mut::<TcpSocket>(self.tcp_handle);
            let peer_changed =
                Self::record_peer_endpoint(&mut self.peer_endpoint, socket.remote_endpoint());

            if !socket.is_open() && self.disconnect_phase == ConsoleDisconnectPhase::Idle {
                let terminal_reason = if socket.state() == TcpState::TimeWait {
                    NetConsoleDisconnectReason::Eof
                } else {
                    NetConsoleDisconnectReason::Reset
                };
                self.peer_endpoint = None;
                reset_session = true;
                if !self.listener_announced {
                    info!(
                        "[cohsh-net] listen tcp 0.0.0.0:{} iface_ip={}",
                        self.listen_port, self.ip
                    );
                }
                match socket.listen(IpListenEndpoint::from(self.listen_port)) {
                    Ok(()) => {
                        NET_DIAG.record_listener_bound();
                        info!(
                            "[net-console] tcp listener bound: port={} iface_ip={}",
                            self.listen_port, self.ip
                        );
                    }
                    Err(err) => {
                        log::error!(
                            "[cohsh-net] listen: tcp/{} failed: {:?}",
                            self.listen_port,
                            err
                        );
                        warn!("[net-console] failed to start TCP console listener: {err}",);
                        return activity;
                    }
                }
                if !self.listener_announced {
                    info!(
                        "[net-console] TCP console listening on 0.0.0.0:{} (iface ip={})",
                        self.listen_port, self.ip
                    );
                    self.listener_announced = true;
                    self.listener_defer_reason = None;
                }
                if self.session_active {
                    self.outbound.reset();
                    self.server.end_session();
                    self.session_active = false;
                    if let Some(conn_id) = self.active_client_id {
                        Self::note_close_reason(&mut log_closed_conn, conn_id, terminal_reason);
                        Self::note_close_reason(&mut record_closed_conn, conn_id, terminal_reason);
                    }
                    self.active_client_id = None;
                }
                reset_tcp_state = Some(socket.state());
            }

            let previous_state = self.session_state.last_state;
            Self::log_tcp_state_change(
                &mut self.session_state,
                socket,
                self.peer_endpoint,
                self.ip,
            );

            if self.session_active && socket.state() == TcpState::Listen {
                Self::log_session_closed(&mut self.session_state, self.peer_endpoint, socket);
                self.outbound.reset();
                self.server.end_session();
                self.session_active = false;
                outbound_pending = false;
                if let Some(conn_id) = self.active_client_id {
                    Self::note_close_reason(
                        &mut log_closed_conn,
                        conn_id,
                        NetConsoleDisconnectReason::Eof,
                    );
                    Self::note_close_reason(
                        &mut record_closed_conn,
                        conn_id,
                        NetConsoleDisconnectReason::Eof,
                    );
                }
                self.active_client_id = None;
                self.peer_endpoint = None;
                Self::set_auth_state(
                    &mut self.auth_state,
                    self.active_client_id,
                    AuthState::Start,
                );
                reset_session = true;
                reset_tcp_state = Some(socket.state());
            }

            if !self.stage_policy.allow_console_io && socket.state() == TcpState::Established {
                activity |= begin_console_disconnect(
                    &mut self.disconnect_phase,
                    &mut self.disconnect_phase_started_ms,
                    &mut self.disconnect_reason,
                    &mut disconnect_entered_this_turn,
                    NetConsoleDisconnectReason::Error,
                );
            }

            let new_established = socket.state() == TcpState::Established
                && self.disconnect_phase == ConsoleDisconnectPhase::Idle
                && (previous_state != Some(TcpState::Established)
                    || !self.session_active
                    || peer_changed);
            if new_established {
                #[cfg(feature = "kernel")]
                Self::clear_cyw43_authenticated_console_peer();
                if self.session_active {
                    if let Some(conn_id) = self.active_client_id {
                        Self::note_close_reason(
                            &mut log_closed_conn,
                            conn_id,
                            NetConsoleDisconnectReason::Reset,
                        );
                        Self::note_close_reason(
                            &mut record_closed_conn,
                            conn_id,
                            NetConsoleDisconnectReason::Reset,
                        );
                    }
                }
                self.outbound.reset();
                self.server.end_session();
                self.session_active = false;
                self.active_client_id = None;
                self.disconnect_phase = ConsoleDisconnectPhase::Idle;
                self.disconnect_phase_started_ms = None;
                self.disconnect_reason = NetConsoleDisconnectReason::Quit;
                NET_DIAG.record_accept_attempt();
                let client_id = self.client_counter.wrapping_add(1);
                self.client_counter = client_id;
                self.active_client_id = Some(client_id);
                self.conn_bytes_read = 0;
                self.conn_bytes_written = 0;
                reset_session = true;
                reset_tcp_state = Some(socket.state());
                let _ =
                    Self::record_peer_endpoint(&mut self.peer_endpoint, socket.remote_endpoint());
                let (peer_label, peer_port) = Self::peer_parts(self.peer_endpoint, socket);
                Self::audit_conn_open(client_id, peer_label.as_str(), peer_port);
                let local_port = socket
                    .local_endpoint()
                    .map(|endpoint| endpoint.port)
                    .unwrap_or(self.listen_port);
                info!(
                    "[cohsh-net] conn new id={} local={}:{} remote={}:{}",
                    client_id, self.ip, local_port, peer_label, peer_port
                );
                let peer = {
                    let mut label = HeaplessString::<32>::new();
                    if FmtWrite::write_fmt(&mut label, format_args!("{peer_label}")).is_ok() {
                        Some(label)
                    } else {
                        None
                    }
                };
                if let Some(endpoint) = socket.remote_endpoint() {
                    info!("[cohsh-net] new TCP client connected from {:?}", endpoint);
                    info!(
                        target: "net-console",
                        "[net-console] conn: accepted from {:?}",
                        endpoint
                    );
                    log::info!(
                        target: "net-console",
                        "[net-console] accept: peer={:?} client_id={}",
                        endpoint,
                        client_id
                    );
                }
                let _ = self.events.push(NetConsoleEvent::Connected {
                    conn_id: client_id,
                    peer,
                });
                Self::trace_conn_new(
                    self.peer_endpoint,
                    IpAddress::Ipv4(self.ip),
                    client_id,
                    socket,
                    self.listen_port,
                );
                if ECHO_MODE {
                    Self::set_auth_state(
                        &mut self.auth_state,
                        self.active_client_id,
                        AuthState::Attached,
                    );
                    self.session_state.logged_first_recv = true;
                    log::info!(
                        "[cohsh-net] conn id={} echo mode enabled; bypassing auth",
                        client_id
                    );
                } else {
                    self.server.begin_session(now_ms, Some(client_id));
                    info!(
                        target: "net-console",
                        "[net-console] auth: waiting for handshake (client_id={})",
                        client_id
                    );
                    Self::set_auth_state(
                        &mut self.auth_state,
                        self.active_client_id,
                        AuthState::WaitingVersion,
                    );
                    info!("[net-console] auth start client={}", client_id);
                    debug!(
                        "[net-console][auth] new connection client={} state={:?}",
                        client_id, self.auth_state
                    );
                    let _ = Self::flush_outbound(
                        &mut self.server,
                        &mut self.outbound,
                        &mut self.telemetry,
                        &mut self.conn_bytes_written,
                        &mut self.counters,
                        socket,
                        now_ms,
                        self.active_client_id,
                        self.auth_state,
                        &mut self.session_state,
                        MAX_CONSOLE_FRAMES_PER_POLL,
                        MAX_CONSOLE_BYTES_PER_POLL,
                    );
                    if activity {
                        debug!(
                            "[net-console][auth] greeting sent client={} state={:?}",
                            client_id, self.auth_state
                        );
                    }
                    Self::set_auth_state(
                        &mut self.auth_state,
                        self.active_client_id,
                        AuthState::AuthRequested,
                    );
                    info!(
                        "[net-console] auth: waiting for client credentials (client_id={})",
                        client_id
                    );
                }
                self.session_active = true;
                self.counters.tcp_accepts = self.counters.tcp_accepts.saturating_add(1);
            }

            if self.disconnect_phase == ConsoleDisconnectPhase::Idle && socket.can_recv() {
                let mut temp = [0u8; TCP_CONSOLE_RECV_CHUNK_BYTES];
                let conn_id = self.active_client_id.unwrap_or(0);
                self.counters.tcp_console_recv_ready =
                    self.counters.tcp_console_recv_ready.saturating_add(1);
                debug!(
                    "[cohsh-net] conn id={} recv-ready state={:?} may_recv={} can_recv={}",
                    conn_id,
                    socket.state(),
                    socket.may_recv(),
                    socket.can_recv()
                );
                log::debug!(
                    target: "cohsh-net",
                    "[tcp] socket can_recv={} may_recv={} state={:?}",
                    socket.can_recv(),
                    socket.may_recv(),
                    socket.state()
                );
                let mut recv_chunks = 0usize;
                let mut recv_bytes = 0usize;
                let mut budget_exhausted = false;
                while socket.can_recv() {
                    if recv_chunks >= MAX_TCP_CONSOLE_RECV_CHUNKS_PER_POLL {
                        budget_exhausted = true;
                        break;
                    }
                    let remaining_budget =
                        MAX_TCP_CONSOLE_RECV_BYTES_PER_POLL.saturating_sub(recv_bytes);
                    if remaining_budget == 0 {
                        budget_exhausted = true;
                        break;
                    }
                    let mut copied = 0usize;
                    let disclose_payload = self.server.is_authenticated();
                    let recv_result = socket.recv(|data| {
                        if disclose_payload {
                            let preview_len = core::cmp::min(data.len(), 32);
                            log::debug!(
                                target: "net-console",
                                "[tcp] recv on console socket: len={} first_bytes={:02x?}",
                                data.len(),
                                &data[..preview_len],
                            );
                        } else {
                            log::debug!(
                                target: "net-console",
                                "[tcp] recv auth payload redacted: len={}",
                                data.len(),
                            );
                        }
                        let copy_len = core::cmp::min(
                            data.len(),
                            core::cmp::min(temp.len(), remaining_budget),
                        );
                        let _ = maybe_report_str_write(
                            temp.as_mut_ptr(),
                            copy_len,
                            data.as_ptr(),
                            copy_len,
                            "tcp_console.recv",
                        );
                        temp[..copy_len].copy_from_slice(&data[..copy_len]);
                        copied = copy_len;
                        (copy_len, ())
                    });
                    match recv_result {
                        Ok(()) if copied == 0 => break,
                        Ok(()) => {
                            let conn_id = self.active_client_id.unwrap_or(0);
                            self.conn_bytes_read =
                                self.conn_bytes_read.saturating_add(copied as u64);
                            NET_DIAG.add_bytes_read(copied as u64);
                            self.counters.tcp_rx_bytes =
                                self.counters.tcp_rx_bytes.saturating_add(copied as u64);
                            recv_chunks = recv_chunks.saturating_add(1);
                            recv_bytes = recv_bytes.saturating_add(copied);
                            #[cfg(feature = "net-trace-31337")]
                            {
                                let (peer_label, peer_port) =
                                    Self::peer_parts(self.peer_endpoint, socket);
                                let dump_len = core::cmp::min(copied, 32);
                                trace!(
                                    "[cohsh-net][tcp] recv: nbytes={} from {}:{} state={:?}",
                                    copied,
                                    peer_label,
                                    peer_port,
                                    socket.state()
                                );
                                if disclose_payload {
                                    trace!("[cohsh-net][tcp] recv hex: {:02x?}", &temp[..dump_len]);
                                } else {
                                    trace!(
                                        "[cohsh-net][tcp] recv auth payload redacted: nbytes={}",
                                        copied
                                    );
                                }
                            }
                            Self::trace_conn_recv(conn_id, &temp[..copied], disclose_payload);
                            if ECHO_MODE {
                                match socket.send_slice(&temp[..copied]) {
                                    Ok(sent) => {
                                        self.conn_bytes_written =
                                            self.conn_bytes_written.saturating_add(sent as u64);
                                        NET_DIAG.add_bytes_written(sent as u64);
                                        self.counters.tcp_tx_bytes =
                                            self.counters.tcp_tx_bytes.saturating_add(sent as u64);
                                        Self::trace_conn_send(
                                            conn_id,
                                            &temp[..sent.min(copied)],
                                            true,
                                        );
                                    }
                                    Err(err) => {
                                        log::warn!(
                                            "[cohsh-net] echo send error conn_id={} err={:?}",
                                            conn_id,
                                            err
                                        );
                                    }
                                }
                                activity = true;
                                continue;
                            }
                            if self.auth_state == AuthState::AuthRequested
                                && !self.session_state.logged_first_recv
                            {
                                let (peer_label, peer_port) =
                                    Self::peer_parts(self.peer_endpoint, socket);
                                info!(
                                    "[cohsh-net][auth] received candidate auth frame len={} from {}:{}",
                                    copied,
                                    peer_label,
                                    peer_port
                                );
                            }
                            self.session_state.logged_first_recv = true;
                            match self.server.ingest(&temp[..copied], now_ms) {
                                SessionEvent::None => {}
                                SessionEvent::Authenticated => {
                                    let conn_id = self.active_client_id.unwrap_or(0);
                                    Self::set_auth_state(
                                        &mut self.auth_state,
                                        self.active_client_id,
                                        AuthState::Attached,
                                    );
                                    #[cfg(feature = "kernel")]
                                    if !Self::publish_cyw43_authenticated_console_peer(
                                        self.server.is_authenticated()
                                            && self.auth_state == AuthState::Attached,
                                        self.wifi_connection_generation,
                                        self.active_client_id,
                                        self.listen_port,
                                        self.peer_endpoint,
                                        socket,
                                    ) {
                                        warn!(
                                            "[net-console] authenticated CYW43 peer publication rejected generation={} conn_id={}",
                                            self.wifi_connection_generation,
                                            conn_id,
                                        );
                                    }
                                    info!(
                                        target: "net-console",
                                        "[net-console] authenticated TCP session {} frame_bytes={} state={:?}",
                                        conn_id,
                                        copied,
                                        self.auth_state,
                                    );
                                    info!(
                                        "[cohsh-net][auth] auth OK, session established (generation={} conn_id={})",
                                        self.wifi_connection_generation,
                                        conn_id
                                    );
                                    NET_DIAG.record_accept_success();
                                    self.counters.tcp_auth_sessions =
                                        self.counters.tcp_auth_sessions.saturating_add(1);
                                    let _ = self
                                        .events
                                        .push(NetConsoleEvent::Authenticated { conn_id });
                                    let _ = Self::flush_outbound(
                                        &mut self.server,
                                        &mut self.outbound,
                                        &mut self.telemetry,
                                        &mut self.conn_bytes_written,
                                        &mut self.counters,
                                        socket,
                                        now_ms,
                                        self.active_client_id,
                                        self.auth_state,
                                        &mut self.session_state,
                                        MAX_CONSOLE_FRAMES_PER_POLL,
                                        MAX_CONSOLE_BYTES_PER_POLL,
                                    );
                                    activity = true;
                                }
                                SessionEvent::AuthFailed(reason) => {
                                    log::warn!(
                                        "[cohsh-net][auth] closing connection due to auth failure (reason={})",
                                        reason
                                    );
                                    Self::set_auth_state(
                                        &mut self.auth_state,
                                        self.active_client_id,
                                        AuthState::Failed,
                                    );
                                    let bytes_before = self.conn_bytes_written;
                                    activity |= Self::flush_outbound(
                                        &mut self.server,
                                        &mut self.outbound,
                                        &mut self.telemetry,
                                        &mut self.conn_bytes_written,
                                        &mut self.counters,
                                        socket,
                                        now_ms,
                                        self.active_client_id,
                                        self.auth_state,
                                        &mut self.session_state,
                                        MAX_CONSOLE_FRAMES_PER_POLL,
                                        MAX_CONSOLE_BYTES_PER_POLL,
                                    );
                                    if self.conn_bytes_written == bytes_before {
                                        if Self::send_auth_failure_ack(
                                            &mut self.server,
                                            &mut self.conn_bytes_written,
                                            &mut self.counters,
                                            socket,
                                            self.active_client_id,
                                            self.auth_state,
                                            &mut self.session_state,
                                            now_ms,
                                            reason,
                                        ) {
                                            activity = true;
                                        } else {
                                            warn!(
                                                "[cohsh-net][auth] unable to flush auth failure ack before close (reason={})",
                                                reason
                                            );
                                        }
                                    }
                                    activity |= begin_console_disconnect(
                                        &mut self.disconnect_phase,
                                        &mut self.disconnect_phase_started_ms,
                                        &mut self.disconnect_reason,
                                        &mut disconnect_entered_this_turn,
                                        NetConsoleDisconnectReason::Error,
                                    );
                                    break;
                                }
                                SessionEvent::Close => {
                                    let _ = Self::flush_outbound(
                                        &mut self.server,
                                        &mut self.outbound,
                                        &mut self.telemetry,
                                        &mut self.conn_bytes_written,
                                        &mut self.counters,
                                        socket,
                                        now_ms,
                                        self.active_client_id,
                                        self.auth_state,
                                        &mut self.session_state,
                                        MAX_CONSOLE_FRAMES_PER_POLL,
                                        MAX_CONSOLE_BYTES_PER_POLL,
                                    );
                                    activity |= begin_console_disconnect(
                                        &mut self.disconnect_phase,
                                        &mut self.disconnect_phase_started_ms,
                                        &mut self.disconnect_reason,
                                        &mut disconnect_entered_this_turn,
                                        NetConsoleDisconnectReason::Eof,
                                    );
                                    break;
                                }
                            }
                        }
                        Err(err) => {
                            let reason = match err {
                                TcpRecvError::Finished => {
                                    info!(
                                        "[net-console] TCP client #{} closed (clean shutdown)",
                                        self.active_client_id.unwrap_or(0)
                                    );
                                    NetConsoleDisconnectReason::Eof
                                }
                                other => {
                                    warn!(
                                        "[net-console] TCP client #{} error={other} (closing connection)",
                                        self.active_client_id.unwrap_or(0)
                                    );
                                    warn!(
                                        "[net-console] closing connection: reason={} state={:?}",
                                        REASON_RECV_ERROR, self.auth_state
                                    );
                                    NetConsoleDisconnectReason::Error
                                }
                            };
                            Self::set_auth_state(
                                &mut self.auth_state,
                                self.active_client_id,
                                AuthState::Failed,
                            );
                            debug!(
                                "[net-console][auth] state={:?} recv error from client={}",
                                self.auth_state,
                                self.active_client_id.unwrap_or(0)
                            );
                            activity |= begin_console_disconnect(
                                &mut self.disconnect_phase,
                                &mut self.disconnect_phase_started_ms,
                                &mut self.disconnect_reason,
                                &mut disconnect_entered_this_turn,
                                reason,
                            );
                            break;
                        }
                    }
                }
                if budget_exhausted && socket.can_recv() {
                    self.counters.tcp_console_recv_budget_hits =
                        self.counters.tcp_console_recv_budget_hits.saturating_add(1);
                }
            }
            if self.disconnect_phase == ConsoleDisconnectPhase::Idle
                && self.session_active
                && socket.state() == TcpState::Established
                && !socket.may_recv()
            {
                activity |= begin_console_disconnect(
                    &mut self.disconnect_phase,
                    &mut self.disconnect_phase_started_ms,
                    &mut self.disconnect_reason,
                    &mut disconnect_entered_this_turn,
                    NetConsoleDisconnectReason::Eof,
                );
            }
            if self.disconnect_phase == ConsoleDisconnectPhase::Idle
                && self.session_active
                && !self.server.is_authenticated()
                && self.server.auth_timed_out(now_ms)
            {
                warn!(
                    "[net-console] TCP client #{} auth timeout",
                    self.active_client_id.unwrap_or(0)
                );
                log::error!(
                    "[cohsh-net] error during handshake: auth-timeout (state={:?})",
                    self.auth_state
                );
                debug!(
                    "[net-console][auth] state={:?} auth timeout client={} now_ms={}",
                    self.auth_state,
                    self.active_client_id.unwrap_or(0),
                    now_ms
                );
                warn!(
                    "[net-console] closing connection: reason=auth-timeout state={:?}",
                    self.auth_state
                );
                let _ = self.server.enqueue_outbound(ERR_AUTH_REASON_TIMEOUT);
                activity |= Self::flush_outbound(
                    &mut self.server,
                    &mut self.outbound,
                    &mut self.telemetry,
                    &mut self.conn_bytes_written,
                    &mut self.counters,
                    socket,
                    now_ms,
                    self.active_client_id,
                    self.auth_state,
                    &mut self.session_state,
                    MAX_CONSOLE_FRAMES_PER_POLL,
                    MAX_CONSOLE_BYTES_PER_POLL,
                );
                Self::set_auth_state(
                    &mut self.auth_state,
                    self.active_client_id,
                    AuthState::Failed,
                );
                activity |= begin_console_disconnect(
                    &mut self.disconnect_phase,
                    &mut self.disconnect_phase_started_ms,
                    &mut self.disconnect_reason,
                    &mut disconnect_entered_this_turn,
                    NetConsoleDisconnectReason::Error,
                );
            }

            if self.disconnect_phase == ConsoleDisconnectPhase::Idle
                && self.session_active
                && self.server.should_timeout(now_ms)
            {
                warn!(
                    "[net-console] TCP client #{} timed out due to inactivity",
                    self.active_client_id.unwrap_or(0)
                );
                debug!(
                    "[net-console][auth] state={:?} inactivity timeout client={} now_ms={}",
                    self.auth_state,
                    self.active_client_id.unwrap_or(0),
                    now_ms
                );
                warn!(
                    "[net-console] closing connection: reason={} state={:?}",
                    REASON_INACTIVITY_TIMEOUT, self.auth_state
                );
                let _ = self.server.enqueue_outbound(ERR_CONSOLE_REASON_TIMEOUT);
                activity |= Self::flush_outbound(
                    &mut self.server,
                    &mut self.outbound,
                    &mut self.telemetry,
                    &mut self.conn_bytes_written,
                    &mut self.counters,
                    socket,
                    now_ms,
                    self.active_client_id,
                    self.auth_state,
                    &mut self.session_state,
                    MAX_CONSOLE_FRAMES_PER_POLL,
                    MAX_CONSOLE_BYTES_PER_POLL,
                );
                Self::set_auth_state(
                    &mut self.auth_state,
                    self.active_client_id,
                    AuthState::Failed,
                );
                activity |= begin_console_disconnect(
                    &mut self.disconnect_phase,
                    &mut self.disconnect_phase_started_ms,
                    &mut self.disconnect_reason,
                    &mut disconnect_entered_this_turn,
                    NetConsoleDisconnectReason::Error,
                );
            }

            let tcp_state = socket.state();
            if self.disconnect_phase == ConsoleDisconnectPhase::Idle
                && tcp_state == TcpState::CloseWait
            {
                info!(
                    "[net-console] TCP client #{} peer half-close (state={:?}) action=drain-then-fin",
                    self.active_client_id.unwrap_or(0),
                    tcp_state
                );
                activity |= begin_console_disconnect(
                    &mut self.disconnect_phase,
                    &mut self.disconnect_phase_started_ms,
                    &mut self.disconnect_reason,
                    &mut disconnect_entered_this_turn,
                    NetConsoleDisconnectReason::Eof,
                );
            } else if self.disconnect_phase == ConsoleDisconnectPhase::Idle
                && matches!(
                    tcp_state,
                    TcpState::FinWait1 | TcpState::FinWait2 | TcpState::Closing | TcpState::LastAck
                )
            {
                // A local close outside the console-QUIT path has already
                // staged its FIN. Preserve smoltcp's handshake state; aborting
                // here converts ordinary connection churn into a wire RST.
                allow_flush = false;
            }

            if self.disconnect_phase == ConsoleDisconnectPhase::Idle
                && matches!(socket.state(), TcpState::Closed)
                && self.session_active
            {
                Self::log_session_closed(&mut self.session_state, self.peer_endpoint, socket);
                self.outbound.reset();
                self.server.end_session();
                self.session_active = false;
                if let Some(conn_id) = self.active_client_id {
                    Self::note_close_reason(
                        &mut log_closed_conn,
                        conn_id,
                        NetConsoleDisconnectReason::Reset,
                    );
                    Self::note_close_reason(
                        &mut record_closed_conn,
                        conn_id,
                        NetConsoleDisconnectReason::Reset,
                    );
                }
                self.active_client_id = None;
                self.peer_endpoint = None;
                Self::set_auth_state(
                    &mut self.auth_state,
                    self.active_client_id,
                    AuthState::Start,
                );
                reset_session = true;
                reset_tcp_state = Some(socket.state());
                allow_flush = false;
            }

            if matches!(
                self.disconnect_phase,
                ConsoleDisconnectPhase::PeerCloseWait | ConsoleDisconnectPhase::Closing
            ) {
                allow_flush = false;
            }

            if allow_flush {
                activity |= Self::flush_outbound(
                    &mut self.server,
                    &mut self.outbound,
                    &mut self.telemetry,
                    &mut self.conn_bytes_written,
                    &mut self.counters,
                    socket,
                    now_ms,
                    self.active_client_id,
                    self.auth_state,
                    &mut self.session_state,
                    MAX_CONSOLE_FRAMES_PER_POLL,
                    MAX_CONSOLE_BYTES_PER_POLL,
                );
                outbound_pending |= self.server.has_outbound();
            }

            if self.disconnect_phase != ConsoleDisconnectPhase::Idle {
                self.disconnect_phase_started_ms = arm_console_disconnect_phase_deadline(
                    self.disconnect_phase,
                    self.disconnect_phase_started_ms,
                    now_ms,
                );
                let inbound_queued = self.server.ingest_snapshot().queued;
                let application_output_drained = console_disconnect_application_queues_drained(
                    self.server.has_outbound(),
                    self.outbound.has_pending(),
                    inbound_queued,
                    disconnect_entered_this_turn,
                );
                // Smoltcp retains transmitted bytes here until the peer ACKs them. Empty
                // application queues alone only prove that output was staged into the socket.
                let tcp_send_queue_empty = socket.send_queue() == 0;
                let action = console_disconnect_action(
                    self.disconnect_phase,
                    socket.state(),
                    application_output_drained,
                    tcp_send_queue_empty,
                    self.disconnect_reason == NetConsoleDisconnectReason::Quit,
                    now_ms,
                    self.disconnect_phase_started_ms.unwrap_or(now_ms),
                );
                let mut disconnect_terminal = false;
                let terminal_reason =
                    console_disconnect_terminal_reason(action, self.disconnect_reason);
                match action {
                    ConsoleDisconnectAction::Wait => {}
                    ConsoleDisconnectAction::StartPeerCloseWait => {
                        info!(
                            "[net-console] quit peer-close wait conn={} state={:?} grace_ms={} app_drained={} inbound={} send_queue={}",
                            self.active_client_id.unwrap_or(0),
                            socket.state(),
                            DISCONNECT_PEER_CLOSE_GRACE_MS,
                            application_output_drained,
                            inbound_queued,
                            socket.send_queue()
                        );
                        self.disconnect_phase = ConsoleDisconnectPhase::PeerCloseWait;
                        self.disconnect_phase_started_ms = Some(now_ms);
                        activity = true;
                    }
                    ConsoleDisconnectAction::StartClose => {
                        info!(
                            "[net-console] quit close start conn={} state={:?} app_drained={} inbound={} send_queue={}",
                            self.active_client_id.unwrap_or(0),
                            socket.state(),
                            application_output_drained,
                            inbound_queued,
                            socket.send_queue()
                        );
                        Self::log_session_closed(
                            &mut self.session_state,
                            self.peer_endpoint,
                            socket,
                        );
                        if !matches!(socket.state(), TcpState::Closed | TcpState::TimeWait) {
                            socket.close();
                        }
                        self.disconnect_phase = ConsoleDisconnectPhase::Closing;
                        self.disconnect_phase_started_ms = Some(now_ms);
                        activity = true;
                    }
                    ConsoleDisconnectAction::ContinueClose => {
                        self.disconnect_phase = ConsoleDisconnectPhase::Closing;
                        self.disconnect_phase_started_ms = Some(now_ms);
                        activity = true;
                    }
                    ConsoleDisconnectAction::Complete => {
                        disconnect_terminal = true;
                    }
                    ConsoleDisconnectAction::Abort => {
                        self.disconnect_forced_aborts =
                            self.disconnect_forced_aborts.saturating_add(1);
                        warn!(
                            "[net-console] disconnect deadline expired conn={} origin={} phase={:?} state={:?} app_drained={} inbound={} send_queue={} forced_aborts={}",
                            self.active_client_id.unwrap_or(0),
                            self.disconnect_reason.as_str(),
                            self.disconnect_phase,
                            socket.state(),
                            application_output_drained,
                            inbound_queued,
                            socket.send_queue(),
                            self.disconnect_forced_aborts,
                        );
                        if socket.state() != TcpState::Closed {
                            socket.abort();
                        }
                        disconnect_terminal = true;
                    }
                }

                if disconnect_terminal {
                    Self::log_session_closed(&mut self.session_state, self.peer_endpoint, socket);
                    self.server.end_session();
                    self.outbound.reset();
                    self.session_active = false;
                    self.disconnect_phase = ConsoleDisconnectPhase::Idle;
                    self.disconnect_phase_started_ms = None;
                    outbound_pending = false;
                    if let Some(conn_id) = self.active_client_id {
                        Self::note_close_reason(&mut log_closed_conn, conn_id, terminal_reason);
                        Self::note_close_reason(&mut record_closed_conn, conn_id, terminal_reason);
                    }
                    self.active_client_id = None;
                    self.peer_endpoint = None;
                    Self::set_auth_state(
                        &mut self.auth_state,
                        self.active_client_id,
                        AuthState::Start,
                    );
                    self.listener_announced = false;
                    self.disconnect_reason = NetConsoleDisconnectReason::Quit;
                    reset_session = true;
                    reset_tcp_state = Some(socket.state());
                    activity = true;
                }
            }

            let snapshot = PollSnapshot {
                session_active: self.session_active,
                auth_state: self.auth_state,
                listener_ready: self.listener_announced,
                tcp_state: socket.state(),
                can_recv: socket.can_recv(),
                can_send: socket.can_send(),
                staged_events: self.events.len(),
            };
            (snapshot, socket.state())
        };

        self.log_poll_snapshot(snapshot);
        last_tcp_state = tcp_state;

        if reset_session {
            let state = reset_tcp_state.or(Some(last_tcp_state));
            self.reset_session_state_with(state);
        } else if self.session_state.last_state.is_none() {
            self.session_state.last_state = Some(last_tcp_state);
        }

        if let Some((conn_id, reason)) = log_closed_conn {
            self.log_conn_summary(conn_id);
            Self::audit_conn_close(conn_id, reason);
        }
        if let Some((conn_id, reason)) = record_closed_conn {
            self.record_conn_closed(conn_id, reason);
        }
        if self.disconnect_phase == ConsoleDisconnectPhase::Idle {
            activity |= self.promote_console_standby(now_ms);
        } else {
            activity |= self.service_console_standby(now_ms);
        }

        activity || outbound_pending
    }

    #[inline]
    // Activity-only logging to prevent endless spam in steady state.
    fn should_log_flush_blocked(
        session_state: &SessionState,
        snapshot: CohshBlockedSnapshot,
        now_ms: u64,
        preconnect: bool,
    ) -> bool {
        if preconnect {
            return !session_state.flush_blocked_logged_preconnect;
        }
        let activity = session_state
            .last_blocked_snapshot
            .map_or(true, |prev| prev != snapshot);
        let heartbeat =
            now_ms.saturating_sub(session_state.last_flush_log_ms) >= FLUSH_BLOCKED_HEARTBEAT_MS;
        activity || heartbeat
    }

    fn flush_outbound(
        server: &mut TcpConsoleServer,
        outbound: &mut OutboundCoalescer,
        telemetry: &mut NetTelemetry,
        conn_bytes_written: &mut u64,
        counters: &mut NetCounters,
        socket: &mut TcpSocket,
        now_ms: u64,
        conn_id: Option<u64>,
        auth_state: AuthState,
        session_state: &mut SessionState,
        max_frames: u32,
        max_bytes: usize,
    ) -> bool {
        if !socket.can_send() {
            #[cfg(feature = "cohesix-dev")]
            {
                let queued = server.has_outbound() || outbound.has_pending();
                if queued {
                    let preconnect = !session_state.connect_reported;
                    let blocked_snapshot = CohshBlockedSnapshot {
                        tcp_state: socket.state(),
                        auth_state,
                        queued,
                    };
                    if Self::should_log_flush_blocked(
                        session_state,
                        blocked_snapshot,
                        now_ms,
                        preconnect,
                    ) {
                        let tcp_state = tcp_state_label(socket.state());
                        let send_queue = socket.send_queue();
                        let send_capacity = socket.send_capacity();
                        let mut message: HeaplessString<128> = HeaplessString::new();
                        let _ = message.push_str("audit tcp.flush.blocked state=");
                        let _ = message.push_str(tcp_state);
                        let _ = write!(
                            message,
                            " queue={}/{} auth={:?}",
                            send_queue, send_capacity, auth_state
                        );
                        crate::debug_uart::debug_uart_line(message.as_str());
                        session_state.last_flush_log_ms = now_ms;
                        if preconnect {
                            session_state.flush_blocked_logged_preconnect = true;
                        }
                    }
                    session_state.flush_blocked_since.get_or_insert(now_ms);
                    session_state.last_blocked_snapshot = Some(blocked_snapshot);
                    session_state.last_flush_state = Some(socket.state());
                    session_state.last_flush_auth_state = Some(auth_state);
                }
            }
            return false;
        }
        let pre_auth = !server.is_authenticated();
        let mut activity = false;
        let state_changed = session_state.last_flush_state != Some(socket.state());
        let auth_changed = session_state.last_flush_auth_state != Some(auth_state);
        let queued = server.has_outbound() || outbound.has_pending();
        let blocked_by_auth = pre_auth
            && server
                .peek_outbound()
                .map(|line| {
                    let line = line.as_str();
                    !(line.starts_with("OK AUTH") || line.starts_with("ERR AUTH"))
                })
                .unwrap_or(false);

        if blocked_by_auth {
            let blocked_snapshot = CohshBlockedSnapshot {
                tcp_state: socket.state(),
                auth_state,
                queued,
            };
            let preconnect = !session_state.connect_reported;
            if Self::should_log_flush_blocked(session_state, blocked_snapshot, now_ms, preconnect) {
                info!(
                    target: "cohsh-net",
                    "[cohsh-net] flush_outbound blocked state={:?} auth_state={:?} queued={}",
                    socket.state(),
                    auth_state,
                    queued,
                );
                session_state.last_flush_log_ms = now_ms;
                if preconnect {
                    session_state.flush_blocked_logged_preconnect = true;
                }
            }
            session_state.flush_blocked_since.get_or_insert(now_ms);
            session_state.last_blocked_snapshot = Some(blocked_snapshot);
            session_state.last_flush_state = Some(socket.state());
            session_state.last_flush_auth_state = Some(auth_state);
            return false;
        }

        session_state.last_blocked_snapshot = None;
        if state_changed || auth_changed {
            info!(
                target: "cohsh-net",
                "[cohsh-net] flush_outbound state={:?} auth_state={:?} queued={} can_send={}",
                socket.state(),
                auth_state,
                queued,
                socket.can_send(),
            );
            session_state.last_flush_log_ms = now_ms;
        }
        session_state.flush_blocked_since = None;
        let mut staged_frames: u32 = 0;
        let mut staged_bytes: usize = 0;
        while let Some(line) = server.pop_outbound() {
            if pre_auth && !(line.starts_with("OK AUTH") || line.starts_with("ERR AUTH")) {
                server.push_outbound_front(line);
                break;
            }
            if staged_frames >= max_frames || staged_bytes >= max_bytes {
                server.push_outbound_front(line);
                break;
            }
            let lane = if TcpConsoleServer::is_priority_line(line.as_str()) {
                OutboundLane::Control
            } else {
                OutboundLane::Log
            };
            let total_len = line.len().saturating_add(4);
            if staged_bytes.saturating_add(total_len) > max_bytes {
                server.push_outbound_front(line);
                break;
            }
            let staged = match lane {
                OutboundLane::Control => outbound.enqueue_control(line.as_bytes()),
                OutboundLane::Log => outbound.enqueue_log_lossless(line.as_bytes()),
            };
            if staged.is_err() {
                server.push_outbound_front(line);
                break;
            }
            staged_frames = staged_frames.saturating_add(1);
            staged_bytes = staged_bytes.saturating_add(total_len);
        }
        let max_payload_frames = usize::try_from(max_frames).unwrap_or(usize::MAX);
        let outcome =
            outbound.flush_bounded(now_ms, max_payload_frames, max_bytes, |payload, lane| {
                Self::send_payload(
                    server,
                    conn_bytes_written,
                    counters,
                    socket,
                    conn_id,
                    auth_state,
                    session_state,
                    now_ms,
                    payload,
                    lane,
                    pre_auth,
                )
            });
        if outcome.sent_frames != 0 {
            activity = true;
        }
        if outcome.would_block {
            telemetry.tx_drops = telemetry.tx_drops.saturating_add(1);
        }
        let stats = outbound.stats();
        NET_DIAG.update_outbound_stats(
            u64::from(stats.queued_lines),
            u64::from(stats.queued_bytes),
            stats.drops,
            stats.frames_sent,
            stats.bytes_sent,
            stats.would_block,
        );
        session_state.last_flush_state = Some(socket.state());
        session_state.last_flush_auth_state = Some(auth_state);
        activity
    }

    fn send_payload(
        server: &mut TcpConsoleServer,
        conn_bytes_written: &mut u64,
        counters: &mut NetCounters,
        socket: &mut TcpSocket,
        conn_id: Option<u64>,
        auth_state: AuthState,
        session_state: &mut SessionState,
        now_ms: u64,
        payload: &[u8],
        lane: OutboundLane,
        pre_auth: bool,
    ) -> Result<(), SendError> {
        if pre_auth && matches!(lane, OutboundLane::Control) {
            info!(
                "[net-console] handshake: sending {}-byte response to client",
                payload.len()
            );
            info!(
                "[cohsh-net] send: auth response len={} role='AUTH'",
                payload.len()
            );
        }
        // Avoid partial TCP writes; the console protocol depends on intact frames.
        let available = socket.send_capacity().saturating_sub(socket.send_queue());
        if payload.len() > available {
            #[cfg(feature = "cohesix-dev")]
            {
                let mut message: HeaplessString<128> = HeaplessString::new();
                let _ = write!(
                    message,
                    "audit tcp.send.blocked len={} avail={} state={:?} auth={:?}",
                    payload.len(),
                    available,
                    socket.state(),
                    auth_state
                );
                crate::debug_uart::debug_uart_line(message.as_str());
            }
            return Err(SendError::WouldBlock);
        }
        match socket.send_slice(payload) {
            Ok(sent) if sent == payload.len() => {
                if pre_auth {
                    log::debug!(
                        target: "net-console",
                        "[tcp] send auth payload redacted: len={}",
                        sent,
                    );
                } else {
                    let preview_len = core::cmp::min(sent, 32);
                    log::debug!(
                        target: "net-console",
                        "[tcp] send on console socket: len={} first_bytes={:02x?}",
                        sent,
                        &payload[..preview_len],
                    );
                }
                *conn_bytes_written = conn_bytes_written.saturating_add(sent as u64);
                NET_DIAG.add_bytes_written(sent as u64);
                counters.tcp_tx_bytes = counters.tcp_tx_bytes.saturating_add(sent as u64);
                if !session_state.logged_first_send {
                    info!(
                        target: "root_task::net",
                        "[tcp] first-send.ok bytes={sent}"
                    );
                    session_state.logged_first_send = true;
                }
                if server.is_authenticated() {
                    server.mark_activity(now_ms);
                }
                let conn_id = conn_id.unwrap_or(0);
                Self::trace_conn_send(conn_id, payload, !pre_auth);
                #[cfg(feature = "net-trace-31337")]
                {
                    let tcp_state = socket.state();
                    if pre_auth {
                        info!(
                            "[cohsh-net] send: {} auth bytes redacted (state={:?}, auth_state={:?})",
                            sent, tcp_state, auth_state,
                        );
                    } else {
                        let dump_len = payload.len().min(32);
                        info!(
                            "[cohsh-net] send: {} bytes (state={:?}, auth_state={:?}): {:02x?}",
                            sent,
                            tcp_state,
                            auth_state,
                            &payload[..dump_len]
                        );
                    }
                }
                if pre_auth && matches!(lane, OutboundLane::Control) {
                    info!(
                        "[net-console] conn {}: sent pre-auth payload len={} state={:?}",
                        conn_id,
                        payload.len(),
                        auth_state,
                    );
                    info!(
                        "[net-console] auth response sent; session state = {:?}",
                        auth_state
                    );
                }
                Ok(())
            }
            Ok(sent) => {
                #[cfg(feature = "cohesix-dev")]
                {
                    let mut message: HeaplessString<128> = HeaplessString::new();
                    let _ = write!(
                        message,
                        "audit tcp.send.partial sent={} expected={} state={:?} auth={:?}",
                        sent,
                        payload.len(),
                        socket.state(),
                        auth_state
                    );
                    crate::debug_uart::debug_uart_line(message.as_str());
                }
                warn!(
                    target: "root_task::net",
                    "[tcp] send.partial sent={} expected={} (aborting console session)",
                    sent,
                    payload.len()
                );
                socket.abort();
                Err(SendError::Fault)
            }
            Err(err) => {
                #[cfg(feature = "cohesix-dev")]
                {
                    let mut message: HeaplessString<128> = HeaplessString::new();
                    let _ = write!(
                        message,
                        "audit tcp.send.error err={:?} state={:?} auth={:?}",
                        err,
                        socket.state(),
                        auth_state
                    );
                    crate::debug_uart::debug_uart_line(message.as_str());
                }
                warn!(
                    target: "root_task::net",
                    "[tcp] send.err err={err:?}"
                );
                Err(SendError::WouldBlock)
            }
        }
    }

    fn send_auth_failure_ack(
        server: &mut TcpConsoleServer,
        conn_bytes_written: &mut u64,
        counters: &mut NetCounters,
        socket: &mut TcpSocket,
        conn_id: Option<u64>,
        auth_state: AuthState,
        session_state: &mut SessionState,
        now_ms: u64,
        reason: &str,
    ) -> bool {
        let mut line = HeaplessString::<DEFAULT_LINE_CAPACITY>::new();
        if line.push_str("ERR AUTH reason=").is_err() || line.push_str(reason).is_err() {
            return false;
        }
        let total_len = line.len().saturating_add(4);
        let Ok(total_len_u32) = u32::try_from(total_len) else {
            return false;
        };
        let mut frame: HeaplessVec<u8, { DEFAULT_LINE_CAPACITY + 4 }> = HeaplessVec::new();
        if frame
            .extend_from_slice(&total_len_u32.to_le_bytes())
            .is_err()
            || frame.extend_from_slice(line.as_bytes()).is_err()
        {
            return false;
        }
        match Self::send_payload(
            server,
            conn_bytes_written,
            counters,
            socket,
            conn_id,
            auth_state,
            session_state,
            now_ms,
            frame.as_slice(),
            OutboundLane::Control,
            true,
        ) {
            Ok(()) => true,
            Err(_) => false,
        }
    }

    fn log_conn_summary(&self, conn_id: u64) {
        info!(
            "[net-console] conn {}: bytes read={}, bytes written={}",
            conn_id, self.conn_bytes_read, self.conn_bytes_written
        );
    }

    fn record_conn_closed(&mut self, conn_id: u64, reason: NetConsoleDisconnectReason) {
        Self::trace_conn_closed(
            conn_id,
            reason.as_str(),
            self.conn_bytes_read,
            self.conn_bytes_written,
        );
        let _ = self.events.push(NetConsoleEvent::Disconnected {
            conn_id,
            reason,
            bytes_read: self.conn_bytes_read,
            bytes_written: self.conn_bytes_written,
        });
    }

    /// Returns the negotiated Ethernet address for the attached network device.
    #[must_use]
    pub fn hardware_address(&self) -> EthernetAddress {
        self.device.mac()
    }

    /// Returns the configured IPv4 address for the interface.
    #[must_use]
    pub fn ipv4_address(&self) -> Ipv4Address {
        self.ip
    }

    /// Returns the configured prefix length for the primary IPv4 address.
    #[must_use]
    pub fn prefix_len(&self) -> u8 {
        self.prefix_len
    }

    /// Returns the configured default gateway if present.
    #[must_use]
    pub fn gateway(&self) -> Option<Ipv4Address> {
        self.gateway
    }

    /// Returns a snapshot of runtime statistics gathered from the driver.
    #[must_use]
    pub fn telemetry(&self) -> NetTelemetry {
        self.telemetry
    }
}

impl DefaultNetStack {
    #[must_use]
    pub fn hardware_address(&self) -> EthernetAddress {
        match self {
            Self::Rtl8139(stack) => stack.hardware_address(),
            Self::GenetDriverTask(stack) => stack.hardware_address(),
            Self::Cyw43DriverTask(stack) => stack.hardware_address(),
            #[cfg(feature = "net-backend-virtio")]
            Self::Virtio(stack) => stack.hardware_address(),
        }
    }

    #[must_use]
    pub fn ipv4_address(&self) -> Ipv4Address {
        match self {
            Self::Rtl8139(stack) => stack.ipv4_address(),
            Self::GenetDriverTask(stack) => stack.ipv4_address(),
            Self::Cyw43DriverTask(stack) => stack.ipv4_address(),
            #[cfg(feature = "net-backend-virtio")]
            Self::Virtio(stack) => stack.ipv4_address(),
        }
    }

    #[must_use]
    pub fn console_listen_port(&self) -> u16 {
        match self {
            Self::Rtl8139(stack) => stack.console_listen_port(),
            Self::GenetDriverTask(stack) => stack.console_listen_port(),
            Self::Cyw43DriverTask(stack) => stack.console_listen_port(),
            #[cfg(feature = "net-backend-virtio")]
            Self::Virtio(stack) => stack.console_listen_port(),
        }
    }

    #[must_use]
    pub fn prefix_len(&self) -> u8 {
        match self {
            Self::Rtl8139(stack) => stack.prefix_len(),
            Self::GenetDriverTask(stack) => stack.prefix_len(),
            Self::Cyw43DriverTask(stack) => stack.prefix_len(),
            #[cfg(feature = "net-backend-virtio")]
            Self::Virtio(stack) => stack.prefix_len(),
        }
    }

    #[must_use]
    pub fn gateway(&self) -> Option<Ipv4Address> {
        match self {
            Self::Rtl8139(stack) => stack.gateway(),
            Self::GenetDriverTask(stack) => stack.gateway(),
            Self::Cyw43DriverTask(stack) => stack.gateway(),
            #[cfg(feature = "net-backend-virtio")]
            Self::Virtio(stack) => stack.gateway(),
        }
    }
}

impl<D: NetDevice> NetPoller for NetStack<D> {
    fn poll(&mut self, now_ms: u64) -> bool {
        let contract = D::driver_task_contract();
        #[cfg(feature = "kernel")]
        {
            if let Some(hot_path) = net_driver_task_hot_path(contract) {
                if D::driver_task_runtime_client() {
                    crate::hal::driver_task::register_driver_task_pointer_free_ring_service(
                        contract,
                        hot_path.as_u32() as usize,
                        crate::drivers::driver_task_net::runtime_ring_service,
                    );
                    if crate::drivers::driver_task_net::cyw43_net_data_pre_poll_continuation_pending(
                        contract,
                    ) {
                        let ring_progress =
                            service_driver_task_pre_poll_burst(contract, hot_path, 0);
                        return self.poll_with_time(now_ms) || ring_progress;
                    }
                    if self.wifi_association_claims_runtime_turn(now_ms) {
                        return self.poll_with_time(now_ms);
                    }
                    if !wifi_driver_task_pre_poll_due(
                        self.device.bringup_status_label(),
                        false,
                        crate::drivers::driver_task_net::driver_task_runtime_pre_poll_allowed(
                            contract,
                        ),
                    ) {
                        return self.poll_with_time(now_ms);
                    }
                    let ring_progress = service_driver_task_pre_poll_burst(contract, hot_path, 0);
                    return self.poll_with_time(now_ms) || ring_progress;
                }
                let _ = hot_path;
                return false;
            }
            let mut context = NetDriverTaskContext::<D> {
                stack: self as *mut NetStack<D> as usize,
                budget: 0,
                now_ms,
                _marker: core::marker::PhantomData,
            };
            // SAFETY: The HAL admits this compatibility callback only for
            // QEMU/host profiles. Physical Pi 4 builds return None without
            // compiling callback slot state.
            if let Some(result) = unsafe {
                crate::hal::driver_task::try_driver_task_compat_service(
                    contract,
                    &mut context as *mut NetDriverTaskContext<D> as usize,
                    net_poll_driver_task::<D>,
                )
            } {
                return result != 0;
            }
            if !crate::hal::driver_task::admit_root_task_compatibility_service(contract) {
                return false;
            }
        }
        self.poll_with_time(now_ms)
    }

    fn poll_with_budget(
        &mut self,
        now_ms: u64,
        budget: &mut DriverServiceBudget,
    ) -> Result<bool, DriverServiceBudgetError> {
        let contract = D::driver_task_contract();
        #[cfg(feature = "kernel")]
        {
            if let Some(hot_path) = net_driver_task_hot_path(contract) {
                if D::driver_task_runtime_client() {
                    crate::hal::driver_task::register_driver_task_pointer_free_ring_service(
                        contract,
                        hot_path.as_u32() as usize,
                        crate::drivers::driver_task_net::runtime_ring_service,
                    );
                    if crate::drivers::driver_task_net::cyw43_net_data_pre_poll_continuation_pending(
                        contract,
                    ) {
                        let ring_progress = service_driver_task_pre_poll_burst_budgeted(
                            contract,
                            hot_path,
                            NET_RING_FLAG_BUDGETED,
                            budget,
                        );
                        return self
                            .poll_budgeted_with_time(now_ms, budget)
                            .map(|root_progress| root_progress || ring_progress);
                    }
                    if self.wifi_association_claims_runtime_turn(now_ms) {
                        return self.poll_budgeted_with_time(now_ms, budget);
                    }
                    if !wifi_driver_task_pre_poll_due(
                        self.device.bringup_status_label(),
                        false,
                        crate::drivers::driver_task_net::driver_task_runtime_pre_poll_allowed(
                            contract,
                        ),
                    ) {
                        return self.poll_budgeted_with_time(now_ms, budget);
                    }
                    let cyw43_inner_pre_poll_owner = hot_path
                        == crate::hal::driver_task::DriverTaskHotPath::Cyw43Wifi
                        && self.cyw43_flush_pre_poll_data_ready();
                    let ring_progress =
                        if budgeted_outer_pre_poll_allowed(hot_path, cyw43_inner_pre_poll_owner) {
                            service_driver_task_pre_poll_burst_budgeted(
                                contract,
                                hot_path,
                                NET_RING_FLAG_BUDGETED,
                                budget,
                            )
                        } else {
                            false
                        };
                    return self
                        .poll_budgeted_with_time(now_ms, budget)
                        .map(|root_progress| root_progress || ring_progress);
                }
                let _ = hot_path;
                return Err(DriverServiceBudgetError::OperationsExhausted);
            }
            let mut context = NetDriverTaskContext::<D> {
                stack: self as *mut NetStack<D> as usize,
                budget: budget as *mut DriverServiceBudget as usize,
                now_ms,
                _marker: core::marker::PhantomData,
            };
            // SAFETY: The HAL admits this compatibility callback only for
            // QEMU/host profiles. Physical Pi 4 builds return None without
            // compiling callback slot state.
            if let Some(result) = unsafe {
                crate::hal::driver_task::try_driver_task_compat_service(
                    contract,
                    &mut context as *mut NetDriverTaskContext<D> as usize,
                    net_poll_budgeted_driver_task::<D>,
                )
            } {
                return unpack_net_poll_result(result);
            }
            if !crate::hal::driver_task::admit_root_task_compatibility_service(contract) {
                return Err(DriverServiceBudgetError::OperationsExhausted);
            }
        }
        self.poll_budgeted_with_time(now_ms, budget)
    }

    fn flush_tcp_with_budget(
        &mut self,
        now_ms: u64,
        budget: &mut DriverServiceBudget,
    ) -> Result<bool, DriverServiceBudgetError> {
        let contract = D::driver_task_contract();
        #[cfg(feature = "kernel")]
        {
            if let Some(hot_path) = net_driver_task_hot_path(contract) {
                if D::driver_task_runtime_client() {
                    crate::hal::driver_task::register_driver_task_pointer_free_ring_service(
                        contract,
                        hot_path.as_u32() as usize,
                        crate::drivers::driver_task_net::runtime_ring_service,
                    );
                    let ring_progress = match hot_path {
                        crate::hal::driver_task::DriverTaskHotPath::Cyw43Wifi => {
                            self.service_cyw43_data_pre_poll_burst_budgeted(contract, budget)
                        }
                        crate::hal::driver_task::DriverTaskHotPath::GenetNic => self
                            .service_genet_tcp_flush_pre_poll_burst_budgeted(
                                contract, hot_path, budget,
                            ),
                        _ => false,
                    };
                    return Ok(
                        self.flush_budgeted_tcp_with_time(now_ms, budget, ring_progress)
                            || ring_progress,
                    );
                }
                let _ = hot_path;
                return Err(DriverServiceBudgetError::OperationsExhausted);
            }
        }
        Ok(self.flush_budgeted_tcp_with_time(now_ms, budget, false))
    }

    fn driver_task_contract(&self) -> crate::hal::driver_task::DriverTaskContract {
        D::driver_task_contract()
    }

    fn telemetry(&self) -> NetTelemetry {
        self.telemetry()
    }

    fn stats(&self) -> NetCounters {
        self.current_counters()
    }

    fn drain_console_lines(&mut self, now_ms: u64, visitor: &mut dyn FnMut(ConsoleLine)) {
        let _ = self.drain_console_lines_bounded(now_ms, usize::MAX, visitor);
    }

    fn drain_console_lines_bounded(
        &mut self,
        now_ms: u64,
        max_lines: usize,
        visitor: &mut dyn FnMut(ConsoleLine),
    ) -> usize {
        if let Some((snapshot, reason)) = readiness::gate() {
            if !self.session_state.not_ready_logged {
                self.session_state.not_ready_logged = true;
                let flags = snapshot.render_flags();
                log::warn!(
                    "[net] not-ready gate tripped: want=console-line reason={} have={}",
                    reason,
                    flags.as_str()
                );
                let mut line = HeaplessString::<DEFAULT_LINE_CAPACITY>::new();
                let _ = write!(line, "ERR not-ready reason={reason}\r\n");
                let _ = self.server.enqueue_outbound(line.as_str());
            }
            self.server.drain_console_lines(now_ms, &mut |_line| {});
            return 0;
        }
        self.session_state.not_ready_logged = false;
        self.server
            .drain_console_lines_bounded(now_ms, max_lines, visitor)
    }

    fn ingest_snapshot(&self) -> IngestSnapshot {
        self.server.ingest_snapshot()
    }

    fn buffered_console_lines_pending(&self) -> bool {
        self.server.ingest_snapshot().queued != 0
    }

    fn send_console_line(&mut self, line: &str) -> bool {
        if !self.stage_policy.allow_console_io
            || !console_output_admitted_during_disconnect(self.disconnect_phase)
        {
            return false;
        }
        if self.stage_policy.allow_tcp {
            let now_ms = self.last_now_ms.unwrap_or(0);
            if !self.validate_console_socket(now_ms) {
                return false;
            }
        }
        #[cfg(feature = "cohesix-dev")]
        if line.starts_with("OK CAT") || line.starts_with("OK ECHO") || line == "END" {
            let socket = self.sockets.get::<TcpSocket>(self.tcp_handle);
            let tcp_state = tcp_state_label(socket.state());
            let send_queue = socket.send_queue();
            let send_capacity = socket.send_capacity();
            let mut message: HeaplessString<128> = HeaplessString::new();
            let _ = message.push_str("audit tcp.send.enqueue line=");
            let _ = message.push_str(if line.starts_with("OK CAT") {
                "OK CAT"
            } else if line.starts_with("OK ECHO") {
                "OK ECHO"
            } else {
                "END"
            });
            let _ = message.push_str(" state=");
            let _ = message.push_str(tcp_state);
            let _ = write!(
                message,
                " queue={}/{} active={} conn_id={:?}",
                send_queue, send_capacity, self.session_active, self.active_client_id
            );
            crate::debug_uart::debug_uart_line(message.as_str());
        }
        let enqueue_result = self.server.enqueue_outbound(line);
        if enqueue_result.is_err() {
            self.telemetry.tx_drops = self.telemetry.tx_drops.saturating_add(1);
            return false;
        }
        if TcpConsoleServer::is_priority_line(line)
            && self.active_client_id.is_some()
            && self.session_active
        {
            let Some(now_ms) = self.last_now_ms else {
                return true;
            };
            let socket = self.sockets.get_mut::<TcpSocket>(self.tcp_handle);
            let _ = Self::flush_outbound(
                &mut self.server,
                &mut self.outbound,
                &mut self.telemetry,
                &mut self.conn_bytes_written,
                &mut self.counters,
                socket,
                now_ms,
                self.active_client_id,
                self.auth_state,
                &mut self.session_state,
                MAX_CONSOLE_FRAMES_PER_POLL,
                MAX_CONSOLE_BYTES_PER_POLL,
            );
        }
        true
    }

    fn request_disconnect(&mut self) {
        if self.disconnect_phase == ConsoleDisconnectPhase::Idle {
            self.disconnect_phase = ConsoleDisconnectPhase::Draining;
            self.disconnect_reason = NetConsoleDisconnectReason::Quit;
            // Arm from the first service turn, not the request timestamp. A
            // delayed linked-runtime poll must still get one chance to flush
            // the queued QUIT response into smoltcp before expiry is judged.
            self.disconnect_phase_started_ms = None;
            let now_ms = self.last_now_ms.unwrap_or(0);
            let _ = self.service_console_standby(now_ms);
        }
    }

    fn console_output_drained(&self, conn_id: u64) -> bool {
        if self.active_client_id != Some(conn_id)
            || self.server.has_outbound()
            || self.outbound.has_pending()
        {
            return false;
        }
        self.sockets.get::<TcpSocket>(self.tcp_handle).send_queue() == 0
    }

    fn drain_console_events(&mut self, visitor: &mut dyn FnMut(NetConsoleEvent)) {
        let mut drained = HeaplessVec::<NetConsoleEvent, SOCKET_CAPACITY>::new();
        while let Some(event) = self.events.pop() {
            let _ = drained.push(event);
        }
        for event in drained {
            visitor(event);
        }
    }

    fn active_console_conn_id(&self) -> Option<u64> {
        self.active_client_id
    }

    fn authenticated_console_conn_id(&self) -> Option<u64> {
        self.active_client_id
            .filter(|_| self.server.is_authenticated())
    }

    fn console_service_pending(&self) -> bool {
        let active = self.sockets.get::<TcpSocket>(self.tcp_handle);
        let standby = self.sockets.get::<TcpSocket>(self.tcp_standby_handle);
        console_socket_service_pending(
            active.state(),
            active.recv_queue(),
            active.send_queue(),
            standby.state(),
            self.server.has_outbound(),
            self.outbound.has_pending(),
            self.disconnect_phase,
        )
    }

    fn icmp_echo_service_due(&self, now_ms: u64) -> bool {
        self.icmp_echo_service_due_at(now_ms)
    }

    fn cyw43_association_runtime_turn_pending(&self, now_ms: u64) -> bool {
        self.wifi_association_claims_runtime_turn(now_ms)
    }

    fn inject_console_line(&mut self, _line: &str) {}

    fn reset(&mut self) {
        #[cfg(feature = "kernel")]
        Self::clear_cyw43_authenticated_console_peer();
        let reset_now_ms = self.last_now_ms.unwrap_or(0);
        self.reset_icmp_echo_socket("stack-reset", reset_now_ms);
        self.abort_console_socket_pair("stack-reset");
        self.server.end_session();
        self.session_active = false;
        self.active_client_id = None;
        self.peer_endpoint = None;
        self.disconnect_phase = ConsoleDisconnectPhase::Idle;
        self.disconnect_phase_started_ms = None;
        self.disconnect_reason = NetConsoleDisconnectReason::Quit;
        self.disconnect_forced_aborts = 0;
        self.listener_announced = false;
        self.listener_defer_reason = None;
        self.auth_state = AuthState::Start;
        self.session_state = SessionState::default();
        self.conn_bytes_read = 0;
        self.conn_bytes_written = 0;
        self.events.clear();
        self.telemetry = NetTelemetry::default();
        self.outbound.reset();
        self.tcp_smoke_outbound_sent = false;
        self.tcp_smoke_outbound_connecting = false;
        self.tcp_smoke_last_attempt_ms = 0;
        self.tx_only_sent = false;
        self.self_test.console_probe_done = false;
        self.self_test.console_probe_banner_seen = false;
        self.self_test.console_probe_established = false;
        self.self_test.console_probe_started_ms = 0;
        self.self_test.console_probe_auth_sent = false;
        self.self_test.console_ok = false;
        self.last_now_ms = None;
        self.same_tick_poll_count = 0;
        self.time_stall_warned = false;
        self.budgeted_phase = BudgetedNetPhase::Interface;
        #[cfg(feature = "net-outbound-probe")]
        {
            self.probe_sent = false;
            self.probe_last_attempt_ms = 0;
            self.probe_fail_count = 0;
            self.probe_last_log_ms = 0;
            self.probe_hint_logged = false;
        }
    }

    fn start_self_test(&mut self, now_ms: u64) -> NetSelfTestStartResult {
        if !self.stage_policy.allow_selftest {
            return NetSelfTestStartResult::PolicyDisabled;
        }
        if !self.self_test.enabled {
            return NetSelfTestStartResult::SelfTestDisabled;
        }
        if let Some(status) = self.device.bringup_status_label() {
            if let Some(result) = NetSelfTestStartResult::from_bringup_status(status) {
                if !self.session_state.not_ready_logged {
                    self.session_state.not_ready_logged = true;
                    log::warn!("[net] not-ready gate tripped: want=net-selftest reason={status}");
                }
                return result;
            }
        }
        if matches!(self.mode, NetMode::Dhcp) && self.ip == Ipv4Address::UNSPECIFIED {
            if !self.session_state.not_ready_logged {
                self.session_state.not_ready_logged = true;
                log::warn!("[net] not-ready gate tripped: want=net-selftest reason=dhcp-pending");
            }
            return NetSelfTestStartResult::DhcpPending;
        }
        if let Some((snapshot, reason)) = readiness::gate() {
            if !self.session_state.not_ready_logged {
                self.session_state.not_ready_logged = true;
                let flags = snapshot.render_flags();
                log::warn!(
                    "[net] not-ready gate tripped: want=net-selftest reason={} have={}",
                    reason,
                    flags.as_str()
                );
            }
            return NetSelfTestStartResult::from_readiness_reason(reason);
        }
        self.session_state.not_ready_logged = false;
        let start_tx_complete = self.device.counters().tx_complete;
        if self.self_test.start(now_ms, start_tx_complete) {
            self.tcp_smoke_outbound_sent = false;
            self.tcp_smoke_outbound_connecting = false;
            self.tcp_smoke_last_attempt_ms = now_ms.saturating_sub(1_000);
            if !self.selftest_console_loopback_enabled() {
                self.self_test.console_probe_done = true;
                self.self_test.console_ok = true;
                info!(
                    "[net-selftest] console listener selftest skipped reason=hardware-direct-link proof=remote-cohsh"
                );
            }
            let udp_target = self.selftest_host_target(UDP_ECHO_PORT);
            let tcp_target = self.selftest_host_target(TCP_SMOKE_PORT);
            info!(
                "[net-selftest] starting run (udp dst={} tcp dst={})",
                udp_target.primary, tcp_target.primary
            );
            if udp_target.forwarded_hint || tcp_target.forwarded_hint {
                info!(
                    "[net-selftest] host capture (hostfwd/tunnel): tcpdump -i lo0 -n 'udp port {} or tcp port {}'",
                    UDP_ECHO_PORT, TCP_SMOKE_PORT
                );
                info!(
                    "[net-selftest] host udp echo (hostfwd/tunnel): echo -n \"ping\" | nc -u -w1 {}",
                    udp_target.primary
                );
                info!(
                    "[net-selftest] host tcp smoke (hostfwd/tunnel): printf \"hi\" | nc -v {}",
                    tcp_target.primary
                );
                info!(
                    "[net-selftest] direct guest access requires bridge/tap networking; guest addr {}",
                    udp_target.direct
                );
            } else if self.backend.uses_dev_virt_defaults() {
                info!(
                    "[net-selftest] host capture (qemu hostfwd): tcpdump -i lo0 -n 'udp port {} or tcp port {}'",
                    UDP_ECHO_PORT, TCP_SMOKE_PORT
                );
                info!(
                    "[net-selftest] qemu user-net without hostfwd → add hostfwd=tcp::31338-:31338,hostfwd=tcp::31339-:31339 and use localhost",
                );
                info!(
                    "[net-selftest] host udp echo (after hostfwd): echo -n \"ping\" | nc -u -w1 {}",
                    udp_target.loopback
                );
                info!(
                    "[net-selftest] host tcp smoke (after hostfwd): printf \"hi\" | nc -v {}",
                    tcp_target.loopback
                );
                info!(
                    "[net-selftest] direct guest address {} requires bridge/tap networking; skip on slirp",
                    udp_target.direct
                );
            } else {
                info!(
                    "[net-selftest] host capture (direct-link): tcpdump -ni <host-iface> 'arp or udp port {} or tcp port {}'",
                    UDP_ECHO_PORT, TCP_SMOKE_PORT
                );
                info!(
                    "[net-selftest] static profile target udp={} tcp={}",
                    udp_target.primary, tcp_target.primary
                );
                info!(
                    "[net-selftest] outbound gateway smoke is peer-assisted on direct hardware; remote cohsh plus netstats are authoritative"
                );
                info!(
                    "[net-selftest] host udp echo: echo -n \"ping\" | nc -u -w1 {}",
                    udp_target.primary
                );
                info!(
                    "[net-selftest] host tcp smoke: printf \"hi\" | nc -v {}",
                    tcp_target.primary
                );
            }
            NetSelfTestStartResult::Started
        } else {
            NetSelfTestStartResult::SelfTestDisabled
        }
    }

    fn console_listen_port(&self) -> u16 {
        self.listen_port
    }

    fn console_listener_ready(&self) -> bool {
        net_console_listener_ready(
            self.stage_policy.allow_tcp,
            self.listener_announced,
            self.listener_defer_reason.is_some(),
            self.wifi_rx_admission_blocked,
        )
    }

    fn self_test_report(&self) -> NetSelfTestReport {
        let udp_target = self.selftest_host_target(UDP_ECHO_PORT);
        let tcp_target = self.selftest_host_target(TCP_SMOKE_PORT);
        NetSelfTestReport {
            enabled: self.self_test.enabled,
            running: self.self_test.running,
            run_generation: self.self_test.run_generation,
            last_result: self.self_test.last_result,
            backend: self.active_driver_label(),
            udp_target: udp_target.primary,
            tcp_target: tcp_target.primary,
        }
    }

    fn status_report(&self) -> NetStatusReport {
        let mut ip = HeaplessString::<32>::new();
        let _ = write!(&mut ip, "{}", self.ip);
        let mut gateway = HeaplessString::<32>::new();
        let _ = write!(
            &mut gateway,
            "{}",
            self.gateway.unwrap_or(Ipv4Address::UNSPECIFIED)
        );
        let active_interface = self.device.interface_label();
        let active_driver = self.active_driver_label();
        let current_counters = self.current_counters();
        let bringup_status = self.device.bringup_status_label();
        let cyw43_blocker =
            cyw43_status_blocker_for(active_driver, active_interface, current_counters);
        let (address_source, dhcp_phase) = if self.wifi_rx_admission_blocked {
            ("wifi-data-rx-admission-blocked", "rx-admission-blocked")
        } else if let Some(status) = bringup_status {
            let phase = if matches!(self.mode, NetMode::Dhcp) {
                dhcp_phase_for_bringup_status(status)
            } else {
                "disabled"
            };
            (status, phase)
        } else if let Some(blocker) = cyw43_blocker {
            (blocker.address_source, blocker.dhcp_phase)
        } else {
            match self.dhcp.as_ref() {
                Some(client) => {
                    let status = client.status();
                    let source = if self.ip == Ipv4Address::UNSPECIFIED {
                        if status.failure.is_some() {
                            "dhcp-failed"
                        } else {
                            "dhcp-pending"
                        }
                    } else {
                        "dhcp-lease"
                    };
                    (source, status.phase.as_str())
                }
                None if self.backend.uses_dev_virt_defaults() => ("dev-virt", "disabled"),
                None if matches!(self.mode, NetMode::Static) => ("manifest-static", "disabled"),
                None => ("dhcp-uninitialized", "disabled"),
            }
        };
        let standby_interface = match (self.interface_policy, self.backend, active_interface) {
            (NetInterfacePolicy::Auto, NetBackend::BcmGenet, "wifi") => "wired",
            (NetInterfacePolicy::Auto, NetBackend::BcmGenet, _) => "wifi",
            (NetInterfacePolicy::Auto, _, _) => "wired",
            _ => "none",
        };
        let listener_ready = self.console_listener_ready();
        let tcp_ready = net_status_tcp_ready(
            listener_ready && cyw43_blocker.is_none(),
            active_driver,
            active_interface,
            current_counters,
        );
        NetStatusReport {
            profile_backend: self.backend.label(),
            backend: active_driver,
            active_driver,
            mode: self.mode.as_str(),
            interface_policy: self.interface_policy.as_str(),
            active_interface,
            standby_interface,
            address_source,
            ip,
            gateway,
            dhcp_phase,
            tcp_ready,
        }
    }
}

#[cfg(feature = "kernel")]
const NET_RING_FLAG_BUDGETED: u16 = 1;
const GENET_DRIVER_TASK_PRE_POLL_BURST_LIMIT: usize = 8;
// A steady CYW43 op8 returns one committed batch containing up to eight
// wire-order frames. Root therefore issues at most one parent here; durable
// queue state, rather than a synthetic loop of per-frame parents, schedules a
// later batch only when the runtime reports remaining work.
const CYW43_DRIVER_TASK_PRE_POLL_BURST_LIMIT: usize = 1;
const DEFAULT_DRIVER_TASK_PRE_POLL_BURST_LIMIT: usize = 1;
const DRIVER_TASK_PRE_POLL_TURN_BYTES: u32 = 2048;
const GENET_TCP_FAST_PATH_EXTRA_TURNS: usize = 1;
const GENET_TCP_POST_DISPATCH_EXTRA_TURNS: usize = 2;

#[cfg(feature = "kernel")]
struct NetDriverTaskContext<D: NetDevice> {
    stack: usize,
    budget: usize,
    now_ms: u64,
    _marker: core::marker::PhantomData<fn() -> D>,
}

#[cfg(feature = "kernel")]
fn net_driver_task_hot_path(
    contract: crate::hal::driver_task::DriverTaskContract,
) -> Option<crate::hal::driver_task::DriverTaskHotPath> {
    if contract == crate::hal::driver_task::GENET_DRIVER_TASK_CONTRACT {
        Some(crate::hal::driver_task::DriverTaskHotPath::GenetNic)
    } else if contract == crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT {
        Some(crate::hal::driver_task::DriverTaskHotPath::Cyw43Wifi)
    } else {
        None
    }
}

#[cfg(feature = "kernel")]
fn driver_task_pre_poll_burst_limit(hot_path: crate::hal::driver_task::DriverTaskHotPath) -> usize {
    match hot_path {
        crate::hal::driver_task::DriverTaskHotPath::GenetNic => {
            GENET_DRIVER_TASK_PRE_POLL_BURST_LIMIT
        }
        crate::hal::driver_task::DriverTaskHotPath::Cyw43Wifi => {
            CYW43_DRIVER_TASK_PRE_POLL_BURST_LIMIT
        }
        _ => DEFAULT_DRIVER_TASK_PRE_POLL_BURST_LIMIT,
    }
}

#[cfg(feature = "kernel")]
const fn genet_tcp_flush_pre_poll_enabled(
    hot_path: crate::hal::driver_task::DriverTaskHotPath,
) -> bool {
    matches!(
        hot_path,
        crate::hal::driver_task::DriverTaskHotPath::GenetNic
    )
}

#[cfg(feature = "kernel")]
fn driver_task_pre_poll_command(
    contract: crate::hal::driver_task::DriverTaskContract,
    hot_path: crate::hal::driver_task::DriverTaskHotPath,
    flags: u16,
) -> crate::hal::driver_task::DriverTaskCommandRecord {
    let frame_flags = match hot_path {
        crate::hal::driver_task::DriverTaskHotPath::Cyw43Wifi => {
            flags | crate::hal::driver_task::DRIVER_TASK_RING_FLAG_QUIET_HOT_PATH
        }
        _ => flags,
    };
    crate::hal::driver_task::DriverTaskCommandRecord::pi4_hot_path(
        0,
        hot_path,
        crate::hal::driver_task::DriverTaskBudgetGrant::from_contract(contract),
        crate::hal::driver_task::DriverFrameDescriptor {
            offset: 0,
            len: 0,
            flags: frame_flags,
        },
    )
}

#[cfg(feature = "kernel")]
fn driver_task_pre_poll_completion_can_continue(
    completion: &crate::hal::driver_task::DriverTaskCompletionRecord,
) -> bool {
    completion.code == crate::hal::driver_task::DriverTaskCompletionCode::FrameReady.as_u16()
        || (completion.code == crate::hal::driver_task::DriverTaskCompletionCode::Progress.as_u16()
            && completion.result != 0)
}

#[cfg(feature = "kernel")]
fn service_driver_task_pre_poll_once(
    contract: crate::hal::driver_task::DriverTaskContract,
    hot_path: crate::hal::driver_task::DriverTaskHotPath,
    flags: u16,
) -> Option<(bool, bool)> {
    let completion = if hot_path == crate::hal::driver_task::DriverTaskHotPath::Cyw43Wifi {
        crate::drivers::driver_task_net::poll_cyw43_driver_task_steady_data_completion(contract)?
    } else {
        let command = driver_task_pre_poll_command(contract, hot_path, flags);
        run_net_driver_task_ring_service(contract, command)?
    };
    let keep_draining = driver_task_pre_poll_completion_can_continue(&completion);
    let progress = crate::drivers::driver_task_net::preserve_driver_task_pre_poll_completion(
        contract, hot_path, completion,
    );
    Some((progress, keep_draining))
}

#[cfg(feature = "kernel")]
fn service_driver_task_pre_poll_burst(
    contract: crate::hal::driver_task::DriverTaskContract,
    hot_path: crate::hal::driver_task::DriverTaskHotPath,
    flags: u16,
) -> bool {
    let mut activity = false;
    for _ in 0..driver_task_pre_poll_burst_limit(hot_path) {
        let Some((progress, keep_draining)) =
            service_driver_task_pre_poll_once(contract, hot_path, flags)
        else {
            break;
        };
        activity |= progress;
        if !keep_draining {
            break;
        }
    }
    activity
}

#[cfg(feature = "kernel")]
fn service_driver_task_pre_poll_burst_budgeted(
    contract: crate::hal::driver_task::DriverTaskContract,
    hot_path: crate::hal::driver_task::DriverTaskHotPath,
    flags: u16,
    budget: &mut DriverServiceBudget,
) -> bool {
    let mut activity = false;
    for _ in 0..driver_task_pre_poll_burst_limit(hot_path) {
        if budget.charge_ops(4).is_err()
            || budget.charge_frames(1).is_err()
            || budget
                .charge_bytes(DRIVER_TASK_PRE_POLL_TURN_BYTES)
                .is_err()
        {
            break;
        }
        let Some((progress, keep_draining)) =
            service_driver_task_pre_poll_once(contract, hot_path, flags)
        else {
            break;
        };
        activity |= progress;
        if !keep_draining {
            break;
        }
    }
    activity
}

#[cfg(feature = "kernel")]
fn run_net_driver_task_ring_service(
    contract: crate::hal::driver_task::DriverTaskContract,
    command: crate::hal::driver_task::DriverTaskCommandRecord,
) -> Option<crate::hal::driver_task::DriverTaskCompletionRecord> {
    if crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active() {
        crate::hal::driver_task::run_driver_task_ring_service_nonblocking(contract, command)
    } else {
        crate::hal::driver_task::run_driver_task_ring_service(contract, command)
    }
}

#[cfg(feature = "kernel")]
unsafe fn net_driver_task_runtime_ring_service(
    context: usize,
    command: crate::hal::driver_task::DriverTaskCommandRecord,
) -> crate::hal::driver_task::DriverTaskCompletionRecord {
    // This compatibility wrapper preserves the exact registered service ABI
    // and forwards the primitive selector context unchanged.
    crate::drivers::driver_task_net::runtime_ring_service(context, command)
}

#[cfg(feature = "kernel")]
unsafe fn net_poll_driver_task<D: NetDevice>(context: usize) -> usize {
    // SAFETY: `context` is built by `NetStack::poll`; root waits synchronously
    // while the active NIC driver TCB borrows the stack.
    let task = unsafe { &mut *(context as *mut NetDriverTaskContext<D>) };
    // SAFETY: `stack` is the `self` pointer from the caller and is exclusively
    // borrowed for the duration of this synchronous driver-task callback.
    let stack = unsafe { &mut *(task.stack as *mut NetStack<D>) };
    stack.poll_with_time(task.now_ms) as usize
}

#[cfg(feature = "kernel")]
unsafe fn net_poll_budgeted_driver_task<D: NetDevice>(context: usize) -> usize {
    // SAFETY: `context` is built by `NetStack::poll_with_budget`; root waits
    // synchronously while the active NIC driver TCB borrows the stack/budget.
    let task = unsafe { &mut *(context as *mut NetDriverTaskContext<D>) };
    // SAFETY: `stack` and `budget` are caller-owned objects borrowed
    // exclusively until the synchronous driver-task callback returns.
    let stack = unsafe { &mut *(task.stack as *mut NetStack<D>) };
    let budget = unsafe { &mut *(task.budget as *mut DriverServiceBudget) };
    pack_net_poll_result(stack.poll_budgeted_with_time(task.now_ms, budget))
}

#[cfg(feature = "kernel")]
fn pack_net_poll_result(result: Result<bool, DriverServiceBudgetError>) -> usize {
    match result {
        Ok(false) => 0,
        Ok(true) => 1,
        Err(DriverServiceBudgetError::ZeroCharge) => 0x100,
        Err(DriverServiceBudgetError::OperationsExhausted) => 0x101,
        Err(DriverServiceBudgetError::BytesExhausted) => 0x102,
        Err(DriverServiceBudgetError::FramesExhausted) => 0x103,
        Err(DriverServiceBudgetError::BlockingForbidden) => 0x104,
        Err(DriverServiceBudgetError::BlockingExhausted) => 0x105,
    }
}

#[cfg(feature = "kernel")]
fn unpack_net_poll_result(word: usize) -> Result<bool, DriverServiceBudgetError> {
    match word {
        0 => Ok(false),
        1 => Ok(true),
        0x100 => Err(DriverServiceBudgetError::ZeroCharge),
        0x101 => Err(DriverServiceBudgetError::OperationsExhausted),
        0x102 => Err(DriverServiceBudgetError::BytesExhausted),
        0x103 => Err(DriverServiceBudgetError::FramesExhausted),
        0x104 => Err(DriverServiceBudgetError::BlockingForbidden),
        0x105 => Err(DriverServiceBudgetError::BlockingExhausted),
        _ => Err(DriverServiceBudgetError::OperationsExhausted),
    }
}

impl NetPoller for DefaultNetStack {
    fn poll(&mut self, now_ms: u64) -> bool {
        match self {
            Self::Rtl8139(stack) => stack.poll(now_ms),
            Self::GenetDriverTask(stack) => stack.poll(now_ms),
            Self::Cyw43DriverTask(stack) => stack.poll(now_ms),
            #[cfg(feature = "net-backend-virtio")]
            Self::Virtio(stack) => stack.poll(now_ms),
        }
    }

    fn poll_with_budget(
        &mut self,
        now_ms: u64,
        budget: &mut DriverServiceBudget,
    ) -> Result<bool, DriverServiceBudgetError> {
        match self {
            Self::Rtl8139(stack) => stack.poll_with_budget(now_ms, budget),
            Self::GenetDriverTask(stack) => stack.poll_with_budget(now_ms, budget),
            Self::Cyw43DriverTask(stack) => stack.poll_with_budget(now_ms, budget),
            #[cfg(feature = "net-backend-virtio")]
            Self::Virtio(stack) => stack.poll_with_budget(now_ms, budget),
        }
    }

    fn flush_tcp_with_budget(
        &mut self,
        now_ms: u64,
        budget: &mut DriverServiceBudget,
    ) -> Result<bool, DriverServiceBudgetError> {
        match self {
            Self::Rtl8139(stack) => stack.flush_tcp_with_budget(now_ms, budget),
            Self::GenetDriverTask(stack) => stack.flush_tcp_with_budget(now_ms, budget),
            Self::Cyw43DriverTask(stack) => stack.flush_tcp_with_budget(now_ms, budget),
            #[cfg(feature = "net-backend-virtio")]
            Self::Virtio(stack) => stack.flush_tcp_with_budget(now_ms, budget),
        }
    }

    fn driver_task_contract(&self) -> crate::hal::driver_task::DriverTaskContract {
        match self {
            Self::Rtl8139(stack) => stack.driver_task_contract(),
            Self::GenetDriverTask(stack) => stack.driver_task_contract(),
            Self::Cyw43DriverTask(stack) => stack.driver_task_contract(),
            #[cfg(feature = "net-backend-virtio")]
            Self::Virtio(stack) => stack.driver_task_contract(),
        }
    }

    fn telemetry(&self) -> NetTelemetry {
        match self {
            Self::Rtl8139(stack) => stack.telemetry(),
            Self::GenetDriverTask(stack) => stack.telemetry(),
            Self::Cyw43DriverTask(stack) => stack.telemetry(),
            #[cfg(feature = "net-backend-virtio")]
            Self::Virtio(stack) => stack.telemetry(),
        }
    }

    fn stats(&self) -> NetCounters {
        match self {
            Self::Rtl8139(stack) => stack.stats(),
            Self::GenetDriverTask(stack) => stack.stats(),
            Self::Cyw43DriverTask(stack) => stack.stats(),
            #[cfg(feature = "net-backend-virtio")]
            Self::Virtio(stack) => stack.stats(),
        }
    }

    fn drain_console_lines(&mut self, now_ms: u64, visitor: &mut dyn FnMut(ConsoleLine)) {
        let _ = self.drain_console_lines_bounded(now_ms, usize::MAX, visitor);
    }

    fn drain_console_lines_bounded(
        &mut self,
        now_ms: u64,
        max_lines: usize,
        visitor: &mut dyn FnMut(ConsoleLine),
    ) -> usize {
        match self {
            Self::Rtl8139(stack) => stack.drain_console_lines_bounded(now_ms, max_lines, visitor),
            Self::GenetDriverTask(stack) => {
                stack.drain_console_lines_bounded(now_ms, max_lines, visitor)
            }
            Self::Cyw43DriverTask(stack) => {
                stack.drain_console_lines_bounded(now_ms, max_lines, visitor)
            }
            #[cfg(feature = "net-backend-virtio")]
            Self::Virtio(stack) => stack.drain_console_lines_bounded(now_ms, max_lines, visitor),
        }
    }

    fn send_console_line(&mut self, line: &str) -> bool {
        match self {
            Self::Rtl8139(stack) => stack.send_console_line(line),
            Self::GenetDriverTask(stack) => stack.send_console_line(line),
            Self::Cyw43DriverTask(stack) => stack.send_console_line(line),
            #[cfg(feature = "net-backend-virtio")]
            Self::Virtio(stack) => stack.send_console_line(line),
        }
    }

    fn request_disconnect(&mut self) {
        match self {
            Self::Rtl8139(stack) => stack.request_disconnect(),
            Self::GenetDriverTask(stack) => stack.request_disconnect(),
            Self::Cyw43DriverTask(stack) => stack.request_disconnect(),
            #[cfg(feature = "net-backend-virtio")]
            Self::Virtio(stack) => stack.request_disconnect(),
        }
    }

    fn console_output_drained(&self, conn_id: u64) -> bool {
        match self {
            Self::Rtl8139(stack) => stack.console_output_drained(conn_id),
            Self::GenetDriverTask(stack) => stack.console_output_drained(conn_id),
            Self::Cyw43DriverTask(stack) => stack.console_output_drained(conn_id),
            #[cfg(feature = "net-backend-virtio")]
            Self::Virtio(stack) => stack.console_output_drained(conn_id),
        }
    }

    fn drain_console_events(&mut self, visitor: &mut dyn FnMut(NetConsoleEvent)) {
        match self {
            Self::Rtl8139(stack) => stack.drain_console_events(visitor),
            Self::GenetDriverTask(stack) => stack.drain_console_events(visitor),
            Self::Cyw43DriverTask(stack) => stack.drain_console_events(visitor),
            #[cfg(feature = "net-backend-virtio")]
            Self::Virtio(stack) => stack.drain_console_events(visitor),
        }
    }

    fn ingest_snapshot(&self) -> IngestSnapshot {
        match self {
            Self::Rtl8139(stack) => stack.ingest_snapshot(),
            Self::GenetDriverTask(stack) => stack.ingest_snapshot(),
            Self::Cyw43DriverTask(stack) => stack.ingest_snapshot(),
            #[cfg(feature = "net-backend-virtio")]
            Self::Virtio(stack) => stack.ingest_snapshot(),
        }
    }

    fn buffered_console_lines_pending(&self) -> bool {
        match self {
            Self::Rtl8139(stack) => stack.buffered_console_lines_pending(),
            Self::GenetDriverTask(stack) => stack.buffered_console_lines_pending(),
            Self::Cyw43DriverTask(stack) => stack.buffered_console_lines_pending(),
            #[cfg(feature = "net-backend-virtio")]
            Self::Virtio(stack) => stack.buffered_console_lines_pending(),
        }
    }

    fn active_console_conn_id(&self) -> Option<u64> {
        match self {
            Self::Rtl8139(stack) => stack.active_console_conn_id(),
            Self::GenetDriverTask(stack) => stack.active_console_conn_id(),
            Self::Cyw43DriverTask(stack) => stack.active_console_conn_id(),
            #[cfg(feature = "net-backend-virtio")]
            Self::Virtio(stack) => stack.active_console_conn_id(),
        }
    }

    fn authenticated_console_conn_id(&self) -> Option<u64> {
        match self {
            Self::Rtl8139(stack) => stack.authenticated_console_conn_id(),
            Self::GenetDriverTask(stack) => stack.authenticated_console_conn_id(),
            Self::Cyw43DriverTask(stack) => stack.authenticated_console_conn_id(),
            #[cfg(feature = "net-backend-virtio")]
            Self::Virtio(stack) => stack.authenticated_console_conn_id(),
        }
    }

    fn console_service_pending(&self) -> bool {
        match self {
            Self::Rtl8139(stack) => stack.console_service_pending(),
            Self::GenetDriverTask(stack) => stack.console_service_pending(),
            Self::Cyw43DriverTask(stack) => stack.console_service_pending(),
            #[cfg(feature = "net-backend-virtio")]
            Self::Virtio(stack) => stack.console_service_pending(),
        }
    }

    fn icmp_echo_service_due(&self, now_ms: u64) -> bool {
        match self {
            Self::Rtl8139(stack) => stack.icmp_echo_service_due(now_ms),
            Self::GenetDriverTask(stack) => stack.icmp_echo_service_due(now_ms),
            Self::Cyw43DriverTask(stack) => stack.icmp_echo_service_due(now_ms),
            #[cfg(feature = "net-backend-virtio")]
            Self::Virtio(stack) => stack.icmp_echo_service_due(now_ms),
        }
    }

    fn cyw43_association_runtime_turn_pending(&self, now_ms: u64) -> bool {
        match self {
            Self::Rtl8139(stack) => stack.cyw43_association_runtime_turn_pending(now_ms),
            Self::GenetDriverTask(stack) => stack.cyw43_association_runtime_turn_pending(now_ms),
            Self::Cyw43DriverTask(stack) => stack.cyw43_association_runtime_turn_pending(now_ms),
            #[cfg(feature = "net-backend-virtio")]
            Self::Virtio(stack) => stack.cyw43_association_runtime_turn_pending(now_ms),
        }
    }

    fn inject_console_line(&mut self, line: &str) {
        match self {
            Self::Rtl8139(stack) => stack.inject_console_line(line),
            Self::GenetDriverTask(stack) => stack.inject_console_line(line),
            Self::Cyw43DriverTask(stack) => stack.inject_console_line(line),
            #[cfg(feature = "net-backend-virtio")]
            Self::Virtio(stack) => stack.inject_console_line(line),
        }
    }

    fn reset(&mut self) {
        match self {
            Self::Rtl8139(stack) => stack.reset(),
            Self::GenetDriverTask(stack) => stack.reset(),
            Self::Cyw43DriverTask(stack) => stack.reset(),
            #[cfg(feature = "net-backend-virtio")]
            Self::Virtio(stack) => stack.reset(),
        }
    }

    fn console_listen_port(&self) -> u16 {
        match self {
            Self::Rtl8139(stack) => stack.console_listen_port(),
            Self::GenetDriverTask(stack) => stack.console_listen_port(),
            Self::Cyw43DriverTask(stack) => stack.console_listen_port(),
            #[cfg(feature = "net-backend-virtio")]
            Self::Virtio(stack) => stack.console_listen_port(),
        }
    }

    fn console_listener_ready(&self) -> bool {
        match self {
            Self::Rtl8139(stack) => stack.console_listener_ready(),
            Self::GenetDriverTask(stack) => stack.console_listener_ready(),
            Self::Cyw43DriverTask(stack) => stack.console_listener_ready(),
            #[cfg(feature = "net-backend-virtio")]
            Self::Virtio(stack) => stack.console_listener_ready(),
        }
    }

    fn start_self_test(&mut self, now_ms: u64) -> NetSelfTestStartResult {
        match self {
            Self::Rtl8139(stack) => stack.start_self_test(now_ms),
            Self::GenetDriverTask(stack) => stack.start_self_test(now_ms),
            Self::Cyw43DriverTask(stack) => stack.start_self_test(now_ms),
            #[cfg(feature = "net-backend-virtio")]
            Self::Virtio(stack) => stack.start_self_test(now_ms),
        }
    }

    fn self_test_report(&self) -> NetSelfTestReport {
        match self {
            Self::Rtl8139(stack) => stack.self_test_report(),
            Self::GenetDriverTask(stack) => stack.self_test_report(),
            Self::Cyw43DriverTask(stack) => stack.self_test_report(),
            #[cfg(feature = "net-backend-virtio")]
            Self::Virtio(stack) => stack.self_test_report(),
        }
    }

    fn status_report(&self) -> NetStatusReport {
        match self {
            Self::Rtl8139(stack) => stack.status_report(),
            Self::GenetDriverTask(stack) => stack.status_report(),
            Self::Cyw43DriverTask(stack) => stack.status_report(),
            #[cfg(feature = "net-backend-virtio")]
            Self::Virtio(stack) => stack.status_report(),
        }
    }
}

/// Cooperative polling loop that mirrors the serial console onto the TCP port.
pub fn run_tcp_console<D: NetDevice>(
    console: &mut crate::console::Console,
    stack: &mut NetStack<D>,
) -> ! {
    use core::fmt::Write as _;

    let mut now_ms = 0u64;
    loop {
        let _ = stack.poll_with_time(now_ms);
        stack.server.drain_console_lines(now_ms, &mut |line| {
            let _ = writeln!(console, "{}", line.text);
        });
        now_ms = now_ms.saturating_add(5);
    }
}

#[cfg(test)]
static NET_STACK_STORAGE_TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
fn reset_console_storage_state() {
    // SAFETY: Every caller holds `NET_STACK_STORAGE_TEST_LOCK`, and the
    // preceding test-owned `NetStack` has been dropped before its guard resets
    // this singleton. No socket or buffer reference can remain live here.
    unsafe {
        SOCKET_STORAGE = [SocketStorage::EMPTY; SOCKET_CAPACITY];
    }
    SOCKET_STORAGE_IN_USE.store(false, Ordering::Release);
    SOCKET_STORAGE_OWNER.store(0, Ordering::Release);
    SOCKET_STORAGE_TAG_ID.store(0, Ordering::Release);
    *SOCKET_STORAGE_TAG_LABEL.lock() = None;

    ICMP_ECHO_STORAGE_IN_USE.store(false, Ordering::Release);
    ICMP_ECHO_STORAGE_OWNER.store(0, Ordering::Release);
    ICMP_ECHO_STORAGE_TAG_ID.store(0, Ordering::Release);
    *ICMP_ECHO_STORAGE_TAG_LABEL.lock() = None;

    TCP_RX_STORAGE_IN_USE.store(false, Ordering::Release);
    TCP_RX_STORAGE_OWNER.store(0, Ordering::Release);
    TCP_RX_STORAGE_TAG_ID.store(0, Ordering::Release);
    *TCP_RX_STORAGE_TAG_LABEL.lock() = None;

    TCP_TX_STORAGE_IN_USE.store(false, Ordering::Release);
    TCP_TX_STORAGE_OWNER.store(0, Ordering::Release);
    TCP_TX_STORAGE_TAG_ID.store(0, Ordering::Release);
    *TCP_TX_STORAGE_TAG_LABEL.lock() = None;

    TCP_STANDBY_RX_STORAGE_IN_USE.store(false, Ordering::Release);
    TCP_STANDBY_RX_STORAGE_OWNER.store(0, Ordering::Release);
    TCP_STANDBY_RX_STORAGE_TAG_ID.store(0, Ordering::Release);
    *TCP_STANDBY_RX_STORAGE_TAG_LABEL.lock() = None;

    TCP_STANDBY_TX_STORAGE_IN_USE.store(false, Ordering::Release);
    TCP_STANDBY_TX_STORAGE_OWNER.store(0, Ordering::Release);
    TCP_STANDBY_TX_STORAGE_TAG_ID.store(0, Ordering::Release);
    *TCP_STANDBY_TX_STORAGE_TAG_LABEL.lock() = None;
}

/// Serialize one test-owned production `NetStack` and reset its singleton.
#[cfg(test)]
pub struct TestNetStackStateGuard {
    _lock: spin::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl TestNetStackStateGuard {
    pub(crate) fn acquire() -> Self {
        let lock = NET_STACK_STORAGE_TEST_LOCK.lock();
        NETSTACK_STATE.store(NET_STATE_NEVER, Ordering::Release);
        reset_console_storage_state();
        Self { _lock: lock }
    }
}

#[cfg(test)]
impl Drop for TestNetStackStateGuard {
    fn drop(&mut self) {
        NETSTACK_STATE.store(NET_STATE_NEVER, Ordering::Release);
        reset_console_storage_state();
    }
}

#[cfg(test)]
mod tests {
    use core::convert::Infallible;

    use super::*;
    use smoltcp::phy::{Loopback, Medium};

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_oldgood_dhcp_receipt_is_exact_generation_bound_and_resettable() {
        let _guard = NET_STACK_STORAGE_TEST_LOCK.lock();
        clear_cyw43_oldgood_dhcp_receipt();
        record_cyw43_oldgood_dhcp_start(7, u32::MAX, u64::MAX);
        assert_eq!(
            *CYW43_OLDGOOD_DHCP_RECEIPT.lock(),
            Some(Cyw43OldgoodDhcpReceipt {
                generation: 7,
                transaction_id: u32::MAX,
                start_now_ms: u64::MAX,
                ip: [0; 4],
                prefix_len: 0,
                gateway: [0; 4],
                server_id: [0; 4],
                lease_seconds: 0,
                bound: false,
            }),
        );
        let lease = DhcpLease {
            ip: [192, 168, 86, 154],
            prefix_len: 24,
            gateway: Some([192, 168, 86, 1]),
            server_id: [192, 168, 86, 1],
            lease_seconds: u32::MAX,
        };
        record_cyw43_oldgood_dhcp_bound(7, &lease);
        let receipt = CYW43_OLDGOOD_DHCP_RECEIPT
            .lock()
            .expect("matching DHCP generation is retained");
        assert!(receipt.bound);
        assert_eq!(receipt.transaction_id, u32::MAX);
        assert_eq!(receipt.start_now_ms, u64::MAX);
        assert_eq!(receipt.lease_seconds, u32::MAX);

        record_cyw43_oldgood_dhcp_start(8, 1, 2);
        record_cyw43_oldgood_dhcp_bound(9, &lease);
        assert_eq!(*CYW43_OLDGOOD_DHCP_RECEIPT.lock(), None);
        record_cyw43_oldgood_dhcp_start(0, 1, 2);
        assert_eq!(*CYW43_OLDGOOD_DHCP_RECEIPT.lock(), None);
        clear_cyw43_oldgood_dhcp_receipt();
    }

    #[test]
    fn console_tcp_socket_disables_delayed_ack() {
        let rx = std::boxed::Box::leak(std::boxed::Box::new([0u8; 64]));
        let tx = std::boxed::Box::leak(std::boxed::Box::new([0u8; 64]));
        let socket = new_console_tcp_socket(rx, tx);

        assert_eq!(
            socket.ack_delay(),
            None,
            "interactive ACKs must remain in the receive-coupled TX poll",
        );
    }

    #[test]
    fn console_disconnect_waits_for_smoltcp_send_queue_ack() {
        let started_ms = 100;
        assert_eq!(
            console_disconnect_action(
                ConsoleDisconnectPhase::Draining,
                TcpState::Established,
                true,
                false,
                false,
                started_ms + 1,
                started_ms,
            ),
            ConsoleDisconnectAction::Wait
        );
        assert_eq!(
            console_disconnect_action(
                ConsoleDisconnectPhase::Draining,
                TcpState::Established,
                false,
                true,
                false,
                started_ms + 1,
                started_ms,
            ),
            ConsoleDisconnectAction::Wait
        );
        assert_eq!(
            console_disconnect_action(
                ConsoleDisconnectPhase::Draining,
                TcpState::Established,
                true,
                true,
                false,
                started_ms + 1,
                started_ms,
            ),
            ConsoleDisconnectAction::StartClose
        );
    }

    #[test]
    fn console_disconnect_waits_for_inbound_dispatch_after_peer_fin() {
        assert!(!console_disconnect_application_queues_drained(
            false, false, 1, false,
        ));
        assert!(!console_disconnect_application_queues_drained(
            false, false, 0, true,
        ));
        assert!(console_disconnect_application_queues_drained(
            false, false, 0, false,
        ));
    }

    #[test]
    fn orderly_console_shutdowns_enter_one_draining_lane() {
        for reason in [
            NetConsoleDisconnectReason::Quit,
            NetConsoleDisconnectReason::Eof,
            NetConsoleDisconnectReason::Error,
        ] {
            let mut phase = ConsoleDisconnectPhase::Idle;
            let mut started_ms = Some(99);
            let mut active_reason = NetConsoleDisconnectReason::Reset;
            let mut entered_this_turn = false;
            assert!(begin_console_disconnect(
                &mut phase,
                &mut started_ms,
                &mut active_reason,
                &mut entered_this_turn,
                reason,
            ));
            assert_eq!(phase, ConsoleDisconnectPhase::Draining);
            assert_eq!(started_ms, None);
            assert_eq!(active_reason, reason);
            assert!(entered_this_turn);
        }

        let mut phase = ConsoleDisconnectPhase::Closing;
        let mut started_ms = Some(41);
        let mut active_reason = NetConsoleDisconnectReason::Eof;
        let mut entered_this_turn = false;
        assert!(!begin_console_disconnect(
            &mut phase,
            &mut started_ms,
            &mut active_reason,
            &mut entered_this_turn,
            NetConsoleDisconnectReason::Error,
        ));
        assert_eq!(phase, ConsoleDisconnectPhase::Closing);
        assert_eq!(started_ms, Some(41));
        assert_eq!(active_reason, NetConsoleDisconnectReason::Eof);
        assert!(!entered_this_turn);
    }

    #[test]
    fn socket_capacity_covers_full_profile_with_outbound_probe() {
        const FULL_PROFILE_WITH_OUTBOUND_PROBE: usize = 1 // raw ICMP echo responder
            + 2 // active + standby console
            + 1 // DHCP
            + 2 // UDP beacon + echo
            + 2 // TCP smoke listener + outbound
            + 1; // outbound probe
        assert!(SOCKET_CAPACITY >= FULL_PROFILE_WITH_OUTBOUND_PROBE);
    }

    #[test]
    fn console_disconnect_tracks_fin_handshake_before_relisten() {
        assert!(console_output_admitted_during_disconnect(
            ConsoleDisconnectPhase::Draining
        ));
        assert!(!console_output_admitted_during_disconnect(
            ConsoleDisconnectPhase::PeerCloseWait
        ));
        assert!(!console_output_admitted_during_disconnect(
            ConsoleDisconnectPhase::Closing
        ));
        assert_eq!(
            console_disconnect_action(
                ConsoleDisconnectPhase::Draining,
                TcpState::CloseWait,
                false,
                false,
                true,
                1,
                0,
            ),
            ConsoleDisconnectAction::Wait
        );
        assert_eq!(
            console_disconnect_action(
                ConsoleDisconnectPhase::Draining,
                TcpState::CloseWait,
                true,
                true,
                true,
                1,
                0,
            ),
            ConsoleDisconnectAction::StartClose
        );
        assert_eq!(
            console_disconnect_action(
                ConsoleDisconnectPhase::Draining,
                TcpState::FinWait1,
                true,
                true,
                true,
                1,
                0,
            ),
            ConsoleDisconnectAction::ContinueClose
        );
        for state in [
            TcpState::FinWait1,
            TcpState::FinWait2,
            TcpState::Closing,
            TcpState::LastAck,
        ] {
            assert_eq!(
                console_disconnect_action(
                    ConsoleDisconnectPhase::Closing,
                    state,
                    true,
                    true,
                    true,
                    1,
                    0,
                ),
                ConsoleDisconnectAction::Wait
            );
        }
        for state in [TcpState::TimeWait, TcpState::Closed] {
            assert_eq!(
                console_disconnect_action(
                    ConsoleDisconnectPhase::Closing,
                    state,
                    true,
                    true,
                    true,
                    1,
                    0,
                ),
                ConsoleDisconnectAction::Complete
            );
        }
    }

    #[test]
    fn quit_waits_for_peer_fin_before_starting_local_close() {
        let started_ms = 100;
        assert_eq!(
            console_disconnect_action(
                ConsoleDisconnectPhase::Draining,
                TcpState::Established,
                true,
                true,
                true,
                started_ms + 1,
                started_ms,
            ),
            ConsoleDisconnectAction::StartPeerCloseWait
        );
        assert_eq!(
            console_disconnect_action(
                ConsoleDisconnectPhase::PeerCloseWait,
                TcpState::Established,
                true,
                true,
                true,
                started_ms + DISCONNECT_PEER_CLOSE_GRACE_MS - 1,
                started_ms,
            ),
            ConsoleDisconnectAction::Wait
        );
        assert_eq!(
            console_disconnect_action(
                ConsoleDisconnectPhase::PeerCloseWait,
                TcpState::CloseWait,
                true,
                true,
                true,
                started_ms + 1,
                started_ms,
            ),
            ConsoleDisconnectAction::StartClose
        );
        assert_eq!(
            console_disconnect_action(
                ConsoleDisconnectPhase::PeerCloseWait,
                TcpState::Established,
                true,
                true,
                true,
                started_ms + DISCONNECT_PEER_CLOSE_GRACE_MS,
                started_ms,
            ),
            ConsoleDisconnectAction::Abort,
            "a peer that never sends FIN must be aborted rather than re-enter simultaneous close"
        );
        assert_eq!(
            console_disconnect_terminal_reason(
                ConsoleDisconnectAction::Abort,
                NetConsoleDisconnectReason::Quit,
            ),
            NetConsoleDisconnectReason::Error,
            "a missing peer FIN is a forced recovery, not a graceful QUIT"
        );
    }

    #[test]
    fn smoltcp_quit_policy_observes_peer_fin_before_local_fin() {
        let mut device = Loopback::new(Medium::Ethernet);
        let address = IpAddress::Ipv4(Ipv4Address::new(127, 0, 0, 1));
        let config = IfaceConfig::new(HardwareAddress::Ethernet(EthernetAddress([
            0x02, 0, 0, 0, 0, 1,
        ])));
        let mut interface = Interface::new(config, &mut device, Instant::from_millis(0));
        interface.update_ip_addrs(|addrs| {
            addrs
                .push(IpCidr::new(address, 8))
                .expect("loopback address capacity");
        });

        let mut server_rx = [0u8; 256];
        let mut server_tx = [0u8; 256];
        let mut client_rx = [0u8; 256];
        let mut client_tx = [0u8; 256];
        let mut storage = [SocketStorage::EMPTY; 2];
        let mut sockets = SocketSet::new(&mut storage[..]);
        let server = sockets.add(TcpSocket::new(
            TcpSocketBuffer::new(&mut server_rx[..]),
            TcpSocketBuffer::new(&mut server_tx[..]),
        ));
        let client = sockets.add(TcpSocket::new(
            TcpSocketBuffer::new(&mut client_rx[..]),
            TcpSocketBuffer::new(&mut client_tx[..]),
        ));

        sockets
            .get_mut::<TcpSocket>(server)
            .listen(IpListenEndpoint::from(31_337))
            .expect("server listen");
        sockets
            .get_mut::<TcpSocket>(client)
            .connect(interface.context(), (address, 31_337), 49_152)
            .expect("client connect");

        for now_ms in 0..64 {
            let _ = interface.poll(Instant::from_millis(now_ms), &mut device, &mut sockets);
            if sockets.get::<TcpSocket>(server).state() == TcpState::Established
                && sockets.get::<TcpSocket>(client).state() == TcpState::Established
            {
                break;
            }
        }
        assert_eq!(
            sockets.get::<TcpSocket>(server).state(),
            TcpState::Established
        );
        assert_eq!(
            sockets.get::<TcpSocket>(client).state(),
            TcpState::Established
        );

        sockets
            .get_mut::<TcpSocket>(server)
            .send_slice(b"OK QUIT\n")
            .expect("server QUIT acknowledgement");
        for now_ms in 64..128 {
            let _ = interface.poll(Instant::from_millis(now_ms), &mut device, &mut sockets);
            if sockets.get::<TcpSocket>(client).recv_queue() == b"OK QUIT\n".len()
                && sockets.get::<TcpSocket>(server).send_queue() == 0
            {
                break;
            }
        }
        assert_eq!(
            sockets.get::<TcpSocket>(server).send_queue(),
            0,
            "the QUIT acknowledgement must be peer-ACKed before close policy starts"
        );
        sockets
            .get_mut::<TcpSocket>(client)
            .recv(|payload| {
                assert_eq!(payload, b"OK QUIT\n");
                (payload.len(), ())
            })
            .expect("client reads QUIT acknowledgement");
        sockets.get_mut::<TcpSocket>(client).close();

        assert_eq!(
            console_disconnect_action(
                ConsoleDisconnectPhase::PeerCloseWait,
                sockets.get::<TcpSocket>(server).state(),
                true,
                true,
                true,
                1,
                0,
            ),
            ConsoleDisconnectAction::Wait,
            "a queued peer FIN must be polled before the server can stage its own FIN"
        );

        for now_ms in 128..192 {
            let _ = interface.poll(Instant::from_millis(now_ms), &mut device, &mut sockets);
            if sockets.get::<TcpSocket>(server).state() == TcpState::CloseWait {
                break;
            }
        }
        let server_state = sockets.get::<TcpSocket>(server).state();
        assert_eq!(server_state, TcpState::CloseWait);
        assert_eq!(
            console_disconnect_action(
                ConsoleDisconnectPhase::PeerCloseWait,
                server_state,
                true,
                true,
                true,
                2,
                0,
            ),
            ConsoleDisconnectAction::StartClose
        );
        sockets.get_mut::<TcpSocket>(server).close();

        for now_ms in 192..320 {
            let _ = interface.poll(Instant::from_millis(now_ms), &mut device, &mut sockets);
            if console_active_terminal_state(sockets.get::<TcpSocket>(server).state()) {
                break;
            }
        }
        assert!(
            console_active_terminal_state(sockets.get::<TcpSocket>(server).state()),
            "peer-first close must reach a terminal server state without the simultaneous-close stall"
        );
    }

    #[test]
    fn console_disconnect_deadlines_use_elapsed_time() {
        let started_ms = 7;
        assert_eq!(
            console_disconnect_action(
                ConsoleDisconnectPhase::Draining,
                TcpState::Established,
                false,
                false,
                false,
                started_ms + DISCONNECT_DRAIN_DEADLINE_MS - 1,
                started_ms,
            ),
            ConsoleDisconnectAction::Wait
        );
        assert_eq!(
            console_disconnect_action(
                ConsoleDisconnectPhase::Draining,
                TcpState::Established,
                false,
                false,
                false,
                started_ms + DISCONNECT_DRAIN_DEADLINE_MS,
                started_ms,
            ),
            ConsoleDisconnectAction::Abort
        );
        assert_eq!(
            console_disconnect_terminal_reason(
                ConsoleDisconnectAction::Abort,
                NetConsoleDisconnectReason::Quit,
            ),
            NetConsoleDisconnectReason::Error,
            "forced RST recovery must not be reported as a graceful QUIT"
        );
        assert_eq!(
            console_disconnect_action(
                ConsoleDisconnectPhase::Closing,
                TcpState::LastAck,
                true,
                true,
                false,
                started_ms + DISCONNECT_CLOSE_DEADLINE_MS - 1,
                started_ms,
            ),
            ConsoleDisconnectAction::Wait
        );
        assert_eq!(
            console_disconnect_action(
                ConsoleDisconnectPhase::Closing,
                TcpState::LastAck,
                true,
                true,
                false,
                started_ms + DISCONNECT_CLOSE_DEADLINE_MS,
                started_ms,
            ),
            ConsoleDisconnectAction::Abort
        );

        let first_late_service_ms = started_ms + DISCONNECT_DRAIN_DEADLINE_MS * 4;
        let armed = arm_console_disconnect_phase_deadline(
            ConsoleDisconnectPhase::Draining,
            None,
            first_late_service_ms,
        )
        .expect("the first service turn arms the drain deadline");
        assert_eq!(armed, first_late_service_ms);
        assert_eq!(
            console_disconnect_action(
                ConsoleDisconnectPhase::Draining,
                TcpState::Established,
                false,
                false,
                false,
                first_late_service_ms,
                armed,
            ),
            ConsoleDisconnectAction::Wait,
            "a long scheduling gap before first service cannot expire an unarmed disconnect"
        );
    }

    #[test]
    fn console_standby_arms_only_after_active_shutdown_begins() {
        assert!(!console_standby_should_arm(ConsoleDisconnectPhase::Idle));
        assert!(console_standby_should_arm(ConsoleDisconnectPhase::Draining,));
        assert!(console_standby_should_arm(
            ConsoleDisconnectPhase::PeerCloseWait,
        ));
        assert!(console_standby_should_arm(ConsoleDisconnectPhase::Closing,));
    }

    #[test]
    fn console_service_weighting_requires_exact_socket_or_parser_work() {
        assert!(!console_socket_service_pending(
            TcpState::Established,
            0,
            0,
            TcpState::Closed,
            false,
            false,
            ConsoleDisconnectPhase::Idle,
        ));
        assert!(console_socket_service_pending(
            TcpState::SynReceived,
            0,
            0,
            TcpState::Closed,
            false,
            false,
            ConsoleDisconnectPhase::Idle,
        ));
        assert!(console_socket_service_pending(
            TcpState::Established,
            1,
            0,
            TcpState::Closed,
            false,
            false,
            ConsoleDisconnectPhase::Idle,
        ));
        assert!(console_socket_service_pending(
            TcpState::Established,
            0,
            0,
            TcpState::Established,
            false,
            false,
            ConsoleDisconnectPhase::PeerCloseWait,
        ));
    }

    #[test]
    fn console_standby_pending_connection_is_bounded_and_promotable() {
        assert!(!console_standby_pending_state(TcpState::Listen));
        assert!(console_standby_pending_state(TcpState::SynReceived));
        assert!(console_standby_pending_state(TcpState::Established));
        assert!(!console_standby_pending_state(TcpState::CloseWait));

        for state in [
            TcpState::Listen,
            TcpState::SynReceived,
            TcpState::Established,
        ] {
            assert!(console_standby_promotable_state(state));
        }
        for state in [
            TcpState::Closed,
            TcpState::CloseWait,
            TcpState::FinWait1,
            TcpState::FinWait2,
            TcpState::Closing,
            TcpState::LastAck,
            TcpState::TimeWait,
        ] {
            assert!(!console_standby_promotable_state(state));
        }

        let started_ms = 41;
        assert_eq!(
            CONSOLE_HANDOFF_PENDING_DEADLINE_MS,
            DISCONNECT_DRAIN_DEADLINE_MS
                + DISCONNECT_PEER_CLOSE_GRACE_MS
                + DISCONNECT_CLOSE_DEADLINE_MS,
            "standby authority must outlive every legal active QUIT phase"
        );
        assert!(!console_standby_pending_expired(
            Some(started_ms),
            started_ms + CONSOLE_HANDOFF_PENDING_DEADLINE_MS - 1,
        ));
        assert!(console_standby_pending_expired(
            Some(started_ms),
            started_ms + CONSOLE_HANDOFF_PENDING_DEADLINE_MS,
        ));
        assert!(!console_standby_pending_expired(None, u64::MAX));
    }

    #[test]
    fn console_standby_promotion_requires_terminal_socket_and_cleared_authority() {
        assert!(!console_active_terminal_state(TcpState::Established));
        assert!(!console_active_terminal_state(TcpState::LastAck));
        assert!(console_active_terminal_state(TcpState::Closed));
        assert!(console_active_terminal_state(TcpState::TimeWait));

        assert!(console_handoff_authority_cleared(
            ConsoleDisconnectPhase::Idle,
            false,
            false,
            false,
            AuthState::Start,
            false,
            0,
            false,
        ));
        assert!(!console_handoff_authority_cleared(
            ConsoleDisconnectPhase::Closing,
            false,
            false,
            false,
            AuthState::Start,
            false,
            0,
            false,
        ));
        assert!(!console_handoff_authority_cleared(
            ConsoleDisconnectPhase::Idle,
            true,
            false,
            false,
            AuthState::Start,
            false,
            0,
            false,
        ));
        assert!(!console_handoff_authority_cleared(
            ConsoleDisconnectPhase::Idle,
            false,
            true,
            false,
            AuthState::Start,
            false,
            0,
            false,
        ));
        assert!(!console_handoff_authority_cleared(
            ConsoleDisconnectPhase::Idle,
            false,
            false,
            true,
            AuthState::Attached,
            true,
            1,
            true,
        ));
    }

    #[test]
    fn smoltcp_acceptor_slots_can_listen_on_the_same_console_port() {
        let mut active_rx = [0u8; 128];
        let mut active_tx = [0u8; 128];
        let mut standby_rx = [0u8; 128];
        let mut standby_tx = [0u8; 128];
        let mut storage = [SocketStorage::EMPTY; 2];
        let mut sockets = SocketSet::new(&mut storage[..]);
        let active = sockets.add(TcpSocket::new(
            TcpSocketBuffer::new(&mut active_rx[..]),
            TcpSocketBuffer::new(&mut active_tx[..]),
        ));
        let standby = sockets.add(TcpSocket::new(
            TcpSocketBuffer::new(&mut standby_rx[..]),
            TcpSocketBuffer::new(&mut standby_tx[..]),
        ));

        sockets
            .get_mut::<TcpSocket>(active)
            .listen(IpListenEndpoint::from(31_337))
            .expect("active acceptor should listen");
        sockets
            .get_mut::<TcpSocket>(standby)
            .listen(IpListenEndpoint::from(31_337))
            .expect("standby acceptor should share the console port");

        assert_eq!(sockets.get::<TcpSocket>(active).state(), TcpState::Listen);
        assert_eq!(sockets.get::<TcpSocket>(standby).state(), TcpState::Listen);
    }

    #[test]
    fn pi4_config_reports_profile_backend_and_active_driver_separately() {
        let mut config = ConsoleNetConfig::default();
        config.backend = NetBackend::BcmGenet;
        config.policy.interface = NetInterfacePolicy::Wired;
        assert_eq!(configured_active_driver_label(&config), "bcmgenet-v5");
        assert_eq!(
            active_driver_label_for(config.backend, "wired"),
            "bcmgenet-v5"
        );

        config.policy.interface = NetInterfacePolicy::Wifi;
        assert_eq!(configured_active_driver_label(&config), "cyw43");
        assert_eq!(active_driver_label_for(config.backend, "wifi"), "cyw43");

        config.policy.interface = NetInterfacePolicy::Auto;
        config.wifi_credentials = None;
        assert_eq!(configured_active_driver_label(&config), "bcmgenet-v5");

        config.wifi_credentials =
            Some(WifiCredentials::new("cohesix", "passphrase").expect("valid WiFi credentials"));
        assert_eq!(configured_active_driver_label(&config), "cyw43");
        assert_eq!(config.backend.label(), "bcmgenet-v5");
    }

    #[test]
    fn cyw43_bootstrap_supervisor_classifies_timing_transient_and_configuration_permanent() {
        let timing =
            NetConsoleError::Init(NetStackError::Driver(DefaultDriverError::DriverTaskNet(
                DriverTaskNetError::RuntimeInit("cyw43-function1-ready-timeout"),
            )));
        assert!(cyw43_net_console_bootstrap_error_is_transient(&timing));

        let artifact =
            NetConsoleError::Init(NetStackError::Driver(DefaultDriverError::DriverTaskNet(
                DriverTaskNetError::RuntimeInit("cyw43-firmware-bundle"),
            )));
        assert!(!cyw43_net_console_bootstrap_error_is_transient(&artifact));
        assert!(!cyw43_net_console_bootstrap_error_is_transient(
            &NetConsoleError::InvalidConfig("wifi-credentials-missing")
        ));
        assert!(!cyw43_net_console_bootstrap_error_is_transient(
            &NetConsoleError::NoDevice
        ));
    }

    #[test]
    fn cyw43_pre_poll_generation_change_requires_same_turn_fence() {
        assert!(!cyw43_pre_poll_generation_fence_required(7, 7));
        assert!(cyw43_pre_poll_generation_fence_required(7, 8));
        assert!(cyw43_pre_poll_generation_fence_required(u32::MAX, 0));
    }

    #[test]
    fn cyw43_generation_proof_rejects_old_activity_and_faults() {
        let old_generation_counters = NetCounters {
            tcp_accepts: 4,
            tcp_auth_sessions: 3,
            tcp_rx_bytes: 4_096,
            wifi_rx_pending_drops: 8,
            wifi_rx_pending_drops_boot: 8,
            wifi_rx_runtime_queue_overflow_seen: 1,
            wifi_rx_runtime_overflow_episodes: 5,
            wifi_rx_runtime_overflow_episodes_boot: 5,
            wifi_data_trace_faults: 7,
            wifi_data_trace_tx_retries: 11,
            tx_submit: 64,
            tx_complete: 62,
            ..NetCounters::default()
        };
        let old_generation_baseline =
            Cyw43GenerationProofBaseline::capture(7, NetCounters::default());

        let generation_mismatch = old_generation_baseline.project(8, old_generation_counters);
        assert_eq!(generation_mismatch.tcp_accepts, 0);
        assert_eq!(generation_mismatch.tcp_auth_sessions, 0);
        assert_eq!(generation_mismatch.tcp_rx_bytes, 0);
        assert_eq!(generation_mismatch.wifi_rx_pending_drops, 0);
        assert_eq!(generation_mismatch.wifi_rx_pending_drops_boot, 8);
        assert_eq!(generation_mismatch.wifi_rx_runtime_overflow_episodes, 0);
        assert_eq!(
            generation_mismatch.wifi_rx_runtime_overflow_episodes_boot,
            5
        );
        assert_eq!(generation_mismatch.wifi_rx_runtime_queue_overflow_seen, 0);
        assert_eq!(generation_mismatch.wifi_data_trace_faults, 0);
        assert_eq!(generation_mismatch.wifi_data_trace_tx_retries, 0);
        assert_eq!(generation_mismatch.tx_submit, 0);
        assert_eq!(generation_mismatch.tx_complete, 0);
        assert!(!cyw43_tcp_data_path_proven(
            "cyw43",
            "wifi",
            generation_mismatch
        ));

        let new_generation_baseline =
            Cyw43GenerationProofBaseline::capture(8, old_generation_counters);
        let before_new_activity = new_generation_baseline.project(8, old_generation_counters);
        assert_eq!(before_new_activity.tcp_accepts, 0);
        assert_eq!(before_new_activity.tcp_auth_sessions, 0);
        assert_eq!(before_new_activity.tcp_rx_bytes, 0);
        assert_eq!(before_new_activity.wifi_rx_pending_drops, 0);
        assert_eq!(before_new_activity.wifi_rx_runtime_overflow_episodes, 0);
        assert_eq!(before_new_activity.wifi_rx_runtime_queue_overflow_seen, 0);
        assert_eq!(before_new_activity.wifi_data_trace_faults, 0);
        assert_eq!(before_new_activity.wifi_data_trace_tx_retries, 0);
        assert_eq!(before_new_activity.tx_submit, 0);
        assert_eq!(before_new_activity.tx_complete, 0);
        assert_eq!(
            cyw43_status_blocker_for("cyw43", "wifi", before_new_activity),
            None
        );
        assert!(!cyw43_tcp_data_path_proven(
            "cyw43",
            "wifi",
            before_new_activity
        ));

        let new_generation_counters = NetCounters {
            tcp_accepts: 5,
            tcp_auth_sessions: 4,
            tcp_rx_bytes: 4_224,
            wifi_rx_pending_drops: 9,
            wifi_rx_pending_drops_boot: 9,
            wifi_rx_runtime_queue_overflow_seen: 1,
            wifi_rx_runtime_overflow_episodes: 6,
            wifi_rx_runtime_overflow_episodes_boot: 6,
            wifi_data_trace_faults: 8,
            wifi_data_trace_tx_retries: 13,
            tx_submit: 67,
            tx_complete: 65,
            ..old_generation_counters
        };
        let after_new_activity = new_generation_baseline.project(8, new_generation_counters);
        assert_eq!(after_new_activity.tcp_accepts, 1);
        assert_eq!(after_new_activity.tcp_auth_sessions, 1);
        assert_eq!(after_new_activity.tcp_rx_bytes, 128);
        assert_eq!(after_new_activity.wifi_rx_pending_drops, 1);
        assert_eq!(after_new_activity.wifi_rx_pending_drops_boot, 9);
        assert_eq!(after_new_activity.wifi_rx_runtime_overflow_episodes, 1);
        assert_eq!(after_new_activity.wifi_rx_runtime_overflow_episodes_boot, 6);
        assert_eq!(after_new_activity.wifi_rx_runtime_queue_overflow_seen, 1);
        assert_eq!(after_new_activity.wifi_data_trace_faults, 1);
        assert_eq!(after_new_activity.wifi_data_trace_tx_retries, 2);
        assert_eq!(after_new_activity.tx_submit, 3);
        assert_eq!(after_new_activity.tx_complete, 3);
        assert_eq!(
            cyw43_status_blocker_for("cyw43", "wifi", after_new_activity),
            None,
            "same-generation TCP proof is terminal for the status surface"
        );
        assert!(cyw43_tcp_data_path_proven(
            "cyw43",
            "wifi",
            after_new_activity
        ));
    }

    #[test]
    fn tcp_generation_proof_projection_fails_closed_after_counter_reset() {
        let baseline = Cyw43GenerationProofBaseline::capture(
            12,
            NetCounters {
                tcp_accepts: 9,
                tcp_auth_sessions: 7,
                tcp_rx_bytes: 8_192,
                ..NetCounters::default()
            },
        );

        let projected = baseline.project(12, NetCounters::default());
        assert_eq!(projected.tcp_accepts, 0);
        assert_eq!(projected.tcp_auth_sessions, 0);
        assert_eq!(projected.tcp_rx_bytes, 0);
    }

    #[test]
    fn reservation_releases_on_error() {
        let _guard = NET_STACK_STORAGE_TEST_LOCK.lock();
        reset_console_storage_state();

        TCP_RX_STORAGE_IN_USE.store(true, Ordering::Release);
        let attempt = NetInitAttempt::new("test.reservation");
        let result =
            StorageReservation::acquire::<Infallible>(true, false, &attempt, "test.reservation");
        assert!(matches!(result, Err(NetStackError::TcpRxStorageInUse)));

        assert!(!SOCKET_STORAGE_IN_USE.load(Ordering::Acquire));
        assert!(TCP_RX_STORAGE_IN_USE.load(Ordering::Acquire));

        TCP_RX_STORAGE_IN_USE.store(false, Ordering::Release);
    }

    #[test]
    fn host_selftest_targets_use_guest_ip_by_default() {
        let ip = Ipv4Address::new(10, 0, 2, 15);
        let target = render_host_selftest_target(None, UDP_ECHO_PORT, ip);
        assert_eq!(target.as_str(), "10.0.2.15:31338");
    }

    #[test]
    fn host_selftest_targets_prefer_forward_override() {
        let ip = Ipv4Address::new(10, 0, 2, 15);
        let default_override = render_host_selftest_target(Some("127.0.0.1"), TCP_SMOKE_PORT, ip);
        assert_eq!(default_override.as_str(), "127.0.0.1:31339");

        let explicit = render_host_selftest_target(Some("example.com:5555"), TCP_SMOKE_PORT, ip);
        assert_eq!(explicit.as_str(), "example.com:5555");
    }

    #[test]
    fn host_eapol_pending_suppresses_timebase_stall_warning() {
        assert!(timebase_stall_warning_suppressed(Some(
            "wifi-host-eapol-pending"
        )));
        assert!(!timebase_stall_warning_suppressed(Some(
            "wifi-host-eapol-required"
        )));
        assert!(!timebase_stall_warning_suppressed(None));
    }

    #[test]
    fn repeated_same_tick_polls_delay_timebase_stall_warning() {
        assert!(!timebase_stall_warning_due(
            SAME_TICK_STALL_WARN_POLLS - 1,
            false,
            None
        ));
        assert!(timebase_stall_warning_due(
            SAME_TICK_STALL_WARN_POLLS,
            false,
            None
        ));
        assert!(!timebase_stall_warning_due(
            SAME_TICK_STALL_WARN_POLLS,
            true,
            None
        ));
        assert!(!timebase_stall_warning_due(
            SAME_TICK_STALL_WARN_POLLS,
            false,
            Some("wifi-host-eapol-pending")
        ));
    }

    #[test]
    fn host_eapol_pending_and_required_block_data_path() {
        assert!(wifi_host_eapol_blocks_data_path(Some(
            "wifi-host-eapol-pending"
        )));
        assert!(wifi_host_eapol_blocks_data_path(Some(
            "wifi-host-eapol-required"
        )));
        assert!(!wifi_host_eapol_blocks_data_path(Some(
            "wifi-data-handoff-pending"
        )));
        assert!(!wifi_host_eapol_blocks_data_path(Some("dhcp-pending")));
        assert!(!wifi_host_eapol_blocks_data_path(None));
    }

    #[test]
    fn host_eapol_pending_retains_work_across_single_operation_turns() {
        assert_eq!(
            wifi_host_eapol_stack_service_polls(Some("wifi-host-eapol-pending")),
            1
        );
        assert_eq!(
            wifi_host_eapol_stack_service_polls(Some("wifi-host-eapol-required")),
            1
        );
        assert_eq!(wifi_host_eapol_stack_service_polls(None), 1);
        assert_eq!(wifi_host_eapol_stack_service_polls(Some("dhcp-pending")), 1);
        assert_eq!(CYW43_HOST_EAPOL_BUDGETED_SERVICE_POLLS, 1);
    }

    #[test]
    fn host_eapol_pending_and_required_block_fresh_driver_task_pre_poll() {
        assert!(wifi_host_eapol_blocks_driver_task_pre_poll(Some(
            "wifi-host-eapol-pending"
        )));
        assert!(wifi_host_eapol_blocks_driver_task_pre_poll(Some(
            "wifi-host-eapol-required"
        )));
        assert!(!wifi_host_eapol_blocks_driver_task_pre_poll(Some(
            "dhcp-pending"
        )));
        assert!(!wifi_host_eapol_blocks_driver_task_pre_poll(Some(
            "wifi-data-handoff-pending"
        )));
        assert!(!wifi_host_eapol_blocks_driver_task_pre_poll(None));
    }

    #[test]
    fn retained_net_data_finishes_before_host_eapol_takes_a_fresh_turn() {
        assert!(wifi_driver_task_pre_poll_due(
            Some("wifi-host-eapol-pending"),
            true,
            true,
        ));
        assert!(wifi_driver_task_pre_poll_due(
            Some("wifi-host-eapol-required"),
            true,
            true,
        ));
        assert!(!wifi_driver_task_pre_poll_due(
            Some("wifi-host-eapol-pending"),
            false,
            true,
        ));
        assert!(!wifi_driver_task_pre_poll_due(
            Some("wifi-host-eapol-required"),
            false,
            true,
        ));
        assert!(wifi_driver_task_pre_poll_due(None, false, true));
        assert!(!wifi_driver_task_pre_poll_due(None, false, false));
    }

    #[test]
    fn dhcp_start_defer_reason_ignores_dhcp_frontier_labels() {
        assert_eq!(dhcp_start_defer_reason_for(None), None);
        assert_eq!(dhcp_start_defer_reason_for(Some("dhcp-pending")), None);
        assert_eq!(dhcp_start_defer_reason_for(Some("dhcp-failed")), None);
        assert_eq!(
            dhcp_start_defer_reason_for(Some("wifi-host-eapol-pending")),
            Some("wifi-host-eapol-pending")
        );
        assert_eq!(
            dhcp_start_defer_reason_for(Some("wifi-link-down")),
            Some("wifi-link-down")
        );
        assert_eq!(
            dhcp_start_defer_reason_for(Some("wifi-data-handoff-pending")),
            Some("wifi-data-handoff-pending")
        );
        assert_eq!(
            dhcp_phase_for_bringup_status("wifi-data-handoff-pending"),
            "data-handoff-pending"
        );
    }

    #[test]
    fn cyw43_dhcp_starts_immediately_after_secure_eapol() {
        let mut last_rx = 0;
        let mut quiet_since_ms = None;

        let first =
            cyw43_dhcp_post_secure_eapol_settle(1_000, 1, 18, &mut last_rx, &mut quiet_since_ms);
        assert!(first.ready);
        assert!(first.changed);
        assert_eq!(first.quiet_ms, 0);
        assert_eq!(first.remaining_ms, 0);
        assert_eq!(first.next_ready_ms, Some(1_000));
        assert_eq!(last_rx, 18);
        assert_eq!(quiet_since_ms, Some(1_000));

        let early =
            cyw43_dhcp_post_secure_eapol_settle(1_499, 1, 18, &mut last_rx, &mut quiet_since_ms);
        assert!(early.ready);
        assert!(!early.changed);
        assert_eq!(early.quiet_ms, 0);
        assert_eq!(early.remaining_ms, 0);
        assert_eq!(early.next_ready_ms, Some(1_499));

        let ready =
            cyw43_dhcp_post_secure_eapol_settle(1_500, 1, 18, &mut last_rx, &mut quiet_since_ms);
        assert!(ready.ready);
        assert!(!ready.changed);
        assert_eq!(ready.quiet_ms, 0);
        assert_eq!(ready.remaining_ms, 0);
        assert_eq!(ready.next_ready_ms, Some(1_500));

        let retransmit =
            cyw43_dhcp_post_secure_eapol_settle(1_600, 1, 19, &mut last_rx, &mut quiet_since_ms);
        assert!(retransmit.ready);
        assert!(retransmit.changed);
        assert_eq!(retransmit.quiet_ms, 0);
        assert_eq!(retransmit.remaining_ms, 0);
        assert_eq!(retransmit.next_ready_ms, Some(1_600));
        assert_eq!(last_rx, 19);
        assert_eq!(quiet_since_ms, Some(1_600));
    }

    #[test]
    fn console_listener_gate_waits_for_dhcp_lease() {
        assert_eq!(
            console_listener_defer_reason_for(NetMode::Dhcp, Ipv4Address::UNSPECIFIED, None),
            Some("dhcp-pending")
        );
        assert_eq!(
            console_listener_defer_reason_for(
                NetMode::Dhcp,
                Ipv4Address::new(192, 168, 50, 23),
                None
            ),
            None
        );
        assert_eq!(
            console_listener_defer_reason_for(
                NetMode::Dhcp,
                Ipv4Address::new(192, 168, 50, 23),
                Some("wifi-host-eapol-pending")
            ),
            Some("wifi-host-eapol-pending")
        );
        assert_eq!(
            console_listener_defer_reason_for(NetMode::Static, Ipv4Address::UNSPECIFIED, None),
            Some("ip-unconfigured")
        );
    }

    #[test]
    fn dhcp_lease_replaces_unspecified_primary_interface_addr() {
        let mac = EthernetAddress([0x88, 0xa2, 0x9e, 0x66, 0x59, 0x10]);
        let mut device = Loopback::new(Medium::Ethernet);
        let config = IfaceConfig::new(HardwareAddress::Ethernet(mac));
        let mut interface = Interface::new(config, &mut device, Instant::from_millis(0));
        interface.update_ip_addrs(|addrs| {
            let cidr = IpCidr::new(IpAddress::from(Ipv4Address::UNSPECIFIED), 0);
            let _ = addrs.push(cidr);
        });
        interface.update_ip_addrs(|addrs| {
            let cidr = IpCidr::new(IpAddress::from(Ipv4Address::new(192, 168, 86, 154)), 24);
            set_primary_ipv4_addr!(addrs, cidr);
        });

        assert_eq!(interface.ip_addrs().len(), 1);
        assert_eq!(
            interface.ip_addrs()[0],
            IpCidr::new(IpAddress::from(Ipv4Address::new(192, 168, 86, 154)), 24)
        );
    }

    #[test]
    fn interface_hardware_addr_sync_updates_smoltcp_mac() {
        let initial = EthernetAddress([0x02, 0x43, 0x4f, 0x48, 0x58, 0x32]);
        let runtime = EthernetAddress([0x88, 0xa2, 0x9e, 0x66, 0x59, 0x10]);
        let mut device = Loopback::new(Medium::Ethernet);
        let config = IfaceConfig::new(HardwareAddress::Ethernet(initial));
        let mut interface = Interface::new(config, &mut device, Instant::from_millis(0));

        let previous = NetStack::<DefaultNetDevice>::sync_interface_hardware_addr_value(
            &mut interface,
            runtime,
        );

        assert_eq!(previous, Some(HardwareAddress::Ethernet(initial)));
        assert_eq!(
            interface.hardware_addr(),
            HardwareAddress::Ethernet(runtime)
        );
        assert_eq!(
            NetStack::<DefaultNetDevice>::sync_interface_hardware_addr_value(
                &mut interface,
                runtime
            ),
            None
        );
    }

    #[test]
    fn dhcp_restart_after_mac_sync_is_limited_to_pending_dhcp() {
        assert!(dhcp_restart_required_after_mac_sync(
            NetMode::Dhcp,
            Ipv4Address::UNSPECIFIED,
            true
        ));
        assert!(!dhcp_restart_required_after_mac_sync(
            NetMode::Dhcp,
            Ipv4Address::new(192, 168, 50, 23),
            true
        ));
        assert!(!dhcp_restart_required_after_mac_sync(
            NetMode::Static,
            Ipv4Address::UNSPECIFIED,
            true
        ));
        assert!(!dhcp_restart_required_after_mac_sync(
            NetMode::Dhcp,
            Ipv4Address::UNSPECIFIED,
            false
        ));
    }

    #[test]
    fn budgeted_dhcp_service_is_limited_to_unbound_dhcp_socket() {
        assert!(budgeted_dhcp_service_required(
            NetMode::Dhcp,
            Ipv4Address::UNSPECIFIED,
            true
        ));
        assert!(!budgeted_dhcp_service_required(
            NetMode::Dhcp,
            Ipv4Address::new(192, 168, 86, 154),
            true
        ));
        assert!(!budgeted_dhcp_service_required(
            NetMode::Static,
            Ipv4Address::UNSPECIFIED,
            true
        ));
        assert!(!budgeted_dhcp_service_required(
            NetMode::Dhcp,
            Ipv4Address::UNSPECIFIED,
            false
        ));
    }

    #[test]
    fn bootinfo_net_mark_ok_logs_stay_short_and_canary_free() {
        for mark in [
            "net.init.begin",
            "net.init.device",
            "net.init.interface",
            "net.init.socketset",
            "net.init.post",
        ] {
            assert!(bootinfo_net_mark_ok_log_fits_uart_frame(mark));
            assert!(!mark.contains("pre=0x"));
            assert!(!mark.contains("post=0x"));
        }
    }

    #[test]
    fn console_socket_capacity_guard_matches_expected_buffers() {
        assert!(NetStack::<DefaultNetDevice>::console_socket_capacity_ok(
            TCP_RX_BUFFER,
            TCP_TX_BUFFER
        ));
        assert!(!NetStack::<DefaultNetDevice>::console_socket_capacity_ok(
            3,
            TCP_TX_BUFFER
        ));
        assert!(!NetStack::<DefaultNetDevice>::console_socket_capacity_ok(
            TCP_RX_BUFFER,
            3
        ));
    }

    #[test]
    fn network_poll_quanta_stay_bounded_for_driver_contracts() {
        assert!(MAX_DHCP_RX_PACKETS_PER_POLL <= 2);
        assert!(MAX_UDP_ECHO_PACKETS_PER_POLL <= 2);
        assert!(MAX_TCP_SMOKE_RECV_CHUNKS_PER_POLL <= 2);
        assert_eq!(TCP_CONSOLE_RECV_CHUNK_BYTES, DEFAULT_LINE_CAPACITY + 4);
        assert!(MAX_TCP_CONSOLE_RECV_CHUNKS_PER_POLL <= 64);
        assert!(MAX_TCP_CONSOLE_RECV_BYTES_PER_POLL <= TCP_RX_BUFFER);
        assert!(MAX_CONSOLE_FRAMES_PER_POLL <= 32);
        assert!(MAX_CONSOLE_BYTES_PER_POLL <= TCP_TX_BUFFER);
        assert_eq!(
            TCP_SERVICE_BYTES_PER_TURN,
            (MAX_TCP_CONSOLE_RECV_BYTES_PER_POLL + MAX_CONSOLE_BYTES_PER_POLL) as u32
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn genet_tcp_fast_path_accounting_stays_inside_contract() {
        let contract = crate::hal::driver_task::GENET_DRIVER_TASK_CONTRACT;
        let pre_poll_ops = (GENET_DRIVER_TASK_PRE_POLL_BURST_LIMIT as u16).saturating_mul(4);
        let pre_poll_frames = GENET_DRIVER_TASK_PRE_POLL_BURST_LIMIT as u16;
        let pre_poll_bytes = (GENET_DRIVER_TASK_PRE_POLL_BURST_LIMIT as u32)
            .saturating_mul(DRIVER_TASK_PRE_POLL_TURN_BYTES);
        let base_ops = 1u16;
        let interface_ops = 2u16;
        let interface_frames = 1u16;
        let interface_bytes = 2048u32;
        let tcp_ops = 64u16;
        let tcp_frames = MAX_CONSOLE_FRAMES_PER_POLL as u16;
        let tcp_bytes = TCP_SERVICE_BYTES_PER_TURN;

        assert!(pre_poll_ops.saturating_add(tcp_ops) <= contract.budget.max_ops_per_turn);
        assert!(pre_poll_frames.saturating_add(tcp_frames) <= contract.budget.max_frames_per_turn);
        assert!(pre_poll_bytes.saturating_add(tcp_bytes) <= contract.budget.max_bytes_per_turn);
        assert!(
            (1u16 + GENET_TCP_POST_DISPATCH_EXTRA_TURNS as u16).saturating_mul(tcp_ops)
                <= contract.budget.max_ops_per_turn
        );
        assert!(
            (1u16 + GENET_TCP_POST_DISPATCH_EXTRA_TURNS as u16).saturating_mul(tcp_frames)
                <= contract.budget.max_frames_per_turn
        );
        assert!(
            (1u32 + GENET_TCP_POST_DISPATCH_EXTRA_TURNS as u32).saturating_mul(tcp_bytes)
                <= contract.budget.max_bytes_per_turn
        );
        assert!(
            base_ops
                .saturating_add(pre_poll_ops)
                .saturating_add(tcp_ops)
                .saturating_add(interface_ops)
                <= contract.budget.max_ops_per_turn
        );
        assert!(
            pre_poll_frames
                .saturating_add(tcp_frames)
                .saturating_add(interface_frames)
                <= contract.budget.max_frames_per_turn
        );
        assert!(
            pre_poll_bytes
                .saturating_add(tcp_bytes)
                .saturating_add(interface_bytes)
                <= contract.budget.max_bytes_per_turn
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_tcp_phase_accounting_fits_after_pre_poll() {
        let contract = crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT;
        let pre_poll_ops = (CYW43_DRIVER_TASK_PRE_POLL_BURST_LIMIT as u16).saturating_mul(4);
        let pre_poll_frames = CYW43_DRIVER_TASK_PRE_POLL_BURST_LIMIT as u16;
        let pre_poll_bytes = (CYW43_DRIVER_TASK_PRE_POLL_BURST_LIMIT as u32)
            .saturating_mul(DRIVER_TASK_PRE_POLL_TURN_BYTES);
        let base_ops = 1u16;
        let interface_ops = 2u16;
        let interface_frames = 1u16;
        let interface_bytes = 2048u32;
        let tcp_ops = 64u16;
        let tcp_frames = MAX_CONSOLE_FRAMES_PER_POLL as u16;
        let tcp_bytes = TCP_SERVICE_BYTES_PER_TURN;
        let dhcp_ops = 16u16;
        let dhcp_frames = MAX_DHCP_RX_PACKETS_PER_POLL as u16 + 1;
        let dhcp_bytes = (MAX_DHCP_RX_PACKETS_PER_POLL as u32 + 1) * 1024;
        let selftest_ops = 16u16;
        let selftest_frames = 8u16;
        let selftest_bytes = 8 * 1024u32;

        assert!(pre_poll_ops.saturating_add(tcp_ops) <= contract.budget.max_ops_per_turn);
        assert!(pre_poll_frames.saturating_add(tcp_frames) <= contract.budget.max_frames_per_turn);
        assert!(pre_poll_bytes.saturating_add(tcp_bytes) <= contract.budget.max_bytes_per_turn);
        assert!(
            base_ops
                .saturating_add(pre_poll_ops)
                .saturating_add(tcp_ops)
                .saturating_add(interface_ops)
                <= contract.budget.max_ops_per_turn
        );
        assert!(
            pre_poll_frames
                .saturating_add(tcp_frames)
                .saturating_add(interface_frames)
                <= contract.budget.max_frames_per_turn
        );
        assert!(
            pre_poll_bytes
                .saturating_add(tcp_bytes)
                .saturating_add(interface_bytes)
                <= contract.budget.max_bytes_per_turn
        );
        assert_eq!(
            CYW43_DRIVER_TASK_PRE_POLL_BURST_LIMIT, 1,
            "one CYW43 op8 parent must carry the complete bounded RX batch"
        );
        assert!(
            base_ops
                .saturating_add(pre_poll_ops)
                .saturating_add(tcp_ops)
                .saturating_add(interface_ops)
                .saturating_add(interface_ops)
                <= contract.budget.max_ops_per_turn
        );
        assert!(
            pre_poll_frames
                .saturating_add(tcp_frames)
                .saturating_add(interface_frames)
                .saturating_add(interface_frames)
                <= contract.budget.max_frames_per_turn
        );
        assert!(
            pre_poll_bytes
                .saturating_add(tcp_bytes)
                .saturating_add(interface_bytes)
                .saturating_add(interface_bytes)
                <= contract.budget.max_bytes_per_turn
        );
        assert!(
            pre_poll_ops
                .saturating_add(tcp_ops)
                .saturating_add(dhcp_ops)
                .saturating_add(interface_ops)
                <= contract.budget.max_ops_per_turn
        );
        assert!(
            pre_poll_frames
                .saturating_add(tcp_frames)
                .saturating_add(dhcp_frames)
                .saturating_add(interface_frames)
                <= contract.budget.max_frames_per_turn
        );
        assert!(
            pre_poll_bytes
                .saturating_add(tcp_bytes)
                .saturating_add(dhcp_bytes)
                .saturating_add(interface_bytes)
                <= contract.budget.max_bytes_per_turn
        );
        assert!(
            pre_poll_ops
                .saturating_add(tcp_ops)
                .saturating_add(interface_ops)
                <= contract.budget.max_ops_per_turn
        );
        assert!(
            pre_poll_frames
                .saturating_add(tcp_frames)
                .saturating_add(interface_frames)
                <= contract.budget.max_frames_per_turn
        );
        assert!(
            pre_poll_bytes
                .saturating_add(tcp_bytes)
                .saturating_add(interface_bytes)
                <= contract.budget.max_bytes_per_turn
        );
        assert!(
            pre_poll_ops
                .saturating_add(tcp_ops)
                .saturating_add(interface_ops)
                < pre_poll_ops
                    .saturating_add(tcp_ops)
                    .saturating_add(dhcp_ops)
                    .saturating_add(interface_ops)
        );
        assert!(
            pre_poll_ops
                .saturating_add(selftest_ops)
                .saturating_add(interface_ops)
                <= contract.budget.max_ops_per_turn
        );
        assert!(
            pre_poll_frames
                .saturating_add(selftest_frames)
                .saturating_add(interface_frames)
                <= contract.budget.max_frames_per_turn
        );
        assert!(
            pre_poll_bytes
                .saturating_add(selftest_bytes)
                .saturating_add(interface_bytes)
                <= contract.budget.max_bytes_per_turn
        );
        assert!(
            pre_poll_bytes.saturating_add(tcp_bytes.saturating_mul(2))
                > contract.budget.max_bytes_per_turn
        );
        assert!(tcp_bytes.saturating_mul(2) > contract.budget.max_bytes_per_turn);

        let mut dhcp_budget = DriverServiceBudget::new(contract).expect("valid CYW43 contract");
        dhcp_budget.charge_ops(1).expect("base budget charge fits");
        assert!(
            NetStack::<DefaultNetDevice>::charge_dhcp_budget(&mut dhcp_budget).is_ok(),
            "budgeted DHCP turn must fit after base poll charge"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_flush_pre_poll_allows_active_wifi_dhcp() {
        let cyw43 = crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT;
        let genet = crate::hal::driver_task::GENET_DRIVER_TASK_CONTRACT;
        let ip = Ipv4Address::new(192, 168, 86, 154);

        assert!(cyw43_flush_pre_poll_data_ready_for(
            cyw43,
            "wifi",
            NetMode::Dhcp,
            ip,
            None,
            Some(DhcpPhase::Bound),
            true
        ));
        assert!(!cyw43_flush_pre_poll_data_ready_for(
            cyw43,
            "wifi",
            NetMode::Dhcp,
            ip,
            Some("wifi-host-eapol-pending"),
            Some(DhcpPhase::Bound),
            true
        ));
        assert!(cyw43_runtime_service_pre_poll_ready_for(
            cyw43,
            "wifi",
            NetMode::Dhcp,
            ip,
            Some("wifi-host-eapol-pending"),
            Some(DhcpPhase::Bound),
            true
        ));
        assert!(!cyw43_flush_pre_poll_data_ready_for(
            cyw43,
            "wifi",
            NetMode::Dhcp,
            ip,
            Some("wifi-host-eapol-required"),
            Some(DhcpPhase::Bound),
            true
        ));
        assert!(cyw43_runtime_service_pre_poll_ready_for(
            cyw43,
            "wifi",
            NetMode::Dhcp,
            ip,
            Some("wifi-host-eapol-required"),
            Some(DhcpPhase::Bound),
            true
        ));
        assert!(cyw43_runtime_service_pre_poll_ready_for(
            cyw43,
            "wifi",
            NetMode::Dhcp,
            Ipv4Address::UNSPECIFIED,
            Some("wifi-associating"),
            None,
            false
        ));
        assert!(cyw43_runtime_service_pre_poll_ready_for(
            cyw43,
            "wifi",
            NetMode::Dhcp,
            Ipv4Address::UNSPECIFIED,
            Some("wifi-link-down"),
            None,
            false
        ));
        assert!(cyw43_runtime_service_pre_poll_ready_for(
            cyw43,
            "wifi",
            NetMode::Dhcp,
            Ipv4Address::UNSPECIFIED,
            Some("wifi-data-handoff-pending"),
            None,
            false
        ));
        assert!(cyw43_runtime_service_pre_poll_ready_for(
            cyw43,
            "wifi",
            NetMode::Dhcp,
            Ipv4Address::UNSPECIFIED,
            Some("wifi-data-rx-admission-blocked"),
            None,
            false
        ));
        assert!(!cyw43_runtime_service_pre_poll_ready_for(
            cyw43,
            "wifi",
            NetMode::Off,
            Ipv4Address::UNSPECIFIED,
            Some("wifi-associating"),
            None,
            false
        ));
        assert!(!cyw43_runtime_service_pre_poll_ready_for(
            cyw43,
            "wifi",
            NetMode::Dhcp,
            Ipv4Address::UNSPECIFIED,
            Some("driver-task-ring-client"),
            None,
            false
        ));
        assert!(cyw43_flush_pre_poll_data_ready_for(
            cyw43,
            "wifi",
            NetMode::Dhcp,
            Ipv4Address::UNSPECIFIED,
            None,
            Some(DhcpPhase::Requesting),
            true
        ));
        assert!(cyw43_flush_pre_poll_data_ready_for(
            cyw43,
            "wifi",
            NetMode::Dhcp,
            Ipv4Address::UNSPECIFIED,
            None,
            Some(DhcpPhase::Selecting),
            true
        ));
        assert!(cyw43_flush_pre_poll_data_ready_for(
            cyw43,
            "wifi",
            NetMode::Dhcp,
            Ipv4Address::UNSPECIFIED,
            None,
            Some(DhcpPhase::Disabled),
            true
        ));
        assert!(!cyw43_flush_pre_poll_data_ready_for(
            cyw43,
            "wifi",
            NetMode::Dhcp,
            Ipv4Address::UNSPECIFIED,
            None,
            Some(DhcpPhase::Disabled),
            false
        ));
        assert!(cyw43_flush_pre_poll_data_ready_for(
            cyw43,
            "wifi",
            NetMode::Dhcp,
            Ipv4Address::UNSPECIFIED,
            None,
            Some(DhcpPhase::Bound),
            true
        ));
        assert!(!cyw43_flush_pre_poll_data_ready_for(
            cyw43,
            "wifi",
            NetMode::Dhcp,
            ip,
            None,
            None,
            true
        ));
        assert!(cyw43_flush_pre_poll_data_ready_for(
            cyw43,
            "wifi",
            NetMode::Static,
            ip,
            None,
            Some(DhcpPhase::Bound),
            true
        ));
        assert!(!cyw43_flush_pre_poll_data_ready_for(
            cyw43,
            "wifi",
            NetMode::Off,
            ip,
            None,
            Some(DhcpPhase::Bound),
            true
        ));
        assert!(!cyw43_flush_pre_poll_data_ready_for(
            cyw43,
            "wifi",
            NetMode::Static,
            Ipv4Address::UNSPECIFIED,
            None,
            Some(DhcpPhase::Bound),
            true
        ));
        assert!(!cyw43_flush_pre_poll_data_ready_for(
            genet,
            "wifi",
            NetMode::Dhcp,
            ip,
            None,
            Some(DhcpPhase::Bound),
            true
        ));
        assert!(!cyw43_flush_pre_poll_data_ready_for(
            cyw43,
            "wired",
            NetMode::Dhcp,
            ip,
            None,
            Some(DhcpPhase::Bound),
            true
        ));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn driver_task_pre_poll_burst_limits_are_role_scoped() {
        assert_eq!(
            driver_task_pre_poll_burst_limit(crate::hal::driver_task::DriverTaskHotPath::GenetNic),
            GENET_DRIVER_TASK_PRE_POLL_BURST_LIMIT
        );
        assert_eq!(
            driver_task_pre_poll_burst_limit(crate::hal::driver_task::DriverTaskHotPath::Cyw43Wifi),
            CYW43_DRIVER_TASK_PRE_POLL_BURST_LIMIT
        );
        assert_eq!(
            driver_task_pre_poll_burst_limit(
                crate::hal::driver_task::DriverTaskHotPath::SerialConsole
            ),
            DEFAULT_DRIVER_TASK_PRE_POLL_BURST_LIMIT
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_pre_poll_commands_are_quiet_hot_path() {
        let command = driver_task_pre_poll_command(
            crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT,
            crate::hal::driver_task::DriverTaskHotPath::Cyw43Wifi,
            NET_RING_FLAG_BUDGETED,
        );

        assert_ne!(
            command.flags & crate::hal::driver_task::DRIVER_TASK_RING_FLAG_QUIET_HOT_PATH,
            0
        );
        assert_ne!(
            command.frame.flags & crate::hal::driver_task::DRIVER_TASK_RING_FLAG_QUIET_HOT_PATH,
            0
        );

        let genet_command = driver_task_pre_poll_command(
            crate::hal::driver_task::GENET_DRIVER_TASK_CONTRACT,
            crate::hal::driver_task::DriverTaskHotPath::GenetNic,
            NET_RING_FLAG_BUDGETED,
        );
        assert_eq!(
            genet_command.flags & crate::hal::driver_task::DRIVER_TASK_RING_FLAG_QUIET_HOT_PATH,
            0
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn genet_tcp_flush_pre_poll_is_wired_only() {
        assert!(genet_tcp_flush_pre_poll_enabled(
            crate::hal::driver_task::DriverTaskHotPath::GenetNic
        ));
        assert!(!genet_tcp_flush_pre_poll_enabled(
            crate::hal::driver_task::DriverTaskHotPath::Cyw43Wifi
        ));
        assert!(!genet_tcp_flush_pre_poll_enabled(
            crate::hal::driver_task::DriverTaskHotPath::SerialConsole
        ));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn genet_budgeted_tcp_fast_path_is_wired_only_after_listener_ready() {
        let tcp_policy = NetStagePolicy {
            allow_tcp: true,
            allow_selftest: true,
            allow_outbound_probe: false,
            allow_console_io: true,
            tx_only: false,
        };
        let no_tcp_policy = NetStagePolicy {
            allow_tcp: false,
            ..tcp_policy
        };

        assert!(budgeted_genet_tcp_fast_path_due(
            crate::hal::driver_task::GENET_DRIVER_TASK_CONTRACT,
            tcp_policy,
            None
        ));
        assert!(!budgeted_genet_tcp_fast_path_due(
            crate::hal::driver_task::GENET_DRIVER_TASK_CONTRACT,
            tcp_policy,
            Some("dhcp-pending")
        ));
        assert!(!budgeted_genet_tcp_fast_path_due(
            crate::hal::driver_task::GENET_DRIVER_TASK_CONTRACT,
            no_tcp_policy,
            None
        ));
        assert!(!budgeted_genet_tcp_fast_path_due(
            crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT,
            tcp_policy,
            None
        ));
        assert!(!budgeted_genet_tcp_fast_path_due(
            crate::hal::driver_task::VIRTIO_NET_DRIVER_TASK_CONTRACT,
            tcp_policy,
            None
        ));
        assert!(budgeted_genet_smoltcp_poll_after_tcp_borrow(
            BudgetedNetPhase::Interface
        ));
        assert!(budgeted_genet_smoltcp_poll_after_tcp_borrow(
            BudgetedNetPhase::InterfaceFlush
        ));
        assert!(!budgeted_genet_smoltcp_poll_after_tcp_borrow(
            BudgetedNetPhase::Dhcp
        ));
        assert!(!budgeted_genet_smoltcp_poll_after_tcp_borrow(
            BudgetedNetPhase::Tcp
        ));
        assert!(!budgeted_genet_smoltcp_poll_after_tcp_borrow(
            BudgetedNetPhase::SelfTest
        ));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn cyw43_budgeted_tcp_fast_path_borrows_non_tcp_phases() {
        let tcp_policy = NetStagePolicy {
            allow_tcp: true,
            allow_selftest: true,
            allow_outbound_probe: false,
            allow_console_io: true,
            tx_only: false,
        };
        let no_tcp_policy = NetStagePolicy {
            allow_tcp: false,
            ..tcp_policy
        };

        assert!(budgeted_cyw43_tcp_fast_path_due(
            crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT,
            tcp_policy,
            None
        ));
        assert!(!budgeted_cyw43_tcp_fast_path_due(
            crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT,
            tcp_policy,
            Some("wifi-host-eapol-pending")
        ));
        assert!(!budgeted_cyw43_tcp_fast_path_due(
            crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT,
            no_tcp_policy,
            None
        ));
        assert!(!budgeted_cyw43_tcp_fast_path_due(
            crate::hal::driver_task::GENET_DRIVER_TASK_CONTRACT,
            tcp_policy,
            None
        ));

        assert!(budgeted_cyw43_tcp_phase_borrow_allowed(
            BudgetedNetPhase::Interface
        ));
        assert!(budgeted_cyw43_tcp_phase_borrow_allowed(
            BudgetedNetPhase::InterfaceFlush
        ));
        assert!(budgeted_cyw43_tcp_phase_borrow_allowed(
            BudgetedNetPhase::Dhcp
        ));
        assert!(!budgeted_cyw43_tcp_phase_borrow_allowed(
            BudgetedNetPhase::Tcp
        ));
        assert!(!budgeted_cyw43_tcp_phase_borrow_allowed(
            BudgetedNetPhase::SelfTest
        ));
        assert!(budgeted_cyw43_dhcp_service_preempts_phase(
            crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT,
            BudgetedNetPhase::Interface,
            true
        ));
        assert!(budgeted_cyw43_dhcp_service_preempts_phase(
            crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT,
            BudgetedNetPhase::SelfTest,
            true
        ));
        assert!(!budgeted_cyw43_dhcp_service_preempts_phase(
            crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT,
            BudgetedNetPhase::Dhcp,
            true
        ));
        assert!(!budgeted_cyw43_dhcp_service_preempts_phase(
            crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT,
            BudgetedNetPhase::Interface,
            false
        ));
        assert!(!budgeted_cyw43_dhcp_service_preempts_phase(
            crate::hal::driver_task::GENET_DRIVER_TASK_CONTRACT,
            BudgetedNetPhase::Interface,
            true
        ));
        assert!(budgeted_cyw43_selftest_defers_to_tcp(
            crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT,
            tcp_policy,
            None,
            BudgetedNetPhase::SelfTest,
            true,
            TcpState::Established,
            true
        ));
        assert!(!budgeted_cyw43_selftest_defers_to_tcp(
            crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT,
            tcp_policy,
            None,
            BudgetedNetPhase::SelfTest,
            true,
            TcpState::Established,
            false
        ));
        assert!(!budgeted_cyw43_selftest_defers_to_tcp(
            crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT,
            tcp_policy,
            None,
            BudgetedNetPhase::SelfTest,
            false,
            TcpState::Established,
            true
        ));
        assert!(!budgeted_cyw43_selftest_defers_to_tcp(
            crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT,
            tcp_policy,
            Some("wifi-host-eapol-pending"),
            BudgetedNetPhase::SelfTest,
            true,
            TcpState::Established,
            true
        ));
        assert!(!budgeted_cyw43_selftest_defers_to_tcp(
            crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT,
            tcp_policy,
            None,
            BudgetedNetPhase::Dhcp,
            true,
            TcpState::Established,
            true
        ));
        assert!(!budgeted_cyw43_selftest_defers_to_tcp(
            crate::hal::driver_task::GENET_DRIVER_TASK_CONTRACT,
            tcp_policy,
            None,
            BudgetedNetPhase::SelfTest,
            true,
            TcpState::Established,
            true
        ));
        assert!(budgeted_cyw43_smoltcp_poll_after_tcp_borrow(
            BudgetedNetPhase::Interface
        ));
        assert!(budgeted_cyw43_smoltcp_poll_after_tcp_borrow(
            BudgetedNetPhase::InterfaceFlush
        ));
        assert!(budgeted_cyw43_smoltcp_poll_after_tcp_borrow(
            BudgetedNetPhase::Dhcp
        ));
        assert!(!budgeted_cyw43_smoltcp_poll_after_tcp_borrow(
            BudgetedNetPhase::Tcp
        ));
        assert!(!budgeted_cyw43_smoltcp_poll_after_tcp_borrow(
            BudgetedNetPhase::SelfTest
        ));
    }

    #[test]
    fn cyw43_tcp_ready_requires_observed_host_data_path() {
        let counters = NetCounters {
            tx_submit: 8,
            ..NetCounters::default()
        };

        assert!(!cyw43_tcp_data_path_proven("cyw43", "wifi", counters));
        assert!(cyw43_tcp_data_path_proven(
            "cyw43",
            "wifi",
            NetCounters {
                tcp_accepts: 1,
                ..counters
            }
        ));
        assert!(cyw43_tcp_data_path_proven(
            "cyw43",
            "wifi",
            NetCounters {
                tcp_auth_sessions: 1,
                ..counters
            }
        ));
        assert!(!cyw43_tcp_data_path_proven(
            "bcmgenet-v5",
            "wired",
            NetCounters {
                tcp_accepts: 1,
                ..counters
            }
        ));
    }

    #[test]
    fn physical_pi_tcp_ready_requires_tcp_session_proof() {
        assert!(!net_status_tcp_ready(
            true,
            "bcmgenet-v5",
            "wired",
            NetCounters {
                arp_rx: 1,
                arp_tx: 1,
                ..NetCounters::default()
            }
        ));
        assert!(net_status_tcp_ready(
            true,
            "bcmgenet-v5",
            "wired",
            NetCounters {
                tcp_accepts: 1,
                arp_rx: 1,
                arp_tx: 1,
                ..NetCounters::default()
            }
        ));
        assert!(!net_status_tcp_ready(
            true,
            "cyw43",
            "wifi",
            NetCounters {
                tx_submit: 8,
                ..NetCounters::default()
            }
        ));
        assert!(net_status_tcp_ready(
            true,
            "rtl8139",
            "wired",
            NetCounters::default()
        ));
        assert!(!net_status_tcp_ready(
            false,
            "rtl8139",
            "wired",
            NetCounters {
                tcp_accepts: 1,
                ..NetCounters::default()
            }
        ));
    }

    #[test]
    fn console_listener_ready_requires_bound_non_deferred_admission() {
        assert!(net_console_listener_ready(true, true, false, false));
        assert!(!net_console_listener_ready(false, true, false, false));
        assert!(!net_console_listener_ready(true, false, false, false));
        assert!(!net_console_listener_ready(true, true, true, false));
        assert!(!net_console_listener_ready(true, true, false, true));
    }

    #[test]
    fn cyw43_status_reports_generation_local_rx_loss_without_backlog_false_positive() {
        assert_eq!(
            cyw43_status_blocker_for(
                "cyw43",
                "wifi",
                NetCounters {
                    wifi_rx_runtime_queue_overflow_seen: 1,
                    wifi_rx_runtime_overflow_episodes: 1,
                    wifi_rx_runtime_queue_count: 16,
                    wifi_rx_runtime_queue_high_water: 16,
                    ..NetCounters::default()
                }
            ),
            Some(Cyw43StatusBlocker {
                address_source: "wifi-rx-overflow",
                dhcp_phase: "rx-overflow"
            })
        );
        assert_eq!(
            cyw43_status_blocker_for(
                "cyw43",
                "wifi",
                NetCounters {
                    wifi_rx_runtime_drain_budget_hit: 1,
                    wifi_rx_runtime_queue_count: 4,
                    ..NetCounters::default()
                }
            ),
            None
        );
    }

    #[test]
    fn cyw43_status_reports_typed_tx_terminal_fault_until_host_proof() {
        let counters = NetCounters {
            tx_submit: 60,
            tx_free: 0,
            tx_in_flight: 0,
            wifi_data_trace_faults: 3,
            ..NetCounters::default()
        };

        assert_eq!(
            cyw43_status_blocker_for(
                "cyw43",
                "wifi",
                NetCounters {
                    tx_submit: 60,
                    tx_complete: 60,
                    tx_free: 1,
                    tx_in_flight: 0,
                    wifi_data_trace_tx_retries: 3,
                    ..NetCounters::default()
                }
            ),
            None
        );
        assert_eq!(
            cyw43_status_blocker_for("cyw43", "wifi", counters),
            Some(Cyw43StatusBlocker {
                address_source: "wifi-tx-terminal-fault",
                dhcp_phase: "tx-terminal-fault"
            })
        );
        assert_eq!(
            cyw43_status_blocker_for(
                "cyw43",
                "wifi",
                NetCounters {
                    tx_submit: 60,
                    tx_complete: 59,
                    tx_in_flight: 1,
                    ..NetCounters::default()
                }
            ),
            None,
            "an ordinary exact in-flight TX owner is not a credit or terminal fault"
        );
        assert_eq!(
            cyw43_status_blocker_for(
                "cyw43",
                "wifi",
                NetCounters {
                    tcp_auth_sessions: 1,
                    ..counters
                }
            ),
            None
        );
        assert_eq!(
            cyw43_status_blocker_for("bcmgenet-v5", "wired", counters),
            None
        );
    }

    #[test]
    fn cyw43_status_reports_missing_host_eapol_m3() {
        assert_eq!(
            cyw43_status_blocker_for(
                "cyw43",
                "wifi",
                NetCounters {
                    wifi_host_eapol_m1: 1,
                    wifi_host_eapol_m2: 1,
                    wifi_host_eapol_m3: 0,
                    wifi_host_eapol_secure: 0,
                    ..NetCounters::default()
                }
            ),
            Some(Cyw43StatusBlocker {
                address_source: "host-eapol-m3-missing",
                dhcp_phase: "host-eapol-m3-missing"
            })
        );
        assert_eq!(
            cyw43_status_blocker_for(
                "cyw43",
                "wifi",
                NetCounters {
                    wifi_host_eapol_m1: 1,
                    wifi_host_eapol_m2: 1,
                    wifi_host_eapol_m3: 1,
                    wifi_host_eapol_secure: 1,
                    ..NetCounters::default()
                }
            ),
            None
        );
    }

    #[test]
    fn dhcp_restart_backoff_scales_with_timebase() {
        if cfg!(feature = "timers-arch-counter") {
            assert_eq!(DHCP_RESTART_BACKOFF_MS, 4_000);
        } else {
            assert!(DHCP_RESTART_BACKOFF_MS >= 64_000);
        }
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn net_ring_hot_path_is_limited_to_pi4_nics() {
        assert_eq!(
            net_driver_task_hot_path(crate::hal::driver_task::GENET_DRIVER_TASK_CONTRACT),
            Some(crate::hal::driver_task::DriverTaskHotPath::GenetNic)
        );
        assert_eq!(
            net_driver_task_hot_path(crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT),
            Some(crate::hal::driver_task::DriverTaskHotPath::Cyw43Wifi)
        );
        assert_eq!(
            net_driver_task_hot_path(crate::hal::driver_task::RTL8139_DRIVER_TASK_CONTRACT),
            None
        );
        assert_eq!(
            net_driver_task_hot_path(crate::hal::driver_task::VIRTIO_NET_DRIVER_TASK_CONTRACT),
            None
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn pi4_driver_task_nic_clients_are_pointer_free_runtime_clients() {
        assert!(<GenetDriverTaskDevice as NetDevice>::driver_task_runtime_client());
        assert!(<Cyw43DriverTaskDevice as NetDevice>::driver_task_runtime_client());
        assert_eq!(
            <GenetDriverTaskDevice as NetDevice>::driver_task_contract(),
            crate::hal::driver_task::GENET_DRIVER_TASK_CONTRACT
        );
        assert_eq!(
            <Cyw43DriverTaskDevice as NetDevice>::driver_task_contract(),
            crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn net_driver_task_runtime_ring_service_uses_selector_not_root_pointer() {
        let command = crate::hal::driver_task::DriverTaskCommandRecord::pi4_hot_path(
            31,
            crate::hal::driver_task::DriverTaskHotPath::GenetNic,
            crate::hal::driver_task::DriverTaskBudgetGrant::from_contract(
                crate::hal::driver_task::GENET_DRIVER_TASK_CONTRACT,
            ),
            crate::hal::driver_task::DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        );

        let completion = unsafe {
            net_driver_task_runtime_ring_service(
                crate::hal::driver_task::DriverTaskHotPath::GenetNic.as_u32() as usize,
                command,
            )
        };

        assert_eq!(completion.sequence, 31);
        assert_eq!(
            completion.code,
            crate::hal::driver_task::DriverTaskCompletionCode::Fault.as_u16()
        );
        assert_eq!(
            completion.detail,
            crate::hal::driver_task::DriverTaskFaultCode::DeviceUnavailable.as_u16()
        );
    }

    #[test]
    fn reservation_sets_metadata_and_owner() {
        let _guard = NET_STACK_STORAGE_TEST_LOCK.lock();
        reset_console_storage_state();

        let attempt = NetInitAttempt::new("test.acquisition");
        let reservation =
            StorageReservation::acquire::<Infallible>(false, false, &attempt, "test.acquisition")
                .expect("reservation should succeed");

        assert!(SOCKET_STORAGE_IN_USE.load(Ordering::Acquire));
        assert_ne!(SOCKET_STORAGE_OWNER.load(Ordering::Acquire), 0);
        assert_ne!(SOCKET_STORAGE_TAG_ID.load(Ordering::Acquire), 0);
        assert!(TCP_STANDBY_RX_STORAGE_IN_USE.load(Ordering::Acquire));
        assert_eq!(
            TCP_STANDBY_RX_STORAGE_OWNER.load(Ordering::Acquire),
            attempt.owner_id()
        );
        assert!(TCP_STANDBY_TX_STORAGE_IN_USE.load(Ordering::Acquire));
        assert_eq!(
            TCP_STANDBY_TX_STORAGE_OWNER.load(Ordering::Acquire),
            attempt.owner_id()
        );
        assert!(ICMP_ECHO_STORAGE_IN_USE.load(Ordering::Acquire));
        assert_eq!(
            ICMP_ECHO_STORAGE_OWNER.load(Ordering::Acquire),
            attempt.owner_id()
        );
        assert_ne!(ICMP_ECHO_STORAGE_TAG_ID.load(Ordering::Acquire), 0);

        drop(reservation);

        assert!(!SOCKET_STORAGE_IN_USE.load(Ordering::Acquire));
        assert_eq!(SOCKET_STORAGE_OWNER.load(Ordering::Acquire), 0);
        assert_eq!(SOCKET_STORAGE_TAG_ID.load(Ordering::Acquire), 0);
        assert!(!TCP_STANDBY_RX_STORAGE_IN_USE.load(Ordering::Acquire));
        assert_eq!(TCP_STANDBY_RX_STORAGE_OWNER.load(Ordering::Acquire), 0);
        assert!(!TCP_STANDBY_TX_STORAGE_IN_USE.load(Ordering::Acquire));
        assert_eq!(TCP_STANDBY_TX_STORAGE_OWNER.load(Ordering::Acquire), 0);
        assert!(!ICMP_ECHO_STORAGE_IN_USE.load(Ordering::Acquire));
        assert_eq!(ICMP_ECHO_STORAGE_OWNER.load(Ordering::Acquire), 0);
        assert_eq!(ICMP_ECHO_STORAGE_TAG_ID.load(Ordering::Acquire), 0);
    }

    #[test]
    fn icmp_echo_reservation_failure_releases_earlier_leases() {
        let _guard = NET_STACK_STORAGE_TEST_LOCK.lock();
        reset_console_storage_state();

        ICMP_ECHO_STORAGE_IN_USE.store(true, Ordering::Release);
        ICMP_ECHO_STORAGE_OWNER.store(0x1c4d, Ordering::Release);
        let attempt = NetInitAttempt::new("test.icmp-echo-reservation");
        let result = StorageReservation::acquire::<Infallible>(
            false,
            false,
            &attempt,
            "test.icmp-echo-reservation",
        );

        assert!(matches!(result, Err(NetStackError::IcmpEchoStorageInUse)));
        assert!(!SOCKET_STORAGE_IN_USE.load(Ordering::Acquire));
        assert!(!TCP_RX_STORAGE_IN_USE.load(Ordering::Acquire));
        assert!(!TCP_TX_STORAGE_IN_USE.load(Ordering::Acquire));
        assert!(!TCP_STANDBY_RX_STORAGE_IN_USE.load(Ordering::Acquire));
        assert!(!TCP_STANDBY_TX_STORAGE_IN_USE.load(Ordering::Acquire));
        assert!(ICMP_ECHO_STORAGE_IN_USE.load(Ordering::Acquire));
        assert_eq!(ICMP_ECHO_STORAGE_OWNER.load(Ordering::Acquire), 0x1c4d);

        ICMP_ECHO_STORAGE_IN_USE.store(false, Ordering::Release);
        ICMP_ECHO_STORAGE_OWNER.store(0, Ordering::Release);
    }

    #[test]
    fn standby_reservation_failure_releases_all_earlier_console_leases() {
        let _guard = NET_STACK_STORAGE_TEST_LOCK.lock();
        reset_console_storage_state();

        TCP_STANDBY_RX_STORAGE_IN_USE.store(true, Ordering::Release);
        TCP_STANDBY_RX_STORAGE_OWNER.store(0xfeed, Ordering::Release);
        let attempt = NetInitAttempt::new("test.standby-reservation");
        let result = StorageReservation::acquire::<Infallible>(
            false,
            false,
            &attempt,
            "test.standby-reservation",
        );

        assert!(matches!(
            result,
            Err(NetStackError::TcpStandbyRxStorageInUse)
        ));
        assert!(!SOCKET_STORAGE_IN_USE.load(Ordering::Acquire));
        assert!(!TCP_RX_STORAGE_IN_USE.load(Ordering::Acquire));
        assert!(!TCP_TX_STORAGE_IN_USE.load(Ordering::Acquire));
        assert!(TCP_STANDBY_RX_STORAGE_IN_USE.load(Ordering::Acquire));
        assert!(!TCP_STANDBY_TX_STORAGE_IN_USE.load(Ordering::Acquire));

        TCP_STANDBY_RX_STORAGE_IN_USE.store(false, Ordering::Release);
        TCP_STANDBY_RX_STORAGE_OWNER.store(0, Ordering::Release);
    }

    #[test]
    fn poisoned_flag_is_reported() {
        let _guard = NET_STACK_STORAGE_TEST_LOCK.lock();
        reset_console_storage_state();

        SOCKET_STORAGE_IN_USE.store(true, Ordering::Release);
        SOCKET_STORAGE_OWNER.store(0, Ordering::Release);
        SOCKET_STORAGE_TAG_ID.store(0, Ordering::Release);

        let attempt = NetInitAttempt::new("test.poisoned");
        let result =
            StorageReservation::acquire::<Infallible>(false, false, &attempt, "test.poisoned");

        assert!(matches!(result, Err(NetStackError::SocketStoragePoisoned)));
        assert!(SOCKET_STORAGE_IN_USE.load(Ordering::Acquire));
        assert_eq!(SOCKET_STORAGE_OWNER.load(Ordering::Acquire), 0);
        assert_eq!(SOCKET_STORAGE_TAG_ID.load(Ordering::Acquire), 0);
    }

    #[test]
    fn busy_socket_reports_owner_and_tag() {
        let _guard = NET_STACK_STORAGE_TEST_LOCK.lock();
        reset_console_storage_state();

        SOCKET_STORAGE_OWNER.store(0xdead_beef, Ordering::Release);
        SOCKET_STORAGE_TAG_ID.store(0xcafe_0001, Ordering::Release);
        *SOCKET_STORAGE_TAG_LABEL.lock() = Some("test.busy");
        SOCKET_STORAGE_IN_USE.store(true, Ordering::Release);

        let attempt = NetInitAttempt::new("test.busy");
        let result = StorageReservation::acquire::<Infallible>(false, false, &attempt, "test.busy");

        assert!(matches!(result, Err(NetStackError::SocketStorageInUse)));
        assert!(SOCKET_STORAGE_IN_USE.load(Ordering::Acquire));
        assert_eq!(SOCKET_STORAGE_OWNER.load(Ordering::Acquire), 0xdead_beef);
        assert_eq!(SOCKET_STORAGE_TAG_ID.load(Ordering::Acquire), 0xcafe_0001);
    }

    #[test]
    fn trace_conn_prefix_clamps_to_capacity() {
        let mut payload = [0u8; 32];
        for (i, byte) in payload.iter_mut().enumerate() {
            *byte = i as u8;
        }

        let (prefix, prefix_len) = NetStack::<DefaultNetDevice>::trace_conn_prefix(&payload, true);
        assert_eq!(prefix_len, 16);
        assert_eq!(&prefix[..prefix_len], &payload[..16]);
    }

    #[test]
    fn trace_conn_prefix_handles_short_payload() {
        let payload = [1u8, 2u8, 3u8];
        let (prefix, prefix_len) = NetStack::<DefaultNetDevice>::trace_conn_prefix(&payload, true);
        assert_eq!(prefix_len, payload.len());
        assert_eq!(&prefix[..prefix_len], payload);
    }

    #[test]
    fn trace_conn_prefix_redacts_unauthenticated_payload() {
        let payload = b"AUTH production-secret";
        let (prefix, prefix_len) = NetStack::<DefaultNetDevice>::trace_conn_prefix(payload, false);
        assert_eq!(prefix_len, 0);
        assert_eq!(prefix, [0u8; 16]);
    }

    #[test]
    fn self_test_hint_distinguishes_driver_rx_from_socket_rx() {
        let result = NetSelfTestResult {
            tx_ok: true,
            udp_echo_ok: false,
            tcp_ok: false,
            console_ok: false,
            peer_assisted_ok: false,
        };
        let counters = NetCounters {
            rx_packets: 78,
            udp_rx: 0,
            tcp_accepts: 0,
            ..NetCounters::default()
        };
        assert_eq!(
            self_test_failure_hint(result, counters),
            Some(
                "[net-selftest] hint: driver RX works, but no peer UDP/TCP reached self-test sockets -> run the logged host-side commands on the peer and verify IP/ARP/route",
            )
        );
    }

    #[test]
    fn self_test_run_generation_is_nonzero_and_advances_per_admission() {
        let mut state = SelfTestState::new(true);

        assert_eq!(state.report().run_generation, 0);
        assert!(state.start(100, 0));
        assert_eq!(state.report().run_generation, 1);
        state.reset_for_connection_generation();
        assert_eq!(state.report().run_generation, 1);
        assert!(!state.report().running);
        assert!(state.report().last_result.is_none());
        assert!(state.start(200, 0));
        assert_eq!(state.report().run_generation, 2);
    }

    #[test]
    fn self_test_peer_assisted_ok_accepts_wifi_secure_remote_console_proof() {
        let mut result = NetSelfTestResult {
            tx_ok: true,
            udp_echo_ok: false,
            tcp_ok: false,
            console_ok: true,
            peer_assisted_ok: false,
        };
        let counters = NetCounters {
            rx_packets: 12,
            tcp_auth_sessions: 1,
            wifi_host_eapol_secure: 1,
            ..NetCounters::default()
        };

        result.peer_assisted_ok = self_test_peer_assisted_ok(result, counters);

        assert!(result.peer_assisted_ok);
        assert_eq!(
            self_test_failure_hint(result, counters),
            Some(
                "[net-selftest] hint: peer-assisted echo/smoke checks incomplete, but local TX/RX and authenticated link proof are present",
            )
        );
    }

    #[test]
    fn self_test_udp_beacon_bufferfull_does_not_warn() {
        assert_eq!(
            udp_beacon_send_failure_log_severity(true, 1),
            SelfTestLogSeverity::Debug
        );
        assert_eq!(
            udp_beacon_send_failure_log_severity(true, 2),
            SelfTestLogSeverity::Trace
        );
        assert_eq!(
            udp_beacon_send_failure_log_severity(false, 1),
            SelfTestLogSeverity::Warn
        );
        assert_eq!(
            udp_beacon_send_failure_log_severity(false, 2),
            SelfTestLogSeverity::Debug
        );
    }

    #[test]
    fn self_test_udp_beacon_bind_is_lease_agnostic() {
        let endpoint = udp_beacon_bind_endpoint(UDP_BEACON_PORT);

        assert_eq!(endpoint.port, UDP_BEACON_PORT);
        assert!(endpoint.addr.is_none());
    }

    #[test]
    fn self_test_hint_reports_driver_level_rx_failure_when_no_frames_arrive() {
        let result = NetSelfTestResult {
            tx_ok: true,
            udp_echo_ok: false,
            tcp_ok: false,
            console_ok: false,
            peer_assisted_ok: false,
        };
        let counters = NetCounters::default();
        assert_eq!(
            self_test_failure_hint(result, counters),
            Some(
                "[net-selftest] hint: RX never reaches the driver -> buffers not posted / used ring not read / IRQ missing",
            )
        );
    }
}
