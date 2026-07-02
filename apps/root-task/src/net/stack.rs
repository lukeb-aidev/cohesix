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
//!   should emit auth frame logs showing the exact bytes parsed on the server.
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
    Config as IfaceConfig, Interface, PollResult, SocketHandle, SocketSet, SocketStorage,
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
    EthernetAddress, HardwareAddress, IpAddress, IpCidr, IpEndpoint, IpListenEndpoint, Ipv4Address,
};

use super::{
    console_srv::{SessionEvent, TcpConsoleServer},
    dhcp::{DhcpClient, DhcpEvent, DhcpLease, DhcpPhase, DHCP_CLIENT_PORT, DHCP_SERVER_PORT},
    outbound::{OutboundCoalescer, OutboundLane, SendError},
    ConsoleLine, ConsoleNetConfig, NetBackend, NetConsoleDisconnectReason, NetConsoleEvent,
    NetCounters, NetDevice, NetDriverError, NetInterfacePolicy, NetMode, NetPoller,
    NetSelfTestReport, NetSelfTestResult, NetSelfTestStartResult, NetStage, NetStatusReport,
    NetTelemetry, WifiCredentials, DEV_VIRT_GATEWAY, DEV_VIRT_IP, DEV_VIRT_PREFIX, NET_DIAG,
    NET_STAGE,
};
use crate::bootstrap::bootinfo_snapshot::{BootInfoCanaryError, BootInfoState};
use crate::debug::maybe_report_str_write;
use crate::drivers::driver_task_net::{
    Cyw43DriverTaskDevice, DriverTaskNetError, GenetDriverTaskDevice,
};
use crate::drivers::rtl8139::{DriverError as Rtl8139DriverError, Rtl8139Device};
#[cfg(feature = "net-backend-virtio")]
use crate::drivers::virtio::net::{DriverError as VirtioDriverError, VirtioNetStatic};
use crate::hal::driver_task::{DriverServiceBudget, DriverServiceBudgetError};
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
const CYW43_RESPONSE_FLUSH_FRAMES_PER_TURN: u32 = 16;
const CYW43_RESPONSE_FLUSH_BYTES_PER_TURN: usize = 8 * 1024;
const SAME_TICK_STALL_WARN_POLLS: u16 = 256;
const MAX_DHCP_RX_PACKETS_PER_POLL: usize = 2;
const MAX_UDP_ECHO_PACKETS_PER_POLL: usize = 2;
const TCP_CONSOLE_RECV_CHUNK_BYTES: usize = DEFAULT_LINE_CAPACITY + 4;
const MAX_TCP_CONSOLE_RECV_CHUNKS_PER_POLL: usize = 64;
const MAX_TCP_CONSOLE_RECV_BYTES_PER_POLL: usize = 20 * 1024;
const TCP_SERVICE_BYTES_PER_TURN: u32 =
    (MAX_TCP_CONSOLE_RECV_BYTES_PER_POLL + MAX_CONSOLE_BYTES_PER_POLL) as u32;
