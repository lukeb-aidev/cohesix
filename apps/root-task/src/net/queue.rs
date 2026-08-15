// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Host-side mock network queue and TCP console stack used in tests.
// Author: Lukas Bower

#![cfg(not(feature = "kernel"))]

extern crate alloc;
use alloc::boxed::Box;
use core::fmt;
use core::sync::atomic::Ordering;

use heapless::{spsc::Queue, String as HeaplessString, Vec as HeaplessVec};
use portable_atomic::{AtomicU32, AtomicU64};
use smoltcp::iface::{Config as IfaceConfig, Interface, PollResult, SocketSet, SocketStorage};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::socket::raw::{
    PacketBuffer as RawPacketBuffer, PacketMetadata as RawPacketMetadata,
    RecvError as RawRecvError, SendError as RawSendError, Socket as RawSocket,
};
use smoltcp::time::Instant;
use smoltcp::wire::{
    EthernetAddress, HardwareAddress, IpAddress, IpCidr, IpProtocol, IpVersion, Ipv4Address,
};

use super::{
    console_auth_token, console_srv::TcpConsoleServer, parse_icmp_echo_request, ConsoleLine,
    NetPoller, NetTelemetry, IDLE_TIMEOUT_MS, MAX_FRAME_LEN,
};
use crate::observe::IngestSnapshot;
use crate::serial::DEFAULT_LINE_CAPACITY;

/// Number of frames retained in the RX ring buffer.
pub const RX_QUEUE_DEPTH: usize = 16;

/// Number of frames retained in the TX ring buffer.
pub const TX_QUEUE_DEPTH: usize = 16;

/// Number of sockets provisioned for the interface.
pub const SOCKET_CAPACITY: usize = 4;
const ICMP_ECHO_REPLY_DEADLINE_MS: u64 = 3_000;

/// Errors surfaced by the networking substrate.
#[derive(Debug, PartialEq, Eq)]
pub enum NetError {
    /// The RX queue is full and the frame cannot be enqueued.
    RxQueueFull,
    /// The TX queue is full and the frame cannot be enqueued.
    TxQueueFull,
    /// Frame exceeded the configured MTU.
    FrameTooLarge,
}

impl fmt::Display for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RxQueueFull => write!(f, "rx queue is full"),
            Self::TxQueueFull => write!(f, "tx queue is full"),
            Self::FrameTooLarge => write!(f, "frame exceeds MTU"),
        }
    }
}

/// Fixed-capacity frame used by the heapless queues.
#[derive(Clone, Debug, Default)]
pub struct Frame(HeaplessVec<u8, MAX_FRAME_LEN>);

impl Frame {
    /// Create a new empty frame.
    #[must_use]
    pub fn new() -> Self {
        Self(HeaplessVec::new())
    }

    /// Allocate a frame from the provided payload.
    pub fn from_slice(data: &[u8]) -> Result<Self, NetError> {
        let mut frame = Self::new();
        frame
            .0
            .extend_from_slice(data)
            .map_err(|_| NetError::FrameTooLarge)?;
        Ok(frame)
    }

    fn resize(&mut self, len: usize) -> Result<(), NetError> {
        self.0.resize(len, 0).map_err(|_| NetError::FrameTooLarge)
    }

    fn as_mut_slice(&mut self) -> &mut [u8] {
        self.0.as_mut_slice()
    }

    /// Obtain an immutable view of the frame bytes.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }

    /// Consume the frame and return the owned heapless buffer.
    #[must_use]
    pub fn into_inner(self) -> HeaplessVec<u8, MAX_FRAME_LEN> {
        self.0
    }
}

type RxQueue = Queue<Frame, RX_QUEUE_DEPTH>;
type TxQueue = Queue<Frame, TX_QUEUE_DEPTH>;

/// Shared handle to a queue for tests and diagnostics.
#[derive(Clone, Debug)]
pub struct QueueHandle {
    rx: &'static spin::Mutex<RxQueue>,
    tx: &'static spin::Mutex<TxQueue>,
    tx_drops: &'static AtomicU32,
}

impl QueueHandle {
    fn new(
        rx: &'static spin::Mutex<RxQueue>,
        tx: &'static spin::Mutex<TxQueue>,
        tx_drops: &'static AtomicU32,
    ) -> Self {
        Self { rx, tx, tx_drops }
    }

    /// Inject a frame into the RX queue.
    pub fn push_rx(&self, frame: Frame) -> Result<(), NetError> {
        let mut guard = self.rx.lock();
        guard.enqueue(frame).map_err(|_| NetError::RxQueueFull)
    }

    /// Drain a single frame from the TX queue.
    pub fn pop_tx(&self) -> Option<Frame> {
        let mut guard = self.tx.lock();
        guard.dequeue()
    }

    /// Number of frames dropped due to a saturated TX queue.
    #[must_use]
    pub fn tx_drops(&self) -> u32 {
        self.tx_drops.load(Ordering::Relaxed)
    }

