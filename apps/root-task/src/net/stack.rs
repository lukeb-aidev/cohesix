// Author: Lukas Bower

//! Smoltcp-backed TCP console stack for the in-VM root task.
#![allow(unsafe_code)]
#![cfg(feature = "kernel")]

use heapless::{String as HeaplessString, Vec as HeaplessVec};
use portable_atomic::AtomicBool;
use smoltcp::iface::{
    Config as IfaceConfig, Interface, PollResult, SocketHandle, SocketSet, SocketStorage,
};
use smoltcp::socket::tcp::{
    Socket as TcpSocket, SocketBuffer as TcpSocketBuffer, State as TcpState,
};
use smoltcp::time::Instant;
use smoltcp::wire::{
    EthernetAddress, HardwareAddress, IpAddress, IpCidr, IpListenEndpoint, Ipv4Address,
};

use super::{
    console_srv::{SessionEvent, TcpConsoleServer},
    NetPoller, NetTelemetry, AUTH_TOKEN, CONSOLE_TCP_PORT, IDLE_TIMEOUT_MS,
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
}

impl NetStack {
    /// Constructs a network stack bound to the provided [`KernelEnv`].
    pub fn new<H>(hal: &mut H) -> Result<Self, DriverError>
    where
        H: Hardware<Error = HalError>,
    {
        let ip = Ipv4Address::new(DEFAULT_IP.0, DEFAULT_IP.1, DEFAULT_IP.2, DEFAULT_IP.3);
        let gateway = Ipv4Address::new(DEFAULT_GW.0, DEFAULT_GW.1, DEFAULT_GW.2, DEFAULT_GW.3);
        Self::with_ipv4(hal, ip, DEFAULT_PREFIX, Some(gateway))
    }

    fn with_ipv4(
        hal: &mut impl Hardware<Error = HalError>,
        ip: Ipv4Address,
        prefix: u8,
        gateway: Option<Ipv4Address>,
    ) -> Result<Self, DriverError> {
        let mut device = VirtioNet::new(hal)?;
        let mac = device.mac();

        let clock = NetworkClock::new();
        let mut config = IfaceConfig::new(HardwareAddress::Ethernet(mac));
        config.random_seed = RANDOM_SEED;

        let mut interface = Interface::new(config, &mut device, clock.now());
        interface.update_ip_addrs(|addrs| {
            let cidr = IpCidr::new(IpAddress::from(ip), prefix);
            if addrs.push(cidr).is_err() {
                addrs[0] = cidr;
            }
        });
        if let Some(gw) = gateway {
            let _ = interface.routes_mut().add_default_ipv4_route(gw);
        }

        assert!(
            !SOCKET_STORAGE_IN_USE.swap(true, portable_atomic::Ordering::AcqRel),
            "virtio-net socket storage already initialised"
        );
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
        };
        stack.initialise_socket();
        Ok(stack)
    }

    fn initialise_socket(&mut self) {
        assert!(
            !TCP_RX_STORAGE_IN_USE.swap(true, portable_atomic::Ordering::AcqRel),
            "virtio-net TCP RX storage already initialised"
        );
        assert!(
            !TCP_TX_STORAGE_IN_USE.swap(true, portable_atomic::Ordering::AcqRel),
            "virtio-net TCP TX storage already initialised"
        );
        let rx_buffer = unsafe { TcpSocketBuffer::new(&mut TCP_RX_STORAGE[..]) };
        let tx_buffer = unsafe { TcpSocketBuffer::new(&mut TCP_TX_STORAGE[..]) };
        let tcp_socket = TcpSocket::new(rx_buffer, tx_buffer);
        self.tcp_handle = self.sockets.add(tcp_socket);
    }

    /// Polls the network stack using a host-supplied monotonic timestamp in milliseconds.
    pub fn poll_with_time(&mut self, now_ms: u64) -> bool {
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
        let mut activity = poll_result != PollResult::None;
        if self.process_tcp(now_ms) {
            activity = true;
        }

        self.telemetry.last_poll_ms = now_ms;
        if activity {
            self.telemetry.link_up = true;
        }
        self.telemetry.tx_drops = self.device.tx_drop_count();
        activity
    }

    fn process_tcp(&mut self, now_ms: u64) -> bool {
        let mut activity = false;
        let socket = self.sockets.get_mut::<TcpSocket>(self.tcp_handle);

        if !socket.is_open() {
            let _ = socket.listen(IpListenEndpoint::from(CONSOLE_TCP_PORT));
            if self.session_active {
                self.server.end_session();
                self.session_active = false;
            }
        }

        if socket.state() == TcpState::Established && !self.session_active {
            self.server.begin_session(now_ms);
            self.session_active = true;
        }

        if socket.can_recv() {
            let mut temp = [0u8; 256];
            while socket.can_recv() {
                match socket.recv_slice(&mut temp) {
                    Ok(0) => break,
                    Ok(count) => match self.server.ingest(&temp[..count], now_ms) {
                        SessionEvent::None => {}
                        SessionEvent::Authenticated => {
                            activity = true;
                        }
                        SessionEvent::Close => {
                            socket.close();
                            activity = true;
                            break;
                        }
                    },
                    Err(_) => break,
                }
            }
        }

        if self.session_active && self.server.should_timeout(now_ms) {
            let _ = self.server.enqueue_outbound("ERR CONSOLE reason=timeout");
            socket.close();
            activity = true;
        }

        if socket.can_send() && self.server.is_authenticated() {
            let mut budget = MAX_TX_BUDGET;
            while budget > 0 && socket.can_send() {
                let Some(line) = self.server.pop_outbound() else {
                    break;
                };
                let mut payload: HeaplessVec<u8, { DEFAULT_LINE_CAPACITY + 2 }> =
                    HeaplessVec::new();
                if payload.extend_from_slice(line.as_bytes()).is_err()
                    || payload.extend_from_slice(b"\r\n").is_err()
                {
                    self.server.push_outbound_front(line);
                    self.telemetry.tx_drops = self.telemetry.tx_drops.saturating_add(1);
                    break;
                }
                match socket.send_slice(payload.as_slice()) {
                    Ok(sent) if sent == payload.len() => {
                        self.server.mark_activity(now_ms);
                        activity = true;
                    }
                    Ok(_) | Err(_) => {
                        self.server.push_outbound_front(line);
                        self.telemetry.tx_drops = self.telemetry.tx_drops.saturating_add(1);
                        break;
                    }
                }
                budget -= 1;
            }
        }

        if matches!(socket.state(), TcpState::CloseWait | TcpState::Closed) && self.session_active {
            socket.close();
            self.server.end_session();
            self.session_active = false;
            activity = true;
        }

        activity
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
