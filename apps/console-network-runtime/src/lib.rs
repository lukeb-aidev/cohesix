// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Implement the bounded isolated TCP console and network service state machine.
// Author: Lukas Bower

#![no_std]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Restricted console-network service implementation.

pub use console_network_abi as abi;

use abi::{
    ExchangeKind, RuntimeInitDescriptor, COMMAND_LINE_BYTES, CONSOLE_OUTPUT_BYTES,
    CONSOLE_PAYLOAD_BYTES,
};
use heapless::{Deque, Vec as HeaplessVec};
use smoltcp::iface::{
    Config as InterfaceConfig, Interface, PollResult, SocketHandle, SocketSet, SocketStorage,
};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::socket::tcp::{
    Socket as TcpSocket, SocketBuffer as TcpSocketBuffer, State as TcpState,
};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, HardwareAddress, IpCidr, IpListenEndpoint, Ipv4Address};

const AUTH_PREFIX: &[u8] = b"AUTH ";
const SESSION_EVENT_DEPTH: usize = 8;
const SESSION_OUTPUT_DEPTH: usize = 8;
const FRAME_PREFIX_BYTES: usize = 4;

/// Runtime construction or bounded service error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeError {
    /// Runtime-init validation failed.
    InvalidInit,
    /// The selected static network address is invalid.
    InvalidNetwork,
    /// The sole TCP listener could not bind.
    ListenerBind,
    /// A packet page was empty or exceeded the Ethernet bound.
    PacketBound,
    /// A console frame was malformed or exceeded the payload bound.
    ConsoleFrame,
    /// The bounded service queue was full.
    Backpressure,
    /// Root attempted output before transport authentication.
    Unauthenticated,
    /// The generation has been shut down or revoked.
    Terminal,
}

/// One typed event delivered to root policy over the event page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceEvent {
    kind: ExchangeKind,
    connection_id: u64,
    now_ms: u64,
    payload: HeaplessVec<u8, CONSOLE_PAYLOAD_BYTES>,
}

impl ServiceEvent {
    /// Event kind.
    #[must_use]
    pub const fn kind(&self) -> ExchangeKind {
        self.kind
    }

    /// Exact child-created connection identity.
    #[must_use]
    pub const fn connection_id(&self) -> u64 {
        self.connection_id
    }

    /// Root-supplied monotonic observation time.
    #[must_use]
    pub const fn now_ms(&self) -> u64 {
        self.now_ms
    }

