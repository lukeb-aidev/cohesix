// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide bounded serial console primitives for the root task and host simulations.
// Author: Lukas Bower

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
use spin::Mutex as SpinMutex;

#[cfg(feature = "kernel")]
pub mod bcm2711_mini_uart;
#[cfg(feature = "kernel")]
pub mod kernel_uart;
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

#[cfg(feature = "kernel")]
static UART_TX_LOCK: SpinMutex<()> = SpinMutex::new(());

/// Serialize all physical UART TX access so debug/syslog bytes do not interleave
/// with root-console traffic.
#[inline(always)]
pub(crate) fn with_uart_tx_lock<R>(f: impl FnOnce() -> R) -> R {
    #[cfg(feature = "kernel")]
    {
        let _guard = UART_TX_LOCK.lock();
        f()
    }
    #[cfg(not(feature = "kernel"))]
    {
        f()
    }
}

/// Capacity of the RX staging queue used by [`SerialPort`].
pub const DEFAULT_RX_CAPACITY: usize = 512;

/// Capacity of the TX staging queue used by [`SerialPort`].
pub const DEFAULT_TX_CAPACITY: usize = 4096;

/// Maximum number of UTF-8 codepoints retained in a console line.
pub const DEFAULT_LINE_CAPACITY: usize = 256;

