// Author: Lukas Bower

//! Smoltcp-backed TCP console stack for the in-VM root task.
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
use smoltcp::time::Instant;
use smoltcp::wire::{
    EthernetAddress, HardwareAddress, IpAddress, IpCidr, IpEndpoint, IpListenEndpoint, Ipv4Address,
};

use super::{
    console_srv::{SessionEvent, TcpConsoleServer},
    NetConsoleEvent, NetPoller, NetTelemetry, AUTH_TOKEN, CONSOLE_TCP_PORT, IDLE_TIMEOUT_MS,
};
use crate::drivers::virtio::net::{DriverError, VirtioNet};
use crate::hal::{HalError, Hardware};
use crate::serial::DEFAULT_LINE_CAPACITY;

const TCP_RX_BUFFER: usize = 2048;
const TCP_TX_BUFFER: usize = 2048;
const SOCKET_CAPACITY: usize = 4;
const MAX_TX_BUDGET: usize = 8;
const RANDOM_SEED: u64 = 0x5a5a_5a5a_1234_5678;
const DEFAULT_IP: (u8, u8, u8, u8) = (10, 0, 2, 15);
const DEFAULT_GW: (u8, u8, u8, u8) = (10, 0, 2, 2);
const DEFAULT_PREFIX: u8 = 24;

#[derive(Debug)]
pub enum NetStackError {
    Driver(DriverError),
    SocketStorageInUse,
    TcpRxStorageInUse,
    TcpTxStorageInUse,
}

impl fmt::Display for NetStackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Driver(err) => write!(f, "{err}"),
            Self::SocketStorageInUse => f.write_str("socket storage already in use"),
            Self::TcpRxStorageInUse => f.write_str("TCP RX storage already in use"),
            Self::TcpTxStorageInUse => f.write_str("TCP TX storage already in use"),
        }
    }
}

impl From<DriverError> for NetStackError {
    fn from(value: DriverError) -> Self {
        Self::Driver(value)
    }
}

/// High-level errors surfaced while initialising the TCP console stack.
#[derive(Debug)]
pub enum NetConsoleError {
    /// No virtio-net device was found on any probed virtio-mmio slot.
    NoDevice,
    /// An error occurred during stack bring-up.
    Init(NetStackError),
}

impl fmt::Display for NetConsoleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDevice => f.write_str("no virtio-net device present"),
            Self::Init(err) => write!(f, "{err}"),
        }
    }
}

impl From<NetStackError> for NetConsoleError {
    fn from(err: NetStackError) -> Self {
        match err {
            NetStackError::Driver(DriverError::NoDevice) => Self::NoDevice,
            other => Self::Init(other),
        }
    }
}

struct StorageGuard<'a> {
    flag: &'a AtomicBool,
    release_on_drop: bool,
}