    /// Validated UTF-8 payload when present.
    pub fn payload(&self) -> Result<&str, RuntimeError> {
        core::str::from_utf8(self.payload.as_slice()).map_err(|_| RuntimeError::ConsoleFrame)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AuthState {
    Inactive,
    Waiting,
    Authenticated,
}

/// Bounded transport-authentication and length-prefixed console session.
///
/// This state machine has no command parser or policy authority. It releases a
/// command event only after exact transport authentication; root then performs
/// ticket, role, namespace, and verb authorization.
pub struct TransportSession {
    auth_token: HeaplessVec<u8, { abi::AUTH_TOKEN_BYTES }>,
    auth_timeout_ms: u64,
    idle_timeout_ms: u64,
    state: AuthState,
    connection_id: u64,
    last_activity_ms: u64,
    auth_deadline_ms: u64,
    length_prefix: [u8; FRAME_PREFIX_BYTES],
    length_pos: usize,
    payload_len: Option<usize>,
    payload: HeaplessVec<u8, CONSOLE_PAYLOAD_BYTES>,
    events: Deque<ServiceEvent, SESSION_EVENT_DEPTH>,
    outbound: Deque<HeaplessVec<u8, CONSOLE_OUTPUT_BYTES>, SESSION_OUTPUT_DEPTH>,
    close_after_flush: bool,
    terminal: bool,
}

impl TransportSession {
    /// Construct a fail-closed session from a validated descriptor.
    pub fn new(descriptor: RuntimeInitDescriptor) -> Result<Self, RuntimeError> {
        descriptor
            .validate()
            .map_err(|_| RuntimeError::InvalidInit)?;
        let mut auth_token = HeaplessVec::new();
        auth_token
            .extend_from_slice(descriptor.auth_token())
            .map_err(|_| RuntimeError::InvalidInit)?;
        Ok(Self {
            auth_token,
            auth_timeout_ms: descriptor.auth_timeout_ms as u64,
            idle_timeout_ms: descriptor.idle_timeout_ms as u64,
            state: AuthState::Inactive,
            connection_id: 0,
            last_activity_ms: 0,
            auth_deadline_ms: 0,
            length_prefix: [0; FRAME_PREFIX_BYTES],
            length_pos: 0,
            payload_len: None,
            payload: HeaplessVec::new(),
            events: Deque::new(),
            outbound: Deque::new(),
            close_after_flush: false,
            terminal: false,
        })
    }

    /// Start one child-owned connection generation.
    pub fn begin(&mut self, connection_id: u64, now_ms: u64) -> Result<(), RuntimeError> {
        if self.terminal || connection_id == 0 || self.state != AuthState::Inactive {
            return Err(RuntimeError::Terminal);
        }
        self.connection_id = connection_id;
        self.state = AuthState::Waiting;
        self.last_activity_ms = now_ms;
        self.auth_deadline_ms = now_ms.saturating_add(self.auth_timeout_ms);
        self.reset_frame();
        self.outbound.clear();
        self.close_after_flush = false;
        self.push_event(ExchangeKind::Connected, now_ms, &[])
    }

    /// Consume a bounded TCP byte chunk.
    pub fn ingest(&mut self, bytes: &[u8], now_ms: u64) -> Result<(), RuntimeError> {
        if self.terminal || self.state == AuthState::Inactive {
            return Err(RuntimeError::Terminal);
        }
        for byte in bytes {
            if let Some(expected) = self.payload_len {
                self.payload
                    .push(*byte)
                    .map_err(|_| RuntimeError::ConsoleFrame)?;
                if self.payload.len() == expected {
                    self.last_activity_ms = now_ms;
                    self.finish_frame(now_ms)?;
                    self.reset_frame();
                }
                continue;
            }
            self.length_prefix[self.length_pos] = *byte;
            self.length_pos += 1;
            if self.length_pos == FRAME_PREFIX_BYTES {
                self.length_pos = 0;
                let declared = u32::from_le_bytes(self.length_prefix) as usize;
                if !(FRAME_PREFIX_BYTES..=FRAME_PREFIX_BYTES + CONSOLE_PAYLOAD_BYTES)
                    .contains(&declared)
                {
                    self.reject(now_ms, b"reason=invalid-length")?;
                    return Err(RuntimeError::ConsoleFrame);
                }
                let payload_len = declared - FRAME_PREFIX_BYTES;
                self.payload.clear();
                self.payload_len = Some(payload_len);
                if payload_len == 0 {
                    self.reject(now_ms, b"reason=invalid-length")?;
                    return Err(RuntimeError::ConsoleFrame);
                }
            }
        }
        Ok(())
    }

    /// Queue one root-authorized line without interpreting its ACK/ERR/END body.
    pub fn queue_authorized_line(&mut self, line: &str) -> Result<(), RuntimeError> {
        if self.terminal || self.state != AuthState::Authenticated {
            return Err(RuntimeError::Unauthenticated);
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() || trimmed.len() > CONSOLE_OUTPUT_BYTES {
            return Err(RuntimeError::ConsoleFrame);
        }
        let mut frame = HeaplessVec::new();
        frame
            .extend_from_slice(trimmed.as_bytes())
            .map_err(|_| RuntimeError::ConsoleFrame)?;
        self.outbound
            .push_back(frame)
            .map_err(|_| RuntimeError::Backpressure)
    }

    /// Copy one length-prefixed outgoing frame into `output`.
    pub fn pop_wire_output(&mut self, output: &mut [u8]) -> Result<Option<usize>, RuntimeError> {
        let length = self.peek_wire_output(output)?;
        if length.is_some() {
            self.commit_wire_output()?;
        }
        Ok(length)
    }

    /// Copy, but do not consume, one length-prefixed outgoing frame.
    pub fn peek_wire_output(&self, output: &mut [u8]) -> Result<Option<usize>, RuntimeError> {
        let Some(frame) = self.outbound.front() else {
            return Ok(None);
        };
        let total = FRAME_PREFIX_BYTES.saturating_add(frame.len());
        if output.len() < total {
            return Err(RuntimeError::Backpressure);
        }
        output[..FRAME_PREFIX_BYTES].copy_from_slice(&(total as u32).to_le_bytes());
        output[FRAME_PREFIX_BYTES..total].copy_from_slice(frame.as_slice());
        Ok(Some(total))
    }

    /// Commit the frame most recently copied with [`Self::peek_wire_output`].
    pub fn commit_wire_output(&mut self) -> Result<(), RuntimeError> {
        self.outbound
            .pop_front()
            .map(|_| ())
            .ok_or(RuntimeError::Backpressure)
    }

    /// Pop one typed event for root policy.
    pub fn pop_event(&mut self) -> Option<ServiceEvent> {
        self.events.pop_front()
    }

    /// Request a graceful close after all queued wire output is copied.
    pub fn request_disconnect(&mut self) {
        self.close_after_flush = true;
    }

    /// Enforce authentication and idle deadlines.
    pub fn tick(&mut self, now_ms: u64) -> Result<(), RuntimeError> {
        if self.close_after_flush {
            return Ok(());
        }
        let timed_out = match self.state {
            AuthState::Waiting => now_ms >= self.auth_deadline_ms,
            AuthState::Authenticated => {
                now_ms.saturating_sub(self.last_activity_ms) >= self.idle_timeout_ms
            }
            AuthState::Inactive => false,
        };
        if timed_out {
            self.reject(now_ms, b"reason=timeout")?;
            self.close_after_flush = true;
        }
        Ok(())
    }

    /// Whether the socket should begin its TCP close handshake.
    #[must_use]
    pub fn close_ready(&self) -> bool {
        self.close_after_flush && self.outbound.is_empty()
    }

    /// End the current TCP connection and publish the exact terminal event.
    pub fn end(&mut self, now_ms: u64) -> Result<(), RuntimeError> {
        if self.state != AuthState::Inactive {
            self.push_event(ExchangeKind::Disconnected, now_ms, b"reason=closed")?;
        }
        self.state = AuthState::Inactive;
        self.connection_id = 0;
        self.reset_frame();
        self.outbound.clear();
        self.close_after_flush = false;
        Ok(())
    }

    /// Revoke the complete generation. This state is terminal.
    pub fn revoke(&mut self) {
        self.state = AuthState::Inactive;
        self.connection_id = 0;
        self.reset_frame();
        self.events.clear();
        self.outbound.clear();
        self.close_after_flush = false;
        self.terminal = true;
    }

    /// Whether transport authentication has completed.
    #[must_use]
    pub const fn authenticated(&self) -> bool {
        matches!(self.state, AuthState::Authenticated)
    }

    /// Current child-created connection identity, when a session is live.
    #[must_use]
    pub const fn connection_id(&self) -> Option<u64> {
        if matches!(self.state, AuthState::Inactive) {
            None
        } else {
            Some(self.connection_id)
        }
    }

    /// Whether every root-authorized frame has entered the TCP send queue.
    #[must_use]
    pub fn output_queue_empty(&self) -> bool {
        self.outbound.is_empty()
    }

    fn finish_frame(&mut self, now_ms: u64) -> Result<(), RuntimeError> {
        match self.state {
            AuthState::Waiting => self.authenticate(now_ms),
            AuthState::Authenticated => {
                if self.payload.len() > COMMAND_LINE_BYTES {
                    self.reject(now_ms, b"reason=invalid-command")?;
                    return Err(RuntimeError::ConsoleFrame);
                }
                if core::str::from_utf8(self.payload.as_slice()).is_err() {
                    self.reject(now_ms, b"reason=invalid-utf8")?;
                    return Err(RuntimeError::ConsoleFrame);
                }
                let mut payload = HeaplessVec::new();
                payload
                    .extend_from_slice(self.payload.as_slice())
                    .map_err(|_| RuntimeError::ConsoleFrame)?;
                self.events
                    .push_back(ServiceEvent {
                        kind: ExchangeKind::Command,
                        connection_id: self.connection_id,
                        now_ms,
                        payload,
                    })
                    .map_err(|_| {
                        self.close_after_flush = true;
                        RuntimeError::Backpressure
                    })
            }
            AuthState::Inactive => Err(RuntimeError::Terminal),
        }
    }

    fn authenticate(&mut self, now_ms: u64) -> Result<(), RuntimeError> {
        let payload = self.payload.as_slice();
        let candidate = match payload.strip_prefix(AUTH_PREFIX) {
            Some(candidate) => candidate,
            None => &[],
        };
        let valid = constant_time_equal(candidate, self.auth_token.as_slice());
        if !valid {
            self.reject(now_ms, b"reason=invalid-token")?;
            return Ok(());
        }
        self.state = AuthState::Authenticated;
        self.auth_deadline_ms = 0;
        self.queue_wire_payload(b"OK AUTH")?;
        self.push_event(ExchangeKind::Authenticated, now_ms, &[])
    }

    fn reject(&mut self, now_ms: u64, reason: &[u8]) -> Result<(), RuntimeError> {
        let mut line: HeaplessVec<u8, CONSOLE_OUTPUT_BYTES> = HeaplessVec::new();
        line.extend_from_slice(b"ERR AUTH ")
            .map_err(|_| RuntimeError::ConsoleFrame)?;
        line.extend_from_slice(reason)
            .map_err(|_| RuntimeError::ConsoleFrame)?;
        self.outbound
            .push_back(line)
            .map_err(|_| RuntimeError::Backpressure)?;
        self.push_event(ExchangeKind::Rejected, now_ms, reason)?;
        self.close_after_flush = true;
        Ok(())
    }

    fn queue_wire_payload(&mut self, bytes: &[u8]) -> Result<(), RuntimeError> {
        let mut frame = HeaplessVec::new();
        frame
            .extend_from_slice(bytes)
            .map_err(|_| RuntimeError::ConsoleFrame)?;
        self.outbound
            .push_back(frame)
            .map_err(|_| RuntimeError::Backpressure)
    }

    fn push_event(
        &mut self,
        kind: ExchangeKind,
        now_ms: u64,
        payload: &[u8],
    ) -> Result<(), RuntimeError> {
        let mut bounded = HeaplessVec::new();
        bounded
            .extend_from_slice(payload)
            .map_err(|_| RuntimeError::ConsoleFrame)?;
        self.events
            .push_back(ServiceEvent {
                kind,
                connection_id: self.connection_id,
                now_ms,
                payload: bounded,
            })
            .map_err(|_| RuntimeError::Backpressure)
    }

    fn reset_frame(&mut self) {
        self.length_prefix = [0; FRAME_PREFIX_BYTES];
        self.length_pos = 0;
        self.payload_len = None;
        self.payload.clear();
    }
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    let mut index = 0usize;
    while index < abi::AUTH_TOKEN_BYTES {
        let left_byte = left.get(index).copied().unwrap_or(0);
        let right_byte = right.get(index).copied().unwrap_or(0);
        difference |= usize::from(left_byte ^ right_byte);
        index += 1;
    }
    difference == 0
}

#[derive(Clone, Copy)]
struct EthernetFrame {
    bytes: [u8; abi::ETHERNET_FRAME_BYTES],
    len: usize,
}

impl EthernetFrame {
    const fn empty() -> Self {
        Self {
            bytes: [0; abi::ETHERNET_FRAME_BYTES],
            len: 0,
        }
    }
}

/// Bounded one-packet ingress/egress device used by smoltcp inside the child.
pub struct SharedFrameDevice {
    ingress: Option<EthernetFrame>,
    egress: Option<EthernetFrame>,
    tx_overflow: u64,
}

impl SharedFrameDevice {
    /// Empty service device.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            ingress: None,
            egress: None,
            tx_overflow: 0,
        }
    }

    /// Admit one copied Ethernet frame; backpressure preserves the current one.
    pub fn push_ingress(&mut self, packet: &[u8]) -> Result<(), RuntimeError> {
        if packet.is_empty() || packet.len() > abi::ETHERNET_FRAME_BYTES {
            return Err(RuntimeError::PacketBound);
        }
        if self.ingress.is_some() {
            return Err(RuntimeError::Backpressure);
        }
        let mut frame = EthernetFrame::empty();
        frame.bytes[..packet.len()].copy_from_slice(packet);
        frame.len = packet.len();
        self.ingress = Some(frame);
        Ok(())
    }

    /// Copy one smoltcp-produced Ethernet frame to root's TX page.
    pub fn pop_egress(&mut self, output: &mut [u8]) -> Result<Option<usize>, RuntimeError> {
        let Some(frame) = self.egress else {
            return Ok(None);
        };
        if output.len() < frame.len {
            return Err(RuntimeError::PacketBound);
        }
        output[..frame.len].copy_from_slice(&frame.bytes[..frame.len]);
        self.egress = None;
        Ok(Some(frame.len))
    }

    /// Total oversized transmit requests rejected inside the child.
    #[must_use]
    pub const fn tx_overflow(&self) -> u64 {
        self.tx_overflow
    }
}

impl Default for SharedFrameDevice {
    fn default() -> Self {
        Self::new()
    }
}

/// Owned ingress token for smoltcp.
pub struct SharedRxToken(EthernetFrame);

impl RxToken for SharedRxToken {
    fn consume<R, F>(self, operation: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        operation(&self.0.bytes[..self.0.len])
    }
}

/// Borrowed egress token for smoltcp.
pub struct SharedTxToken<'a>(&'a mut SharedFrameDevice);

