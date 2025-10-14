// Author: Lukas Bower

#![cfg(not(target_os = "none"))]

extern crate alloc;
use core::fmt;
use core::sync::atomic::Ordering;

use heapless::{spsc::Queue, Deque, String as HeaplessString, Vec as HeaplessVec};
use portable_atomic::{AtomicU32, AtomicU64};
use smoltcp::iface::{Config as IfaceConfig, Interface, SocketSet, SocketStorage};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, HardwareAddress, IpAddress, IpCidr, Ipv4Address};

use super::{NetPoller, NetTelemetry, CONSOLE_QUEUE_DEPTH, MAX_FRAME_LEN};
use crate::serial::DEFAULT_LINE_CAPACITY;

/// Number of frames retained in the RX ring buffer.
pub const RX_QUEUE_DEPTH: usize = 16;

/// Number of frames retained in the TX ring buffer.
pub const TX_QUEUE_DEPTH: usize = 16;

/// Number of sockets provisioned for the interface.
pub const SOCKET_CAPACITY: usize = 4;

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
    fn consume<R, F>(mut self, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        f(self.frame.as_mut_slice())
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
    hardware_addr: EthernetAddress,
    telemetry: NetTelemetry,
    console_lines: Deque<HeaplessString<DEFAULT_LINE_CAPACITY>, CONSOLE_QUEUE_DEPTH>,
    outbound_lines: Deque<HeaplessString<DEFAULT_LINE_CAPACITY>, CONSOLE_QUEUE_DEPTH>,
}

impl NetStack {
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

        let stack = Self {
            clock,
            device,
            interface,
            hardware_addr: mac,
            telemetry: NetTelemetry::default(),
            console_lines: Deque::new(),
            outbound_lines: Deque::new(),
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

        let device = &mut self.device;
        let interface = &mut self.interface;
        let storage: &mut [SocketStorage<'static>] = &mut [];
        let mut sockets = SocketSet::new(storage);
        let changed = interface.poll(timestamp, device, &mut sockets);
        let mut activity = changed;

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

    fn flush_outbound_lines(&mut self) -> bool {
        let mut activity = false;
        while let Some(line) = self.outbound_lines.pop_front() {
            let mut payload: HeaplessVec<u8, { DEFAULT_LINE_CAPACITY + 2 }> = HeaplessVec::new();
            if payload.extend_from_slice(line.as_bytes()).is_err() {
                self.telemetry.tx_drops = self.telemetry.tx_drops.saturating_add(1);
                continue;
            }
            if payload.push(b'\n').is_err() {
                self.telemetry.tx_drops = self.telemetry.tx_drops.saturating_add(1);
                continue;
            }

            match Frame::from_slice(payload.as_slice()) {
                Ok(frame) => match self.device.try_enqueue_tx(frame) {
                    Ok(()) => {
                        activity = true;
                    }
                    Err(_) => {
                        let _ = self.outbound_lines.push_front(line);
                        break;
                    }
                },
                Err(_) => {
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
        let mut buf: HeaplessString<DEFAULT_LINE_CAPACITY> = HeaplessString::new();
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if buf.push_str(trimmed).is_err() {
            return;
        }
        let _ = self.console_lines.push_back(buf);
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
        while let Some(line) = self.console_lines.pop_front() {
            visitor(line);
        }
    }

    fn send_console_line(&mut self, line: &str) {
        let mut buf: HeaplessString<DEFAULT_LINE_CAPACITY> = HeaplessString::new();
        if buf.push_str(line).is_err() {
            self.telemetry.tx_drops = self.telemetry.tx_drops.saturating_add(1);
            return;
        }
        match self.outbound_lines.push_back(buf) {
            Ok(()) => {}
            Err(buf) => {
                let _ = self.outbound_lines.pop_front();
                let _ = self.outbound_lines.push_back(buf);
                self.telemetry.tx_drops = self.telemetry.tx_drops.saturating_add(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smoltcp::wire::Ipv4Address;

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

    #[test]
    fn console_line_queue_drains_fifo() {
        let (mut stack, _) = NetStack::new(Ipv4Address::new(10, 0, 2, 99));
        stack.enqueue_console_line("attach queen token\r\n");
        stack.enqueue_console_line("log\n");
        let mut observed: heapless::Vec<HeaplessString<DEFAULT_LINE_CAPACITY>, 4> =
            heapless::Vec::new();
        stack.drain_console_lines(&mut |line| {
            observed.push(line).unwrap();
        });
        assert_eq!(observed.len(), 2);
        assert_eq!(observed[0].as_str(), "attach queen token");
        assert_eq!(observed[1].as_str(), "log");
    }

    #[test]
    fn outbound_lines_are_enqueued_as_frames() {
        let (mut stack, handle) = NetStack::new(Ipv4Address::new(10, 0, 2, 100));
        stack.send_console_line("OK TEST detail=42");
        assert!(handle.pop_tx().is_none(), "frames should be queued on poll");

        assert!(stack.poll_with_time(1));

        let frame = handle.pop_tx().expect("frame not enqueued");
        let rendered = core::str::from_utf8(frame.as_slice()).expect("frame not utf8");
        assert_eq!(rendered, "OK TEST detail=42\n");
    }
}
