// Author: Lukas Bower
// Purpose: Provide bounded serial console primitives for the root task and host simulations.

//! Minimal, no-std friendly serial console primitives used by the root task.
//!
//! The implementation favours bounded, heapless queues so the console can be
//! integrated in both seL4 builds and host-mode simulations. The core
//! responsibilities are:
//!
//! - Pumping bytes between the underlying MMIO/driver implementation and
//!   heapless staging buffers without allocation.
//! - Maintaining back-pressure counters so the event pump can surface
//!   saturation diagnostics via `/proc/boot`.
//! - Sanitising incoming UTF-8 before forwarding to the command parser. The
//!   serial line discipline is intentionally conservative and currently limits
//!   input to ASCII so deterministic behaviour can be verified in tests.

use core::fmt;

use embedded_io::{Error as EmbeddedError, ErrorKind, ErrorType};
use heapless::{spsc::Queue, String as HeaplessString};
use nb::Error as NbError;
use portable_atomic::AtomicU32;
#[cfg(feature = "kernel")]
use portable_atomic::{AtomicU64, Ordering as AtomicOrdering};

#[cfg(feature = "kernel")]
pub mod pl011;
pub mod virtio;

#[cfg(feature = "kernel")]
/// Emit a string to the seL4 debug console using [`crate::sel4::debug_put_char`].
pub fn puts(message: &str) {
    for &byte in message.as_bytes() {
        crate::sel4::debug_put_char(i32::from(byte));
    }
}

#[cfg(not(feature = "kernel"))]
/// Host-mode stub used when the seL4 debug console is unavailable.
#[allow(dead_code)]
pub fn puts(_message: &str) {}

#[cfg(feature = "kernel")]
/// Emit a message at most once, keyed by the pointer to the `&'static str`.
pub fn puts_once(message: &'static str) {
    static SEEN: AtomicU64 = AtomicU64::new(0);

    let ptr = message.as_ptr() as usize;
    let index = ((ptr >> 3) & 63) as u32;
    let mask = 1u64 << index;
    let prev = SEEN.fetch_or(mask, AtomicOrdering::Relaxed);
    if prev & mask == 0 {
        puts(message);
    }
}

#[cfg(not(feature = "kernel"))]
/// Host-mode stub used when the seL4 debug console is unavailable.
#[allow(dead_code)]
pub fn puts_once(_message: &'static str) {}

/// Capacity of the RX staging queue used by [`SerialPort`].
pub const DEFAULT_RX_CAPACITY: usize = 512;

/// Capacity of the TX staging queue used by [`SerialPort`].
pub const DEFAULT_TX_CAPACITY: usize = 256;

/// Maximum number of UTF-8 codepoints retained in a console line.
pub const DEFAULT_LINE_CAPACITY: usize = 192;

/// Error type surfaced by the serial subsystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SerialError {
    /// Serial device reported an unrecoverable failure.
    DeviceFault,
    /// Attempted to enqueue more data than the console permits.
    LineTooLong,
}

impl fmt::Display for SerialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeviceFault => write!(f, "serial device fault"),
            Self::LineTooLong => write!(f, "serial line exceeded maximum length"),
        }
    }
}

impl core::error::Error for SerialError {}

impl EmbeddedError for SerialError {
    fn kind(&self) -> ErrorKind {
        ErrorKind::Other
    }
}

/// Lightweight trait abstracting the MMIO-backed console device.
pub trait SerialDriver: ErrorType {
    /// Attempt to read a single byte from the device.
    fn read_byte(&mut self) -> nb::Result<u8, Self::Error>;

    /// Attempt to write a single byte to the device.
    fn write_byte(&mut self, byte: u8) -> nb::Result<(), Self::Error>;
}

/// Metrics reported by the serial subsystem for observability.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SerialTelemetry {
    /// Number of times the RX queue saturated and dropped a byte.
    pub rx_backpressure: u32,
    /// Number of times the TX queue saturated and dropped a byte.
    pub tx_backpressure: u32,
    /// Number of bytes dropped because they could not be encoded as UTF-8.
    pub utf8_dropped: u32,
}

/// Serial console abstraction with bounded RX/TX queues and UTF-8 sanitisation.
pub struct SerialPort<
    D,
    const RX: usize = DEFAULT_RX_CAPACITY,
    const TX: usize = DEFAULT_TX_CAPACITY,
    const LINE: usize = DEFAULT_LINE_CAPACITY,