impl<'a> StorageGuard<'a> {
    fn acquire(flag: &'a AtomicBool, busy_error: NetStackError) -> Result<Self, NetStackError> {
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
}

static SOCKET_STORAGE_IN_USE: AtomicBool = AtomicBool::new(false);
static mut SOCKET_STORAGE: [SocketStorage<'static>; SOCKET_CAPACITY] =
    [SocketStorage::EMPTY; SOCKET_CAPACITY];
static TCP_RX_STORAGE_IN_USE: AtomicBool = AtomicBool::new(false);
static mut TCP_RX_STORAGE: [u8; TCP_RX_BUFFER] = [0u8; TCP_RX_BUFFER];
static TCP_TX_STORAGE_IN_USE: AtomicBool = AtomicBool::new(false);
static mut TCP_TX_STORAGE: [u8; TCP_TX_BUFFER] = [0u8; TCP_TX_BUFFER];

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

/// Smoltcp-backed network stack that bridges the virtio-net device into the root task.
pub struct NetStack {
    clock: NetworkClock,
    device: VirtioNet,
    interface: Interface,
    sockets: SocketSet<'static>,
    tcp_handle: SocketHandle,
    server: TcpConsoleServer,
    telemetry: NetTelemetry,
    ip: Ipv4Address,
    gateway: Option<Ipv4Address>,
    prefix_len: u8,
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
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct PollSnapshot {
    session_active: bool,
    auth_state: AuthState,
    listener_ready: bool,
}

/// Initialise the network console stack, translating low-level errors into
/// user-facing diagnostics.
pub fn init_net_console<H>(hal: &mut H) -> Result<NetStack, NetConsoleError>
where
    H: Hardware<Error = HalError>,
{
    NetStack::new(hal).map_err(NetConsoleError::from)
}

impl NetStack {
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

    fn log_poll_snapshot(&mut self) {
        let snapshot = PollSnapshot {
            session_active: self.session_active,
            auth_state: self.auth_state,
            listener_ready: self.listener_announced,
        };

        if self.last_poll_snapshot == Some(snapshot) {
            return;
        }

        info!(
            "[cohsh-net] poll state: session_active={} auth_state={:?} listener_ready={} staged_events={}",
            snapshot.session_active,
            snapshot.auth_state,
            snapshot.listener_ready,
            self.events.len(),
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
        let previous = session_state.last_state.unwrap_or(TcpState::Closed);
        if Some(current) == session_state.last_state {
            return;
        }
        let (peer, port) = Self::peer_parts(peer_endpoint, socket);
        let local_port = socket
            .local_endpoint()
            .map(|endpoint| endpoint.port)
            .unwrap_or(CONSOLE_TCP_PORT);

        match (previous, current) {
            (_, TcpState::SynReceived) => info!(
                "[cohsh-net] event: new incoming connection from {}:{}",
                peer, port
            ),
            (_, TcpState::Established) => info!(
                "[cohsh-net] event: connection established (local={}:{}, remote={}:{})",
                iface_ip, local_port, peer, port
            ),
            (TcpState::Established, TcpState::CloseWait | TcpState::Closed) => info!(
                "[cohsh-net] event: connection closed/reset (remote={}:{} state={:?})",
                peer, port, current
            ),
            _ => info!(
                "[cohsh-net] tcp state: {:?} -> {:?} (remote={}:{})",
                previous, current, peer, port
            ),
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
            "[cohsh-net] session closed from {}:{} (final_state={:?})",
            peer,
            port,
            socket.state()
        );
        session_state.close_logged = true;
    }

    /// Constructs a network stack bound to the provided [`KernelEnv`].
    pub fn new<H>(hal: &mut H) -> Result<Self, NetStackError>
    where
        H: Hardware<Error = HalError>,
    {
        info!("[net-console] init: constructing smoltcp stack");
        let ip = Ipv4Address::new(DEFAULT_IP.0, DEFAULT_IP.1, DEFAULT_IP.2, DEFAULT_IP.3);
        let gateway = Ipv4Address::new(DEFAULT_GW.0, DEFAULT_GW.1, DEFAULT_GW.2, DEFAULT_GW.3);
        Self::with_ipv4(hal, ip, DEFAULT_PREFIX, Some(gateway))
    }

    fn with_ipv4(
        hal: &mut impl Hardware<Error = HalError>,
        ip: Ipv4Address,
        prefix: u8,
        gateway: Option<Ipv4Address>,
    ) -> Result<Self, NetStackError> {
        info!(
            "[net-console] init: bringing up virtio-net with ip={ip}/{prefix} gateway={:?}",
            gateway
        );
        info!("[net-console] init: creating VirtioNet device");
        let mut device = VirtioNet::new(hal)?;
        let mac = device.mac();
        info!("[net-console] virtio-net device online: mac={mac}");

        let socket_guard =
            StorageGuard::acquire(&SOCKET_STORAGE_IN_USE, NetStackError::SocketStorageInUse)?;
        let rx_guard =
            StorageGuard::acquire(&TCP_RX_STORAGE_IN_USE, NetStackError::TcpRxStorageInUse)?;
        let tx_guard =
            StorageGuard::acquire(&TCP_TX_STORAGE_IN_USE, NetStackError::TcpTxStorageInUse)?;

        let clock = NetworkClock::new();
        let mut config = IfaceConfig::new(HardwareAddress::Ethernet(mac));
        config.random_seed = RANDOM_SEED;

        let mut interface = Interface::new(config, &mut device, clock.now());
        info!("[net-console] smoltcp interface created; assigning ip={ip}/{prefix}");
        interface.update_ip_addrs(|addrs| {
            let cidr = IpCidr::new(IpAddress::from(ip), prefix);
            if addrs.push(cidr).is_err() {
                addrs[0] = cidr;
            }
        });
        if let Some(gw) = gateway {
            let _ = interface.routes_mut().add_default_ipv4_route(gw);
            info!("[net-console] default gateway set to {gw}");
        }
        let sockets = SocketSet::new(unsafe { &mut SOCKET_STORAGE[..] });

        let mut stack = Self {
            clock,
            device,
            interface,
            sockets,
            tcp_handle: SocketHandle::default(),
            server: TcpConsoleServer::new(AUTH_TOKEN, IDLE_TIMEOUT_MS),
            telemetry: NetTelemetry::default(),
            ip,
            gateway,
            prefix_len: prefix,
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
        };
        stack.initialise_socket();
        socket_guard.disarm();
        rx_guard.disarm();
        tx_guard.disarm();
        info!("[net-console] init: TCP listener socket prepared");
        info!("[net-console] init: success; tcp console wired (non-blocking)");
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

        let poll_result = self
            .interface
            .poll(timestamp, &mut self.device, &mut self.sockets);
        if poll_result != PollResult::None {
            log::info!("[net] smoltcp: events processed at now_ms={}", now_ms);
        }
        let mut activity = poll_result != PollResult::None;
        if self.process_tcp(now_ms) {
            activity = true;
        }

        self.telemetry.last_poll_ms = now_ms;
        if activity {
            self.telemetry.link_up = true;
        }
        self.telemetry.tx_drops = self.device.tx_drop_count();
        self.log_poll_snapshot();
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
            Self::log_tcp_state_change(
                &mut self.session_state,
                socket,
                self.peer_endpoint,
                self.ip,
            );

            if !socket.is_open() {
                self.peer_endpoint = None;
                reset_session = true;
                info!(
                    target: "net",
                    "TCP console: binding listener on {}:{}",
                    self.ip,
                    CONSOLE_TCP_PORT
                );
                match socket.listen(IpListenEndpoint::from(CONSOLE_TCP_PORT)) {
                    Ok(()) => {
                        log::info!(
                            "[cohsh-net] listen: tcp/{} bound (iface_ip={})",
                            CONSOLE_TCP_PORT,
                            self.ip
                        );
                        info!(
                            "[net-console] tcp listener bound: port={} iface_ip={}",
                            CONSOLE_TCP_PORT, self.ip
                        );
                        info!(
                            "[cohsh-net] listener bound on iface ip={} port={} (QEMU hostfwd: 127.0.0.1:{} -> {}:{})",
                            self.ip,
                            CONSOLE_TCP_PORT,
                            CONSOLE_TCP_PORT,
                            self.ip,
                            CONSOLE_TCP_PORT
                        );
                    }
                    Err(err) => {
                        log::error!(
                            "[cohsh-net] listen: tcp/{} failed: {:?}",
                            CONSOLE_TCP_PORT,
                            err
                        );
                        warn!("[net-console] failed to start TCP console listener: {err}",);
                        return activity;
                    }
                }
                if !self.listener_announced {
                    info!(
                        "[cohsh-net] poll loop online; listening on tcp/{}",
                        CONSOLE_TCP_PORT
                    );
                    info!(
                        "[net-console] TCP console listening on 0.0.0.0:{} (iface ip={})",
                        CONSOLE_TCP_PORT, self.ip
                    );
                    self.listener_announced = true;
                }
                if self.session_active {
                    self.server.end_session();
                    self.session_active = false;
                    self.active_client_id = None;
                }
            }

            if socket.state() == TcpState::Established && !self.session_active {
                let client_id = self.client_counter.wrapping_add(1);
                self.client_counter = client_id;
                self.active_client_id = Some(client_id);
                self.conn_bytes_read = 0;
                self.conn_bytes_written = 0;
                reset_session = true;
                Self::record_peer_endpoint(&mut self.peer_endpoint, socket.remote_endpoint());
                let peer = if let Some(endpoint) = socket.remote_endpoint() {
                    info!(
                        target: "net-console",
                        "[net-console] conn: accepted from {:?}",
                        endpoint
                    );
                    let (addr, port) = Self::peer_parts(self.peer_endpoint, socket);
                    info!(
                        "[cohsh-net] accept: new session from {}:{} (state={:?})",
                        addr,
                        port,
                        socket.state()
                    );
                    info!(
                        "[net-console] conn {}: established from {}",
                        client_id, endpoint
                    );
                    info!(
                        "[net-console] connection accepted: remote={:?}",
                        socket.remote_endpoint()
                    );
                    let mut label = HeaplessString::<32>::new();
                    if let Ok(()) = FmtWrite::write_fmt(&mut label, format_args!("{}", endpoint)) {
                        Some(label)
                    } else {
                        None
                    }
                } else {
                    info!("[net-console] conn {}: established", client_id);
                    None
                };
                let peer_label = peer.as_ref().map(|p| p.as_str()).unwrap_or("<unknown>");
                info!(
                    "[net-console] accepted TCP console connection id={} peer={}",
                    client_id, peer_label
                );
                let _ = self.events.push(NetConsoleEvent::Connected {
                    conn_id: client_id,
                    peer,
                });
                self.server.begin_session(now_ms);
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
                    socket,
                    now_ms,
                    self.active_client_id,
                    self.auth_state,
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
                self.session_active = true;
                info!(
                    "[net-console] auth: waiting for client credentials (client_id={})",
                    client_id
                );
            }

            if socket.can_recv() {
                let mut temp = [0u8; 64];
                while socket.can_recv() {
                    match socket.recv_slice(&mut temp) {
                        Ok(0) => break,
                        Ok(count) => {
                            self.conn_bytes_read =
                                self.conn_bytes_read.saturating_add(count as u64);
                            let dump_len = core::cmp::min(count, 32);
                            let (peer_label, peer_port) =
                                Self::peer_parts(self.peer_endpoint, socket);
                            info!(
                                "[cohsh-net][tcp] recv: nbytes={} from {}:{} state={:?}",
                                count,
                                peer_label,
                                peer_port,
                                socket.state()
                            );
                            info!("[cohsh-net][tcp] recv hex: {:02x?}", &temp[..dump_len]);
                            if self.auth_state == AuthState::AuthRequested
                                && !self.session_state.logged_first_recv
                            {
                                info!(
                                    "[cohsh-net][auth] received candidate auth frame len={} from {}:{}",
                                    count,
                                    peer_label,
                                    peer_port
                                );
                                info!(
                                    "[cohsh-net][auth] frame hex: {:02x?}",
                                    &temp[..count.min(32)]
                                );
                            }
                            self.session_state.logged_first_recv = true;
                            match self.server.ingest(&temp[..count], now_ms) {
                                SessionEvent::None => {}
                                SessionEvent::Authenticated => {
                                    let conn_id = self.active_client_id.unwrap_or(0);
                                    Self::set_auth_state(
                                        &mut self.auth_state,
                                        self.active_client_id,
                                        AuthState::Attached,
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
                                    let _ = Self::flush_outbound(
                                        &mut self.server,
                                        &mut self.telemetry,
                                        &mut self.conn_bytes_written,
                                        socket,
                                        now_ms,
                                        self.active_client_id,
                                        self.auth_state,
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
                                        socket,
                                        now_ms,
                                        self.active_client_id,
                                        self.auth_state,
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
                                        "[net-console] closing connection: reason=recv-error state={:?}",
                                        self.auth_state
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
                        Err(err) => {
                            log::warn!(
                                "[cohsh-net] recv error: {:?} (state={:?})",
                                err,
                                socket.state()
                            );
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
                let _ = self.server.enqueue_outbound("ERR AUTH reason=timeout");
                let _ = Self::flush_outbound(
                    &mut self.server,
                    &mut self.telemetry,
                    &mut self.conn_bytes_written,
                    socket,
                    now_ms,
                    self.active_client_id,
                    self.auth_state,
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
                activity = true;
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
                    "[net-console] closing connection: reason=inactivity-timeout state={:?}",
                    self.auth_state
                );
                let _ = self.server.enqueue_outbound("ERR CONSOLE reason=timeout");
                let _ = Self::flush_outbound(
                    &mut self.server,
                    &mut self.telemetry,
                    &mut self.conn_bytes_written,
                    socket,
                    now_ms,
                    self.active_client_id,
                    self.auth_state,
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
                activity = true;
            }

            activity |= Self::flush_outbound(
                &mut self.server,
                &mut self.telemetry,
                &mut self.conn_bytes_written,
                socket,
                now_ms,
                self.active_client_id,
                self.auth_state,
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
                activity = true;
            }
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
        socket: &mut TcpSocket,
        now_ms: u64,
        conn_id: Option<u64>,
        auth_state: AuthState,
    ) -> bool {
        if !socket.can_send() {
            return false;
        }
        let mut activity = false;
        let pre_auth = !server.is_authenticated();
        let mut budget = MAX_TX_BUDGET;
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
            let tcp_state = socket.state();
            match socket.send_slice(payload.as_slice()) {
                Ok(sent) if sent == payload.len() => {
                    *conn_bytes_written = conn_bytes_written.saturating_add(sent as u64);
                    let dump_len = payload.len().min(32);
                    info!(
                        "[cohsh-net] send: {} bytes (state={:?}, auth_state={:?}): {:02x?}",
                        sent,
                        tcp_state,
                        auth_state,
                        &payload[..dump_len]
                    );
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
                        }
                    }
                    if server.is_authenticated() {
                        server.mark_activity(now_ms);
                    }
                    trace!(
                        "[net-auth][conn={}] wrote {} bytes in state {:?}",
                        conn_id.unwrap_or(0),
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
                    warn!("[net-console] TCP client write error: {err}",);
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
        let _ = self.events.push(NetConsoleEvent::Disconnected {
            conn_id,
            bytes_read: self.conn_bytes_read,
            bytes_written: self.conn_bytes_written,
        });
    }

    /// Returns the negotiated Ethernet address for the attached virtio-net device.
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

impl NetPoller for NetStack {
    fn poll(&mut self, now_ms: u64) -> bool {
        self.poll_with_time(now_ms)
    }

    fn telemetry(&self) -> NetTelemetry {
        self.telemetry()
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
}

impl Drop for NetStack {
    fn drop(&mut self) {
        SOCKET_STORAGE_IN_USE.store(false, portable_atomic::Ordering::Release);
        TCP_RX_STORAGE_IN_USE.store(false, portable_atomic::Ordering::Release);
        TCP_TX_STORAGE_IN_USE.store(false, portable_atomic::Ordering::Release);
    }
}

/// Cooperative polling loop that mirrors the serial console onto the TCP port.
pub fn run_tcp_console(console: &mut crate::console::Console, stack: &mut NetStack) -> ! {
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
