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

#![allow(unsafe_code)]

use core::fmt;

use embedded_io::{Error as EmbeddedError, ErrorKind, ErrorType};
use heapless::{spsc::Queue, String as HeaplessString};
use nb::Error as NbError;
use portable_atomic::AtomicU32;
#[cfg(feature = "kernel")]
use portable_atomic::{AtomicU64, Ordering as AtomicOrdering};
#[cfg(feature = "kernel")]
use spin::Mutex as SpinMutex;

use crate::hal::driver_task::{
    DriverServiceBudget, DriverServiceBudgetError, DriverTaskContract, ScheduledHardwareDriver,
    SERIAL_DRIVER_TASK_CONTRACT,
};

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
pub const DEFAULT_TX_CAPACITY: usize = 256;

/// Maximum number of UTF-8 codepoints retained in a console line.
pub const DEFAULT_LINE_CAPACITY: usize = 256;

const BLOCKING_TX_SPIN_LIMIT: usize = 1_000_000;
const SERIAL_DRIVER_LOCAL_RECORD_MAGIC: u32 = 0x5344_4c52;
const SERIAL_DRIVER_LOCAL_RECORD_VERSION: u16 = 1;
const SERIAL_DRIVER_LOCAL_FLAG_ECHO_ENABLED: u16 = 1 << 0;
const SERIAL_DRIVER_LOCAL_FLAG_SUPPRESS_LF: u16 = 1 << 1;
const SERIAL_PENDING_TX_NONE: u16 = u16::MAX;
const SERIAL_OWNER_DESCRIPTOR_TRANSITIONAL_ROOT_CONTEXT: u16 = 1 << 0;

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

/// HAL-enforced scheduling contract for physical serial service.
#[must_use]
pub const fn driver_task_contract() -> DriverTaskContract {
    SERIAL_DRIVER_TASK_CONTRACT
}

/// Lightweight trait abstracting the MMIO-backed console device.
pub trait SerialDriver: ErrorType {
    /// HAL scheduling contract consumed by this serial driver.
    fn driver_task_contract() -> DriverTaskContract
    where
        Self: Sized,
    {
        SERIAL_DRIVER_TASK_CONTRACT
    }

    /// Attempt to read a single byte from the device.
    fn read_byte(&mut self) -> nb::Result<u8, Self::Error>;

    /// Attempt to write a single byte to the device.
    fn write_byte(&mut self, byte: u8) -> nb::Result<(), Self::Error>;
}

impl<T> ScheduledHardwareDriver for T
where
    T: SerialDriver,
{
    fn driver_task_contract() -> DriverTaskContract {
        <T as SerialDriver>::driver_task_contract()
    }
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
    /// Number of service turns stopped by the HAL driver-task budget.
    pub driver_task_budget_overruns: u32,
}

/// Fixed-layout serial owner-state record for the ring-backed migration path.
///
/// The record is primitive-only and intentionally stores no Rust/root pointers.
/// Today it owns the serial line flags and pending TX byte while mirroring queue
/// depths and telemetry for tests/diagnostics. The live MMIO driver and RX/TX
/// queues are still reached through [`SerialPort`], so this record is not
/// registered as `DRIVER_TASK_OWNER_STATE` proof yet.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SerialDriverLocalRuntimeRecord {
    /// Magic identifying the serial owner-state record.
    pub magic: u32,
    /// Schema version for the fixed record.
    pub version: u16,
    /// Primitive flags for echo/newline behavior.
    pub flags: u16,
    /// Current RX queue depth.
    pub rx_depth: u16,
    /// Current TX queue depth.
    pub tx_depth: u16,
    /// Current partial command-line byte length.
    pub line_len: u16,
    /// Pending byte held after TX backpressure, or [`SERIAL_PENDING_TX_NONE`].
    pub pending_tx: u16,
    /// RX backpressure count.
    pub rx_backpressure: u32,
    /// TX backpressure count.
    pub tx_backpressure: u32,
    /// Sanitizer drop count.
    pub utf8_dropped: u32,
    /// Budget-overrun count.
    pub driver_task_budget_overruns: u32,
}

