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
#![cfg(feature = "kernel")]

use core::fmt::{self, Write as FmtWrite};
use heapless::{String as HeaplessString, Vec as HeaplessVec};
use log::{debug, info, trace, warn};
use portable_atomic::{AtomicBool, Ordering};
use smoltcp::iface::{
    Config as IfaceConfig, Interface, PollResult, SocketHandle, SocketSet, SocketStorage,
};
use smoltcp::socket::tcp::{
    RecvError as TcpRecvError, Socket as TcpSocket, SocketBuffer as TcpSocketBuffer,
    State as TcpState,
};
use smoltcp::socket::udp::{
    BindError as UdpBindError, PacketBuffer as UdpPacketBuffer,
    PacketMetadata as UdpPacketMetadata, RecvError as UdpRecvError, Socket as UdpSocket,
};
use smoltcp::time::Instant;
use smoltcp::wire::{
    EthernetAddress, HardwareAddress, IpAddress, IpCidr, IpEndpoint, IpListenEndpoint, Ipv4Address,
};

use super::{
    console_srv::{SessionEvent, TcpConsoleServer},
    ConsoleNetConfig, NetBackend, NetConsoleEvent, NetCounters, NetDevice, NetDriverError,
    NetPoller, NetSelfTestReport, NetSelfTestResult, NetTelemetry, DEFAULT_NET_BACKEND,
    DEV_VIRT_GATEWAY, DEV_VIRT_IP, DEV_VIRT_PREFIX,
};
#[cfg(not(feature = "net-backend-virtio"))]
use crate::drivers::rtl8139::{DriverError as Rtl8139DriverError, Rtl8139Device};
#[cfg(feature = "net-backend-virtio")]
use crate::drivers::virtio::net::{DriverError as VirtioDriverError, VirtioNet};
use crate::hal::{HalError, Hardware};
use crate::serial::DEFAULT_LINE_CAPACITY;
use cohesix_proto::{REASON_INACTIVITY_TIMEOUT, REASON_RECV_ERROR};

const TCP_RX_BUFFER: usize = 2048;
const TCP_TX_BUFFER: usize = 2048;
const TCP_SMOKE_RX_BUFFER: usize = 256;
const TCP_SMOKE_TX_BUFFER: usize = 256;
const SOCKET_CAPACITY: usize = 6;
const MAX_TX_BUDGET: usize = 8;
const RANDOM_SEED: u64 = 0x5a5a_5a5a_1234_5678;
const ECHO_MODE: bool = cfg!(feature = "tcp-echo-31337");
const ERR_AUTH_REASON_TIMEOUT: &str = "ERR AUTH reason=timeout";
const ERR_CONSOLE_REASON_TIMEOUT: &str = "ERR CONSOLE reason=timeout";
const UDP_METADATA_CAPACITY: usize = 8;
const UDP_PAYLOAD_CAPACITY: usize = 512;
const UDP_ECHO_PORT: u16 = 31_338;
const UDP_BEACON_PORT: u16 = 40_000;
const TCP_SMOKE_PORT: u16 = 31_339;
const SELF_TEST_ENABLED: bool = cfg!(feature = "dev-virt") || cfg!(feature = "net-selftest");
const SELF_TEST_BEACON_INTERVAL_MS: u64 = 250;
const SELF_TEST_BEACON_WINDOW_MS: u64 = 5_000;
const SELF_TEST_WINDOW_MS: u64 = 15_000;

#[cfg(feature = "net-backend-virtio")]
type DefaultNetDevice = VirtioNet;
#[cfg(feature = "net-backend-virtio")]
type DefaultDriverError = VirtioDriverError;

#[cfg(not(feature = "net-backend-virtio"))]
type DefaultNetDevice = Rtl8139Device;
#[cfg(not(feature = "net-backend-virtio"))]
type DefaultDriverError = Rtl8139DriverError;

pub type DefaultNetStack = NetStack<DefaultNetDevice>;
pub type DefaultNetStackError = NetStackError<DefaultDriverError>;
pub type DefaultNetConsoleError = NetConsoleError<DefaultDriverError>;

#[derive(Debug)]
pub enum NetStackError<DE> {
    Driver(DE),
    SocketStorageInUse,
    TcpRxStorageInUse,
    TcpTxStorageInUse,
    TcpSmokeRxStorageInUse,
    TcpSmokeTxStorageInUse,
    UdpBeaconStorageInUse,
    UdpEchoStorageInUse,
}

impl<DE: fmt::Display> fmt::Display for NetStackError<DE> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Driver(err) => write!(f, "{err}"),
            Self::SocketStorageInUse => f.write_str("socket storage already in use"),
            Self::TcpRxStorageInUse => f.write_str("TCP RX storage already in use"),
            Self::TcpTxStorageInUse => f.write_str("TCP TX storage already in use"),
            Self::TcpSmokeRxStorageInUse => f.write_str("TCP smoke test RX storage already in use"),
            Self::TcpSmokeTxStorageInUse => f.write_str("TCP smoke test TX storage already in use"),
            Self::UdpBeaconStorageInUse => f.write_str("UDP beacon storage already in use"),
            Self::UdpEchoStorageInUse => f.write_str("UDP echo storage already in use"),
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
    /// An error occurred during stack bring-up.
    Init(NetStackError<DE>),
}

impl<DE: fmt::Display> fmt::Display for NetConsoleError<DE> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDevice => f.write_str("network device not present"),
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

struct StorageGuard<'a> {
    flag: &'a AtomicBool,
    release_on_drop: bool,
}