    /// Reset the underlying queues and drop counters, emulating a PHY reset.
    pub fn reset(&self) {
        {
            let mut rx = self.rx.lock();
            while rx.dequeue().is_some() {}
        }
        {
            let mut tx = self.tx.lock();
            while tx.dequeue().is_some() {}
        }
        self.tx_drops.store(0, Ordering::Relaxed);
    }
}

/// PHY implementation backed by bounded heapless queues.
#[derive(Debug)]
struct QueuePhy {
    rx: &'static spin::Mutex<RxQueue>,
    tx: &'static spin::Mutex<TxQueue>,
    tx_drops: &'static AtomicU32,
}

impl QueuePhy {
    fn new() -> (Self, QueueHandle) {
        let rx = Box::leak(Box::new(spin::Mutex::new(Queue::new())));
        let tx = Box::leak(Box::new(spin::Mutex::new(Queue::new())));
        let tx_drops = Box::leak(Box::new(AtomicU32::new(0)));

        let phy = Self { rx, tx, tx_drops };
        let handle = QueueHandle::new(phy.rx, phy.tx, phy.tx_drops);
        (phy, handle)
    }

    fn try_enqueue_tx(&self, frame: Frame) -> Result<(), NetError> {
        let mut guard = self.tx.lock();
        guard.enqueue(frame).map_err(|_| {
            self.tx_drops.fetch_add(1, Ordering::Relaxed);
            NetError::TxQueueFull
        })
    }

    fn tx_drop_count(&self) -> u32 {
        self.tx_drops.load(Ordering::Relaxed)
    }

    fn reset(&self) {
        {
            let mut rx = self.rx.lock();
            while rx.dequeue().is_some() {}
        }
        {
            let mut tx = self.tx.lock();
            while tx.dequeue().is_some() {}
        }
        self.tx_drops.store(0, Ordering::Relaxed);
    }
}

impl Device for QueuePhy {
    type RxToken<'a>
        = QueueRxToken
    where
        Self: 'a;
    type TxToken<'a>
        = QueueTxToken<'a>
    where
        Self: 'a;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let frame = {
            let mut guard = self.rx.lock();
            guard.dequeue()
        }?;
        Some((QueueRxToken { frame }, QueueTxToken { phy: self }))
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        Some(QueueTxToken { phy: self })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = MAX_FRAME_LEN;
        caps.medium = Medium::Ethernet;
        caps
    }
}

/// RX token exposing frames to smoltcp.
pub struct QueueRxToken {
    frame: Frame,
}

impl RxToken for QueueRxToken {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        f(self.frame.as_slice())
    }
}

/// TX token used by the queue-backed PHY.
pub struct QueueTxToken<'a> {
    phy: &'a QueuePhy,
}

impl<'a> TxToken for QueueTxToken<'a> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut frame = Frame::new();
        frame.resize(len).expect("frame larger than MTU");
        let result = f(frame.as_mut_slice());
        let _ = self.phy.try_enqueue_tx(frame);
        result
    }
}

/// Shared monotonic clock for the interface.
#[derive(Debug, Default)]
pub struct NetworkClock {
    ticks_ms: AtomicU64,
}

impl NetworkClock {
    /// Create a new clock initialised to zero.
    #[must_use]
    pub fn new() -> Self {
        Self {
            ticks_ms: AtomicU64::new(0),
        }
    }

    /// Advance the clock by `delta_ms` and return the resulting instant.
    pub fn advance(&self, delta_ms: u32) -> Instant {
        let delta = u64::from(delta_ms);
        let updated = self
            .ticks_ms
            .fetch_add(delta, Ordering::Relaxed)
            .saturating_add(delta);
        let millis = i64::try_from(updated).unwrap_or(i64::MAX);
        Instant::from_millis(millis)
    }

    /// Return the current instant without mutating the clock.
    #[must_use]
    pub fn now(&self) -> Instant {
        let current = self.ticks_ms.load(Ordering::Relaxed);
        let millis = i64::try_from(current).unwrap_or(i64::MAX);
        Instant::from_millis(millis)
    }
}

/// Network interface wrapper combining smoltcp with the bounded PHY.
pub struct NetStack {
    clock: NetworkClock,
    device: QueuePhy,
    interface: Interface,
    sockets: SocketSet<'static>,
    icmp_echo_handle: smoltcp::iface::SocketHandle,
    icmp_echo_pending_since_ms: Option<u64>,
    ip: Ipv4Address,
    hardware_addr: EthernetAddress,
    telemetry: NetTelemetry,
    server: TcpConsoleServer,
    session_active: bool,
}