impl SerialDriverLocalRuntimeRecord {
    const fn new() -> Self {
        Self {
            magic: SERIAL_DRIVER_LOCAL_RECORD_MAGIC,
            version: SERIAL_DRIVER_LOCAL_RECORD_VERSION,
            flags: SERIAL_DRIVER_LOCAL_FLAG_ECHO_ENABLED,
            rx_depth: 0,
            tx_depth: 0,
            line_len: 0,
            pending_tx: SERIAL_PENDING_TX_NONE,
            rx_backpressure: 0,
            tx_backpressure: 0,
            utf8_dropped: 0,
            driver_task_budget_overruns: 0,
        }
    }

    fn with_observed_state(
        mut self,
        rx_depth: usize,
        tx_depth: usize,
        line_len: usize,
        telemetry: SerialTelemetry,
    ) -> Self {
        self.rx_depth = saturating_u16(rx_depth);
        self.tx_depth = saturating_u16(tx_depth);
        self.line_len = saturating_u16(line_len);
        self.rx_backpressure = telemetry.rx_backpressure;
        self.tx_backpressure = telemetry.tx_backpressure;
        self.utf8_dropped = telemetry.utf8_dropped;
        self.driver_task_budget_overruns = telemetry.driver_task_budget_overruns;
        self
    }

    #[must_use]
    const fn pending_tx_byte(self) -> Option<u8> {
        if self.pending_tx <= u8::MAX as u16 {
            Some(self.pending_tx as u8)
        } else {
            None
        }
    }

    fn take_pending_tx(&mut self) -> Option<u8> {
        let byte = self.pending_tx_byte();
        self.pending_tx = SERIAL_PENDING_TX_NONE;
        byte
    }

    fn set_pending_tx(&mut self, byte: Option<u8>) {
        self.pending_tx = match byte {
            Some(byte) => u16::from(byte),
            None => SERIAL_PENDING_TX_NONE,
        };
    }

    #[must_use]
    const fn echo_enabled(self) -> bool {
        self.flags & SERIAL_DRIVER_LOCAL_FLAG_ECHO_ENABLED != 0
    }

    #[must_use]
    const fn suppress_lf(self) -> bool {
        self.flags & SERIAL_DRIVER_LOCAL_FLAG_SUPPRESS_LF != 0
    }

    fn set_suppress_lf(&mut self, suppress: bool) {
        if suppress {
            self.flags |= SERIAL_DRIVER_LOCAL_FLAG_SUPPRESS_LF;
        } else {
            self.flags &= !SERIAL_DRIVER_LOCAL_FLAG_SUPPRESS_LF;
        }
    }
}

const fn saturating_u16(value: usize) -> u16 {
    if value > u16::MAX as usize {
        u16::MAX
    } else {
        value as u16
    }
}

/// Whether serial can be credited as driver-owned owner-state proof.
///
/// This remains false because the live ring service still needs a root-owned
/// [`SerialPort`] pointer to reach the MMIO driver and heapless queues.
#[must_use]
pub const fn serial_owner_state_acceptance_ready() -> bool {
    false
}

#[cfg_attr(not(test), allow(dead_code))]
const fn serial_owner_state_descriptor(
) -> Option<crate::hal::driver_task::DriverTaskOwnerStateDescriptor> {
    crate::hal::driver_task::DriverTaskOwnerStateDescriptor::new(
        crate::hal::driver_task::DriverTaskHotPath::SerialConsole,
        crate::hal::driver_task::DRIVER_TASK_OWNER_STATE_OFFSET as u32,
        core::mem::size_of::<SerialDriverLocalRuntimeRecord>() as u16,
        crate::hal::driver_task::DRIVER_TASK_RING_FRAME_OFFSET as u32,
        crate::hal::driver_task::MAX_DRIVER_TASK_FRAME_BYTES as u16,
        SERIAL_OWNER_DESCRIPTOR_TRANSITIONAL_ROOT_CONTEXT,
    )
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
    driver_local: SerialDriverLocalRuntimeRecord,
    telemetry: SerialTelemetryCounters,
}