impl<'a> StorageGuard<'a> {
    fn acquire<DE>(
        flag: &'a AtomicBool,
        busy_error: NetStackError<DE>,
    ) -> Result<Self, NetStackError<DE>> {
        if flag.swap(true, Ordering::AcqRel) {
            Err(busy_error)
        } else {
            Ok(Self {
                flag,
                release_on_drop: true,
            })
        }
    }

    fn disarm(mut self) {
        self.release_on_drop = false;
    }
}

impl Drop for StorageGuard<'_> {
    fn drop(&mut self) {
        if self.release_on_drop {
            self.flag.store(false, Ordering::Release);
        }
    }
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

#[derive(Debug, Default)]
struct SessionState {
    last_state: Option<TcpState>,
    close_logged: bool,
    logged_accept: bool,
    logged_first_recv: bool,
    connect_reported: bool,
    logged_first_send: bool,
}

static SOCKET_STORAGE_IN_USE: AtomicBool = AtomicBool::new(false);
static mut SOCKET_STORAGE: [SocketStorage<'static>; SOCKET_CAPACITY] =
    [SocketStorage::EMPTY; SOCKET_CAPACITY];
static TCP_RX_STORAGE_IN_USE: AtomicBool = AtomicBool::new(false);
static mut TCP_RX_STORAGE: [u8; TCP_RX_BUFFER] = [0u8; TCP_RX_BUFFER];
static TCP_TX_STORAGE_IN_USE: AtomicBool = AtomicBool::new(false);
static mut TCP_TX_STORAGE: [u8; TCP_TX_BUFFER] = [0u8; TCP_TX_BUFFER];
static TCP_SMOKE_RX_STORAGE_IN_USE: AtomicBool = AtomicBool::new(false);
static mut TCP_SMOKE_RX_STORAGE: [u8; TCP_SMOKE_RX_BUFFER] = [0u8; TCP_SMOKE_RX_BUFFER];
static TCP_SMOKE_TX_STORAGE_IN_USE: AtomicBool = AtomicBool::new(false);
static mut TCP_SMOKE_TX_STORAGE: [u8; TCP_SMOKE_TX_BUFFER] = [0u8; TCP_SMOKE_TX_BUFFER];
static UDP_BEACON_STORAGE_IN_USE: AtomicBool = AtomicBool::new(false);
static UDP_ECHO_STORAGE_IN_USE: AtomicBool = AtomicBool::new(false);
static mut UDP_BEACON_RX_METADATA: [UdpPacketMetadata; UDP_METADATA_CAPACITY] =
    [UdpPacketMetadata::EMPTY; UDP_METADATA_CAPACITY];
static mut UDP_BEACON_TX_METADATA: [UdpPacketMetadata; UDP_METADATA_CAPACITY] =
    [UdpPacketMetadata::EMPTY; UDP_METADATA_CAPACITY];
static mut UDP_ECHO_RX_METADATA: [UdpPacketMetadata; UDP_METADATA_CAPACITY] =
    [UdpPacketMetadata::EMPTY; UDP_METADATA_CAPACITY];
static mut UDP_ECHO_TX_METADATA: [UdpPacketMetadata; UDP_METADATA_CAPACITY] =
    [UdpPacketMetadata::EMPTY; UDP_METADATA_CAPACITY];
static mut UDP_BEACON_RX_STORAGE: [u8; UDP_PAYLOAD_CAPACITY] = [0u8; UDP_PAYLOAD_CAPACITY];
static mut UDP_BEACON_TX_STORAGE: [u8; UDP_PAYLOAD_CAPACITY] = [0u8; UDP_PAYLOAD_CAPACITY];
static mut UDP_ECHO_RX_STORAGE: [u8; UDP_PAYLOAD_CAPACITY] = [0u8; UDP_PAYLOAD_CAPACITY];
static mut UDP_ECHO_TX_STORAGE: [u8; UDP_PAYLOAD_CAPACITY] = [0u8; UDP_PAYLOAD_CAPACITY];

/// Shared monotonic clock for the interface.
#[derive(Debug, Default)]
pub struct NetworkClock {
    ticks_ms: portable_atomic::AtomicU64,
}

impl NetworkClock {
    /// Creates a monotonic clock initialised to zero milliseconds.
    #[must_use]
    pub fn new() -> Self {
        Self {
            ticks_ms: portable_atomic::AtomicU64::new(0),
        }
    }

    /// Advances the clock by `delta_ms` and returns the resulting [`Instant`].
    pub fn advance(&self, delta_ms: u32) -> Instant {
        let delta = u64::from(delta_ms);
        let updated = self
            .ticks_ms
            .fetch_add(delta, portable_atomic::Ordering::Relaxed)
            .saturating_add(delta);
        let millis = i64::try_from(updated).unwrap_or(i64::MAX);
        Instant::from_millis(millis)
    }

    /// Reads the current [`Instant`] without modifying the clock value.
    #[must_use]
    pub fn now(&self) -> Instant {
        let current = self.ticks_ms.load(portable_atomic::Ordering::Relaxed);
        let millis = i64::try_from(current).unwrap_or(i64::MAX);
        Instant::from_millis(millis)
    }
}

/// Smoltcp-backed network stack that bridges the selected network device into the root task.
pub struct NetStack<D: NetDevice> {
    clock: NetworkClock,
    device: D,
    interface: Interface,
    sockets: SocketSet<'static>,
    tcp_handle: SocketHandle,
    server: TcpConsoleServer,
    telemetry: NetTelemetry,
    ip: Ipv4Address,
    gateway: Option<Ipv4Address>,
    prefix_len: u8,
    listen_port: u16,
    session_active: bool,
    listener_announced: bool,
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
    udp_beacon_handle: Option<SocketHandle>,
    udp_echo_handle: Option<SocketHandle>,
    tcp_smoke_handle: Option<SocketHandle>,
    counters: NetCounters,
    self_test: SelfTestState,
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

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct SelfTestState {
    enabled: bool,
    running: bool,
    started_ms: u64,
    last_beacon_ms: u64,
    beacon_seq: u32,
    beacons_sent: u32,
    tx_ok: bool,
    udp_echo_ok: bool,
    tcp_ok: bool,
    tcp_accept_seen: bool,
    last_result: Option<NetSelfTestResult>,
}

impl SelfTestState {
    fn new(enabled: bool) -> Self {
        Self {
            enabled,
            ..Self::default()
        }
    }

    fn reset(&mut self, now_ms: u64) {
        self.running = true;
        self.started_ms = now_ms;
        self.last_beacon_ms = now_ms.saturating_sub(SELF_TEST_BEACON_INTERVAL_MS);
        self.beacon_seq = 0;
        self.beacons_sent = 0;
        self.tx_ok = false;
        self.udp_echo_ok = false;
        self.tcp_ok = false;
        self.tcp_accept_seen = false;
        self.last_result = None;
    }

    fn start(&mut self, now_ms: u64) -> bool {
        if !self.enabled {
            return false;
        }
        self.reset(now_ms);
        true
    }

    fn record_tx(&mut self) {
        self.tx_ok = true;
    }

    fn record_udp_echo(&mut self) {
        self.udp_echo_ok = true;
    }

    fn record_tcp_ok(&mut self) {
        self.tcp_ok = true;
    }

    fn conclude_if_needed(&mut self, now_ms: u64) -> Option<NetSelfTestResult> {
        if !self.running {
            return None;
        }
        let deadline_reached = now_ms.saturating_sub(self.started_ms) >= SELF_TEST_WINDOW_MS;
        if self.tx_ok && self.udp_echo_ok && self.tcp_ok || deadline_reached {
            let result = NetSelfTestResult {
                tx_ok: self.tx_ok,
                udp_echo_ok: self.udp_echo_ok,
                tcp_ok: self.tcp_ok,
            };
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
        }
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

/// Initialise the network console stack, translating low-level errors into
/// user-facing diagnostics.
pub fn init_net_console<H>(
    hal: &mut H,
    config: ConsoleNetConfig,
) -> Result<DefaultNetStack, DefaultNetConsoleError>
where
    H: Hardware<Error = HalError>,
{
    NetStack::new(hal, config, DEFAULT_NET_BACKEND).map_err(NetConsoleError::from)
}

impl<D: NetDevice> NetStack<D> {
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
        self.session_state = SessionState::default();
        self.conn_bytes_read = 0;
        self.conn_bytes_written = 0;
    }

    fn record_peer_endpoint(
        peer_endpoint: &mut Option<(IpAddress, u16)>,
        endpoint: Option<IpEndpoint>,
    ) {
        if peer_endpoint.is_none() {
            if let Some(endpoint) = endpoint {
                *peer_endpoint = Some((endpoint.addr, endpoint.port));
            }
        }
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

    fn trace_conn_recv(conn_id: u64, payload: &[u8]) {
        let prefix = payload.len().min(16);
        log::info!(
            "[cohsh-net] conn id={} recv bytes={} first16={:02x?}",
            conn_id,
            payload.len(),
            &payload[..prefix]
        );
    }

    fn trace_conn_send(conn_id: u64, payload: &[u8]) {
        let prefix = payload.len().min(16);
        log::info!(
            "[cohsh-net] conn id={} send bytes={} first16={:02x?}",
            conn_id,
            payload.len(),
            &payload[..prefix]
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
    ) -> Result<Self, NetStackError<D::Error>>
    where
        H: Hardware<Error = HalError>,
    {
        info!("[net-console] init: constructing smoltcp stack");
        debug_assert_ne!(config.listen_port, 0, "TCP console port must be non-zero");
        if cfg!(feature = "dev-virt") {
            debug_assert_eq!(config.listen_port, super::COHESIX_TCP_CONSOLE_PORT);
            debug_assert_eq!(config.address.ip, DEV_VIRT_IP);
            debug_assert_eq!(config.address.prefix_len, DEV_VIRT_PREFIX);
            debug_assert_eq!(config.address.gateway, Some(DEV_VIRT_GATEWAY));
        }

        let ip = Ipv4Address::new(
            config.address.ip[0],
            config.address.ip[1],
            config.address.ip[2],
            config.address.ip[3],
        );
        let gateway = config
            .address
            .gateway
            .map(|gateway| Ipv4Address::new(gateway[0], gateway[1], gateway[2], gateway[3]));
        Self::with_ipv4(hal, ip, config.address.prefix_len, gateway, config, backend)
    }

    fn with_ipv4(
        hal: &mut impl Hardware<Error = HalError>,
        ip: Ipv4Address,
        prefix: u8,
        gateway: Option<Ipv4Address>,
        console_config: ConsoleNetConfig,
        backend: NetBackend,
    ) -> Result<Self, NetStackError<D::Error>> {
        let netmask = prefix_to_netmask(prefix);
        let gateway_label = gateway.unwrap_or(Ipv4Address::UNSPECIFIED);
        let backend_label = backend.label();
        debug_assert_eq!(backend_label, D::name());
        info!(
            "[net-console] init: bringing up {backend_label} with ip={}/{} netmask={} gateway={}",
            ip, prefix, netmask, gateway_label
        );
        info!(
            "[net-console] init: creating {backend_label} device (listen_port={})",
            console_config.listen_port
        );
        let mut device = D::create(hal)?;
        let mac = device.mac();
        info!("[net-console] {backend_label} device online: mac={mac}");

        let socket_guard =
            StorageGuard::acquire(&SOCKET_STORAGE_IN_USE, NetStackError::SocketStorageInUse)?;
        let rx_guard =
            StorageGuard::acquire(&TCP_RX_STORAGE_IN_USE, NetStackError::TcpRxStorageInUse)?;
        let tx_guard =
            StorageGuard::acquire(&TCP_TX_STORAGE_IN_USE, NetStackError::TcpTxStorageInUse)?;
        let tcp_smoke_rx_guard = if SELF_TEST_ENABLED {
            Some(StorageGuard::acquire(
                &TCP_SMOKE_RX_STORAGE_IN_USE,
                NetStackError::TcpSmokeRxStorageInUse,
            )?)
        } else {
            None
        };
        let tcp_smoke_tx_guard = if SELF_TEST_ENABLED {
            Some(StorageGuard::acquire(
                &TCP_SMOKE_TX_STORAGE_IN_USE,
                NetStackError::TcpSmokeTxStorageInUse,
            )?)
        } else {
            None
        };
        let udp_beacon_guard = if SELF_TEST_ENABLED {
            Some(StorageGuard::acquire(
                &UDP_BEACON_STORAGE_IN_USE,
                NetStackError::UdpBeaconStorageInUse,
            )?)
        } else {
            None
        };
        let udp_echo_guard = if SELF_TEST_ENABLED {
            Some(StorageGuard::acquire(
                &UDP_ECHO_STORAGE_IN_USE,
                NetStackError::UdpEchoStorageInUse,
            )?)
        } else {
            None
        };

        let clock = NetworkClock::new();
        let mut iface_config = IfaceConfig::new(HardwareAddress::Ethernet(mac));
        iface_config.random_seed = RANDOM_SEED;

        let mut interface = Interface::new(iface_config, &mut device, clock.now());
        info!(
            "[net-console] smoltcp interface created; assigning ip={}/{} netmask={}",
            ip, prefix, netmask
        );
        interface.update_ip_addrs(|addrs| {
            let cidr = IpCidr::new(IpAddress::from(ip), prefix);
            if addrs.push(cidr).is_err() {
                addrs[0] = cidr;
            }
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
        let sockets = SocketSet::new(unsafe { &mut SOCKET_STORAGE[..] });

        let mut stack = Self {
            clock,
            device,
            interface,
            sockets,
            tcp_handle: SocketHandle::default(),
            server: TcpConsoleServer::new(
                console_config.auth_token,
                console_config.idle_timeout_ms,
            ),
            telemetry: NetTelemetry::default(),
            ip,
            gateway,
            prefix_len: prefix,
            listen_port: console_config.listen_port,
            session_active: false,
            listener_announced: false,
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
            udp_beacon_handle: None,
            udp_echo_handle: None,
            tcp_smoke_handle: None,
            counters: NetCounters::default(),
            self_test: SelfTestState::new(SELF_TEST_ENABLED),
        };
        stack.initialise_socket();
        stack.initialise_self_test_sockets();
        socket_guard.disarm();
        rx_guard.disarm();
        tx_guard.disarm();
        if let Some(guard) = tcp_smoke_rx_guard {
            guard.disarm();
        }
        if let Some(guard) = tcp_smoke_tx_guard {
            guard.disarm();
        }
        if let Some(guard) = udp_beacon_guard {
            guard.disarm();
        }
        if let Some(guard) = udp_echo_guard {
            guard.disarm();
        }
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
        Ok(stack)
    }

    fn initialise_socket(&mut self) {
        debug_assert!(SOCKET_STORAGE_IN_USE.load(Ordering::Acquire));
        debug_assert!(TCP_RX_STORAGE_IN_USE.load(Ordering::Acquire));
        debug_assert!(TCP_TX_STORAGE_IN_USE.load(Ordering::Acquire));
        let rx_buffer = unsafe { TcpSocketBuffer::new(&mut TCP_RX_STORAGE[..]) };
        let tx_buffer = unsafe { TcpSocketBuffer::new(&mut TCP_TX_STORAGE[..]) };
        let tcp_socket = TcpSocket::new(rx_buffer, tx_buffer);
        self.tcp_handle = self.sockets.add(tcp_socket);
    }

    fn initialise_self_test_sockets(&mut self) {
        if !SELF_TEST_ENABLED {
            return;
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
                        "[net-selftest] udp-echo ready on 0.0.0.0:{} (beacon dst=10.0.2.2:{})",
                        UDP_ECHO_PORT, UDP_ECHO_PORT
                    );
                    self.udp_echo_handle = Some(self.sockets.add(echo_socket));
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
            }
        }
    }

    /// Polls the network stack using a host-supplied monotonic timestamp in milliseconds.
    pub fn poll_with_time(&mut self, now_ms: u64) -> bool {
        if !self.service_logged {
            info!("[net-console] service loop running");
            self.service_logged = true;
        }

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

        self.bump_poll_counter();
        let mut poll_result = self
            .interface
            .poll(timestamp, &mut self.device, &mut self.sockets);
        if poll_result != PollResult::None {
            log::info!("[net] smoltcp: events processed at now_ms={}", now_ms);
        }
        let mut activity = poll_result != PollResult::None;
        let tcp_activity = self.process_tcp(now_ms);
        if tcp_activity {
            activity = true;
        }

        // Run a second poll pass when TCP work was observed so any queued
        // responses (including AUTH acknowledgements) are flushed to the wire
        // without waiting for the next timer tick.
        if tcp_activity {
            self.bump_poll_counter();
            poll_result = self
                .interface
                .poll(timestamp, &mut self.device, &mut self.sockets);
            if poll_result != PollResult::None {
                log::info!("[net] smoltcp: post-tcp poll now_ms={}", now_ms);
                activity = true;
            }
        }

        if self.service_self_test(now_ms, timestamp) {
            activity = true;
        }

        self.telemetry.last_poll_ms = now_ms;
        if activity {
            self.telemetry.link_up = true;
        }
        self.telemetry.tx_drops = self.device.tx_drop_count();
        self.sync_device_counters();
        activity
    }

    fn bump_poll_counter(&mut self) {
        self.counters.smoltcp_polls = self.counters.smoltcp_polls.saturating_add(1);
    }

    fn sync_device_counters(&mut self) {
        let device_counters = self.device.counters();
        self.counters.rx_packets = device_counters.rx_packets;
        self.counters.tx_packets = device_counters.tx_packets;
        self.counters.rx_used_advances = device_counters.rx_used_advances;
        self.counters.tx_used_advances = device_counters.tx_used_advances;
    }

    fn log_self_test_result(&self, result: NetSelfTestResult) {
        info!(
            "[net-selftest] result tx_ok={} udp_echo_ok={} tcp_ok={}",
            result.tx_ok, result.udp_echo_ok, result.tcp_ok
        );
        if !result.tx_ok {
            info!("[net-selftest] hint: TX not visible → queue notify / cache / descriptors");
        } else if !result.udp_echo_ok {
            info!(
                "[net-selftest] hint: RX never arrives → buffers not posted / used ring not read / IRQ missing"
            );
        } else if !result.tcp_ok {
            info!("[net-selftest] hint: TCP accepts but no bytes → poll loop scheduling / RX path");
        }
    }

    fn service_self_test(&mut self, now_ms: u64, timestamp: Instant) -> bool {
        if !SELF_TEST_ENABLED {
            return false;
        }

        if let Some(result) = self.self_test.conclude_if_needed(now_ms) {
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

        activity |= self.poll_udp_echo();
        activity |= self.poll_tcp_smoke(now_ms);

        if activity {
            self.bump_poll_counter();
            let poll_result = self
                .interface
                .poll(timestamp, &mut self.device, &mut self.sockets);
            if poll_result != PollResult::None {
                info!("[net-selftest] post-selftest poll flushed pending work");
            }
        }

        if let Some(result) = self.self_test.conclude_if_needed(now_ms) {
            self.log_self_test_result(result);
        }

        activity
    }

    fn send_udp_beacon(&mut self) -> bool {
        let Some(handle) = self.udp_beacon_handle else {
            return false;
        };
        let socket = self.sockets.get_mut::<UdpSocket>(handle);
        if !socket.can_send() {
            return false;
        }

        let mut payload = HeaplessString::<64>::new();
        let _ = write!(&mut payload, "COHESIX_NET_OK {}", self.self_test.beacon_seq);
        let gateway_addr = Ipv4Address::from(DEV_VIRT_GATEWAY);
        let endpoint = IpEndpoint::new(gateway_addr.into(), UDP_ECHO_PORT);
        match socket.send_slice(payload.as_bytes(), endpoint) {
            Ok(()) => {
                self.counters.udp_tx = self.counters.udp_tx.saturating_add(1);
                self.self_test.beacon_seq = self.self_test.beacon_seq.wrapping_add(1);
                self.self_test.beacons_sent = self.self_test.beacons_sent.saturating_add(1);
                self.self_test.record_tx();
                info!(
                    "[net-selftest] udp-beacon queued seq={} -> {}:{} payload='{}'",
                    self.self_test.beacon_seq.saturating_sub(1),
                    gateway_addr,
                    UDP_ECHO_PORT,
                    payload
                );
                true
            }
            Err(err) => {
                warn!(
                    "[net-selftest] udp-beacon send failed seq={} err={:?}",
                    self.self_test.beacon_seq, err
                );
                false
            }
        }
    }

    fn poll_udp_echo(&mut self) -> bool {
        let Some(handle) = self.udp_echo_handle else {
            return false;
        };
        let socket = self.sockets.get_mut::<UdpSocket>(handle);
        let mut activity = false;
        loop {
            match socket.recv() {
                Ok((payload, meta)) => {
                    let endpoint = meta.endpoint;
                    let mut reply_len = 0usize;
                    let mut reply = [0u8; UDP_PAYLOAD_CAPACITY];
                    let prefix = b"ECHO:";
                    reply[..prefix.len()].copy_from_slice(prefix);
                    let copy_len =
                        core::cmp::min(payload.len(), reply.len().saturating_sub(prefix.len()));
                    reply[prefix.len()..prefix.len() + copy_len]
                        .copy_from_slice(&payload[..copy_len]);
                    reply_len = prefix.len() + copy_len;
                    self.counters.udp_rx = self.counters.udp_rx.saturating_add(1);
                    if self.self_test.running {
                        self.self_test.record_udp_echo();
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
                                self.self_test.record_udp_echo();
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
            return false;
        }

        let mut activity = false;
        if socket.state() == TcpState::Established {
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
            while socket.can_recv() {
                let recv_result = socket.recv(|data| {
                    let copy_len = core::cmp::min(data.len(), temp.len());
                    temp[..copy_len].copy_from_slice(&data[..copy_len]);
                    copied = copy_len;
                    (copy_len, ())
                });
                if recv_result.is_err() || copied == 0 {
                    break;
                }
                self.counters.tcp_rx_bytes =
                    self.counters.tcp_rx_bytes.saturating_add(copied as u64);
                info!(
                    "[net-selftest] tcp-smoke recv bytes={} state={:?}",
                    copied,
                    socket.state()
                );
                activity = true;
                break;
            }

            if socket.can_send() && (copied > 0 || !socket.can_recv()) {
                match socket.send_slice(b"OK\n") {
                    Ok(sent) => {
                        self.counters.tcp_tx_bytes =
                            self.counters.tcp_tx_bytes.saturating_add(sent as u64);
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
        }

        activity
    }

    fn process_tcp(&mut self, now_ms: u64) -> bool {
        let mut activity = false;
        let mut log_closed_conn: Option<u64> = None;
        let mut record_closed_conn: Option<u64> = None;
        let mut reset_session = false;

        {
            let socket = self.sockets.get_mut::<TcpSocket>(self.tcp_handle);
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
                }
                if self.session_active {
                    self.server.end_session();
                    self.session_active = false;
                    self.active_client_id = None;
                }
            }

            Self::log_tcp_state_change(
                &mut self.session_state,
                socket,
                self.peer_endpoint,
                self.ip,
            );

            if socket.state() == TcpState::Established && !self.session_active {
                let client_id = self.client_counter.wrapping_add(1);
                self.client_counter = client_id;
                self.active_client_id = Some(client_id);
                self.conn_bytes_read = 0;
                self.conn_bytes_written = 0;
                reset_session = true;
                Self::record_peer_endpoint(&mut self.peer_endpoint, socket.remote_endpoint());
                let (peer_label, peer_port) = Self::peer_parts(self.peer_endpoint, socket);
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
                        &mut self.telemetry,
                        &mut self.conn_bytes_written,
                        &mut self.counters,
                        socket,
                        now_ms,
                        self.active_client_id,
                        self.auth_state,
                        &mut self.session_state,
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
                let mut temp = [0u8; 64];
                let conn_id = self.active_client_id.unwrap_or(0);
                info!(
                    "[cohsh-net] conn id={} recv-ready state={:?} may_recv={} can_recv={}",
                    conn_id,
                    socket.state(),
                    socket.may_recv(),
                    socket.can_recv()
                );
                log::info!(
                    target: "cohsh-net",
                    "[tcp] socket can_recv={} may_recv={} state={:?}",
                    socket.can_recv(),
                    socket.may_recv(),
                    socket.state()
                );
                while socket.can_recv() {
                    let mut copied = 0usize;
                    let recv_result = socket.recv(|data| {
                        let preview_len = core::cmp::min(data.len(), 32);
                        log::debug!(
                            target: "net-console",
                            "[tcp] recv on console socket: len={} first_bytes={:02x?}",
                            data.len(),
                            &data[..preview_len],
                        );
                        let copy_len = core::cmp::min(data.len(), temp.len());
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
                            self.counters.tcp_rx_bytes =
                                self.counters.tcp_rx_bytes.saturating_add(copied as u64);
                            let dump_len = core::cmp::min(copied, 32);
                            let (peer_label, peer_port) =
                                Self::peer_parts(self.peer_endpoint, socket);
                            log::info!(
                                target: "cohsh-net",
                                "[tcp] recv bytes={} first={:02x?} peer={}:{} state={:?}",
                                copied,
                                &temp[..dump_len],
                                peer_label,
                                peer_port,
                                socket.state()
                            );
                            #[cfg(feature = "net-trace-31337")]
                            {
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
                                    activity |= Self::flush_outbound(
                                        &mut self.server,
                                        &mut self.telemetry,
                                        &mut self.conn_bytes_written,
                                        &mut self.counters,
                                        socket,
                                        now_ms,
                                        self.active_client_id,
                                        self.auth_state,
                                        &mut self.session_state,
                                    );
                                    Self::log_session_closed(
                                        &mut self.session_state,
                                        self.peer_endpoint,
                                        socket,
                                    );
                                    socket.close();
                                    self.server.end_session();
                                    self.session_active = false;
                                    reset_session = true;
                                    self.peer_endpoint = None;
                                    self.active_client_id = None;
                                    break;
                                }
                                SessionEvent::Close => {
                                    let _ = Self::flush_outbound(
                                        &mut self.server,
                                        &mut self.telemetry,
                                        &mut self.conn_bytes_written,
                                        &mut self.counters,
                                        socket,
                                        now_ms,
                                        self.active_client_id,
                                        self.auth_state,
                                        &mut self.session_state,
                                    );
                                    Self::log_session_closed(
                                        &mut self.session_state,
                                        self.peer_endpoint,
                                        socket,
                                    );
                                    socket.close();
                                    self.server.end_session();
                                    self.session_active = false;
                                    self.peer_endpoint = None;
                                    reset_session = true;
                                    self.active_client_id = None;
                                    activity = true;
                                    break;
                                }
                            }
                        }
                        Err(err) => {
                            match err {
                                TcpRecvError::Finished => {
                                    info!(
                                        "[net-console] TCP client #{} closed (clean shutdown)",
                                        self.active_client_id.unwrap_or(0)
                                    );
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
                                }
                            }
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
                            self.server.end_session();
                            self.session_active = false;
                            self.peer_endpoint = None;
                            reset_session = true;
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
                    &mut self.telemetry,
                    &mut self.conn_bytes_written,
                    &mut self.counters,
                    socket,
                    now_ms,
                    self.active_client_id,
                    self.auth_state,
                    &mut self.session_state,
                );
                Self::log_session_closed(&mut self.session_state, self.peer_endpoint, socket);
                socket.close();
                self.server.end_session();
                self.session_active = false;
                let conn_id = self.active_client_id.unwrap_or(0);
                self.peer_endpoint = None;
                Self::set_auth_state(
                    &mut self.auth_state,
                    self.active_client_id,
                    AuthState::Failed,
                );
                log_closed_conn = Some(conn_id);
                record_closed_conn = Some(conn_id);
                self.active_client_id = None;
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
                    &mut self.telemetry,
                    &mut self.conn_bytes_written,
                    &mut self.counters,
                    socket,
                    now_ms,
                    self.active_client_id,
                    self.auth_state,
                    &mut self.session_state,
                );
                Self::log_session_closed(&mut self.session_state, self.peer_endpoint, socket);
                socket.close();
                self.server.end_session();
                self.session_active = false;
                let conn_id = self.active_client_id.unwrap_or(0);
                self.peer_endpoint = None;
                Self::set_auth_state(
                    &mut self.auth_state,
                    self.active_client_id,
                    AuthState::Failed,
                );
                log_closed_conn = Some(conn_id);
                record_closed_conn = Some(conn_id);
                self.active_client_id = None;
                activity |= true;
            }

            activity |= Self::flush_outbound(
                &mut self.server,
                &mut self.telemetry,
                &mut self.conn_bytes_written,
                &mut self.counters,
                socket,
                now_ms,
                self.active_client_id,
                self.auth_state,
                &mut self.session_state,
            );

            if matches!(socket.state(), TcpState::CloseWait | TcpState::Closed)
                && self.session_active
            {
                info!(
                    "[net-console] TCP client #{} closed (state={:?})",
                    self.active_client_id.unwrap_or(0),
                    socket.state()
                );
                debug!(
                    "[net-console][auth] state={:?} client={} closing socket state={:?}",
                    self.auth_state,
                    self.active_client_id.unwrap_or(0),
                    socket.state()
                );
                Self::log_session_closed(&mut self.session_state, self.peer_endpoint, socket);
                socket.close();
                self.server.end_session();
                self.session_active = false;
                let conn_id = self.active_client_id.unwrap_or(0);
                log_closed_conn = Some(conn_id);
                record_closed_conn = Some(conn_id);
                self.active_client_id = None;
                self.peer_endpoint = None;
                Self::set_auth_state(
                    &mut self.auth_state,
                    self.active_client_id,
                    AuthState::Start,
                );
                activity |= true;
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
            self.log_poll_snapshot(snapshot);
        }

        if reset_session {
            self.reset_session_state();
        }

        if let Some(conn_id) = log_closed_conn {
            self.log_conn_summary(conn_id);
        }
        if let Some(conn_id) = record_closed_conn {
            self.record_conn_closed(conn_id);
        }

        activity
    }

    fn flush_outbound(
        server: &mut TcpConsoleServer,
        telemetry: &mut NetTelemetry,
        conn_bytes_written: &mut u64,
        counters: &mut NetCounters,
        socket: &mut TcpSocket,
        now_ms: u64,
        conn_id: Option<u64>,
        auth_state: AuthState,
        session_state: &mut SessionState,
    ) -> bool {
        if !socket.can_send() {
            return false;
        }
        let mut activity = false;
        let pre_auth = !server.is_authenticated();
        let mut budget = MAX_TX_BUDGET;
        info!(
            "[cohsh-net] flush_outbound: can_send={} pre_auth={} auth_state={:?}",
            socket.can_send(),
            pre_auth,
            auth_state
        );
        while budget > 0 && socket.can_send() {
            let Some(line) = server.pop_outbound() else {
                break;
            };
            if pre_auth && !(line.starts_with("OK AUTH") || line.starts_with("ERR AUTH")) {
                server.push_outbound_front(line);
                break;
            }
            let mut payload: HeaplessVec<u8, { DEFAULT_LINE_CAPACITY + 2 }> = HeaplessVec::new();
            if payload.extend_from_slice(line.as_bytes()).is_err()
                || payload.extend_from_slice(b"\r\n").is_err()
            {
                server.push_outbound_front(line);
                telemetry.tx_drops = telemetry.tx_drops.saturating_add(1);
                break;
            }
            if pre_auth {
                info!(
                    "[net-console] handshake: sending {}-byte response to client",
                    payload.len()
                );
                info!(
                    "[cohsh-net] send: auth response len={} role='AUTH'",
                    payload.len()
                );
            }
            match socket.send_slice(payload.as_slice()) {
                Ok(sent) if sent == payload.len() => {
                    let preview_len = core::cmp::min(sent, 32);
                    log::debug!(
                        target: "net-console",
                        "[tcp] send on console socket: len={} first_bytes={:02x?}",
                        sent,
                        &payload[..preview_len],
                    );
                    *conn_bytes_written = conn_bytes_written.saturating_add(sent as u64);
                    counters.tcp_tx_bytes = counters.tcp_tx_bytes.saturating_add(sent as u64);
                    if !session_state.logged_first_send {
                        info!(
                            target: "root_task::net",
                            "[tcp] first-send.ok bytes={sent}"
                        );
                        session_state.logged_first_send = true;
                    }
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
                    if pre_auth {
                        info!(
                            "[net-console] conn {}: sent pre-auth line '{}' ({} bytes)",
                            conn_id.unwrap_or(0),
                            core::str::from_utf8(line.as_bytes()).unwrap_or("<invalid>"),
                            sent
                        );
                        if line.starts_with("OK AUTH") || line.starts_with("ERR AUTH") {
                            info!(
                                "[net-console] auth response sent; session state = {:?}",
                                auth_state
                            );
                            log::info!(
                                target: "net-console",
                                "[net-console] send ACK on TCP session {}: {}",
                                conn_id.unwrap_or(0),
                                core::str::from_utf8(line.as_bytes()).unwrap_or("<invalid>")
                            );
                        }
                    }
                    if server.is_authenticated() {
                        server.mark_activity(now_ms);
                    }
                    let conn_id = conn_id.unwrap_or(0);
                    Self::trace_conn_send(conn_id, payload.as_slice());
                    #[cfg(feature = "net-trace-31337")]
                    trace!(
                        "[net-auth][conn={}] wrote {} bytes in state {:?}",
                        conn_id,
                        sent,
                        auth_state
                    );
                    activity = true;
                }
                Ok(_) => {
                    server.push_outbound_front(line);
                    telemetry.tx_drops = telemetry.tx_drops.saturating_add(1);
                    break;
                }
                Err(err) => {
                    warn!(
                        target: "root_task::net",
                        "[tcp] send.err err={err:?}"
                    );
                    server.push_outbound_front(line);
                    telemetry.tx_drops = telemetry.tx_drops.saturating_add(1);
                    break;
                }
            }
            budget -= 1;
        }
        activity
    }

    fn log_conn_summary(&self, conn_id: u64) {
        info!(
            "[net-console] conn {}: bytes read={}, bytes written={}",
            conn_id, self.conn_bytes_read, self.conn_bytes_written
        );
    }

    fn record_conn_closed(&mut self, conn_id: u64) {
        Self::trace_conn_closed(
            conn_id,
            "disconnect",
            self.conn_bytes_read,
            self.conn_bytes_written,
        );
        let _ = self.events.push(NetConsoleEvent::Disconnected {
            conn_id,
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

impl<D: NetDevice> NetPoller for NetStack<D> {
    fn poll(&mut self, now_ms: u64) -> bool {
        self.poll_with_time(now_ms)
    }

    fn telemetry(&self) -> NetTelemetry {
        self.telemetry()
    }

    fn stats(&self) -> NetCounters {
        self.counters
    }

    fn drain_console_lines(
        &mut self,
        visitor: &mut dyn FnMut(HeaplessString<DEFAULT_LINE_CAPACITY>),
    ) {
        self.server.drain_console_lines(visitor);
    }

    fn send_console_line(&mut self, line: &str) {
        if self.server.enqueue_outbound(line).is_err() {
            self.telemetry.tx_drops = self.telemetry.tx_drops.saturating_add(1);
        }
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

    fn inject_console_line(&mut self, _line: &str) {}

    fn reset(&mut self) {
        self.server.end_session();
        self.session_active = false;
        self.telemetry = NetTelemetry::default();
    }

    fn start_self_test(&mut self, now_ms: u64) -> bool {
        if !SELF_TEST_ENABLED {
            return false;
        }
        if self.self_test.start(now_ms) {
            info!(
                "[net-selftest] starting run (udp dst=10.0.2.2:{} tcp dst=127.0.0.1:{})",
                UDP_ECHO_PORT, TCP_SMOKE_PORT
            );
            info!(
                "[net-selftest] host capture: tcpdump -i lo0 -n udp port {}",
                UDP_ECHO_PORT
            );
            info!(
                "[net-selftest] host udp echo: echo -n \"ping\" | nc -u -w1 127.0.0.1 {}",
                UDP_ECHO_PORT
            );
            info!(
                "[net-selftest] host tcp smoke: printf \"hi\" | nc -v 127.0.0.1 {}",
                TCP_SMOKE_PORT
            );
            true
        } else {
            false
        }
    }

    fn self_test_report(&self) -> NetSelfTestReport {
        self.self_test.report()
    }
}

impl<D: NetDevice> Drop for NetStack<D> {
    fn drop(&mut self) {
        SOCKET_STORAGE_IN_USE.store(false, portable_atomic::Ordering::Release);
        TCP_RX_STORAGE_IN_USE.store(false, portable_atomic::Ordering::Release);
        TCP_TX_STORAGE_IN_USE.store(false, portable_atomic::Ordering::Release);
        TCP_SMOKE_RX_STORAGE_IN_USE.store(false, portable_atomic::Ordering::Release);
        TCP_SMOKE_TX_STORAGE_IN_USE.store(false, portable_atomic::Ordering::Release);
        UDP_BEACON_STORAGE_IN_USE.store(false, portable_atomic::Ordering::Release);
        UDP_ECHO_STORAGE_IN_USE.store(false, portable_atomic::Ordering::Release);
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
        stack.server.drain_console_lines(&mut |line| {
            let _ = writeln!(console, "{line}");
        });
        now_ms = now_ms.saturating_add(5);
    }
}