impl NetStack {
    fn new_icmp_echo_socket_set() -> (SocketSet<'static>, smoltcp::iface::SocketHandle) {
        let socket_storage = Box::leak(Box::new([SocketStorage::EMPTY; SOCKET_CAPACITY]));
        let rx_metadata = Box::leak(Box::new([RawPacketMetadata::EMPTY; 2]));
        let tx_metadata = Box::leak(Box::new([RawPacketMetadata::EMPTY; 1]));
        let rx_storage = Box::leak(Box::new([0u8; MAX_FRAME_LEN * 2]));
        let tx_storage = Box::leak(Box::new([0u8; MAX_FRAME_LEN]));
        let rx_buffer = RawPacketBuffer::new(&mut rx_metadata[..], &mut rx_storage[..]);
        let tx_buffer = RawPacketBuffer::new(&mut tx_metadata[..], &mut tx_storage[..]);
        let socket = RawSocket::new(
            Some(IpVersion::Ipv4),
            Some(IpProtocol::Icmp),
            rx_buffer,
            tx_buffer,
        );
        let mut sockets = SocketSet::new(&mut socket_storage[..]);
        let handle = sockets.add(socket);
        (sockets, handle)
    }

    /// Construct a new stack configured with the supplied IPv4 address.
    pub fn new(ip: Ipv4Address) -> (Self, QueueHandle) {
        let (mut device, handle) = QueuePhy::new();
        let clock = NetworkClock::new();
        let mac = EthernetAddress::from_bytes(&[0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
        let mut config = IfaceConfig::new(HardwareAddress::Ethernet(mac));
        config.random_seed = 0x5a5a_5a5a_1234_5678;

        let mut interface = Interface::new(config, &mut device, clock.now());
        interface.update_ip_addrs(|addrs| {
            if addrs.push(IpCidr::new(IpAddress::from(ip), 24)).is_err() {
                addrs[0] = IpCidr::new(IpAddress::from(ip), 24);
            }
        });
        let (sockets, icmp_echo_handle) = Self::new_icmp_echo_socket_set();

        let stack = Self {
            clock,
            device,
            interface,
            sockets,
            icmp_echo_handle,
            icmp_echo_pending_since_ms: None,
            ip,
            hardware_addr: mac,
            telemetry: NetTelemetry::default(),
            server: TcpConsoleServer::new(console_auth_token(), IDLE_TIMEOUT_MS),
            session_active: false,
        };
        (stack, handle)
    }

    /// Poll the interface using the supplied wall-clock timestamp in milliseconds.
    pub fn poll_with_time(&mut self, now_ms: u64) -> bool {
        let last = self.telemetry.last_poll_ms;
        let delta = now_ms.saturating_sub(last);
        let delta_ms = core::cmp::min(delta, u64::from(u32::MAX)) as u32;
        let timestamp = if delta_ms == 0 {
            self.clock.now()
        } else {
            self.clock.advance(delta_ms)
        };

        let mut activity = self.expire_icmp_echo_if_due(now_ms);
        let changed = self
            .interface
            .poll(timestamp, &mut self.device, &mut self.sockets);
        activity |= changed != PollResult::None;
        activity |= self.service_icmp_echo(now_ms);

        if self.flush_outbound_lines() {
            activity = true;
        }

        self.telemetry.last_poll_ms = now_ms;
        if activity || now_ms > 0 {
            self.telemetry.link_up = true;
        }
        self.telemetry.tx_drops = self.device.tx_drop_count();
        activity
    }

    fn reset_icmp_echo_socket(&mut self) {
        let (sockets, handle) = Self::new_icmp_echo_socket_set();
        self.sockets = sockets;
        self.icmp_echo_handle = handle;
        self.icmp_echo_pending_since_ms = None;
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
            self.reset_icmp_echo_socket();
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
        }
        if rx_bytes == 0 {
            return false;
        }
        let mut packet = [0u8; MAX_FRAME_LEN];
        let packet_len = match self
            .sockets
            .get_mut::<RawSocket>(self.icmp_echo_handle)
            .recv_slice(&mut packet)
        {
            Ok(packet_len) => packet_len,
            Err(RawRecvError::Exhausted) => return false,
            Err(RawRecvError::Truncated) => return true,
        };
        if tx_bytes != 0 {
            return true;
        }
        let request = match parse_icmp_echo_request(&packet[..packet_len], self.ip) {
            Ok(request) => request,
            Err(_) => return true,
        };
        let reply_len = request.reply_len();
        let emit_result = match self
            .sockets
            .get_mut::<RawSocket>(self.icmp_echo_handle)
            .send(reply_len)
        {
            Ok(output) => request.emit_reply(self.ip, output),
            Err(RawSendError::BufferFull) => return true,
        };
        if emit_result.is_err() {
            self.reset_icmp_echo_socket();
            return true;
        }
        self.icmp_echo_pending_since_ms = Some(now_ms);
        true
    }

    fn flush_outbound_lines(&mut self) -> bool {
        let mut activity = false;
        let pre_auth = !self.server.is_authenticated();
        while let Some(line) = self.server.pop_outbound() {
            if pre_auth && !TcpConsoleServer::is_preauth_transmit_allowed(line.as_str()) {
                self.server.push_outbound_front(line);
                break;
            }
            let trimmed = line.trim_end_matches(['\r', '\n']);
            let mut payload: HeaplessVec<u8, { DEFAULT_LINE_CAPACITY + 4 }> = HeaplessVec::new();
            if Self::encode_frame(trimmed, &mut payload).is_err() {
                self.telemetry.tx_drops = self.telemetry.tx_drops.saturating_add(1);
                continue;
            }

            match Frame::from_slice(payload.as_slice()) {
                Ok(frame) => match self.device.try_enqueue_tx(frame) {
                    Ok(()) => {
                        activity = true;
                    }
                    Err(_) => {
                        self.server.push_outbound_front(line);
                        break;
                    }
                },
                Err(_) => {
                    self.server.push_outbound_front(line);
                    self.telemetry.tx_drops = self.telemetry.tx_drops.saturating_add(1);
                }
            }
        }

        activity
    }

    /// Expose the configured hardware address.
    #[must_use]
    pub fn hardware_address(&self) -> EthernetAddress {
        self.hardware_addr
    }

    /// Retrieve telemetry captured during the most recent poll.
    #[must_use]
    pub fn telemetry(&self) -> NetTelemetry {
        self.telemetry
    }

    /// Access the underlying queue handle for diagnostics or tests.
    #[must_use]
    pub fn queue_handle(&self) -> QueueHandle {
        QueueHandle::new(self.device.rx, self.device.tx, self.device.tx_drops)
    }

    /// Inject a line into the TCP console loopback queue (test/support helper).
    pub fn enqueue_console_line(&mut self, line: &str) {
        if !self.session_active {
            self.server.begin_session(0, None);
            let mut auth_payload: HeaplessVec<u8, { DEFAULT_LINE_CAPACITY + 12 }> =
                HeaplessVec::new();
            let mut auth_line = HeaplessString::<DEFAULT_LINE_CAPACITY>::new();
            let _ = auth_line.push_str("AUTH ");
            let _ = auth_line.push_str(console_auth_token());
            let _ = Self::encode_frame(auth_line.as_str(), &mut auth_payload);
            let _ = self.server.ingest(auth_payload.as_slice(), 0);
            self.session_active = true;
        }

        let trimmed = line.trim_end_matches(['\r', '\n']);
        let mut payload: HeaplessVec<u8, { DEFAULT_LINE_CAPACITY + 8 }> = HeaplessVec::new();
        if Self::encode_frame(trimmed, &mut payload).is_err() {
            return;
        }
        let _ = self.server.ingest(payload.as_slice(), 1);
    }

    fn encode_frame<const N: usize>(
        line: &str,
        payload: &mut HeaplessVec<u8, N>,
    ) -> Result<(), ()> {
        let total_len = line.len().saturating_add(4);
        let len: u32 = total_len.try_into().map_err(|_| ())?;
        payload
            .extend_from_slice(&len.to_le_bytes())
            .map_err(|_| ())?;
        payload.extend_from_slice(line.as_bytes()).map_err(|_| ())?;
        Ok(())
    }

    /// Reset the queue-backed PHY and clear console session state.
    pub fn force_reset(&mut self) {
        self.device.reset();
        self.reset_icmp_echo_socket();
        self.server.end_session();
        self.session_active = false;
        self.telemetry = NetTelemetry::default();
    }
}

impl NetPoller for NetStack {
    fn poll(&mut self, now_ms: u64) -> bool {
        self.poll_with_time(now_ms)
    }