impl<D, const RX: usize, const TX: usize, const LINE: usize> SerialPort<D, RX, TX, LINE>
where
    D: SerialDriver,
{
    /// Construct a new serial port backed by the supplied driver.
    pub fn new(driver: D) -> Self {
        debug_assert_eq!(
            <D as SerialDriver>::driver_task_contract().validate(),
            Ok(())
        );
        Self {
            driver,
            rx: Queue::new(),
            tx: Queue::new(),
            line: HeaplessString::new(),
            driver_local: SerialDriverLocalRuntimeRecord::new(),
            telemetry: SerialTelemetryCounters::default(),
        }
    }

    /// Access the underlying telemetry snapshot.
    #[must_use]
    pub fn telemetry(&self) -> SerialTelemetry {
        self.telemetry.snapshot()
    }

    /// Snapshot the fixed-layout serial owner-state record.
    #[must_use]
    pub fn owner_runtime_record(&self) -> SerialDriverLocalRuntimeRecord {
        self.driver_local.with_observed_state(
            self.rx.len(),
            self.tx.len(),
            self.line.len(),
            self.telemetry(),
        )
    }

    /// Whether bytes remain staged for the serial TX driver.
    #[must_use]
    pub fn tx_pending(&self) -> bool {
        self.driver_local.pending_tx_byte().is_some() || !self.tx.is_empty()
    }

    /// HAL scheduling contract consumed by this port.
    #[must_use]
    pub fn driver_task_contract(&self) -> DriverTaskContract {
        <D as SerialDriver>::driver_task_contract()
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
        let contract = <D as SerialDriver>::driver_task_contract();
        #[cfg(feature = "kernel")]
        {
            if crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active() {
                crate::hal::driver_task::register_driver_task_pointer_free_ring_service(
                    contract,
                    crate::hal::driver_task::DriverTaskHotPath::SerialConsole.as_u32() as usize,
                    serial_runtime_ring_service_driver_task,
                );
                let command = crate::hal::driver_task::DriverTaskCommandRecord::pi4_hot_path(
                    0,
                    crate::hal::driver_task::DriverTaskHotPath::SerialConsole,
                    crate::hal::driver_task::DriverTaskBudgetGrant::from_contract(contract),
                    crate::hal::driver_task::DriverFrameDescriptor {
                        offset: 0,
                        len: 0,
                        flags: 0,
                    },
                );
                if let Some(completion) =
                    crate::hal::driver_task::run_driver_task_ring_service(contract, command)
                {
                    return completion.code
                        == crate::hal::driver_task::DriverTaskCompletionCode::Progress.as_u16()
                        && completion.result != 0;
                }
                self.telemetry.driver_task_budget_overrun();
                return false;
            }
            crate::hal::driver_task::register_driver_task_root_context_ring_service(
                contract,
                self as *mut Self as usize,
                serial_ring_service_driver_task::<D, RX, TX, LINE>,
            );
            let command = crate::hal::driver_task::DriverTaskCommandRecord::pi4_hot_path(
                0,
                crate::hal::driver_task::DriverTaskHotPath::SerialConsole,
                crate::hal::driver_task::DriverTaskBudgetGrant::from_contract(contract),
                crate::hal::driver_task::DriverFrameDescriptor {
                    offset: 0,
                    len: 0,
                    flags:
                        crate::hal::driver_task::DRIVER_TASK_RING_FLAG_ROOT_CONTEXT_NON_ACCEPTANCE,
                },
            );
            if let Some(completion) =
                crate::hal::driver_task::run_driver_task_ring_service(contract, command)
            {
                return completion.code
                    == crate::hal::driver_task::DriverTaskCompletionCode::Progress.as_u16()
                    && completion.result != 0;
            }
            // SAFETY: The HAL admits this compatibility callback only for
            // QEMU/host profiles. Physical Pi 4 builds return None without
            // compiling callback slot state.
            if let Some(result) = unsafe {
                crate::hal::driver_task::try_driver_task_compat_service(
                    contract,
                    self as *mut Self as usize,
                    serial_poll_io_driver_task::<D, RX, TX, LINE>,
                )
            } {
                return result != 0;
            }
            if !crate::hal::driver_task::admit_root_task_compatibility_service(contract) {
                self.telemetry.driver_task_budget_overrun();
                return false;
            }
        }
        self.poll_io_current_tcb(contract)
    }

    fn poll_io_current_tcb(&mut self, contract: DriverTaskContract) -> bool {
        let mut budget = match DriverServiceBudget::new(contract) {
            Ok(budget) => budget,
            Err(_) => {
                self.telemetry.driver_task_budget_overrun();
                return false;
            }
        };
        let mut budget_exhausted = false;
        let mut rx_activity = false;
        // Drain RX side first so newly available bytes can be processed in the
        // same cycle.
        loop {
            if self.serial_byte_budget_available(&budget).is_err() {
                budget_exhausted = true;
                break;
            }
            match self.driver.read_byte() {
                Ok(byte) => {
                    if self.charge_serial_byte(&mut budget).is_err() {
                        self.telemetry.driver_task_budget_overrun();
                        break;
                    }
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

        if budget_exhausted {
            self.telemetry.driver_task_budget_overrun();
        } else {
            with_uart_tx_lock(|| self.flush_tx_unlocked(&mut budget));
        }
        rx_activity
    }

    fn flush_tx_locked(&mut self) {
        let contract = <D as SerialDriver>::driver_task_contract();
        #[cfg(feature = "kernel")]
        {
            if crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active() {
                crate::hal::driver_task::register_driver_task_pointer_free_ring_service(
                    contract,
                    crate::hal::driver_task::DriverTaskHotPath::SerialConsole.as_u32() as usize,
                    serial_runtime_ring_service_driver_task,
                );
                let command = crate::hal::driver_task::DriverTaskCommandRecord::flush(
                    0,
                    crate::hal::driver_task::DriverTaskBudgetGrant::from_contract(contract),
                );
                if crate::hal::driver_task::run_driver_task_ring_service(contract, command)
                    .is_some()
                {
                    return;
                }
                self.telemetry.driver_task_budget_overrun();
                return;
            }
            crate::hal::driver_task::register_driver_task_root_context_ring_service(
                contract,
                self as *mut Self as usize,
                serial_ring_service_driver_task::<D, RX, TX, LINE>,
            );
            let mut command = crate::hal::driver_task::DriverTaskCommandRecord::flush(
                0,
                crate::hal::driver_task::DriverTaskBudgetGrant::from_contract(contract),
            );
            command.flags =
                crate::hal::driver_task::DRIVER_TASK_RING_FLAG_ROOT_CONTEXT_NON_ACCEPTANCE;
            command.frame.flags =
                crate::hal::driver_task::DRIVER_TASK_RING_FLAG_ROOT_CONTEXT_NON_ACCEPTANCE;
            if crate::hal::driver_task::run_driver_task_ring_service(contract, command).is_some() {
                return;
            }
            // SAFETY: The HAL admits this compatibility callback only for
            // QEMU/host profiles. Physical Pi 4 builds return None without
            // compiling callback slot state.
            if unsafe {
                crate::hal::driver_task::try_driver_task_compat_service(
                    contract,
                    self as *mut Self as usize,
                    serial_flush_tx_driver_task::<D, RX, TX, LINE>,
                )
            }
            .is_some()
            {
                return;
            }
            if !crate::hal::driver_task::admit_root_task_compatibility_service(contract) {
                self.telemetry.driver_task_budget_overrun();
                return;
            }
        }
        self.flush_tx_current_tcb(contract);
    }

    fn flush_tx_current_tcb(&mut self, contract: DriverTaskContract) {
        let mut budget = match DriverServiceBudget::new(contract) {
            Ok(budget) => budget,
            Err(_) => {
                self.telemetry.driver_task_budget_overrun();
                return;
            }
        };
        with_uart_tx_lock(|| self.flush_tx_unlocked(&mut budget));
    }

    fn flush_tx_unlocked(&mut self, budget: &mut DriverServiceBudget) {
        // Flush staged TX bytes to the device until it reports back-pressure.
        if let Some(byte) = self.driver_local.take_pending_tx() {
            if self.serial_byte_budget_available(budget).is_err() {
                self.telemetry.driver_task_budget_overrun();
                self.driver_local.set_pending_tx(Some(byte));
                return;
            }
            match self.driver.write_byte(byte) {
                Ok(()) => {
                    if self.charge_serial_byte(budget).is_err() {
                        self.telemetry.driver_task_budget_overrun();
                        return;
                    }
                }
                Err(NbError::WouldBlock) => {
                    self.driver_local.set_pending_tx(Some(byte));
                    return;
                }
                Err(NbError::Other(_)) => {
                    self.telemetry.tx_overflow();
                    return;
                }
            }
        }

        while !self.tx.is_empty() {
            if self.serial_byte_budget_available(budget).is_err() {
                self.telemetry.driver_task_budget_overrun();
                return;
            }
            let Some(byte) = self.tx.dequeue() else {
                break;
            };
            match self.driver.write_byte(byte) {
                Ok(()) => {
                    if self.charge_serial_byte(budget).is_err() {
                        self.telemetry.driver_task_budget_overrun();
                        return;
                    }
                }
                Err(NbError::WouldBlock) => {
                    self.driver_local.set_pending_tx(Some(byte));
                    return;
                }
                Err(NbError::Other(_)) => {
                    self.telemetry.tx_overflow();
                    return;
                }
            }
        }
    }

    fn serial_byte_budget_available(
        &self,
        budget: &DriverServiceBudget,
    ) -> Result<(), DriverServiceBudgetError> {
        if budget.ops_left() == 0 {
            return Err(DriverServiceBudgetError::OperationsExhausted);
        }
        if budget.bytes_left() == 0 {
            return Err(DriverServiceBudgetError::BytesExhausted);
        }
        if budget.frames_left() == 0 {
            return Err(DriverServiceBudgetError::FramesExhausted);
        }
        Ok(())
    }

    fn charge_serial_byte(
        &self,
        budget: &mut DriverServiceBudget,
    ) -> Result<(), DriverServiceBudgetError> {
        self.serial_byte_budget_available(budget)?;
        budget.charge_ops(1)?;
        budget.charge_bytes(1)?;
        budget.charge_frames(1)
    }

    fn flush_tx_blocking_unlocked(&mut self) {
        if let Some(byte) = self.driver_local.take_pending_tx() {
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
            if self.driver_local.suppress_lf() && byte == b'\n' {
                self.driver_local.set_suppress_lf(false);
                continue;
            }
            match byte {
                b'\r' => {
                    self.driver_local.set_suppress_lf(true);
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
                    if self.line.pop().is_some() && self.driver_local.echo_enabled() {
                        self.enqueue_tx(b"\x08 \x08");
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
                    if self.driver_local.echo_enabled() {
                        self.enqueue_tx(&[byte]);
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
        if self.driver_local.echo_enabled() {
            self.enqueue_tx(b"\r\n");
        }
    }

    /// Access the driver mutably (used by tests for inspection).
    pub fn driver_mut(&mut self) -> &mut D {
        &mut self.driver
    }
}

#[cfg(feature = "kernel")]
unsafe fn serial_runtime_ring_service_driver_task(
    context: usize,
    command: crate::hal::driver_task::DriverTaskCommandRecord,
) -> crate::hal::driver_task::DriverTaskCompletionRecord {
    let expected_hot_path = crate::hal::driver_task::DriverTaskHotPath::SerialConsole;
    if context != expected_hot_path.as_u32() as usize {
        return crate::hal::driver_task::DriverTaskCompletionRecord::fault(
            command.sequence,
            crate::hal::driver_task::DriverTaskFaultCode::RejectedCommand,
        );
    }
    if command.opcode == crate::hal::driver_task::DriverTaskOpcode::Service.as_u16()
        && command.arg0 == expected_hot_path.as_u32()
        && command.arg1 == expected_hot_path.role_bit() as u32
        && command.frame.len == 0
    {
        return crate::hal::driver_task::DriverTaskCompletionRecord::idle(command.sequence);
    }
    if command.opcode == crate::hal::driver_task::DriverTaskOpcode::Flush.as_u16() {
        return crate::hal::driver_task::DriverTaskCompletionRecord::idle(command.sequence);
    }
    crate::hal::driver_task::DriverTaskCompletionRecord::fault(
        command.sequence,
        crate::hal::driver_task::DriverTaskFaultCode::RejectedCommand,
    )
}

#[cfg(feature = "kernel")]
unsafe fn serial_ring_service_driver_task<D, const RX: usize, const TX: usize, const LINE: usize>(
    context: usize,
    command: crate::hal::driver_task::DriverTaskCommandRecord,
) -> crate::hal::driver_task::DriverTaskCompletionRecord
where
    D: SerialDriver,
{
    // SAFETY: `context` is registered by `SerialPort` before submitting a
    // synchronous ring command, and the root TCB does not mutate the port until
    // the driver TCB publishes a matching completion sequence. This root pointer
    // is transitional service context and is not owner-state acceptance proof.
    let port = unsafe { &mut *(context as *mut SerialPort<D, RX, TX, LINE>) };
    let contract = <D as SerialDriver>::driver_task_contract();
    if command.opcode == crate::hal::driver_task::DriverTaskOpcode::Service.as_u16() {
        let progress = port.poll_io_current_tcb(contract);
        return crate::hal::driver_task::DriverTaskCompletionRecord::progress(
            command.sequence,
            progress as u32,
        );
    }
    if command.opcode == crate::hal::driver_task::DriverTaskOpcode::Flush.as_u16() {
        port.flush_tx_current_tcb(contract);
        return crate::hal::driver_task::DriverTaskCompletionRecord::idle(command.sequence);
    }
    crate::hal::driver_task::DriverTaskCompletionRecord::fault(
        command.sequence,
        crate::hal::driver_task::DriverTaskFaultCode::RejectedCommand,
    )
}

#[cfg(feature = "kernel")]
unsafe fn serial_poll_io_driver_task<D, const RX: usize, const TX: usize, const LINE: usize>(
    context: usize,
) -> usize
where
    D: SerialDriver,
{
    // SAFETY: `context` is provided only by `SerialPort::poll_io` while the
    // root TCB is synchronously waiting for the dedicated serial TCB to finish.
    // The callback-pointer ABI is compatibility-only, not owner-state proof.
    let port = unsafe { &mut *(context as *mut SerialPort<D, RX, TX, LINE>) };
    let contract = <D as SerialDriver>::driver_task_contract();
    port.poll_io_current_tcb(contract) as usize
}

#[cfg(feature = "kernel")]
unsafe fn serial_flush_tx_driver_task<D, const RX: usize, const TX: usize, const LINE: usize>(
    context: usize,
) -> usize
where
    D: SerialDriver,
{
    // SAFETY: `context` is provided only by `SerialPort::flush_tx_locked` while
    // the root TCB is synchronously waiting for the dedicated serial TCB.
    // The callback-pointer ABI is compatibility-only, not owner-state proof.
    let port = unsafe { &mut *(context as *mut SerialPort<D, RX, TX, LINE>) };
    let contract = <D as SerialDriver>::driver_task_contract();
    port.flush_tx_current_tcb(contract);
    0
}

/// Internal telemetry counters backed by atomics so interrupt handlers can
/// update statistics without locks.
#[derive(Debug, Default)]
struct SerialTelemetryCounters {
    rx_backpressure: AtomicU32,
    tx_backpressure: AtomicU32,
    utf8_dropped: AtomicU32,
    driver_task_budget_overruns: AtomicU32,
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
            driver_task_budget_overruns: self
                .driver_task_budget_overruns
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

    fn driver_task_budget_overrun(&self) {
        self.driver_task_budget_overruns
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
    fn serial_declares_valid_realtime_driver_task_contract() {
        let contract = driver_task_contract();

        assert_eq!(contract.name, "serial");
        assert!(contract.preempts_network_data());
        assert_eq!(contract.validate(), Ok(()));
    }

    #[test]
    fn serial_owner_runtime_record_is_fixed_layout_but_not_acceptance_ready() {
        assert!(
            core::mem::size_of::<SerialDriverLocalRuntimeRecord>()
                <= crate::hal::driver_task::DRIVER_TASK_OWNER_STATE_BYTES
        );
        let descriptor =
            serial_owner_state_descriptor().expect("serial owner-state record must fit the ring");
        assert_eq!(
            descriptor.hot_path,
            crate::hal::driver_task::DriverTaskHotPath::SerialConsole
        );
        assert_eq!(
            descriptor.state_len as usize,
            core::mem::size_of::<SerialDriverLocalRuntimeRecord>()
        );
        assert_eq!(
            descriptor.buffer_offset as usize,
            crate::hal::driver_task::DRIVER_TASK_RING_FRAME_OFFSET
        );
        assert!(!serial_owner_state_acceptance_ready());
    }

    #[test]
    fn serial_owner_runtime_record_tracks_line_and_queue_state() {
        let driver = LoopbackSerial::<64>::new();
        let mut port: SerialPort<_, 16, 16, 8> = SerialPort::new(driver);

        port.enqueue_tx(b"abcd");
        port.driver_mut().push_rx(b"xy");
        assert!(port.poll_io());
        let record = port.owner_runtime_record();

        assert_eq!(record.magic, SERIAL_DRIVER_LOCAL_RECORD_MAGIC);
        assert_eq!(record.version, SERIAL_DRIVER_LOCAL_RECORD_VERSION);
        assert_eq!(record.rx_depth, 2);
        assert_eq!(record.tx_depth, 0);
        assert_eq!(record.line_len, 0);
        assert_eq!(record.pending_tx_byte(), None);

        assert!(port.next_line().is_none());
        let record = port.owner_runtime_record();
        assert_eq!(record.rx_depth, 0);
        assert_eq!(record.line_len, 2);
    }

    #[test]
    fn poll_io_obeys_driver_task_budget() {
        let driver = LoopbackSerial::<1024>::new();
        let mut port: SerialPort<_, 1024, 1024, 16> = SerialPort::new(driver);
        let input = [b'a'; 128];
        port.driver_mut().push_rx(&input);

        assert!(port.poll_io());

        assert_eq!(port.rx.len(), 64);
        assert_eq!(port.driver_mut().rx.borrow().len(), 64);
        assert!(port.telemetry().driver_task_budget_overruns > 0);
    }

    #[test]
    fn flush_tx_obeys_driver_task_budget() {
        let driver = LoopbackSerial::<1024>::new();
        let mut port: SerialPort<_, 1024, 1024, 16> = SerialPort::new(driver);
        let output = [b'x'; 128];
        port.enqueue_tx(&output);

        port.flush_tx();

        let emitted = port.driver_mut().drain_tx();
        assert_eq!(emitted.len(), 64);
        assert!(port.telemetry().driver_task_budget_overruns > 0);
    }

    #[test]
    fn flush_tx_backpressure_does_not_count_as_budget_overrun() {
        let driver = LoopbackSerial::<4>::new();
        let mut port: SerialPort<_, 16, 16, 16> = SerialPort::new(driver);
        let output = [b'x'; 10];
        port.enqueue_tx(&output);

        port.flush_tx();

        let emitted = port.driver_mut().drain_tx();
        assert!(emitted.len() < output.len());
        assert_eq!(port.telemetry().driver_task_budget_overruns, 0);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn ring_service_poll_turn_uses_fixed_command_record() {
        let driver = LoopbackSerial::<16>::new();
        let mut port: SerialPort<_, 16, 16, 16> = SerialPort::new(driver);
        port.driver_mut().push_rx(b"z");
        let command = crate::hal::driver_task::DriverTaskCommandRecord::service(
            9,
            crate::hal::driver_task::DriverTaskBudgetGrant::from_contract(driver_task_contract()),
        );

        let completion = unsafe {
            serial_ring_service_driver_task::<LoopbackSerial<16>, 16, 16, 16>(
                &mut port as *mut SerialPort<_, 16, 16, 16> as usize,
                command,
            )
        };

        assert_eq!(completion.sequence, 9);
        assert_eq!(
            completion.code,
            crate::hal::driver_task::DriverTaskCompletionCode::Progress.as_u16()
        );
        assert_eq!(completion.result, 1);
        assert_eq!(port.rx.len(), 1);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn ring_service_flush_turn_rejects_callback_context_shape() {
        let driver = LoopbackSerial::<16>::new();
        let mut port: SerialPort<_, 16, 16, 16> = SerialPort::new(driver);
        port.enqueue_tx(b"abc");
        let command = crate::hal::driver_task::DriverTaskCommandRecord::flush(
            10,
            crate::hal::driver_task::DriverTaskBudgetGrant::from_contract(driver_task_contract()),
        );

        let completion = unsafe {
            serial_ring_service_driver_task::<LoopbackSerial<16>, 16, 16, 16>(
                &mut port as *mut SerialPort<_, 16, 16, 16> as usize,
                command,
            )
        };

        assert_eq!(completion.sequence, 10);
        assert_eq!(
            completion.code,
            crate::hal::driver_task::DriverTaskCompletionCode::Idle.as_u16()
        );
        assert_eq!(port.driver_mut().drain_tx().as_slice(), b"abc");
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn runtime_ring_service_uses_selector_without_serial_port_pointer() {
        let command = crate::hal::driver_task::DriverTaskCommandRecord::pi4_hot_path(
            11,
            crate::hal::driver_task::DriverTaskHotPath::SerialConsole,
            crate::hal::driver_task::DriverTaskBudgetGrant::from_contract(driver_task_contract()),
            crate::hal::driver_task::DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        );

        let completion = unsafe {
            serial_runtime_ring_service_driver_task(
                crate::hal::driver_task::DriverTaskHotPath::SerialConsole.as_u32() as usize,
                command,
            )
        };

        assert_eq!(completion.sequence, 11);
        assert_eq!(
            completion.code,
            crate::hal::driver_task::DriverTaskCompletionCode::Idle.as_u16()
        );
    }

    #[test]
    fn poll_io_preserves_full_tx_budget_after_idle_rx_probe() {
        let driver = LoopbackSerial::<1024>::new();
        let mut port: SerialPort<_, 1024, 1024, 16> = SerialPort::new(driver);
        let output = [b'x'; 64];
        port.enqueue_tx(&output);

        assert!(!port.poll_io());

        let emitted = port.driver_mut().drain_tx();
        assert_eq!(emitted.len(), 64);
        assert_eq!(port.telemetry().driver_task_budget_overruns, 0);
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
}