impl TxToken for SharedTxToken<'_> {
    fn consume<R, F>(self, len: usize, operation: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        if len > abi::ETHERNET_FRAME_BYTES {
            self.0.tx_overflow = self.0.tx_overflow.saturating_add(1);
            return operation(&mut []);
        }
        let mut frame = EthernetFrame::empty();
        frame.len = len;
        let result = operation(&mut frame.bytes[..len]);
        self.0.egress = Some(frame);
        result
    }
}

impl Device for SharedFrameDevice {
    type RxToken<'a>
        = SharedRxToken
    where
        Self: 'a;
    type TxToken<'a>
        = SharedTxToken<'a>
    where
        Self: 'a;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        if self.egress.is_some() {
            return None;
        }
        let ingress = self.ingress.take()?;
        Some((SharedRxToken(ingress), SharedTxToken(self)))
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        self.egress.is_none().then_some(SharedTxToken(self))
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut capabilities = DeviceCapabilities::default();
        capabilities.medium = Medium::Ethernet;
        capabilities.max_transmission_unit = 1500;
        capabilities
    }
}

/// Operational single-listener smoltcp service owned by the isolated child.
pub struct ConsoleNetworkService<'a> {
    interface: Interface,
    device: SharedFrameDevice,
    sockets: SocketSet<'a>,
    tcp_handle: SocketHandle,
    listener_port: u16,
    session: TransportSession,
    next_connection_id: u64,
    last_tcp_state: TcpState,
    terminal: bool,
}