    fn driver_task_contract(&self) -> crate::hal::driver_task::DriverTaskContract {
        crate::hal::driver_task::VIRTIO_NET_DRIVER_TASK_CONTRACT
    }

    fn telemetry(&self) -> NetTelemetry {
        self.telemetry()
    }

    fn drain_console_lines(&mut self, now_ms: u64, visitor: &mut dyn FnMut(ConsoleLine)) {
        let _ = self
            .server
            .drain_console_lines_bounded(now_ms, usize::MAX, visitor);
    }

    fn drain_console_lines_bounded(
        &mut self,
        now_ms: u64,
        max_lines: usize,
        visitor: &mut dyn FnMut(ConsoleLine),
    ) -> usize {
        self.server
            .drain_console_lines_bounded(now_ms, max_lines, visitor)
    }

    fn ingest_snapshot(&self) -> IngestSnapshot {
        self.server.ingest_snapshot()
    }

    fn send_console_line(&mut self, line: &str) -> bool {
        if self.server.enqueue_outbound(line).is_err() {
            self.telemetry.tx_drops = self.telemetry.tx_drops.saturating_add(1);
            return false;
        }
        true
    }

    fn icmp_echo_service_due(&self, now_ms: u64) -> bool {
        let socket = self.sockets.get::<RawSocket>(self.icmp_echo_handle);
        socket.recv_queue() != 0
            || socket.send_queue() != 0
            || self.icmp_echo_pending_since_ms.is_some_and(|started| {
                now_ms.saturating_sub(started) >= ICMP_ECHO_REPLY_DEADLINE_MS
            })
    }

    fn inject_console_line(&mut self, line: &str) {
        self.enqueue_console_line(line);
    }