const BLOCKING_TX_SPIN_LIMIT: usize = 1_000_000;
const SERIAL_RX_POLL_BUDGET: usize = 128;
const SERIAL_TX_POLL_BUDGET: usize = 1024;

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

    /// Attempt to stage TX bytes without flushing or retrying on saturation.
    ///
    /// This is for secondary mirrors where the caller must keep the event loop
    /// moving even if the serial peer is slow. It records one backpressure event
    /// and returns as soon as the queue is full.
    pub fn enqueue_tx_best_effort(&mut self, data: &[u8]) -> usize {
        let mut accepted = 0usize;
        for &byte in data {
            if self.tx.enqueue(byte).is_err() {
                self.telemetry.tx_overflow();
                break;
            }
            accepted = accepted.saturating_add(1);
        }
        accepted
    }

    /// Stage one ordered serial record without blocking or partially enqueueing it.
    ///
    /// Slow serial peers must not hold up HDMI or USB keyboard service. This
    /// method therefore fails the whole record when the bounded queue cannot
    /// hold it, preserving transcript ordering for records that are accepted.
    pub fn try_enqueue_tx_record(&mut self, parts: &[&[u8]]) -> bool {
        let mut total = 0usize;
        for part in parts {
            total = total.saturating_add(part.len());
        }
        let pending = self
            .tx
            .len()
            .saturating_add(usize::from(self.pending_tx.is_some()));
        let capacity = TX.saturating_sub(1);
        if total > capacity.saturating_sub(pending) {
            self.telemetry.tx_overflow();
            return false;
        }
        for part in parts {
            for &byte in *part {
                if self.tx.enqueue(byte).is_err() {
                    self.telemetry.tx_overflow();
                    return false;
                }
            }
        }
        true
    }

    /// Stage a complete line record with CRLF serial line ending.
    pub fn try_enqueue_line_record(&mut self, line: &str) -> bool {
        self.try_enqueue_tx_record(&[line.as_bytes(), b"\r\n"])
    }

    /// Flush currently staged TX bytes without polling RX.
    pub fn flush_tx(&mut self) {
        self.flush_tx_locked();
    }

    /// Emit bytes directly to the device while holding the shared UART TX lock.
    pub fn write_bytes_blocking(&mut self, data: &[u8]) {
        with_uart_tx_lock(|| {
            self.flush_tx_blocking_unlocked();
            for &byte in data {
                self.write_byte_blocking_unlocked(byte);
            }
        });
    }

    /// Emit a complete console line without allowing other UART producers to interleave.
    pub fn write_line_blocking(&mut self, line: &str) {
        with_uart_tx_lock(|| {
            self.flush_tx_blocking_unlocked();
            for &byte in line.as_bytes() {
                self.write_byte_blocking_unlocked(byte);
            }
            self.write_byte_blocking_unlocked(b'\r');
            self.write_byte_blocking_unlocked(b'\n');
        });
    }

    /// Attempt to move data between the driver and staging buffers.
    ///
    /// Returns true when RX bytes were staged for this polling cycle.
    pub fn poll_io(&mut self) -> bool {
        let mut rx_activity = false;
        // Drain RX side first so newly available bytes can be processed in the
        // same cycle.
        for _ in 0..SERIAL_RX_POLL_BUDGET {
            match self.driver.read_byte() {
                Ok(byte) => {
                    rx_activity = true;
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

        self.flush_tx_budgeted(SERIAL_TX_POLL_BUDGET);
        rx_activity
    }

    fn flush_tx_locked(&mut self) {
        with_uart_tx_lock(|| {
            self.flush_tx_unlocked();
        });
    }

    fn flush_tx_unlocked(&mut self) {
        self.flush_tx_budgeted_unlocked(usize::MAX);
    }

    fn flush_tx_budgeted(&mut self, max_bytes: usize) {
        with_uart_tx_lock(|| {
            self.flush_tx_budgeted_unlocked(max_bytes);
        });
    }

    fn flush_tx_budgeted_unlocked(&mut self, mut max_bytes: usize) {
        // Flush staged TX bytes to the device until it reports back-pressure.
        if max_bytes == 0 {
            return;
        }
        if let Some(byte) = self.pending_tx.take() {
            match self.driver.write_byte(byte) {
                Ok(()) => {
                    max_bytes = max_bytes.saturating_sub(1);
                }
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

        while max_bytes > 0 {
            let Some(byte) = self.tx.dequeue() else {
                break;
            };
            match self.driver.write_byte(byte) {
                Ok(()) => {
                    max_bytes = max_bytes.saturating_sub(1);
                }
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

    fn flush_tx_blocking_unlocked(&mut self) {
        if let Some(byte) = self.pending_tx.take() {
            self.write_byte_blocking_unlocked(byte);
        }
        while let Some(byte) = self.tx.dequeue() {
            self.write_byte_blocking_unlocked(byte);
        }
    }

    fn write_byte_blocking_unlocked(&mut self, byte: u8) {
        for _ in 0..BLOCKING_TX_SPIN_LIMIT {
            match self.driver.write_byte(byte) {
                Ok(()) => return,
                Err(NbError::WouldBlock) => core::hint::spin_loop(),
                Err(NbError::Other(_)) => {
                    self.telemetry.tx_overflow();
                    return;
                }
            }
        }
        self.telemetry.tx_overflow();
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
                    if self.line.pop().is_some() && self.echo {
                        let _ = self.try_enqueue_tx_record(&[b"\x08 \x08"]);
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
                        let _ = self.try_enqueue_tx_record(&[core::slice::from_ref(&byte)]);
                    }
                }
            }
        }
        None
    }

    /// Drop a partial line that has not reached the command parser yet.
    ///
    /// Returns true when a partial line was present.
    pub fn clear_partial_line(&mut self) -> bool {
        let had_partial = !self.line.is_empty();
        self.line.clear();
        had_partial
    }

    fn emit_newline(&mut self) {
        if self.echo {
            let _ = self.try_enqueue_tx_record(&[b"\r\n"]);
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
    fn best_effort_tx_stops_at_first_queue_saturation() {
        let driver = LoopbackSerial::<8>::new();
        let mut port: SerialPort<_, 4, 2, 16> = SerialPort::new(driver);

        let accepted = port.enqueue_tx_best_effort(b"abcd");

        assert!(accepted < 4);
        assert_eq!(port.telemetry().tx_backpressure, 1);
        port.poll_io();
        let emitted = port.driver_mut().drain_tx();
        assert_eq!(emitted.len(), accepted);
        assert_eq!(emitted.as_slice(), &b"abcd"[..accepted]);
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

    #[test]
    fn blocking_line_output_preserves_full_line_after_staged_tx() {
        let driver = LoopbackSerial::<128>::new();
        let mut port: SerialPort<_, 8, 8, 32> = SerialPort::new(driver);

        port.enqueue_tx(b"ab");
        port.write_line_blocking("0123456789abcdef");

        let emitted = port.driver_mut().drain_tx();
        assert_eq!(emitted.as_slice(), b"ab0123456789abcdef\r\n");
        assert_eq!(port.telemetry().tx_backpressure, 0);
    }

    #[test]
    fn record_enqueue_is_atomic_when_queue_lacks_space() {
        let driver = LoopbackSerial::<8>::new();
        let mut port: SerialPort<_, 4, 4, 16> = SerialPort::new(driver);

        assert!(port.try_enqueue_tx_record(&[b"abc"]));
        assert!(!port.try_enqueue_tx_record(&[b"de"]));

        port.poll_io();
        let emitted = port.driver_mut().drain_tx();
        assert_eq!(emitted.as_slice(), b"abc");
        assert_eq!(port.telemetry().tx_backpressure, 1);
    }

    #[test]
    fn serial_echo_is_best_effort_and_nonblocking() {
        let driver = LoopbackSerial::<4>::new();
        let mut port: SerialPort<_, 8, 3, 8> = SerialPort::new(driver);

        assert!(port.try_enqueue_tx_record(&[b"xy"]));
        port.driver_mut().push_rx(b"a\n");
        port.poll_io();
        let line = port
            .next_line()
            .expect("line should parse even when echo drops");

        assert_eq!(line.as_str(), "a");
        assert!(port.telemetry().tx_backpressure > 0);
    }

    #[test]
    fn poll_io_uses_bounded_rx_and_tx_service() {
        const RX_INPUT: usize = SERIAL_RX_POLL_BUDGET + 7;
        const TX_INPUT: usize = SERIAL_TX_POLL_BUDGET + 7;
        let driver = LoopbackSerial::<2048>::new();
        let mut port: SerialPort<_, 512, 2048, 16> = SerialPort::new(driver);
        let rx_input = [b'a'; RX_INPUT];
        let tx_input = [b'b'; TX_INPUT];

        port.driver_mut().push_rx(&rx_input);
        port.enqueue_tx(&tx_input);
        let rx_activity = port.poll_io();

        assert!(rx_activity);
        assert_eq!(port.rx.len(), SERIAL_RX_POLL_BUDGET);
        let emitted = port.driver_mut().drain_tx();
        assert_eq!(emitted.len(), SERIAL_TX_POLL_BUDGET);

        port.poll_io();

        assert!(port.rx.len() > SERIAL_RX_POLL_BUDGET);
        let emitted = port.driver_mut().drain_tx();
        assert_eq!(emitted.len(), TX_INPUT - SERIAL_TX_POLL_BUDGET);
    }
}