const TCP_RESPONSE_FLUSH_BYTES_PER_TURN: u32 = CYW43_RESPONSE_FLUSH_BYTES_PER_TURN as u32;
const MAX_TCP_SMOKE_RECV_CHUNKS_PER_POLL: usize = 2;
const TCP_SMOKE_RX_BUFFER: usize = 256;
const TCP_SMOKE_TX_BUFFER: usize = 256;
const SOCKET_CAPACITY: usize = 6;
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
const CYW43_DHCP_POST_SECURE_EAPOL_QUIET_MS: u64 = 500;
const CYW43_DHCP_POST_SECURE_EAPOL_OVERSHOOT_LOG_MS: u64 = 1_000;
const CYW43_DHCP_RX_ADMISSION_RETRY_MS: u64 = 250;
const UDP_ECHO_PORT: u16 = 31_338;
const UDP_BEACON_PORT: u16 = 40_000;
const TCP_SMOKE_PORT: u16 = 31_339;
const TCP_SMOKE_OUT_LOCAL_PORT: u16 = 31_340;
const TCP_CONSOLE_SELFTEST_LOCAL_PORT: u16 = 31_341;
const CONSOLE_SELFTEST_RECOVERY_DEADLINE_MS: u64 = 3_000;
const CONSOLE_SELFTEST_RETRY_MS: u64 = 250;
const DISCONNECT_GRACE_MS: u64 = 250;
const DISCONNECT_GRACE_POLLS: u8 = 64;
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
    if counters.wifi_rx_runtime_queue_overflow_seen != 0 || counters.wifi_rx_pending_drops != 0 {
        return Some(Cyw43StatusBlocker {
            address_source: "wifi-rx-overflow",
            dhcp_phase: "rx-overflow",
        });
    }
    if counters.wifi_rx_runtime_drain_budget_hit != 0
        && (counters.wifi_rx_runtime_queue_count != 0
            || counters.wifi_rx_runtime_queue_high_water != 0
            || counters.wifi_rx_pending_queue_count != 0)
    {
        return Some(Cyw43StatusBlocker {
            address_source: "wifi-rx-starvation",
            dhcp_phase: "rx-starvation",
        });
    }
    if counters.wifi_data_trace_faults != 0
        || (counters.tx_submit > counters.tx_complete
            && (counters.wifi_data_trace_tx_retries != 0 || counters.tx_in_flight != 0))
        || (counters.wifi_data_trace_tx_retries != 0
            && counters.tx_free == 0
            && counters.tx_in_flight == 0
            && counters.tx_submit != 0)
    {
        return Some(Cyw43StatusBlocker {
            address_source: "wifi-tx-credit-anomaly",
            dhcp_phase: "tx-credit-anomaly",
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
    TcpSmokeRxStorageInUse,
    TcpSmokeTxStorageInUse,
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
            Self::TcpSmokeRxStorageInUse => f.write_str("TCP smoke test RX storage already in use"),
            Self::TcpSmokeTxStorageInUse => f.write_str("TCP smoke test TX storage already in use"),
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
    dhcp: Option<StorageLease>,
    tcp_smoke_rx: Option<StorageLease>,
    tcp_smoke_tx: Option<StorageLease>,
    tcp_smoke_out_rx: Option<StorageLease>,
    tcp_smoke_out_tx: Option<StorageLease>,
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
            dhcp,
            tcp_smoke_rx,
            tcp_smoke_tx,
            tcp_smoke_out_rx,
            tcp_smoke_out_tx,
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
static mut UDP_BEACON_RX_STORAGE: [u8; UDP_PAYLOAD_CAPACITY] = [0u8; UDP_PAYLOAD_CAPACITY];
static mut UDP_BEACON_TX_STORAGE: [u8; UDP_PAYLOAD_CAPACITY] = [0u8; UDP_PAYLOAD_CAPACITY];
static mut UDP_ECHO_RX_STORAGE: [u8; UDP_PAYLOAD_CAPACITY] = [0u8; UDP_PAYLOAD_CAPACITY];
static mut UDP_ECHO_TX_STORAGE: [u8; UDP_PAYLOAD_CAPACITY] = [0u8; UDP_PAYLOAD_CAPACITY];
static mut DHCP_RX_STORAGE: [u8; DHCP_PAYLOAD_CAPACITY] = [0u8; DHCP_PAYLOAD_CAPACITY];
static mut DHCP_TX_STORAGE: [u8; DHCP_PAYLOAD_CAPACITY] = [0u8; DHCP_PAYLOAD_CAPACITY];

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
    tcp_handle: SocketHandle,
    server: TcpConsoleServer,
    outbound: OutboundCoalescer,
    telemetry: NetTelemetry,
    backend: NetBackend,
    mode: NetMode,
    interface_policy: NetInterfacePolicy,
    wifi_credentials: Option<WifiCredentials>,
    ip: Ipv4Address,
    gateway: Option<Ipv4Address>,
    prefix_len: u8,
    listen_port: u16,
    session_active: bool,
    disconnect_requested: bool,
    disconnect_requested_at_ms: Option<u64>,
    disconnect_requested_polls: u8,
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
        && matches!(mode, NetMode::Dhcp)
        && bringup_status.is_none()
        && (matches!(
            dhcp_phase,
            Some(DhcpPhase::Selecting | DhcpPhase::Requesting | DhcpPhase::Bound)
        ) || (dhcp_socket_ready
            && ip == Ipv4Address::UNSPECIFIED
            && matches!(dhcp_phase, Some(DhcpPhase::Disabled))))
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct SelfTestState {
    enabled: bool,
    running: bool,
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
) -> Result<DefaultNetStack, DefaultNetConsoleError>
where
    H: Hardware<Error = HalError>,
{
    crate::drivers::driver_task_net::init_cyw43_runtime(hal, &config, NET_STAGE)
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

fn configured_active_driver_label(config: &ConsoleNetConfig) -> &'static str {
    match (config.backend, config.policy.interface) {
        (NetBackend::BcmGenet, NetInterfacePolicy::Wifi) => "cyw43",
        (NetBackend::BcmGenet, NetInterfacePolicy::Auto) if config.wifi_credentials.is_some() => {
            "cyw43"
        }
        (NetBackend::BcmGenet, _) => "bcmgenet-v5",
        (NetBackend::Rtl8139, _) => "rtl8139",
        #[cfg(feature = "net-backend-virtio")]
        (NetBackend::Virtio, _) => "virtio-net",
    }
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
    log_socket_tripwire(concat!(file!(), ":", line!()));

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
            ),
            NetInterfacePolicy::Auto => {
                if config.wifi_credentials.is_some() {
                    init_cyw43_driver_task_console(
                        hal,
                        config,
                        backend,
                        "net.init.wrap.after-new.cyw43-driver-task-auto",
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
        NetStackError::TcpSmokeRxStorageInUse => {
            NetConsoleError::Init(NetStackError::TcpSmokeRxStorageInUse)
        }
        NetStackError::TcpSmokeTxStorageInUse => {
            NetConsoleError::Init(NetStackError::TcpSmokeTxStorageInUse)
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
        match (self.backend, self.device.interface_label()) {
            (NetBackend::BcmGenet, "wifi") => "cyw43",
            (NetBackend::BcmGenet, _) => "bcmgenet-v5",
            (NetBackend::Rtl8139, _) => "rtl8139",
            #[cfg(feature = "net-backend-virtio")]
            (NetBackend::Virtio, _) => "virtio-net",
        }
    }

    fn console_listener_defer_reason(&self) -> Option<&'static str> {
        if self.wifi_rx_admission_blocked {
            return Some("wifi-data-rx-admission-blocked");
        }
        console_listener_defer_reason_for(self.mode, self.ip, self.device.bringup_status_label())
    }

    fn rebuild_console_socket(
        &mut self,
        now_ms: u64,
        rx_capacity: usize,
        tx_capacity: usize,
        rx_queue: usize,
        tx_queue: usize,
    ) -> bool {
        error!(
            "[net-console] console socket buffer corruption detected (rx_capacity={}, tx_capacity={}, rx_queue={}, tx_queue={}); rebuilding socket",
            rx_capacity,
            tx_capacity,
            rx_queue,
            tx_queue
        );
        self.outbound.reset();
        self.server.end_session();
        self.session_active = false;
        self.active_client_id = None;
        self.peer_endpoint = None;
        self.listener_announced = false;
        self.reset_session_state_with(None);
        let defer_reason = self.console_listener_defer_reason();
        let _ = self.sockets.remove(self.tcp_handle);
        let rx_buffer = unsafe { TcpSocketBuffer::new(&mut TCP_RX_STORAGE[..]) };
        let tx_buffer = unsafe { TcpSocketBuffer::new(&mut TCP_TX_STORAGE[..]) };
        let mut tcp_socket = TcpSocket::new(rx_buffer, tx_buffer);
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
        true
    }

    fn validate_console_socket(&mut self, now_ms: u64) -> bool {
        let (rx_capacity, tx_capacity, rx_queue, tx_queue) = {
            let socket = self.sockets.get::<TcpSocket>(self.tcp_handle);
            (
                socket.recv_capacity(),
                socket.send_capacity(),
                socket.recv_queue(),
                socket.send_queue(),
            )
        };
        let capacity_ok = Self::console_socket_capacity_ok(rx_capacity, tx_capacity);
        let queue_ok = rx_queue <= rx_capacity && tx_queue <= tx_capacity;
        if capacity_ok && queue_ok {
            return true;
        }
        self.rebuild_console_socket(now_ms, rx_capacity, tx_capacity, rx_queue, tx_queue);
        false
    }

    fn set_auth_state(auth_state: &mut AuthState, active_client_id: Option<u64>, next: AuthState) {
        if next != *auth_state {
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
        self.auth_state = AuthState::Start;
        let preconnect_logged = self.session_state.flush_blocked_logged_preconnect;
        self.session_state = SessionState::default();
        self.session_state.flush_blocked_logged_preconnect = preconnect_logged;
        self.conn_bytes_read = 0;
        self.conn_bytes_written = 0;
        self.disconnect_requested = false;
        self.disconnect_requested_at_ms = None;
        self.disconnect_requested_polls = 0;
    }

    fn reset_session_state_with(&mut self, tcp_state: Option<TcpState>) {
        self.reset_session_state();
        if let Some(state) = tcp_state {
            self.session_state.last_state = Some(state);
        }
    }

    fn force_relisten(socket: &mut TcpSocket, listen_port: u16) {
        if socket.state() != TcpState::Closed {
            socket.abort();
        }
        if socket.state() == TcpState::Closed {
            match socket.listen(IpListenEndpoint::from(listen_port)) {
                Ok(()) => {
                    NET_DIAG.record_listener_bound();
                }
                Err(err) => {
                    warn!("[net-console] failed to re-listen after close: {err}");
                }
            }
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
    fn trace_conn_prefix(payload: &[u8]) -> ([u8; 16], usize) {
        const PREFIX_CAP: usize = 16;
        let mut prefix = [0u8; PREFIX_CAP];
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

    fn trace_conn_recv(conn_id: u64, payload: &[u8]) {
        if !log::log_enabled!(log::Level::Debug) {
            return;
        }
        let (prefix, prefix_len) = Self::trace_conn_prefix(payload);
        log::debug!(
            "[cohsh-net] conn id={} recv bytes={} first16={:02x?}",
            conn_id,
            payload.len(),
            &prefix[..prefix_len]
        );
    }

    fn trace_conn_send(conn_id: u64, payload: &[u8]) {
        if !log::log_enabled!(log::Level::Debug) {
            return;
        }
        let (prefix, prefix_len) = Self::trace_conn_prefix(payload);
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

        let mut stack = Box::new(Self {
            clock,
            device,
            interface,
            sockets,
            _reservation: reservation,
            init_attempt: attempt,
            tcp_handle: SocketHandle::default(),
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
            ip,
            gateway,
            prefix_len: prefix,
            listen_port: console_config.listen_port,
            session_active: false,
            disconnect_requested: false,
            disconnect_requested_at_ms: None,
            disconnect_requested_polls: 0,
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

    fn initialise_socket(&mut self) -> Result<(), NetStackError<D::Error>> {
        debug_assert!(SOCKET_STORAGE_IN_USE.load(Ordering::Acquire));
        debug_assert!(TCP_RX_STORAGE_IN_USE.load(Ordering::Acquire));
        debug_assert!(TCP_TX_STORAGE_IN_USE.load(Ordering::Acquire));
        let rx_buffer = unsafe { TcpSocketBuffer::new(&mut TCP_RX_STORAGE[..]) };
        let tx_buffer = unsafe { TcpSocketBuffer::new(&mut TCP_TX_STORAGE[..]) };
        let tcp_socket = TcpSocket::new(rx_buffer, tx_buffer);
        self.tcp_handle = self.sockets.add(tcp_socket);
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
            let beacon_endpoint = IpListenEndpoint {
                addr: Some(IpAddress::Ipv4(self.ip)),
                port: UDP_BEACON_PORT,
            };
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

    fn begin_poll_turn(&mut self, now_ms: u64) -> Instant {
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

        timestamp
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

    fn poll_smoltcp_once(&mut self, timestamp: Instant, now_ms: u64, label: &'static str) -> bool {
        self.bump_poll_counter();
        let poll_result = self
            .interface
            .poll(timestamp, self.device.as_mut(), &mut self.sockets);
        let activity = poll_result != PollResult::None;
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

    fn charge_tcp_response_flush_budget(
        budget: &mut DriverServiceBudget,
    ) -> Result<(), DriverServiceBudgetError> {
        budget.charge_ops(16)?;
        budget.charge_frames(CYW43_RESPONSE_FLUSH_FRAMES_PER_TURN as u16)?;
        budget.charge_bytes(TCP_RESPONSE_FLUSH_BYTES_PER_TURN)
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
            || self.disconnect_requested;
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
    fn service_cyw43_data_pre_poll_burst_budgeted(
        &self,
        contract: crate::hal::driver_task::DriverTaskContract,
        budget: &mut DriverServiceBudget,
    ) -> bool {
        if net_driver_task_hot_path(contract)
            != Some(crate::hal::driver_task::DriverTaskHotPath::Cyw43Wifi)
            || !self.cyw43_flush_pre_poll_data_ready()
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

    fn service_budgeted_tcp_response_flush_turn(
        &mut self,
        timestamp: Instant,
        now_ms: u64,
        post_label: &'static str,
    ) -> bool {
        if !self.stage_policy.allow_tcp || !self.session_active {
            return false;
        }
        let activity = {
            let socket = self.sockets.get_mut::<TcpSocket>(self.tcp_handle);
            if socket.state() != TcpState::Established {
                return false;
            }
            Self::flush_outbound(
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
                CYW43_RESPONSE_FLUSH_FRAMES_PER_TURN,
                CYW43_RESPONSE_FLUSH_BYTES_PER_TURN,
            )
        };
        if activity {
            self.poll_smoltcp_once(timestamp, now_ms, post_label)
        } else {
            false
        }
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
        let timestamp = self.begin_poll_turn(now_ms);
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
        } else if self.budgeted_cyw43_tcp_fast_path_due() && activity {
            for _ in 0..CYW43_TCP_POST_DISPATCH_EXTRA_TURNS {
                if Self::charge_tcp_response_flush_budget(budget).is_err() {
                    break;
                }
                let turn_activity = self.service_budgeted_tcp_response_flush_turn(
                    timestamp,
                    now_ms,
                    "budgeted-cyw43-response-flush",
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
                client.start(device_mac.0, now_ms);
                self.dhcp_restart_after_ms = None;
                info!(
                    "[dhcp] restart reason=hardware-address-sync interface={} mac={} now_ms={}",
                    self.device.interface_label(),
                    device_mac,
                    now_ms
                );
            }
        }
        true
    }

    /// Polls the network stack using a host-supplied monotonic timestamp in milliseconds.
    pub fn poll_with_time(&mut self, now_ms: u64) -> bool {
        let timestamp = self.begin_poll_turn(now_ms);

        if self.stage_policy.allow_tcp && !self.validate_console_socket(now_ms) {
            self.finish_poll_turn(now_ms, false);
            return true;
        }

        let mut activity = self.sync_interface_hardware_addr(now_ms);
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
        if wifi_host_eapol_blocks_data_path(self.device.bringup_status_label()) {
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

        if self.stage_policy.tx_only && !self.tx_only_sent {
            budget.charge_ops(2)?;
            budget.charge_frames(1)?;
            budget.charge_bytes(256)?;

            let timestamp = self.begin_poll_turn(now_ms);
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
            let timestamp = self.begin_poll_turn(now_ms);
            if self.stage_policy.allow_tcp && !self.validate_console_socket(now_ms) {
                self.finish_poll_turn(now_ms, false);
                return Ok(true);
            }
            let mut activity = self.sync_interface_hardware_addr(now_ms);
            activity |= self.service_wifi_host_eapol_slice(now_ms);
            activity |= self.sync_interface_hardware_addr(now_ms);
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
                for _ in 0..CYW43_TCP_FAST_PATH_RESPONSE_FLUSH_TURNS {
                    Self::charge_tcp_response_flush_budget(budget)?;
                }
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

        let timestamp = self.begin_poll_turn(now_ms);
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
            activity |=
                self.drain_cyw43_pre_poll_activity(timestamp, now_ms, budget, pre_poll_activity);
            activity
        };
        #[cfg(not(feature = "kernel"))]
        let mut activity = false;
        activity |= self.sync_interface_hardware_addr(now_ms);
        activity |= self.service_wifi_host_eapol_slice(now_ms);
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
            BudgetedNetPhase::Interface
                if cyw43_tcp_phase_borrow
                    && budgeted_cyw43_smoltcp_poll_after_tcp_borrow(phase) =>
            {
                self.poll_smoltcp_once(timestamp, now_ms, "budgeted-cyw43-main-after-tcp-borrow")
            }
            BudgetedNetPhase::Interface if tcp_phase_borrow => false,
            BudgetedNetPhase::Interface => {
                self.poll_smoltcp_once(timestamp, now_ms, "budgeted-main")
            }
            BudgetedNetPhase::Dhcp if cyw43_tcp_phase_borrow && !dhcp_service_required => {
                self.poll_smoltcp_once(timestamp, now_ms, "budgeted-cyw43-dhcp-after-tcp-borrow")
            }
            BudgetedNetPhase::Dhcp => self.service_budgeted_dhcp_turn(timestamp, now_ms),
            BudgetedNetPhase::Tcp => self.service_budgeted_tcp_turn(
                timestamp,
                now_ms,
                "budgeted-tcp-pre",
                "budgeted-post-tcp",
            ),
            BudgetedNetPhase::InterfaceFlush
                if cyw43_tcp_phase_borrow
                    && budgeted_cyw43_smoltcp_poll_after_tcp_borrow(phase) =>
            {
                self.poll_smoltcp_once(timestamp, now_ms, "budgeted-cyw43-flush-after-tcp-borrow")
            }
            BudgetedNetPhase::InterfaceFlush if tcp_phase_borrow => false,
            BudgetedNetPhase::InterfaceFlush => {
                self.poll_smoltcp_once(timestamp, now_ms, "budgeted-flush")
            }
            BudgetedNetPhase::SelfTest if cyw43_selftest_tcp_defer => {
                let mut tcp_activity = self.service_budgeted_tcp_turn(
                    timestamp,
                    now_ms,
                    "budgeted-cyw43-selftest-defer-tcp-pre",
                    "budgeted-cyw43-selftest-defer-tcp-post",
                );
                if tcp_activity {
                    for _ in 0..CYW43_TCP_FAST_PATH_RESPONSE_FLUSH_TURNS {
                        let turn_activity = self.service_budgeted_tcp_response_flush_turn(
                            timestamp,
                            now_ms,
                            "budgeted-cyw43-selftest-defer-response-flush",
                        );
                        if !turn_activity {
                            break;
                        }
                        tcp_activity = true;
                    }
                }
                tcp_activity
            }
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
        } else if cyw43_tcp_fast_path && phase == BudgetedNetPhase::Tcp && activity {
            for _ in 0..CYW43_TCP_FAST_PATH_RESPONSE_FLUSH_TURNS {
                if Self::charge_tcp_response_flush_budget(budget).is_err() {
                    break;
                }
                let turn_activity = self.service_budgeted_tcp_response_flush_turn(
                    timestamp,
                    now_ms,
                    "budgeted-cyw43-tcp-phase-response-flush",
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
        #[cfg(feature = "kernel")]
        {
            if D::driver_task_contract() != crate::hal::driver_task::CYW43_WIFI_DRIVER_TASK_CONTRACT
            {
                return false;
            }
            let Some(credentials) = self.wifi_credentials else {
                return false;
            };
            return crate::drivers::driver_task_net::service_cyw43_host_eapol_slice(
                credentials,
                1,
                now_ms,
            );
        }
        #[cfg(not(feature = "kernel"))]
        {
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
        client.start(self.device.mac().0, now_ms);
        self.dhcp_started = true;
        self.dhcp_restart_after_ms = None;
        self.wifi_dhcp_eapol_settle_logged = false;
        info!(
            "[dhcp] start ready interface={} now_ms={}",
            self.device.interface_label(),
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
                self.apply_dhcp_lease(lease);
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
                self.dhcp_restart_after_ms = Some(restart_after_ms);
                true
            }
        }
    }

    fn apply_dhcp_lease(&mut self, lease: DhcpLease) {
        self.dhcp_restart_after_ms = None;
        let ip = Ipv4Address::new(lease.ip[0], lease.ip[1], lease.ip[2], lease.ip[3]);
        let gateway = lease
            .gateway
            .map(|value| Ipv4Address::new(value[0], value[1], value[2], value[3]));
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
            "[dhcp] lease bound ip={}/{} gateway={} server={}.{}.{}.{} lease_s={}",
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
        self.counters.wifi_rx_runtime_queue_count = device_counters.wifi_rx_runtime_queue_count;
        self.counters.wifi_rx_runtime_queue_high_water =
            device_counters.wifi_rx_runtime_queue_high_water;
        self.counters.wifi_rx_runtime_queue_overflow_seen =
            device_counters.wifi_rx_runtime_queue_overflow_seen;
        self.counters.wifi_rx_runtime_drain_budget_hit =
            device_counters.wifi_rx_runtime_drain_budget_hit;
        self.counters.wifi_rx_runtime_max_drained_per_turn =
            device_counters.wifi_rx_runtime_max_drained_per_turn;
        self.counters.wifi_data_trace_faults = device_counters.wifi_data_trace_faults;
        self.counters.wifi_data_trace_tx_retries = device_counters.wifi_data_trace_tx_retries;
        self.counters.dropped_zero_len_tx = device_counters.dropped_zero_len_tx;
        self.counters.wifi_assoc = device_counters.wifi_assoc;
        self.counters.wifi_link_up = device_counters.wifi_link_up;
        self.counters.wifi_host_eapol_rx = device_counters.wifi_host_eapol_rx;
        self.counters.wifi_host_eapol_start = device_counters.wifi_host_eapol_start;
        self.counters.wifi_host_eapol_secure = device_counters.wifi_host_eapol_secure;
    }

    fn current_counters(&self) -> NetCounters {
        let device_counters = self.device.counters();
        NetCounters {
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
            wifi_rx_runtime_queue_count: device_counters.wifi_rx_runtime_queue_count,
            wifi_rx_runtime_queue_high_water: device_counters.wifi_rx_runtime_queue_high_water,
            wifi_rx_runtime_queue_overflow_seen: device_counters
                .wifi_rx_runtime_queue_overflow_seen,
            wifi_rx_runtime_drain_budget_hit: device_counters.wifi_rx_runtime_drain_budget_hit,
            wifi_rx_runtime_max_drained_per_turn: device_counters
                .wifi_rx_runtime_max_drained_per_turn,
            wifi_data_trace_faults: device_counters.wifi_data_trace_faults,
            wifi_data_trace_tx_retries: device_counters.wifi_data_trace_tx_retries,
            dropped_zero_len_tx: device_counters.dropped_zero_len_tx,
            wifi_assoc: device_counters.wifi_assoc,
            wifi_link_up: device_counters.wifi_link_up,
            wifi_host_eapol_rx: device_counters.wifi_host_eapol_rx,
            wifi_host_eapol_start: device_counters.wifi_host_eapol_start,
            wifi_host_eapol_secure: device_counters.wifi_host_eapol_secure,
        }
    }

    fn log_self_test_result(&self, result: NetSelfTestResult) {
        let counters = self.current_counters();
        info!(
            "[net-selftest] result tx_ok={} udp_echo_ok={} tcp_ok={} console_ok={} peer_assisted_ok={}",
            result.tx_ok,
            result.udp_echo_ok,
            result.tcp_ok,
            result.console_ok,
            result.peer_assisted_ok,
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
            let poll_result =
                self.interface
                    .poll(timestamp, self.device.as_mut(), &mut self.sockets);
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
            let poll_result =
                self.interface
                    .poll(timestamp, self.device.as_mut(), &mut self.sockets);
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
        let listen_port = self.listen_port;
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
            let socket = self.sockets.get_mut::<TcpSocket>(self.tcp_handle);
            if !self.session_active && socket.is_open() {
                socket.abort();
            }
            return false;
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

        let (snapshot, tcp_state) = {
            let socket = self.sockets.get_mut::<TcpSocket>(self.tcp_handle);
            let peer_changed =
                Self::record_peer_endpoint(&mut self.peer_endpoint, socket.remote_endpoint());

            if !socket.is_open() {
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
                socket.close();
                return true;
            }

            let new_established = socket.state() == TcpState::Established
                && (previous_state != Some(TcpState::Established)
                    || !self.session_active
                    || peer_changed);
            if new_established {
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
                self.disconnect_requested = false;
                self.disconnect_requested_at_ms = None;
                self.disconnect_requested_polls = 0;
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

            if socket.can_recv() {
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
                    let recv_result = socket.recv(|data| {
                        let preview_len = core::cmp::min(data.len(), 32);
                        log::debug!(
                            target: "net-console",
                            "[tcp] recv on console socket: len={} first_bytes={:02x?}",
                            data.len(),
                            &data[..preview_len],
                        );
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
                                trace!("[cohsh-net][tcp] recv hex: {:02x?}", &temp[..dump_len]);
                            }
                            Self::trace_conn_recv(conn_id, &temp[..copied]);
                            if ECHO_MODE {
                                match socket.send_slice(&temp[..copied]) {
                                    Ok(sent) => {
                                        self.conn_bytes_written =
                                            self.conn_bytes_written.saturating_add(sent as u64);
                                        NET_DIAG.add_bytes_written(sent as u64);
                                        self.counters.tcp_tx_bytes =
                                            self.counters.tcp_tx_bytes.saturating_add(sent as u64);
                                        Self::trace_conn_send(conn_id, &temp[..sent.min(copied)]);
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
                                info!(
                                    "[cohsh-net][auth] frame hex: {:02x?}",
                                    &temp[..copied.min(32)]
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
                                    let mut preview: HeaplessString<DEFAULT_LINE_CAPACITY> =
                                        HeaplessString::new();
                                    for &byte in &temp[..copied.min(preview.capacity())] {
                                        if byte == b'\n' || byte == b'\r' {
                                            break;
                                        }
                                        let _ = preview.push(byte as char);
                                    }
                                    info!(
                                        target: "net-console",
                                        "[net-console] recv line on TCP session {}: {}",
                                        conn_id,
                                        preview
                                    );
                                    info!(
	                                        "[cohsh-net][auth] auth OK, session established (conn_id={})",
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
                                    Self::log_session_closed(
                                        &mut self.session_state,
                                        self.peer_endpoint,
                                        socket,
                                    );
                                    socket.close();
                                    self.outbound.reset();
                                    self.server.end_session();
                                    self.session_active = false;
                                    if let Some(conn_id) = self.active_client_id {
                                        Self::note_close_reason(
                                            &mut log_closed_conn,
                                            conn_id,
                                            NetConsoleDisconnectReason::Error,
                                        );
                                        Self::note_close_reason(
                                            &mut record_closed_conn,
                                            conn_id,
                                            NetConsoleDisconnectReason::Error,
                                        );
                                    }
                                    reset_session = true;
                                    self.peer_endpoint = None;
                                    self.active_client_id = None;
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
                                    Self::log_session_closed(
                                        &mut self.session_state,
                                        self.peer_endpoint,
                                        socket,
                                    );
                                    socket.close();
                                    self.outbound.reset();
                                    self.server.end_session();
                                    self.session_active = false;
                                    self.peer_endpoint = None;
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
                                    reset_session = true;
                                    self.active_client_id = None;
                                    activity = true;
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
                            Self::log_session_closed(
                                &mut self.session_state,
                                self.peer_endpoint,
                                socket,
                            );
                            socket.close();
                            self.outbound.reset();
                            self.server.end_session();
                            self.session_active = false;
                            self.peer_endpoint = None;
                            reset_session = true;
                            if let Some(conn_id) = self.active_client_id {
                                Self::note_close_reason(&mut log_closed_conn, conn_id, reason);
                                Self::note_close_reason(&mut record_closed_conn, conn_id, reason);
                            }
                            info!(
                                "[net-console] conn {}: bytes read={}, bytes written={}",
                                self.active_client_id.unwrap_or(0),
                                self.conn_bytes_read,
                                self.conn_bytes_written
                            );
                            self.active_client_id = None;
                            break;
                        }
                    }
                }
                if budget_exhausted && socket.can_recv() {
                    self.counters.tcp_console_recv_budget_hits =
                        self.counters.tcp_console_recv_budget_hits.saturating_add(1);
                }
            }
            if self.session_active && socket.state() == TcpState::Established && !socket.may_recv()
            {
                Self::log_session_closed(&mut self.session_state, self.peer_endpoint, socket);
                self.outbound.reset();
                socket.close();
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
                allow_flush = false;
            }
            if self.session_active
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
                Self::log_session_closed(&mut self.session_state, self.peer_endpoint, socket);
                socket.close();
                self.outbound.reset();
                self.server.end_session();
                self.session_active = false;
                self.peer_endpoint = None;
                Self::set_auth_state(
                    &mut self.auth_state,
                    self.active_client_id,
                    AuthState::Failed,
                );
                if let Some(conn_id) = self.active_client_id {
                    Self::note_close_reason(
                        &mut log_closed_conn,
                        conn_id,
                        NetConsoleDisconnectReason::Error,
                    );
                    Self::note_close_reason(
                        &mut record_closed_conn,
                        conn_id,
                        NetConsoleDisconnectReason::Error,
                    );
                }
                self.active_client_id = None;
                reset_session = true;
                reset_tcp_state = Some(socket.state());
                activity |= true;
            }

            if self.session_active && self.server.should_timeout(now_ms) {
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
                Self::log_session_closed(&mut self.session_state, self.peer_endpoint, socket);
                socket.close();
                self.outbound.reset();
                self.server.end_session();
                self.session_active = false;
                self.peer_endpoint = None;
                Self::set_auth_state(
                    &mut self.auth_state,
                    self.active_client_id,
                    AuthState::Failed,
                );
                if let Some(conn_id) = self.active_client_id {
                    Self::note_close_reason(
                        &mut log_closed_conn,
                        conn_id,
                        NetConsoleDisconnectReason::Error,
                    );
                    Self::note_close_reason(
                        &mut record_closed_conn,
                        conn_id,
                        NetConsoleDisconnectReason::Error,
                    );
                }
                self.active_client_id = None;
                reset_session = true;
                reset_tcp_state = Some(socket.state());
                activity |= true;
            }

            let tcp_state = socket.state();
            if matches!(
                tcp_state,
                TcpState::CloseWait
                    | TcpState::FinWait1
                    | TcpState::FinWait2
                    | TcpState::LastAck
                    | TcpState::TimeWait
            ) {
                info!(
                    "[net-console] TCP client #{} closing (state={:?})",
                    self.active_client_id.unwrap_or(0),
                    tcp_state
                );
                debug!(
                    "[net-console][auth] state={:?} client={} closing socket state={:?}",
                    self.auth_state,
                    self.active_client_id.unwrap_or(0),
                    tcp_state
                );
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
                if socket.state() != TcpState::Closed {
                    socket.abort();
                }
                reset_session = true;
                reset_tcp_state = Some(socket.state());
                allow_flush = false;
            }

            if matches!(socket.state(), TcpState::Closed) && self.session_active {
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

            if self.disconnect_requested {
                self.disconnect_requested_polls = self.disconnect_requested_polls.saturating_add(1);
                let outbound_clear = !self.server.has_outbound() && !self.outbound.has_pending();
                let grace_elapsed = self
                    .disconnect_requested_at_ms
                    .map(|start| now_ms.saturating_sub(start) >= DISCONNECT_GRACE_MS)
                    .unwrap_or(false)
                    || self.disconnect_requested_polls >= DISCONNECT_GRACE_POLLS;
                if outbound_clear || grace_elapsed {
                    Self::log_session_closed(&mut self.session_state, self.peer_endpoint, socket);
                    if outbound_clear {
                        if socket.state() != TcpState::Closed {
                            socket.close();
                        }
                    } else if socket.state() != TcpState::Closed {
                        socket.abort();
                    }
                    self.server.end_session();
                    self.outbound.reset();
                    self.session_active = false;
                    self.disconnect_requested = false;
                    self.disconnect_requested_at_ms = None;
                    self.disconnect_requested_polls = 0;
                    outbound_pending = false;
                    if let Some(conn_id) = self.active_client_id {
                        Self::note_close_reason(
                            &mut log_closed_conn,
                            conn_id,
                            NetConsoleDisconnectReason::Quit,
                        );
                        Self::note_close_reason(
                            &mut record_closed_conn,
                            conn_id,
                            NetConsoleDisconnectReason::Quit,
                        );
                    }
                    self.active_client_id = None;
                    self.peer_endpoint = None;
                    Self::set_auth_state(
                        &mut self.auth_state,
                        self.active_client_id,
                        AuthState::Start,
                    );
                    if console_listener_defer_reason_for(
                        self.mode,
                        self.ip,
                        self.device.bringup_status_label(),
                    )
                    .is_none()
                    {
                        Self::force_relisten(socket, listen_port);
                    } else {
                        self.listener_announced = false;
                    }
                    reset_session = true;
                    reset_tcp_state = Some(socket.state());
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
                let preview_len = core::cmp::min(sent, 32);
                log::debug!(
                    target: "net-console",
                    "[tcp] send on console socket: len={} first_bytes={:02x?}",
                    sent,
                    &payload[..preview_len],
                );
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
                Self::trace_conn_send(conn_id, payload);
                #[cfg(feature = "net-trace-31337")]
                {
                    let tcp_state = socket.state();
                    let dump_len = payload.len().min(32);
                    info!(
                        "[cohsh-net] send: {} bytes (state={:?}, auth_state={:?}): {:02x?}",
                        sent,
                        tcp_state,
                        auth_state,
                        &payload[..dump_len]
                    );
                }
                if pre_auth && matches!(lane, OutboundLane::Control) {
                    info!(
                        "[net-console] conn {}: sent pre-auth payload len={} first_bytes={:02x?}",
                        conn_id,
                        payload.len(),
                        &payload[..core::cmp::min(payload.len(), 32)]
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
                    if wifi_host_eapol_blocks_driver_task_pre_poll(
                        self.device.bringup_status_label(),
                    ) {
                        return self.poll_with_time(now_ms);
                    }
                    if !crate::drivers::driver_task_net::driver_task_runtime_pre_poll_allowed(
                        contract,
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
                    if wifi_host_eapol_blocks_driver_task_pre_poll(
                        self.device.bringup_status_label(),
                    ) {
                        return self.poll_budgeted_with_time(now_ms, budget);
                    }
                    if !crate::drivers::driver_task_net::driver_task_runtime_pre_poll_allowed(
                        contract,
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
                    let ring_progress =
                        if hot_path == crate::hal::driver_task::DriverTaskHotPath::Cyw43Wifi {
                            self.service_cyw43_data_pre_poll_burst_budgeted(contract, budget)
                        } else {
                            false
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
        self.counters
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

    fn send_console_line(&mut self, line: &str) -> bool {
        if !self.stage_policy.allow_console_io {
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
        self.disconnect_requested = true;
        self.disconnect_requested_at_ms = self.last_now_ms;
        self.disconnect_requested_polls = 0;
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

    fn inject_console_line(&mut self, _line: &str) {}

    fn reset(&mut self) {
        self.server.end_session();
        self.session_active = false;
        self.disconnect_requested = false;
        self.disconnect_requested_at_ms = None;
        self.disconnect_requested_polls = 0;
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

    fn self_test_report(&self) -> NetSelfTestReport {
        let udp_target = self.selftest_host_target(UDP_ECHO_PORT);
        let tcp_target = self.selftest_host_target(TCP_SMOKE_PORT);
        NetSelfTestReport {
            enabled: self.self_test.enabled,
            running: self.self_test.running,
            last_result: self.self_test.last_result,
            backend: self.backend.label(),
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
        let listener_ready = self.stage_policy.allow_tcp
            && self.listener_announced
            && self.listener_defer_reason.is_none()
            && !self.wifi_rx_admission_blocked;
        let tcp_ready = listener_ready && cyw43_blocker.is_none();
        NetStatusReport {
            profile_backend: self.backend.label(),
            backend: self.backend.label(),
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
const CYW43_DRIVER_TASK_PRE_POLL_BURST_LIMIT: usize = 8;
const DEFAULT_DRIVER_TASK_PRE_POLL_BURST_LIMIT: usize = 1;
const DRIVER_TASK_PRE_POLL_TURN_BYTES: u32 = 2048;
const GENET_TCP_FAST_PATH_EXTRA_TURNS: usize = 1;
const GENET_TCP_POST_DISPATCH_EXTRA_TURNS: usize = 2;
const CYW43_TCP_FAST_PATH_RESPONSE_FLUSH_TURNS: usize = 1;
const CYW43_TCP_POST_DISPATCH_EXTRA_TURNS: usize = 1;

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
    // SAFETY: This test/compatibility wrapper preserves the exact registered
    // service ABI and forwards the primitive selector context unchanged.
    unsafe { crate::drivers::driver_task_net::runtime_ring_service(context, command) }
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

    fn active_console_conn_id(&self) -> Option<u64> {
        match self {
            Self::Rtl8139(stack) => stack.active_console_conn_id(),
            Self::GenetDriverTask(stack) => stack.active_console_conn_id(),
            Self::Cyw43DriverTask(stack) => stack.active_console_conn_id(),
            #[cfg(feature = "net-backend-virtio")]
            Self::Virtio(stack) => stack.active_console_conn_id(),
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
mod tests {
    use core::convert::Infallible;

    use super::*;
    use smoltcp::phy::{Loopback, Medium};

    static NET_STACK_STORAGE_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn reset_socket_and_tcp_rx_state() {
        SOCKET_STORAGE_IN_USE.store(false, Ordering::Release);
        SOCKET_STORAGE_OWNER.store(0, Ordering::Release);
        SOCKET_STORAGE_TAG_ID.store(0, Ordering::Release);
        *SOCKET_STORAGE_TAG_LABEL.lock() = None;

        TCP_RX_STORAGE_IN_USE.store(false, Ordering::Release);
        TCP_RX_STORAGE_OWNER.store(0, Ordering::Release);
        TCP_RX_STORAGE_TAG_ID.store(0, Ordering::Release);
        *TCP_RX_STORAGE_TAG_LABEL.lock() = None;
    }

    #[test]
    fn pi4_config_reports_profile_backend_and_active_driver_separately() {
        let mut config = ConsoleNetConfig::default();
        config.backend = NetBackend::BcmGenet;
        config.policy.interface = NetInterfacePolicy::Wired;
        assert_eq!(configured_active_driver_label(&config), "bcmgenet-v5");

        config.policy.interface = NetInterfacePolicy::Wifi;
        assert_eq!(configured_active_driver_label(&config), "cyw43");

        config.policy.interface = NetInterfacePolicy::Auto;
        config.wifi_credentials = None;
        assert_eq!(configured_active_driver_label(&config), "bcmgenet-v5");

        config.wifi_credentials =
            Some(WifiCredentials::new("cohesix", "passphrase").expect("valid WiFi credentials"));
        assert_eq!(configured_active_driver_label(&config), "cyw43");
        assert_eq!(config.backend.label(), "bcmgenet-v5");
    }

    #[test]
    fn reservation_releases_on_error() {
        let _guard = NET_STACK_STORAGE_TEST_LOCK.lock();
        reset_socket_and_tcp_rx_state();

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
        assert!(!wifi_host_eapol_blocks_data_path(Some("dhcp-pending")));
        assert!(!wifi_host_eapol_blocks_data_path(None));
    }

    #[test]
    fn host_eapol_pending_and_required_block_driver_task_pre_poll() {
        assert!(wifi_host_eapol_blocks_driver_task_pre_poll(Some(
            "wifi-host-eapol-pending"
        )));
        assert!(wifi_host_eapol_blocks_driver_task_pre_poll(Some(
            "wifi-host-eapol-required"
        )));
        assert!(!wifi_host_eapol_blocks_driver_task_pre_poll(Some(
            "dhcp-pending"
        )));
        assert!(!wifi_host_eapol_blocks_driver_task_pre_poll(None));
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
    }

    #[test]
    fn cyw43_dhcp_waits_for_post_secure_eapol_quiet_window() {
        let mut last_rx = 0;
        let mut quiet_since_ms = None;

        let first =
            cyw43_dhcp_post_secure_eapol_settle(1_000, 1, 18, &mut last_rx, &mut quiet_since_ms);
        assert!(!first.ready);
        assert!(first.changed);
        assert_eq!(first.quiet_ms, 0);
        assert_eq!(first.remaining_ms, CYW43_DHCP_POST_SECURE_EAPOL_QUIET_MS);
        assert_eq!(first.next_ready_ms, Some(1_500));
        assert_eq!(last_rx, 18);
        assert_eq!(quiet_since_ms, Some(1_000));

        let early =
            cyw43_dhcp_post_secure_eapol_settle(1_499, 1, 18, &mut last_rx, &mut quiet_since_ms);
        assert!(!early.ready);
        assert!(!early.changed);
        assert_eq!(early.quiet_ms, 499);
        assert_eq!(early.remaining_ms, 1);
        assert_eq!(early.next_ready_ms, Some(1_500));

        let ready =
            cyw43_dhcp_post_secure_eapol_settle(1_500, 1, 18, &mut last_rx, &mut quiet_since_ms);
        assert!(ready.ready);
        assert!(!ready.changed);
        assert_eq!(ready.quiet_ms, CYW43_DHCP_POST_SECURE_EAPOL_QUIET_MS);
        assert_eq!(ready.remaining_ms, 0);
        assert_eq!(ready.next_ready_ms, Some(1_500));

        let retransmit =
            cyw43_dhcp_post_secure_eapol_settle(1_600, 1, 19, &mut last_rx, &mut quiet_since_ms);
        assert!(!retransmit.ready);
        assert!(retransmit.changed);
        assert_eq!(retransmit.quiet_ms, 0);
        assert_eq!(
            retransmit.remaining_ms,
            CYW43_DHCP_POST_SECURE_EAPOL_QUIET_MS
        );
        assert_eq!(retransmit.next_ready_ms, Some(2_100));
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
        assert!(CYW43_RESPONSE_FLUSH_FRAMES_PER_TURN <= MAX_CONSOLE_FRAMES_PER_POLL);
        assert!(CYW43_RESPONSE_FLUSH_BYTES_PER_TURN <= MAX_CONSOLE_BYTES_PER_POLL);
        assert_eq!(
            TCP_SERVICE_BYTES_PER_TURN,
            (MAX_TCP_CONSOLE_RECV_BYTES_PER_POLL + MAX_CONSOLE_BYTES_PER_POLL) as u32
        );
        assert_eq!(
            TCP_RESPONSE_FLUSH_BYTES_PER_TURN,
            CYW43_RESPONSE_FLUSH_BYTES_PER_TURN as u32
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
        let response_flush_ops = 16u16;
        let response_flush_frames = CYW43_RESPONSE_FLUSH_FRAMES_PER_TURN as u16;
        let response_flush_bytes = TCP_RESPONSE_FLUSH_BYTES_PER_TURN;
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
        assert!(
            pre_poll_ops.saturating_mul(2).saturating_add(tcp_ops)
                <= contract.budget.max_ops_per_turn
        );
        assert!(
            pre_poll_frames.saturating_mul(2).saturating_add(tcp_frames)
                <= contract.budget.max_frames_per_turn
        );
        assert!(
            pre_poll_bytes.saturating_mul(2).saturating_add(tcp_bytes)
                > contract.budget.max_bytes_per_turn
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
        assert_eq!(CYW43_TCP_POST_DISPATCH_EXTRA_TURNS, 1);
        assert!(response_flush_bytes < tcp_bytes);
        assert!(tcp_ops.saturating_add(response_flush_ops) <= contract.budget.max_ops_per_turn);
        assert!(
            tcp_frames.saturating_add(response_flush_frames) <= contract.budget.max_frames_per_turn
        );
        assert!(
            tcp_bytes.saturating_add(response_flush_bytes) <= contract.budget.max_bytes_per_turn
        );
        assert!(
            pre_poll_ops
                .saturating_add(tcp_ops)
                .saturating_add(response_flush_ops)
                <= contract.budget.max_ops_per_turn
        );
        assert!(
            pre_poll_frames
                .saturating_add(tcp_frames)
                .saturating_add(response_flush_frames)
                <= contract.budget.max_frames_per_turn
        );
        assert!(
            pre_poll_bytes
                .saturating_add(tcp_bytes)
                .saturating_add(response_flush_bytes)
                <= contract.budget.max_bytes_per_turn
        );
        assert_eq!(CYW43_TCP_FAST_PATH_RESPONSE_FLUSH_TURNS, 1);
        assert!(
            pre_poll_ops.saturating_add(tcp_ops).saturating_add(
                response_flush_ops.saturating_mul(CYW43_TCP_FAST_PATH_RESPONSE_FLUSH_TURNS as u16)
            ) <= contract.budget.max_ops_per_turn
        );
        assert!(
            pre_poll_frames.saturating_add(tcp_frames).saturating_add(
                response_flush_frames
                    .saturating_mul(CYW43_TCP_FAST_PATH_RESPONSE_FLUSH_TURNS as u16)
            ) <= contract.budget.max_frames_per_turn
        );
        assert!(
            pre_poll_bytes.saturating_add(tcp_bytes).saturating_add(
                response_flush_bytes
                    .saturating_mul(CYW43_TCP_FAST_PATH_RESPONSE_FLUSH_TURNS as u32)
            ) <= contract.budget.max_bytes_per_turn
        );

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
        assert!(!cyw43_flush_pre_poll_data_ready_for(
            cyw43,
            "wifi",
            NetMode::Dhcp,
            ip,
            Some("wifi-host-eapol-required"),
            Some(DhcpPhase::Bound),
            true
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
        assert!(!cyw43_flush_pre_poll_data_ready_for(
            cyw43,
            "wifi",
            NetMode::Static,
            ip,
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
    fn cyw43_status_reports_rx_overflow_and_starvation_blockers() {
        assert_eq!(
            cyw43_status_blocker_for(
                "cyw43",
                "wifi",
                NetCounters {
                    wifi_rx_runtime_queue_overflow_seen: 1,
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
            Some(Cyw43StatusBlocker {
                address_source: "wifi-rx-starvation",
                dhcp_phase: "rx-starvation"
            })
        );
    }

    #[test]
    fn cyw43_status_reports_tx_credit_anomaly_until_host_proof() {
        let counters = NetCounters {
            tx_submit: 60,
            tx_free: 0,
            tx_in_flight: 0,
            wifi_data_trace_tx_retries: 3,
            ..NetCounters::default()
        };

        assert_eq!(
            cyw43_status_blocker_for("cyw43", "wifi", counters),
            Some(Cyw43StatusBlocker {
                address_source: "wifi-tx-credit-anomaly",
                dhcp_phase: "tx-credit-anomaly"
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
            Some(Cyw43StatusBlocker {
                address_source: "wifi-tx-credit-anomaly",
                dhcp_phase: "tx-credit-anomaly"
            })
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
        reset_socket_and_tcp_rx_state();

        let attempt = NetInitAttempt::new("test.acquisition");
        let reservation =
            StorageReservation::acquire::<Infallible>(false, false, &attempt, "test.acquisition")
                .expect("reservation should succeed");

        assert!(SOCKET_STORAGE_IN_USE.load(Ordering::Acquire));
        assert_ne!(SOCKET_STORAGE_OWNER.load(Ordering::Acquire), 0);
        assert_ne!(SOCKET_STORAGE_TAG_ID.load(Ordering::Acquire), 0);

        drop(reservation);

        assert!(!SOCKET_STORAGE_IN_USE.load(Ordering::Acquire));
        assert_eq!(SOCKET_STORAGE_OWNER.load(Ordering::Acquire), 0);
        assert_eq!(SOCKET_STORAGE_TAG_ID.load(Ordering::Acquire), 0);
    }

    #[test]
    fn poisoned_flag_is_reported() {
        let _guard = NET_STACK_STORAGE_TEST_LOCK.lock();
        reset_socket_and_tcp_rx_state();

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
        reset_socket_and_tcp_rx_state();

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

        let (prefix, prefix_len) = NetStack::<DefaultNetDevice>::trace_conn_prefix(&payload);
        assert_eq!(prefix_len, 16);
        assert_eq!(&prefix[..prefix_len], &payload[..16]);
    }

    #[test]
    fn trace_conn_prefix_handles_short_payload() {
        let payload = [1u8, 2u8, 3u8];
        let (prefix, prefix_len) = NetStack::<DefaultNetDevice>::trace_conn_prefix(&payload);
        assert_eq!(prefix_len, payload.len());
        assert_eq!(&prefix[..prefix_len], payload);
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