> where
    D: SerialDriver,
{
    driver: D,
    rx: Queue<u8, RX>,
    tx: Queue<u8, TX>,
    line: HeaplessString<LINE>,
    pending_tx: Option<u8>,
    telemetry: SerialTelemetryCounters,
    echo: bool,
    suppress_lf: bool,
}

impl<D, const RX: usize, const TX: usize, const LINE: usize> SerialPort<D, RX, TX, LINE>
where
    D: SerialDriver,
{
    /// Construct a new serial port backed by the supplied driver.
    pub fn new(driver: D) -> Self {
        Self {
            driver,
            rx: Queue::new(),
            tx: Queue::new(),
            line: HeaplessString::new(),
            pending_tx: None,
            telemetry: SerialTelemetryCounters::default(),
            echo: true,
            suppress_lf: false,
        }
    }

    /// Access the underlying telemetry snapshot.
    #[must_use]
    pub fn telemetry(&self) -> SerialTelemetry {
        self.telemetry.snapshot()
    }

    /// Inject data that should be transmitted to the remote peer.
    pub fn enqueue_tx(&mut self, data: &[u8]) {
        for &byte in data {
            let mut attempts = 0usize;
            while self.tx.enqueue(byte).is_err() {
                self.telemetry.tx_overflow();
                self.flush_tx();
                attempts = attempts.saturating_add(1);
                if attempts > TX {
                    break;
                }
            }
        }
    }

    /// Attempt to move data between the driver and staging buffers.
    pub fn poll_io(&mut self) {
        // Drain RX side first so newly available bytes can be processed in the
        // same cycle.
        loop {
            match self.driver.read_byte() {
                Ok(byte) => {
                    if self.rx.enqueue(byte).is_err() {
                        self.telemetry.rx_overflow();
                    }
                }
                Err(NbError::WouldBlock) => break,
                Err(NbError::Other(_)) => {
                    self.telemetry.rx_overflow();
                    break;
                }
            }
        }

        self.flush_tx();
    }

    fn flush_tx(&mut self) {
        // Flush staged TX bytes to the device until it reports back-pressure.
        if let Some(byte) = self.pending_tx.take() {
            match self.driver.write_byte(byte) {
                Ok(()) => {}
                Err(NbError::WouldBlock) => {
                    self.pending_tx = Some(byte);
                    return;
                }
                Err(NbError::Other(_)) => {
                    self.telemetry.tx_overflow();
                    return;
                }
            }
        }

        loop {
            let Some(byte) = self.tx.dequeue() else { break };
            match self.driver.write_byte(byte) {
                Ok(()) => {}
                Err(NbError::WouldBlock) => {
                    self.pending_tx = Some(byte);
                    return;
                }
                Err(NbError::Other(_)) => {
                    self.telemetry.tx_overflow();
                    return;
                }
            }
        }
    }

    /// Retrieve the next sanitised console line, if available.
    pub fn next_line(&mut self) -> Option<HeaplessString<LINE>> {
        while let Some(byte) = self.rx.dequeue() {
            if self.suppress_lf && byte == b'\n' {
                self.suppress_lf = false;
                continue;
            }
            match byte {
                b'\r' => {
                    self.suppress_lf = true;
                    self.emit_newline();
                    let mut completed = HeaplessString::new();
                    core::mem::swap(&mut completed, &mut self.line);
                    return Some(completed);
                }
                b'\n' => {
                    self.emit_newline();
                    let mut completed = HeaplessString::new();
                    core::mem::swap(&mut completed, &mut self.line);
                    return Some(completed);
                }
                0x08 | 0x7f => {
                    if self.line.pop().is_some() {
                        if self.echo {
                            self.enqueue_tx(b"\x08 \x08");
                        }
                    }
                }
                byte if byte.is_ascii_control() => {
                    self.telemetry.utf8_drop();
                }
                byte => {
                    if self.line.push(byte as char).is_err() {
                        self.telemetry.utf8_drop();
                        continue;
                    }
                    if self.echo {
                        self.enqueue_tx(&[byte]);
                    }
                }
            }
        }
        None
    }

    fn emit_newline(&mut self) {
        if self.echo {
            self.enqueue_tx(b"\r\n");
        }
    }

    /// Access the driver mutably (used by tests for inspection).
    pub fn driver_mut(&mut self) -> &mut D {
        &mut self.driver
    }
}

/// Internal telemetry counters backed by atomics so interrupt handlers can
/// update statistics without locks.
#[derive(Debug, Default)]
struct SerialTelemetryCounters {
    rx_backpressure: AtomicU32,
    tx_backpressure: AtomicU32,
    utf8_dropped: AtomicU32,
}