impl<'a> ConsoleNetworkService<'a> {
    /// Construct the sole static-IPv4 QEMU listener from sealed runtime state.
    pub fn new(
        descriptor: RuntimeInitDescriptor,
        tcp_rx: &'a mut [u8],
        tcp_tx: &'a mut [u8],
        socket_storage: &'a mut [SocketStorage<'a>],
    ) -> Result<Self, RuntimeError> {
        descriptor
            .validate()
            .map_err(|_| RuntimeError::InvalidInit)?;
        if socket_storage.len() != 1 || tcp_rx.len() < 1024 || tcp_tx.len() < 1024 {
            return Err(RuntimeError::InvalidInit);
        }
        let address = Ipv4Address::from(descriptor.ipv4);
        if address.is_unspecified() || descriptor.prefix_len == 0 {
            return Err(RuntimeError::InvalidNetwork);
        }
        let mut device = SharedFrameDevice::new();
        let mut interface_config =
            InterfaceConfig::new(HardwareAddress::Ethernet(EthernetAddress(descriptor.mac)));
        interface_config.random_seed = descriptor.generation;
        let mut interface = Interface::new(interface_config, &mut device, Instant::from_millis(0));
        let mut address_installed = false;
        interface.update_ip_addrs(|addresses| {
            addresses.clear();
            address_installed = addresses
                .push(IpCidr::new(address.into(), descriptor.prefix_len))
                .is_ok();
        });
        if !address_installed {
            return Err(RuntimeError::InvalidNetwork);
        }
        if descriptor.gateway != [0; 4] {
            interface
                .routes_mut()
                .add_default_ipv4_route(Ipv4Address::from(descriptor.gateway))
                .map_err(|_| RuntimeError::InvalidNetwork)?;
        }
        let mut sockets = SocketSet::new(socket_storage);
        let mut tcp = TcpSocket::new(TcpSocketBuffer::new(tcp_rx), TcpSocketBuffer::new(tcp_tx));
        tcp.listen(IpListenEndpoint::from(descriptor.listener_port))
            .map_err(|_| RuntimeError::ListenerBind)?;
        let tcp_handle = sockets.add(tcp);
        Ok(Self {
            interface,
            device,
            sockets,
            tcp_handle,
            listener_port: descriptor.listener_port,
            session: TransportSession::new(descriptor)?,
            next_connection_id: 1,
            last_tcp_state: TcpState::Listen,
            terminal: false,
        })
    }

    /// Copy one driver-delivered frame into the child-owned smoltcp ingress.
    pub fn ingest_packet(&mut self, packet: &[u8]) -> Result<(), RuntimeError> {
        if self.terminal {
            return Err(RuntimeError::Terminal);
        }
        self.device.push_ingress(packet)
    }

    /// Copy one smoltcp egress frame for the admitted NIC transport.
    pub fn take_packet(&mut self, output: &mut [u8]) -> Result<Option<usize>, RuntimeError> {
        self.device.pop_egress(output)
    }

    /// Apply one root-authorized output control.
    pub fn apply_control(&mut self, kind: ExchangeKind, payload: &str) -> Result<(), RuntimeError> {
        if self.terminal {
            return Err(RuntimeError::Terminal);
        }
        match kind {
            ExchangeKind::SendLine => self.session.queue_authorized_line(payload),
            ExchangeKind::Disconnect => {
                if !payload.is_empty() {
                    return Err(RuntimeError::ConsoleFrame);
                }
                self.session.request_disconnect();
                Ok(())
            }
            _ => Err(RuntimeError::ConsoleFrame),
        }
    }

    /// Run one bounded smoltcp/TCP/authentication service turn.
    pub fn poll(&mut self, now_ms: u64) -> Result<bool, RuntimeError> {
        if self.terminal {
            return Err(RuntimeError::Terminal);
        }
        let timestamp = Instant::from_millis(now_ms.min(i64::MAX as u64) as i64);
        let mut activity = matches!(
            self.interface
                .poll(timestamp, &mut self.device, &mut self.sockets),
            PollResult::SocketStateChanged
        );
        let state = self.sockets.get::<TcpSocket>(self.tcp_handle).state();
        if state == TcpState::Established && self.last_tcp_state == TcpState::Listen {
            let connection_id = self.next_connection_id;
            self.next_connection_id = self.next_connection_id.saturating_add(1).max(1);
            self.session.begin(connection_id, now_ms)?;
            activity = true;
        }

        let mut chunk = [0u8; CONSOLE_PAYLOAD_BYTES + FRAME_PREFIX_BYTES];
        let received = {
            let socket = self.sockets.get_mut::<TcpSocket>(self.tcp_handle);
            if socket.can_recv() {
                match socket.recv_slice(&mut chunk) {
                    Ok(bytes) => bytes,
                    Err(_) => return Err(RuntimeError::Terminal),
                }
            } else {
                0
            }
        };
        if received != 0 {
            match self.session.ingest(&chunk[..received], now_ms) {
                Ok(()) | Err(RuntimeError::ConsoleFrame) => activity = true,
                Err(error) => return Err(error),
            }
        }
        self.session.tick(now_ms)?;

        let mut output = [0u8; CONSOLE_PAYLOAD_BYTES + FRAME_PREFIX_BYTES];
        if let Some(length) = self.session.peek_wire_output(&mut output)? {
            let available = {
                let socket = self.sockets.get::<TcpSocket>(self.tcp_handle);
                socket.send_capacity().saturating_sub(socket.send_queue())
            };
            if available >= length {
                let sent = match self
                    .sockets
                    .get_mut::<TcpSocket>(self.tcp_handle)
                    .send_slice(&output[..length])
                {
                    Ok(bytes) => bytes,
                    Err(_) => return Err(RuntimeError::Backpressure),
                };
                if sent != length {
                    return Err(RuntimeError::Backpressure);
                }
                self.session.commit_wire_output()?;
                activity = true;
            }
        }
        if self.session.close_ready() {
            self.sockets.get_mut::<TcpSocket>(self.tcp_handle).close();
            activity = true;
        }

        let current = self.sockets.get::<TcpSocket>(self.tcp_handle).state();
        if current == TcpState::Closed && self.last_tcp_state != TcpState::Closed {
            self.session.end(now_ms)?;
            self.sockets
                .get_mut::<TcpSocket>(self.tcp_handle)
                .listen(IpListenEndpoint::from(self.listener_port))
                .map_err(|_| RuntimeError::ListenerBind)?;
            activity = true;
        }
        self.last_tcp_state = self.sockets.get::<TcpSocket>(self.tcp_handle).state();
        Ok(activity)
    }

    /// Pop one authenticated transport event for root policy.
    pub fn pop_event(&mut self) -> Option<ServiceEvent> {
        self.session.pop_event()
    }

    /// Connection whose root-authorized bytes have fully left smoltcp's send queue.
    #[must_use]
    pub fn output_drained_connection(&self) -> Option<u64> {
        let connection_id = self.session.connection_id()?;
        let socket = self.sockets.get::<TcpSocket>(self.tcp_handle);
        (self.session.output_queue_empty() && socket.send_queue() == 0).then_some(connection_id)
    }

    /// Revoke all old-generation state and stop service permanently.
    pub fn revoke(&mut self) {
        self.sockets.get_mut::<TcpSocket>(self.tcp_handle).abort();
        self.session.revoke();
        self.terminal = true;
    }

    /// Whether exactly one socket remains in LISTEN state.
    #[must_use]
    pub fn listener_ready(&self) -> bool {
        self.sockets.get::<TcpSocket>(self.tcp_handle).state() == TcpState::Listen
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use core::mem::size_of;

    use super::*;
    use abi::{
        RuntimeInitDescriptor, ABI_VERSION, AUTH_TOKEN_BYTES, CHILD_CSPACE_SLOTS, CHILD_WAKE_MASK,
        CHILD_WAKE_NOTIFICATION_SLOT, ETHERNET_FRAME_BYTES, FAULT_ENDPOINT_SLOT,
        PACKET_TX_WAKE_NOTIFICATION_SLOT, REQUIRED_INIT_FLAGS, ROOT_WAKE_MASK, RUNTIME_INIT_MAGIC,
        SHARED_PAGE_BYTES, SUPERVISOR_WAKE_NOTIFICATION_SLOT,
    };

    fn descriptor() -> RuntimeInitDescriptor {
        let mut token = [0; AUTH_TOKEN_BYTES];
        token[..6].copy_from_slice(b"secret");
        RuntimeInitDescriptor {
            magic: RUNTIME_INIT_MAGIC,
            abi_version: ABI_VERSION,
            descriptor_bytes: size_of::<RuntimeInitDescriptor>() as u16,
            flags: REQUIRED_INIT_FLAGS,
            reserved0: 0,
            generation: 7,
            child_wake_notification_slot: CHILD_WAKE_NOTIFICATION_SLOT,
            packet_tx_wake_notification_slot: PACKET_TX_WAKE_NOTIFICATION_SLOT,
            supervisor_wake_notification_slot: SUPERVISOR_WAKE_NOTIFICATION_SLOT,
            fault_endpoint_slot: FAULT_ENDPOINT_SLOT,
            child_cspace_slots: CHILD_CSPACE_SLOTS,
            root_wake_mask: ROOT_WAKE_MASK,
            child_wake_mask: CHILD_WAKE_MASK,
            ipc_buffer_vaddr: 0x7200_0000,
            packet_rx_vaddr: 0x7202_0000,
            packet_tx_vaddr: 0x7202_1000,
            command_vaddr: 0x7202_2000,
            event_vaddr: 0x7202_3000,
            shared_frame_bytes: SHARED_PAGE_BYTES as u32,
            ethernet_frame_bytes: ETHERNET_FRAME_BYTES as u16,
            listener_port: 31_337,
            max_packets_per_wake: 1,
            max_commands_per_wake: 1,
            max_control_inflight: 1,
            prefix_len: 24,
            auth_token_len: 6,
            reserved1: 0,
            mac: [2, 0, 0, 0, 0, 1],
            ipv4: [10, 0, 2, 15],
            gateway: [10, 0, 2, 2],
            reserved2: [0; 2],
            auth_timeout_ms: 5000,
            idle_timeout_ms: 300_000,
            timer_clock_hz: 62_500_000,
            auth_token: token,
            seal: 0,
        }
        .sealed()
    }

    fn framed(payload: &[u8]) -> std::vec::Vec<u8> {
        let mut frame = (payload.len() as u32 + 4).to_le_bytes().to_vec();
        frame.extend_from_slice(payload);
        frame
    }

    #[test]
    fn authentication_and_command_release_are_child_owned() {
        let mut session = TransportSession::new(descriptor()).unwrap();
        session.begin(1, 10).unwrap();
        assert_eq!(session.pop_event().unwrap().kind(), ExchangeKind::Connected);
        session.ingest(&framed(b"AUTH secret"), 20).unwrap();
        assert!(session.authenticated());
        assert_eq!(
            session.pop_event().unwrap().kind(),
            ExchangeKind::Authenticated
        );
        let mut wire = [0u8; 64];
        let len = session.pop_wire_output(&mut wire).unwrap().unwrap();
        assert_eq!(&wire[4..len], b"OK AUTH");
        session.ingest(&framed(b"cat /proc/boot"), 30).unwrap();
        let command = session.pop_event().unwrap();
        assert_eq!(command.kind(), ExchangeKind::Command);
        assert_eq!(command.payload().unwrap(), "cat /proc/boot");
    }

    #[test]
    fn malformed_auth_and_partial_frame_fail_closed() {
        let mut session = TransportSession::new(descriptor()).unwrap();
        session.begin(1, 0).unwrap();
        let _ = session.pop_event();
        session.ingest(&framed(b"AUTH wrong"), 1).unwrap();
        assert_eq!(session.pop_event().unwrap().kind(), ExchangeKind::Rejected);
        assert!(!session.close_ready());
        let mut wire = [0u8; 64];
        let len = session.pop_wire_output(&mut wire).unwrap().unwrap();
        assert_eq!(&wire[4..len], b"ERR AUTH reason=invalid-token");
        assert!(session.close_ready());
    }

    #[test]
    fn authentication_comparison_scans_the_fixed_token_bound() {
        assert!(constant_time_equal(b"secret", b"secret"));
        assert!(!constant_time_equal(b"secreu", b"secret"));
        assert!(!constant_time_equal(b"secret-extra", b"secret"));
        assert!(!constant_time_equal(b"", b"secret"));
    }

    #[test]
    fn authenticated_command_bound_matches_root_parser_capacity() {
        let mut session = TransportSession::new(descriptor()).unwrap();
        session.begin(1, 0).unwrap();
        let _ = session.pop_event();
        session.ingest(&framed(b"AUTH secret"), 1).unwrap();
        let _ = session.pop_event();

        let oversized = [b'x'; abi::COMMAND_LINE_BYTES + 1];
        assert_eq!(
            session.ingest(&framed(&oversized), 2),
            Err(RuntimeError::ConsoleFrame)
        );
        let rejected = session.pop_event().unwrap();
        assert_eq!(rejected.kind(), ExchangeKind::Rejected);
        assert_eq!(rejected.payload().unwrap(), "reason=invalid-command");
    }

    #[test]
    fn root_output_preserves_ack_err_end_bytes_and_auth_gate() {
        let mut session = TransportSession::new(descriptor()).unwrap();
        assert_eq!(
            session.queue_authorized_line("OK CAT bytes=3"),
            Err(RuntimeError::Unauthenticated)
        );
        session.begin(1, 0).unwrap();
        let _ = session.pop_event();
        session.ingest(&framed(b"AUTH secret"), 1).unwrap();
        let _ = session.pop_event();
        let mut discard = [0u8; 64];
        let _ = session.pop_wire_output(&mut discard).unwrap();
        for line in ["OK CAT bytes=3", "ERR CAT denied", "END CAT"] {
            session.queue_authorized_line(line).unwrap();
            let len = session.pop_wire_output(&mut discard).unwrap().unwrap();
            assert_eq!(&discard[4..len], line.as_bytes());
        }
    }

    #[test]
    fn wire_output_is_retained_until_tcp_accepts_the_complete_frame() {
        let mut session = TransportSession::new(descriptor()).unwrap();
        session.begin(1, 0).unwrap();
        let _ = session.pop_event();
        session.ingest(&framed(b"AUTH secret"), 1).unwrap();
        let _ = session.pop_event();

        let mut first = [0u8; 64];
        let first_len = session.peek_wire_output(&mut first).unwrap().unwrap();
        let mut second = [0u8; 64];
        let second_len = session.peek_wire_output(&mut second).unwrap().unwrap();
        assert_eq!(first_len, second_len);
        assert_eq!(&first[..first_len], &second[..second_len]);
        session.commit_wire_output().unwrap();
        assert_eq!(session.peek_wire_output(&mut second).unwrap(), None);
    }

    #[test]
    fn one_packet_device_backpressures_without_overwrite() {
        let mut device = SharedFrameDevice::new();
        device.push_ingress(&[1, 2, 3]).unwrap();
        assert_eq!(device.push_ingress(&[4]), Err(RuntimeError::Backpressure));
        let timestamp = Instant::from_millis(0);
        let (rx, tx) = device.receive(timestamp).unwrap();
        assert_eq!(rx.consume(|packet| packet.to_vec()), [1, 2, 3]);
        tx.consume(3, |packet| packet.copy_from_slice(&[4, 5, 6]));
        let mut output = [0u8; 8];
        assert_eq!(device.pop_egress(&mut output).unwrap(), Some(3));
        assert_eq!(&output[..3], &[4, 5, 6]);
    }

    #[test]
    fn operational_smoltcp_service_has_exactly_one_listener() {
        let mut rx = [0u8; 4096];
        let mut tx = [0u8; 4096];
        let mut storage = [SocketStorage::EMPTY];
        let service =
            ConsoleNetworkService::new(descriptor(), &mut rx, &mut tx, &mut storage).unwrap();
        assert!(service.listener_ready());
    }
}