    fn reset(&mut self) {
        self.force_reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smoltcp::phy::ChecksumCapabilities;
    use smoltcp::wire::{
        ArpOperation, ArpPacket, ArpRepr, EthernetFrame, EthernetProtocol, EthernetRepr,
        Icmpv4Packet, Icmpv4Repr, IpProtocol, Ipv4Address, Ipv4Packet, Ipv4Repr,
    };

    fn frame_line<const N: usize>(line: &str) -> HeaplessVec<u8, N> {
        let mut buf = HeaplessVec::new();
        let total_len = line.len().saturating_add(4);
        let len: u32 = total_len.try_into().unwrap_or(u32::MAX);
        let _ = buf.extend_from_slice(&len.to_le_bytes());
        let _ = buf.extend_from_slice(line.as_bytes());
        buf
    }

    fn decode_frames(frame: &[u8]) -> heapless::Vec<heapless::String<96>, 4> {
        let mut lines = heapless::Vec::new();
        let mut offset = 0usize;
        while offset + 4 <= frame.len() {
            let mut len_buf = [0u8; 4];
            len_buf.copy_from_slice(&frame[offset..offset + 4]);
            let total_len = u32::from_le_bytes(len_buf) as usize;
            if total_len < 4 || offset + total_len > frame.len() {
                break;
            }
            let payload = &frame[offset + 4..offset + total_len];
            if let Ok(text) = core::str::from_utf8(payload) {
                let mut line = heapless::String::new();
                let _ = line.push_str(text);
                let _ = lines.push(line);
            }
            offset = offset.saturating_add(total_len);
        }
        lines
    }

    #[test]
    fn queue_overflow_increments_drop_counter() {
        let (stack, handle) = NetStack::new(Ipv4Address::new(10, 0, 2, 15));
        let mut overflow_triggered = false;
        for _ in 0..=TX_QUEUE_DEPTH {
            let frame = Frame::from_slice(&[0u8; 60]).unwrap();
            if let Err(err) = stack.device.try_enqueue_tx(frame) {
                assert_eq!(err, NetError::TxQueueFull);
                overflow_triggered = true;
                break;
            }
        }
        assert!(overflow_triggered, "queue did not report saturation");
        assert!(handle.tx_drops() > 0);
    }

    #[test]
    fn poll_advances_clock() {
        let (mut stack, handle) = NetStack::new(Ipv4Address::new(10, 0, 2, 42));
        assert_eq!(handle.tx_drops(), 0);
        stack.poll_with_time(10);
        stack.poll_with_time(25);
        assert!(stack.clock.now() >= Instant::from_millis(15));
    }

    #[test]
    fn telemetry_updates_after_poll() {
        let (mut stack, handle) = NetStack::new(Ipv4Address::new(10, 0, 2, 7));
        assert_eq!(stack.telemetry().last_poll_ms, 0);
        stack.poll_with_time(25);
        let telemetry = stack.telemetry();
        assert!(telemetry.link_up);
        assert_eq!(telemetry.last_poll_ms, 25);
        assert_eq!(telemetry.tx_drops, handle.tx_drops());
    }

    fn echo_request_frame(
        local_mac: EthernetAddress,
        local_ip: Ipv4Address,
        peer_mac: EthernetAddress,
        peer_ip: Ipv4Address,
        ident: u16,
        sequence: u16,
        data: &[u8],
    ) -> Frame {
        let ethernet_repr = EthernetRepr {
            src_addr: peer_mac,
            dst_addr: local_mac,
            ethertype: EthernetProtocol::Ipv4,
        };
        let icmp_repr = Icmpv4Repr::EchoRequest {
            ident,
            seq_no: sequence,
            data,
        };
        let ipv4_repr = Ipv4Repr {
            src_addr: peer_ip,
            dst_addr: local_ip,
            next_header: IpProtocol::Icmp,
            payload_len: icmp_repr.buffer_len(),
            hop_limit: 64,
        };
        let ethernet_len = ethernet_repr.buffer_len();
        let ipv4_len = ipv4_repr.buffer_len();
        let frame_len = ethernet_len + ipv4_len + icmp_repr.buffer_len();
        let mut request = [0u8; 64];

        ethernet_repr.emit(&mut EthernetFrame::new_unchecked(&mut request[..frame_len]));
        ipv4_repr.emit(
            &mut Ipv4Packet::new_unchecked(&mut request[ethernet_len..frame_len]),
            &ChecksumCapabilities::default(),
        );
        icmp_repr.emit(
            &mut Icmpv4Packet::new_unchecked(&mut request[ethernet_len + ipv4_len..frame_len]),
            &ChecksumCapabilities::default(),
        );
        Frame::from_slice(&request[..frame_len]).expect("valid echo request frame")
    }

    fn arp_frame(
        operation: ArpOperation,
        local_mac: EthernetAddress,
        local_ip: Ipv4Address,
        peer_mac: EthernetAddress,
        peer_ip: Ipv4Address,
    ) -> Frame {
        let ethernet_repr = EthernetRepr {
            src_addr: peer_mac,
            dst_addr: if operation == ArpOperation::Request {
                EthernetAddress::BROADCAST
            } else {
                local_mac
            },
            ethertype: EthernetProtocol::Arp,
        };
        let arp_repr = ArpRepr::EthernetIpv4 {
            operation,
            source_hardware_addr: peer_mac,
            source_protocol_addr: peer_ip,
            target_hardware_addr: if operation == ArpOperation::Request {
                EthernetAddress::from_bytes(&[0; 6])
            } else {
                local_mac
            },
            target_protocol_addr: local_ip,
        };
        let frame_len = ethernet_repr.buffer_len() + arp_repr.buffer_len();
        let mut frame = [0u8; 64];
        ethernet_repr.emit(&mut EthernetFrame::new_unchecked(&mut frame[..frame_len]));
        arp_repr.emit(&mut ArpPacket::new_unchecked(
            &mut frame[ethernet_repr.buffer_len()..frame_len],
        ));
        Frame::from_slice(&frame[..frame_len]).expect("valid ARP frame")
    }

    fn assert_echo_reply(
        reply: Frame,
        local_mac: EthernetAddress,
        local_ip: Ipv4Address,
        peer_mac: EthernetAddress,
        peer_ip: Ipv4Address,
        ident: u16,
        sequence: u16,
        data: &[u8],
    ) {
        let ethernet_packet =
            EthernetFrame::new_checked(reply.as_slice()).expect("valid reply Ethernet frame");
        let reply_ethernet =
            EthernetRepr::parse(&ethernet_packet).expect("valid reply Ethernet header");
        assert_eq!(reply_ethernet.src_addr, local_mac);
        assert_eq!(reply_ethernet.dst_addr, peer_mac);
        assert_eq!(reply_ethernet.ethertype, EthernetProtocol::Ipv4);

        let ipv4_packet =
            Ipv4Packet::new_checked(ethernet_packet.payload()).expect("valid reply IPv4 packet");
        let reply_ipv4 = Ipv4Repr::parse(&ipv4_packet, &ChecksumCapabilities::default())
            .expect("valid reply IPv4 header");
        assert_eq!(reply_ipv4.src_addr, local_ip);
        assert_eq!(reply_ipv4.dst_addr, peer_ip);
        assert_eq!(reply_ipv4.next_header, IpProtocol::Icmp);

        let icmp_packet =
            Icmpv4Packet::new_checked(ipv4_packet.payload()).expect("valid reply ICMP packet");
        let reply_icmp = Icmpv4Repr::parse(&icmp_packet, &ChecksumCapabilities::default())
            .expect("valid reply ICMP representation");
        assert_eq!(
            reply_icmp,
            Icmpv4Repr::EchoReply {
                ident,
                seq_no: sequence,
                data,
            }
        );
    }

    #[test]
    fn cold_neighbor_icmp_echo_reply_survives_arp_without_host_retry() {
        let local_ip = Ipv4Address::new(10, 0, 2, 15);
        let peer_ip = Ipv4Address::new(10, 0, 2, 2);
        let wrong_peer_ip = Ipv4Address::new(10, 0, 2, 3);
        let peer_mac = EthernetAddress::from_bytes(&[0x02, 0x00, 0x00, 0x00, 0x00, 0x02]);
        let wrong_peer_mac = EthernetAddress::from_bytes(&[0x02, 0x00, 0x00, 0x00, 0x00, 0x03]);
        let echo_data = [0xaa, 0x00, 0x00, 0xff];
        let (mut stack, handle) = NetStack::new(local_ip);
        let local_mac = stack.hardware_address();

        handle
            .push_rx(echo_request_frame(
                local_mac, local_ip, peer_mac, peer_ip, 0x1234, 0xabcd, &echo_data,
            ))
            .expect("RX queue accepts first echo request");
        assert!(
            stack.poll_with_time(1),
            "first request must enter raw socket"
        );
        assert!(
            handle.pop_tx().is_none(),
            "reply construction must not perform a second hardware poll"
        );
        assert!(stack.icmp_echo_service_due(1));

        let _ = stack.poll_with_time(2);
        let arp_request = handle.pop_tx().expect("cold neighbor ARP request missing");
        let arp_ethernet =
            EthernetFrame::new_checked(arp_request.as_slice()).expect("valid ARP Ethernet frame");
        assert_eq!(arp_ethernet.ethertype(), EthernetProtocol::Arp);
        let arp_packet =
            ArpPacket::new_checked(arp_ethernet.payload()).expect("valid ARP request packet");
        assert_eq!(
            ArpRepr::parse(&arp_packet).expect("valid ARP request representation"),
            ArpRepr::EthernetIpv4 {
                operation: ArpOperation::Request,
                source_hardware_addr: local_mac,
                source_protocol_addr: local_ip,
                target_hardware_addr: EthernetAddress::BROADCAST,
                target_protocol_addr: peer_ip,
            }
        );

        handle
            .push_rx(arp_frame(
                ArpOperation::Reply,
                local_mac,
                local_ip,
                wrong_peer_mac,
                wrong_peer_ip,
            ))
            .expect("RX queue accepts unrelated ARP reply");
        handle
            .push_rx(echo_request_frame(
                local_mac, local_ip, peer_mac, peer_ip, 0x1234, 0xabce, &echo_data,
            ))
            .expect("RX queue accepts bounded competing echo");
        assert!(stack.poll_with_time(3));
        assert!(
            handle.pop_tx().is_none(),
            "wrong-peer ARP and a second echo must not release or replace the first reply"
        );

        handle
            .push_rx(arp_frame(
                ArpOperation::Reply,
                local_mac,
                local_ip,
                peer_mac,
                peer_ip,
            ))
            .expect("RX queue accepts matching ARP reply");
        assert!(stack.poll_with_time(4));
        let reply = match handle.pop_tx() {
            Some(reply) => reply,
            None => {
                assert!(stack.poll_with_time(5));
                handle.pop_tx().expect("retained ICMP echo reply missing")
            }
        };
        assert_echo_reply(
            reply, local_mac, local_ip, peer_mac, peer_ip, 0x1234, 0xabcd, &echo_data,
        );
        assert!(
            handle.pop_tx().is_none(),
            "reply must be emitted exactly once"
        );
        let _ = stack.poll_with_time(6);
        assert!(
            handle.pop_tx().is_none(),
            "later polls must not duplicate reply"
        );
        assert_eq!(
            stack
                .sockets
                .get::<RawSocket>(stack.icmp_echo_handle)
                .send_queue(),
            0
        );
    }

    #[test]
    fn warm_neighbor_icmp_echo_emits_exactly_one_reply() {
        let local_ip = Ipv4Address::new(10, 0, 2, 15);
        let peer_ip = Ipv4Address::new(10, 0, 2, 2);
        let peer_mac = EthernetAddress::from_bytes(&[0x02, 0x00, 0x00, 0x00, 0x00, 0x02]);
        let echo_data = [1, 2, 3, 4];
        let (mut stack, handle) = NetStack::new(local_ip);
        let local_mac = stack.hardware_address();

        handle
            .push_rx(arp_frame(
                ArpOperation::Request,
                local_mac,
                local_ip,
                peer_mac,
                peer_ip,
            ))
            .expect("RX queue accepts ARP warmup");
        assert!(stack.poll_with_time(1));
        assert_eq!(
            EthernetFrame::new_checked(handle.pop_tx().expect("ARP reply missing").as_slice())
                .expect("valid ARP reply")
                .ethertype(),
            EthernetProtocol::Arp
        );

        handle
            .push_rx(echo_request_frame(
                local_mac, local_ip, peer_mac, peer_ip, 7, 9, &echo_data,
            ))
            .expect("RX queue accepts echo");
        assert!(stack.poll_with_time(2));
        assert!(handle.pop_tx().is_none());
        assert!(stack.poll_with_time(3));
        assert_echo_reply(
            handle.pop_tx().expect("warm echo reply missing"),
            local_mac,
            local_ip,
            peer_mac,
            peer_ip,
            7,
            9,
            &echo_data,
        );
        let _ = stack.poll_with_time(4);
        assert!(handle.pop_tx().is_none(), "warm reply must not duplicate");
    }

    #[test]
    fn icmp_echo_reset_and_expiry_drop_stale_pending_reply() {
        let local_ip = Ipv4Address::new(10, 0, 2, 15);
        let peer_ip = Ipv4Address::new(10, 0, 2, 2);
        let expiry_peer_ip = Ipv4Address::new(10, 0, 2, 4);
        let peer_mac = EthernetAddress::from_bytes(&[0x02, 0x00, 0x00, 0x00, 0x00, 0x02]);
        let expiry_peer_mac = EthernetAddress::from_bytes(&[0x02, 0x00, 0x00, 0x00, 0x00, 0x04]);
        let (mut stack, handle) = NetStack::new(local_ip);
        let local_mac = stack.hardware_address();
        let request = echo_request_frame(local_mac, local_ip, peer_mac, peer_ip, 1, 0, &[0x5a]);

        handle.push_rx(request.clone()).expect("RX accepts echo");
        assert!(stack.poll_with_time(1));
        assert!(stack.icmp_echo_service_due(1));
        stack.force_reset();
        handle
            .push_rx(arp_frame(
                ArpOperation::Reply,
                local_mac,
                local_ip,
                peer_mac,
                peer_ip,
            ))
            .expect("RX accepts post-reset ARP");
        let _ = stack.poll_with_time(2);
        assert!(
            handle.pop_tx().is_none(),
            "reset must purge old-lifetime reply"
        );

        handle
            .push_rx(echo_request_frame(
                local_mac,
                local_ip,
                expiry_peer_mac,
                expiry_peer_ip,
                2,
                0,
                &[0x5a],
            ))
            .expect("RX accepts second echo");
        assert!(stack.poll_with_time(3));
        let _ = stack.poll_with_time(4);
        assert_eq!(
            EthernetFrame::new_checked(handle.pop_tx().expect("ARP request missing").as_slice())
                .expect("valid ARP frame")
                .ethertype(),
            EthernetProtocol::Arp
        );
        assert!(stack.poll_with_time(3u64.saturating_add(ICMP_ECHO_REPLY_DEADLINE_MS)));
        handle
            .push_rx(arp_frame(
                ArpOperation::Reply,
                local_mac,
                local_ip,
                expiry_peer_mac,
                expiry_peer_ip,
            ))
            .expect("RX accepts late ARP");
        let _ = stack.poll_with_time(4u64.saturating_add(ICMP_ECHO_REPLY_DEADLINE_MS));
        assert!(handle.pop_tx().is_none(), "expired reply must not escape");
    }

    #[test]
    fn icmp_echo_rejects_nonlocal_invalid_source_and_bad_checksum() {
        let local_ip = Ipv4Address::new(10, 0, 2, 15);
        let peer_ip = Ipv4Address::new(10, 0, 2, 2);
        let peer_mac = EthernetAddress::from_bytes(&[0x02, 0x00, 0x00, 0x00, 0x00, 0x02]);
        let (mut stack, handle) = NetStack::new(local_ip);
        let local_mac = stack.hardware_address();

        handle
            .push_rx(echo_request_frame(
                local_mac,
                Ipv4Address::new(10, 0, 2, 99),
                peer_mac,
                peer_ip,
                1,
                1,
                &[1],
            ))
            .expect("RX accepts nonlocal frame");
        handle
            .push_rx(echo_request_frame(
                local_mac,
                local_ip,
                peer_mac,
                Ipv4Address::BROADCAST,
                2,
                2,
                &[2],
            ))
            .expect("RX accepts invalid-source frame");
        let mut bad_checksum =
            echo_request_frame(local_mac, local_ip, peer_mac, peer_ip, 3, 3, &[3]);
        let final_byte = bad_checksum.0.len().saturating_sub(1);
        bad_checksum.0[final_byte] ^= 0xff;
        handle
            .push_rx(bad_checksum)
            .expect("RX accepts bad-checksum frame");

        let _ = stack.poll_with_time(1);
        let _ = stack.poll_with_time(2);
        let _ = stack.poll_with_time(3);
        let _ = stack.poll_with_time(4);
        assert!(
            handle.pop_tx().is_none(),
            "rejected raw packets must not create ARP or Echo Reply traffic"
        );
    }

    #[test]
    fn console_line_queue_drains_fifo() {
        let (mut stack, _) = NetStack::new(Ipv4Address::new(10, 0, 2, 99));
        stack.enqueue_console_line("attach queen token\r\n");
        stack.enqueue_console_line("log\n");
        let mut observed: heapless::Vec<HeaplessString<{ cohsh_core::MAX_LINE_LEN }>, 4> =
            heapless::Vec::new();
        stack.drain_console_lines(10, &mut |line| {
            observed.push(line.text).unwrap();
        });
        assert_eq!(observed.len(), 2);
        assert_eq!(observed[0].as_str(), "attach queen token");
        assert_eq!(observed[1].as_str(), "log");
    }

    #[test]
    fn outbound_lines_are_enqueued_only_after_authentication() {
        use super::super::console_srv::SessionEvent;

        let (mut stack, handle) = NetStack::new(Ipv4Address::new(10, 0, 2, 100));
        stack.server.begin_session(0, None);
        stack.session_active = true;
        stack.send_console_line("OK TEST detail=42");
        assert!(handle.pop_tx().is_none(), "frames should be queued on poll");

        assert!(
            !stack.poll_with_time(1),
            "lines must not transmit before authentication"
        );

        let auth_payload =
            frame_line::<{ DEFAULT_LINE_CAPACITY + 8 }>(&format!("AUTH {}", console_auth_token()));
        let event = stack.server.ingest(auth_payload.as_slice(), 1);
        assert_eq!(event, SessionEvent::Authenticated);

        assert!(stack.poll_with_time(2));

        let first = handle.pop_tx().expect("first frame missing");
        let second = handle.pop_tx().expect("second frame missing");
        let mut observed: heapless::Vec<heapless::String<96>, 8> = heapless::Vec::new();
        for line in decode_frames(first.as_slice()) {
            let _ = observed.push(line);
        }
        for line in decode_frames(second.as_slice()) {
            let _ = observed.push(line);
        }
        assert!(observed
            .iter()
            .any(|line| line.as_str() == "OK TEST detail=42"));
        assert!(observed.iter().any(|line| line.as_str() == "OK AUTH"));
    }

    #[test]
    fn auth_failures_flush_error_before_closing() {
        use super::super::console_srv::SessionEvent;

        let (mut stack, handle) = NetStack::new(Ipv4Address::new(10, 0, 2, 150));
        stack.server.begin_session(0, None);
        stack.session_active = true;
        let auth_payload = frame_line::<{ DEFAULT_LINE_CAPACITY + 8 }>("AUTH wrong");
        let event = stack.server.ingest(auth_payload.as_slice(), 1);
        assert!(matches!(event, SessionEvent::AuthFailed(_)));

        assert!(stack.poll_with_time(1));

        let frame = handle.pop_tx().expect("auth failure frame missing");
        let lines = decode_frames(frame.as_slice());
        assert!(lines
            .iter()
            .any(|line| line.as_str().starts_with("ERR AUTH")));
    }
}