impl SerialTelemetryCounters {
    fn snapshot(&self) -> SerialTelemetry {
        SerialTelemetry {
            rx_backpressure: self
                .rx_backpressure
                .load(core::sync::atomic::Ordering::Relaxed),
            tx_backpressure: self
                .tx_backpressure
                .load(core::sync::atomic::Ordering::Relaxed),
            utf8_dropped: self
                .utf8_dropped
                .load(core::sync::atomic::Ordering::Relaxed),
        }
    }

    fn rx_overflow(&self) {
        self.rx_backpressure
            .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    }

    fn tx_overflow(&self) {
        self.tx_backpressure
            .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    }

    fn utf8_drop(&self) {
        self.utf8_dropped
            .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    }
}

/// Serial driver used by tests to emulate asynchronous RX/TX behaviour.
#[cfg(any(test, not(feature = "kernel")))]
pub mod test_support {
    use super::*;
    use core::cell::RefCell;

    /// In-memory serial stub backed by heapless queues.
    pub struct LoopbackSerial<const CAP: usize = 512> {
        pub(crate) rx: RefCell<Queue<u8, CAP>>,
        pub(crate) tx: RefCell<Queue<u8, CAP>>,
    }

    impl<const CAP: usize> Default for LoopbackSerial<CAP> {
        fn default() -> Self {
            Self::new()
        }
    }

    impl<const CAP: usize> LoopbackSerial<CAP> {
        /// Create a new loopback serial driver.
        pub fn new() -> Self {
            Self {
                rx: RefCell::new(Queue::new()),
                tx: RefCell::new(Queue::new()),
            }
        }

        /// Inject bytes that should be observed by the serial port on the next poll.
        pub fn push_rx(&self, data: &[u8]) {
            let mut guard = self.rx.borrow_mut();
            for &byte in data {
                let _ = guard.enqueue(byte);
            }
        }

        /// Drain bytes that have been emitted by the serial port.
        pub fn drain_tx(&self) -> heapless::Vec<u8, CAP> {
            let mut guard = self.tx.borrow_mut();
            let mut out = heapless::Vec::new();
            while let Some(byte) = guard.dequeue() {
                let _ = out.push(byte);
            }
            out
        }
    }

    impl<const CAP: usize> ErrorType for LoopbackSerial<CAP> {
        type Error = SerialError;
    }

    impl<const CAP: usize> SerialDriver for LoopbackSerial<CAP> {
        fn read_byte(&mut self) -> nb::Result<u8, Self::Error> {
            let mut guard = self.rx.borrow_mut();
            guard.dequeue().ok_or(NbError::WouldBlock)
        }

        fn write_byte(&mut self, byte: u8) -> nb::Result<(), Self::Error> {
            let mut guard = self.tx.borrow_mut();
            guard.enqueue(byte).map_err(|_| NbError::WouldBlock)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::LoopbackSerial;
    use super::*;

    #[test]
    fn utf8_sanitisation_drops_control_bytes() {
        let driver = LoopbackSerial::<8>::new();
        let mut port: SerialPort<_> = SerialPort::new(driver);
        port.driver_mut().push_rx(&[0x01, b'h', b'i', b'\n']);
        port.poll_io();
        let line = port.next_line().unwrap();
        assert_eq!(line.as_str(), "hi");
        let telemetry = port.telemetry();
        assert_eq!(telemetry.utf8_dropped, 1);
    }

    #[test]
    fn queue_backpressure_is_recorded() {
        let driver = LoopbackSerial::<4>::new();
        let mut port: SerialPort<_, 4, 4, 16> = SerialPort::new(driver);
        port.enqueue_tx(b"abcd");
        port.enqueue_tx(b"efgh");
        port.poll_io();
        let telemetry = port.telemetry();
        assert!(telemetry.tx_backpressure > 0);
    }

    #[test]
    fn echoes_input_and_handles_backspace() {
        let driver = LoopbackSerial::<16>::new();
        let mut port: SerialPort<_, 16, 16, 8> = SerialPort::new(driver);
        port.driver_mut().push_rx(b"ab\x08c\r");
        port.poll_io();

        let line = port.next_line().unwrap();
        assert_eq!(line.as_str(), "ac");

        port.poll_io();
        let echoed = port.driver_mut().drain_tx();
        assert_eq!(echoed.as_slice(), b"ab\x08 \x08c\r\n");
    }
}
