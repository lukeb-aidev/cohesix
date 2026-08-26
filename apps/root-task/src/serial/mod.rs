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
#[cfg(feature = "kernel")]
use heapless::Vec as HeaplessVec;
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
#[cfg(all(feature = "kernel", feature = "release-qemu"))]
static QEMU_UART_TX_LOCK_DEFERRALS: AtomicU64 = AtomicU64::new(0);

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

/// Attempt one serialized UART operation without waiting behind a preempted
/// MCS owner. Callers retain their staged bytes when this returns `None`.
#[cfg(feature = "release-qemu")]
#[inline(always)]
pub(crate) fn try_with_uart_tx_lock<R>(f: impl FnOnce() -> R) -> Option<R> {
    #[cfg(feature = "kernel")]
    {
        let guard = UART_TX_LOCK.try_lock()?;
        let result = f();
        drop(guard);
        Some(result)
    }
    #[cfg(not(feature = "kernel"))]
    {
        Some(f())
    }
}

#[cfg(all(feature = "kernel", feature = "release-qemu"))]
fn record_qemu_uart_tx_lock_deferral() {
    QEMU_UART_TX_LOCK_DEFERRALS.fetch_add(1, AtomicOrdering::Relaxed);
}

/// Number of QEMU root-control UART flushes deferred rather than spinning
/// behind a preempted lock owner.
#[cfg(feature = "release-qemu")]
pub(crate) fn qemu_uart_tx_lock_deferrals() -> u64 {
    #[cfg(feature = "kernel")]
    {
        QEMU_UART_TX_LOCK_DEFERRALS.load(AtomicOrdering::Relaxed)
    }
    #[cfg(not(feature = "kernel"))]
    {
        0
    }
}

/// Capacity of the RX staging queue used by [`SerialPort`].
pub const DEFAULT_RX_CAPACITY: usize = 512;

/// Capacity of the TX staging queue used by [`SerialPort`].
pub const DEFAULT_TX_CAPACITY: usize = 4096;

/// Maximum number of UTF-8 codepoints retained in a console line.
pub const DEFAULT_LINE_CAPACITY: usize = 256;

const BLOCKING_TX_SPIN_LIMIT: usize = 1_000_000;
const SERIAL_DRIVER_LOCAL_RECORD_MAGIC: u32 = 0x5344_4c52;
const SERIAL_DRIVER_LOCAL_RECORD_VERSION: u16 = 1;
const SERIAL_DRIVER_LOCAL_FLAG_ECHO_ENABLED: u16 = 1 << 0;
const SERIAL_DRIVER_LOCAL_FLAG_SUPPRESS_LF: u16 = 1 << 1;
const SERIAL_PENDING_TX_NONE: u16 = u16::MAX;
#[cfg(feature = "kernel")]
const SERIAL_RUNTIME_RX_DRAIN_DURING_TX_INTERVAL: usize = 8;
#[cfg(feature = "kernel")]
const SERIAL_LINKED_TX_TURN_BYTES: usize = 128;
const SERIAL_OWNER_DESCRIPTOR_FLAGS: u16 =
    crate::hal::driver_task::DRIVER_TASK_OWNER_STATE_REQUIRED_FLAGS;
#[cfg(feature = "kernel")]
const SERIAL_RUNTIME_AUX_INIT: u32 = 0x5345_5249;
#[cfg(feature = "kernel")]
const SERIAL_RUNTIME_AUX_TX_IDLE: u32 = pi4_driver_abi::DRIVER_RUNTIME_SERIAL_TX_IDLE_AUX;

/// Admission cursor for one root-control Operator serial service turn.
///
/// The isolated QEMU VirtIO profile gives Operator, Runtime, and Network their
/// own outer MCS turns. Operator may enqueue any bounded response work, but it
/// shares one smaller byte cap across every serial helper it reaches before
/// yielding. Bytes that do not fit remain in the ordinary queues for the next
/// Operator turn. When RX and a previously queued TX tail are both ready, RX
/// leaves half of the same cap for TX so sustained input cannot starve ordered
/// ACK/ERR/END output.
#[derive(Debug, Default)]
struct OrdinaryRootControlSerialTurn {
    active: bool,
    bytes_left: u32,
    tx_reserve: u32,
}

impl OrdinaryRootControlSerialTurn {
    fn begin(&mut self, byte_limit: u32, tx_pending_at_turn_start: bool) {
        debug_assert!(!self.active);
        self.active = true;
        self.bytes_left = byte_limit;
        self.tx_reserve = if tx_pending_at_turn_start {
            byte_limit.saturating_add(1) / 2
        } else {
            0
        };
    }

    fn finish(&mut self) {
        debug_assert!(self.active);
        self.active = false;
        self.bytes_left = 0;
        self.tx_reserve = 0;
    }

    const fn byte_available(&self) -> bool {
        !self.active || self.bytes_left != 0
    }

    const fn rx_byte_available(&self) -> bool {
        self.byte_available() && (!self.active || self.bytes_left > self.tx_reserve)
    }

    fn charge_byte(&mut self) {
        if self.active {
            debug_assert!(self.bytes_left != 0);
            self.bytes_left = self.bytes_left.saturating_sub(1);
        }
    }

    const fn active(&self) -> bool {
        self.active
    }
}

/// Immutable linked-runtime TX bytes retained until their exact completion.
///
/// The child can physically emit bytes before root observes its completion.
/// Keeping the submitted prefix outside the ordinary queue prevents an
/// issued-but-unknown turn from being appended to the tail and replayed.
#[cfg(feature = "kernel")]
#[derive(Debug)]
struct LinkedSerialTxCursor {
    bytes: HeaplessVec<u8, { crate::hal::driver_task::MAX_DRIVER_TASK_FRAME_BYTES }>,
    ticket: u64,
    command: Option<crate::hal::driver_task::DriverTaskCommandRecord>,
    poisoned: bool,
}

#[cfg(feature = "kernel")]
impl LinkedSerialTxCursor {
    const fn new() -> Self {
        Self {
            bytes: HeaplessVec::new(),
            ticket: 0,
            command: None,
            poisoned: false,
        }
    }

    fn begin_action(
        &mut self,
        ticket: u64,
        command: crate::hal::driver_task::DriverTaskCommandRecord,
    ) {
        debug_assert!(!self.bytes.is_empty());
        debug_assert!(self.command.is_none());
        self.ticket = ticket;
        self.command = Some(command);
    }

    fn consume_prefix(&mut self, written: usize) -> bool {
        if written > self.bytes.len() {
            return false;
        }
        if written == 0 {
            return true;
        }
        let remaining = self.bytes.len().saturating_sub(written);
        self.bytes.as_mut_slice().copy_within(written.., 0);
        self.bytes.truncate(remaining);
        true
    }

    fn poison(&mut self) {
        self.bytes.clear();
        self.command = None;
        self.poisoned = true;
    }
}

/// Retained proof that every accepted UART byte has left the hardware.
///
/// Emptying root's software queues proves only that the child accepted the
/// bytes. A separate immutable command samples the physical transmitter-idle
/// bit, and an indeterminate completion remains authoritative until it is
/// resolved or the linked generation is poisoned.
#[cfg(feature = "kernel")]
#[derive(Debug)]
struct LinkedSerialTxIdleCursor {
    required: bool,
    command: Option<crate::hal::driver_task::DriverTaskCommandRecord>,
    ticket: u64,
    poisoned: bool,
}

#[cfg(feature = "kernel")]
impl LinkedSerialTxIdleCursor {
    const fn new() -> Self {
        Self {
            required: false,
            command: None,
            ticket: 0,
            poisoned: false,
        }
    }

    fn poison(&mut self) {
        self.command = None;
        self.ticket = 0;
        self.required = true;
        self.poisoned = true;
    }
}

#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LinkedSerialTurnOutcome {
    Pending,
    Complete { activity: bool },
    Failed,
}

/// Exact drain state for an operator response retained by the serial owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SerialTxDrainOutcome {
    /// Bytes or an immutable linked-runtime command remain in flight.
    Pending,
    /// Every byte accepted before the observation has completed.
    Complete,
    /// The linked-runtime generation was poisoned; an empty queue is not proof.
    Failed,
}

/// Result of one retained linked-runtime transmitter-idle service turn.
#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SerialTxIdleTurnOutcome {
    /// The exact poll remains in flight or the transmitter is still busy.
    Pending,
    /// The child proved the physical UART transmitter is idle.
    Complete,
    /// The linked generation or completion shape is invalid.
    Failed,
}

#[cfg(feature = "kernel")]
static SERIAL_DRIVER_RUNTIME: SpinMutex<
    Option<
        SerialPort<
            kernel_uart::KernelSerialDriver,
            DEFAULT_RX_CAPACITY,
            DEFAULT_TX_CAPACITY,
            DEFAULT_LINE_CAPACITY,
        >,
    >,
> = SpinMutex::new(None);
#[cfg(feature = "kernel")]
static SERIAL_RUNTIME_INIT_LEASE: SpinMutex<Option<kernel_uart::KernelUartMmio>> =
    SpinMutex::new(None);
#[cfg(feature = "kernel")]
static SERIAL_CLIENT_RX: SpinMutex<Queue<u8, DEFAULT_RX_CAPACITY>> = SpinMutex::new(Queue::new());
#[cfg(feature = "kernel")]
static SERIAL_LINKED_RUNTIME_ATTACHED: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "kernel")]
static SERIAL_DRIVER_TASK_CLIENT_ACTIVE: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "kernel")]
static SERIAL_DRIVER_TASK_CLIENT_SERVICE_PROVEN: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "kernel")]
static SERIAL_DRIVER_TASK_CLIENT_RX_PROVEN: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "kernel")]
static SERIAL_SPSC_GENERATION: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "kernel")]
static SERIAL_RUNTIME_FAULT_FENCED: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "kernel")]
static SERIAL_RUNTIME_ATTACH_PHASE: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "kernel")]
static SERIAL_ROOT_UART_RELEASED_FOR_LINKED_RUNTIME: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "kernel")]
static SERIAL_INPUT_ROUTE_LOGGED: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "kernel")]
static SERIAL_INPUT_RX_TRACE_COUNT: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "kernel")]
static SERIAL_INPUT_LINE_TRACE_COUNT: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "kernel")]
static SERIAL_PROMPT_INPUT_SHADOW: SpinMutex<HeaplessString<DEFAULT_LINE_CAPACITY>> =
    SpinMutex::new(HeaplessString::new());
#[cfg(feature = "kernel")]
static SERIAL_PROMPT_INPUT_SHADOW_VALID: AtomicU32 = AtomicU32::new(1);

#[cfg(feature = "kernel")]
const SERIAL_RUNTIME_ATTACH_DESCRIPTOR: u32 = 0;
#[cfg(feature = "kernel")]
const SERIAL_RUNTIME_ATTACH_INIT: u32 = 1;
#[cfg(feature = "kernel")]
const SERIAL_RUNTIME_ATTACH_PROBE: u32 = 2;
#[cfg(feature = "kernel")]
const SERIAL_RUNTIME_ATTACH_READY: u32 = 3;
#[cfg(feature = "kernel")]
const SERIAL_RUNTIME_ATTACH_FAILED: u32 = 4;

#[cfg(feature = "kernel")]
fn next_serial_spsc_generation() -> Option<u32> {
    let mut current = SERIAL_SPSC_GENERATION.load(AtomicOrdering::Acquire);
    loop {
        let next = current.wrapping_add(1).max(1);
        match SERIAL_SPSC_GENERATION.compare_exchange_weak(
            current,
            next,
            AtomicOrdering::AcqRel,
            AtomicOrdering::Acquire,
        ) {
            Ok(_) => return Some(next),
            Err(observed) => current = observed,
        }
    }
}

#[cfg(feature = "kernel")]
const fn serial_spsc_initialization_allowed(fault_fenced: u32) -> bool {
    fault_fenced == 0
}

#[cfg(feature = "kernel")]
fn initialize_serial_spsc_generation() -> bool {
    if !serial_spsc_initialization_allowed(
        SERIAL_RUNTIME_FAULT_FENCED.load(AtomicOrdering::Acquire),
    ) {
        return false;
    }
    let Some(generation) = next_serial_spsc_generation() else {
        return false;
    };
    if serial_spsc_initialization_allowed(SERIAL_RUNTIME_FAULT_FENCED.load(AtomicOrdering::Acquire))
        && crate::hal::driver_task::initialize_driver_task_serial_spsc(generation)
        && serial_spsc_initialization_allowed(
            SERIAL_RUNTIME_FAULT_FENCED.load(AtomicOrdering::Acquire),
        )
    {
        true
    } else {
        SERIAL_SPSC_GENERATION.store(0, AtomicOrdering::Release);
        false
    }
}

#[cfg(feature = "kernel")]
fn publish_serial_generation_owned_state_for(
    generation: &AtomicU32,
    expected_generation: u32,
    state: &AtomicU32,
    live_value: u32,
    fenced_value: u32,
) -> bool {
    if expected_generation == 0 || generation.load(AtomicOrdering::Acquire) != expected_generation {
        return false;
    }
    state.store(live_value, AtomicOrdering::Release);
    if generation.load(AtomicOrdering::Acquire) != expected_generation {
        state.store(fenced_value, AtomicOrdering::Release);
        return false;
    }
    true
}

#[cfg(feature = "kernel")]
fn publish_serial_generation_owned_state(
    expected_generation: u32,
    state: &AtomicU32,
    live_value: u32,
    fenced_value: u32,
) -> bool {
    publish_serial_generation_owned_state_for(
        &SERIAL_SPSC_GENERATION,
        expected_generation,
        state,
        live_value,
        fenced_value,
    )
}

#[cfg(feature = "kernel")]
fn fence_serial_driver_task_runtime_state_for(
    fault_fenced: &AtomicU32,
    generation: &AtomicU32,
    inactive_states: &[&AtomicU32],
    attach_phase: &AtomicU32,
    failed_phase: u32,
) {
    // The irreversible latch prevents generation-zero from becoming an ABA
    // restart boundary. Generation is then the publication fence for every
    // later positive state transition: a racing publisher either observes the
    // fence before its store or rolls its store back on the final recheck.
    fault_fenced.store(1, AtomicOrdering::Release);
    generation.store(0, AtomicOrdering::Release);
    for state in inactive_states {
        state.store(0, AtomicOrdering::Release);
    }
    attach_phase.store(failed_phase, AtomicOrdering::Release);
}

/// Irreversibly fence root's serial client after the isolated runtime faults.
///
/// The root UART ownership latch deliberately remains set: containment cannot
/// reclaim physical MMIO from a generation that may have touched the device.
#[cfg(feature = "kernel")]
pub(crate) fn fence_serial_driver_task_runtime_after_fault() {
    fence_serial_driver_task_runtime_state_for(
        &SERIAL_RUNTIME_FAULT_FENCED,
        &SERIAL_SPSC_GENERATION,
        &[
            &SERIAL_LINKED_RUNTIME_ATTACHED,
            &SERIAL_DRIVER_TASK_CLIENT_ACTIVE,
            &SERIAL_DRIVER_TASK_CLIENT_SERVICE_PROVEN,
            &SERIAL_DRIVER_TASK_CLIENT_RX_PROVEN,
        ],
        &SERIAL_RUNTIME_ATTACH_PHASE,
        SERIAL_RUNTIME_ATTACH_FAILED,
    );
}

#[cfg(all(feature = "kernel", test))]
static SERIAL_LINKED_RUNTIME_ONLY_TEST_ACTIVE: AtomicU32 = AtomicU32::new(0);
#[cfg(all(feature = "kernel", test))]
static SERIAL_LINKED_RUNTIME_ONLY_TEST_RX: SpinMutex<Queue<u8, DEFAULT_RX_CAPACITY>> =
    SpinMutex::new(Queue::new());
#[cfg(all(feature = "kernel", test))]
static SERIAL_LINKED_RUNTIME_ONLY_TEST_TX: SpinMutex<Queue<u8, DEFAULT_TX_CAPACITY>> =
    SpinMutex::new(Queue::new());
#[cfg(all(feature = "kernel", test))]
static SERIAL_LINKED_RUNTIME_ONLY_TEST_TX_IDLE_MISSES: AtomicU32 = AtomicU32::new(0);

#[cfg(feature = "kernel")]
fn prompt_shadow_push(byte: u8) {
    let mut shadow = SERIAL_PROMPT_INPUT_SHADOW.lock();
    if SERIAL_PROMPT_INPUT_SHADOW_VALID.swap(1, AtomicOrdering::AcqRel) == 0 {
        shadow.clear();
    }
    let _ = shadow.push(byte as char);
}

#[cfg(feature = "kernel")]
fn prompt_shadow_pop() {
    let mut shadow = SERIAL_PROMPT_INPUT_SHADOW.lock();
    if SERIAL_PROMPT_INPUT_SHADOW_VALID.swap(1, AtomicOrdering::AcqRel) == 0 {
        shadow.clear();
    } else {
        let _ = shadow.pop();
    }
}

#[cfg(feature = "kernel")]
fn prompt_shadow_clear() {
    SERIAL_PROMPT_INPUT_SHADOW.lock().clear();
    SERIAL_PROMPT_INPUT_SHADOW_VALID.store(1, AtomicOrdering::Release);
}

/// Invalidate prompt-shadow bytes without acquiring a lock in Recovery.
///
/// The next ordinary serial mutation clears the retained buffer while holding
/// its existing lock. Prompt refresh suppresses the stale bytes until then.
#[cfg(feature = "kernel")]
pub(crate) fn invalidate_prompt_input_shadow_quiet() {
    SERIAL_PROMPT_INPUT_SHADOW_VALID.store(0, AtomicOrdering::Release);
}

#[cfg(feature = "kernel")]
pub(crate) fn emit_prompt_refresh_with_input_shadow_unlocked(prompt: &[u8]) {
    crate::sel4::debug_put_bytes_unlocked(prompt);
    if SERIAL_PROMPT_INPUT_SHADOW_VALID.load(AtomicOrdering::Acquire) == 0 {
        return;
    }
    let shadow = SERIAL_PROMPT_INPUT_SHADOW.lock();
    if SERIAL_PROMPT_INPUT_SHADOW_VALID.load(AtomicOrdering::Acquire) != 0 && !shadow.is_empty() {
        crate::sel4::debug_put_bytes_unlocked(shadow.as_bytes());
    }
}

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

    /// Whether every accepted TX byte has left the physical transmitter.
    ///
    /// Memory-backed and host drivers complete writes synchronously. Physical
    /// UART implementations override this with their hardware idle bit.
    fn transmitter_idle(&self) -> bool {
        true
    }

    /// Switch the live console endpoint to the linked serial driver task.
    #[cfg(feature = "kernel")]
    fn try_use_driver_task_client_after_attach(&mut self) -> bool {
        false
    }
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

fn serial_driver_task_rx_budget(
    contract: DriverTaskContract,
    available_capacity: usize,
) -> Option<crate::hal::driver_task::DriverTaskBudgetGrant> {
    let grant = crate::hal::driver_task::DriverTaskBudgetGrant::from_contract(contract);
    let max_bytes = (grant.max_bytes as usize)
        .min(usize::from(grant.max_ops))
        .min(usize::from(grant.max_frames))
        .min(crate::hal::driver_task::MAX_DRIVER_TASK_FRAME_BYTES)
        .min(available_capacity);
    if max_bytes == 0 {
        return None;
    }
    Some(crate::hal::driver_task::DriverTaskBudgetGrant {
        max_ops: grant.max_ops.min(saturating_u16(max_bytes)),
        max_frames: grant.max_frames.min(saturating_u16(max_bytes)),
        max_bytes: grant.max_bytes.min(max_bytes as u32),
    })
}

#[cfg(feature = "kernel")]
const fn serial_rx_queue_available<const RX: usize>(queued: usize) -> usize {
    RX.saturating_sub(1).saturating_sub(queued)
}

#[cfg(feature = "kernel")]
fn drain_serial_rx_queue_exact<const SOURCE: usize, const DESTINATION: usize>(
    source: &mut Queue<u8, SOURCE>,
    destination: &mut Queue<u8, DESTINATION>,
) -> Option<usize> {
    let mut accepted = 0usize;
    while serial_rx_queue_available::<DESTINATION>(destination.len()) != 0 {
        let Some(byte) = source.dequeue() else {
            break;
        };
        if destination.enqueue(byte).is_err() {
            // The sentinel-aware precondition makes this unreachable unless
            // the queue contract drifts. Report failure rather than treating
            // an already-dequeued byte as accepted.
            return None;
        }
        accepted = accepted.saturating_add(1);
    }
    Some(accepted)
}

#[cfg(feature = "kernel")]
fn serial_runtime_rx_turn_limit(budget: crate::hal::driver_task::DriverTaskBudgetGrant) -> usize {
    usize::from(budget.max_ops)
        .min(usize::from(budget.max_frames))
        .min(budget.max_bytes as usize)
        .min(crate::hal::driver_task::MAX_DRIVER_TASK_FRAME_BYTES)
}

/// Whether serial can be credited as driver-owned owner-state proof.
///
/// This is true only for the physical Pi linked-runtime path: root keeps the
/// ring client, while mini-UART MMIO/RX/TX progress runs inside the mapped
/// serial driver image.
#[must_use]
pub const fn serial_owner_state_acceptance_ready() -> bool {
    true
}

#[cfg(feature = "kernel")]
fn emit_serial_runtime_state(owner: &str, status: &str, acceptance: &str) {
    let mut line = HeaplessString::<160>::new();
    let _ = fmt::Write::write_fmt(
        &mut line,
        format_args!(
            "SERIAL_RUNTIME_STATE owner={owner} stage=serial-runtime-init status={status} acceptance={acceptance}",
        ),
    );
    crate::bootstrap::log::force_uart_line(line.as_str());
}

/// Construct the physical UART runtime behind the serial driver-task ring.
#[cfg(feature = "kernel")]
pub fn init_serial_driver_task_runtime() -> bool {
    if SERIAL_LINKED_RUNTIME_ATTACHED.load(AtomicOrdering::Acquire) != 0 {
        return true;
    }
    SERIAL_DRIVER_TASK_CLIENT_SERVICE_PROVEN.store(0, AtomicOrdering::Release);
    let contract = driver_task_contract();
    let physical_owner_state =
        crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active();
    if physical_owner_state && !initialize_serial_spsc_generation() {
        emit_serial_runtime_state("driver", "spsc-init-failed", "red");
        return false;
    }
    crate::hal::driver_task::register_driver_task_pointer_free_ring_service(
        contract,
        crate::hal::driver_task::DriverTaskHotPath::SerialConsole.as_u32() as usize,
        serial_runtime_ring_service_driver_task,
    );
    let mut command = crate::hal::driver_task::DriverTaskCommandRecord::pi4_hot_path(
        0,
        crate::hal::driver_task::DriverTaskHotPath::SerialConsole,
        crate::hal::driver_task::DriverTaskBudgetGrant::from_contract(contract),
        crate::hal::driver_task::DriverFrameDescriptor {
            offset: 0,
            len: 0,
            flags: 0,
        },
    );
    command.aux0 = SERIAL_RUNTIME_AUX_INIT;
    emit_serial_runtime_state("driver", "begin", "red");
    let completion =
        crate::hal::driver_task::run_driver_task_ring_service_bootstrap(contract, command);
    let ok = completion.is_some_and(|completion| {
        completion.code == crate::hal::driver_task::DriverTaskCompletionCode::Progress.as_u16()
            && completion.result == 1
    });
    let status = if ok {
        "ready"
    } else if completion.is_some() {
        "unexpected-completion"
    } else {
        "no-reply"
    };
    crate::hal::driver_task::emit_driver_task_resource_init_status(
        contract,
        crate::hal::driver_task::DriverTaskHotPath::SerialConsole,
        "serial-runtime-init",
        status,
        completion,
    );
    if ok {
        let owner_state_registered = serial_owner_state_descriptor().is_some_and(|descriptor| {
            crate::hal::driver_task::register_driver_task_owner_state_descriptor(
                contract, descriptor,
            )
        });
        if !owner_state_registered {
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                contract,
                crate::hal::driver_task::DriverTaskHotPath::SerialConsole,
                "serial-owner-state",
                "descriptor-rejected",
                None,
            );
            SERIAL_LINKED_RUNTIME_ATTACHED.store(0, AtomicOrdering::Release);
            SERIAL_DRIVER_TASK_CLIENT_SERVICE_PROVEN.store(0, AtomicOrdering::Release);
            emit_serial_runtime_state("driver", "owner-state-rejected", "red");
            return false;
        }
        if physical_owner_state {
            let generation = SERIAL_SPSC_GENERATION.load(AtomicOrdering::Acquire);
            if !publish_serial_generation_owned_state(
                generation,
                &SERIAL_LINKED_RUNTIME_ATTACHED,
                1,
                0,
            ) {
                SERIAL_DRIVER_TASK_CLIENT_SERVICE_PROVEN.store(0, AtomicOrdering::Release);
                SERIAL_RUNTIME_ATTACH_PHASE
                    .store(SERIAL_RUNTIME_ATTACH_FAILED, AtomicOrdering::Release);
                let _ = SERIAL_RUNTIME_INIT_LEASE.lock().take();
                emit_serial_runtime_state("driver", "generation-fenced", "red");
                return false;
            }
        } else {
            SERIAL_LINKED_RUNTIME_ATTACHED.store(1, AtomicOrdering::Release);
        }
        crate::hal::driver_task::emit_driver_task_resource_init_status(
            contract,
            crate::hal::driver_task::DriverTaskHotPath::SerialConsole,
            "serial-owner-state",
            "ready",
            completion,
        );
        emit_serial_runtime_state("driver", "ready", "green");
        crate::hal::driver_task::emit_owner_state_transition_boot_contract_proof(
            crate::hal::driver_task::DriverTaskHotPath::SerialConsole,
        );
    } else {
        SERIAL_LINKED_RUNTIME_ATTACHED.store(0, AtomicOrdering::Release);
        SERIAL_DRIVER_TASK_CLIENT_SERVICE_PROVEN.store(0, AtomicOrdering::Release);
        emit_serial_runtime_state("driver", status, "red");
    }
    if ok || completion.is_some() {
        let _ = SERIAL_RUNTIME_INIT_LEASE.lock().take();
    }
    ok
}

/// Outcome of one retained post-prompt serial-runtime attachment turn.
#[cfg(feature = "kernel")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SerialRuntimeAttachTurn {
    /// The exact descriptor, init, or proof action must resume on a later turn.
    Pending,
    /// Runtime init and one child service proof both completed exactly.
    Complete,
    /// The attach generation failed closed and must not be replayed.
    Failed,
}

#[cfg(feature = "kernel")]
const fn serial_runtime_attach_phase_is_descriptor(phase: u32, attached: bool) -> bool {
    !attached && phase == SERIAL_RUNTIME_ATTACH_DESCRIPTOR
}

/// Whether post-prompt serial attachment is still in its descriptor-only
/// phase, before the linked runtime may touch the UART.
///
/// Root may keep the emergency UART responsive while this phase advances. The
/// EventPump must establish a strict transmitter-idle fence before admitting
/// the later init ticket and must never return to root UART I/O afterwards.
#[cfg(feature = "kernel")]
#[must_use]
pub(crate) fn serial_runtime_attach_descriptor_phase_active() -> bool {
    serial_runtime_attach_phase_is_descriptor(
        SERIAL_RUNTIME_ATTACH_PHASE.load(AtomicOrdering::Acquire),
        serial_driver_task_runtime_attached(),
    )
}

/// Whether descriptor or runtime attachment has failed before the caller can
/// establish linked transport ownership.
#[cfg(feature = "kernel")]
#[must_use]
pub(crate) fn serial_runtime_attach_failed() -> bool {
    SERIAL_RUNTIME_ATTACH_PHASE.load(AtomicOrdering::Acquire) == SERIAL_RUNTIME_ATTACH_FAILED
}

/// Irreversibly transfer root UART authority to the linked-runtime lane.
///
/// This latch is published before the first INIT ticket because INIT may touch
/// UART MMIO before the linked client can truthfully report itself active.
/// There is intentionally no production reset or reclaim operation.
#[cfg(feature = "kernel")]
pub(crate) fn release_root_uart_for_linked_runtime() {
    SERIAL_ROOT_UART_RELEASED_FOR_LINKED_RUNTIME.store(1, AtomicOrdering::Release);
}

/// Whether root/current-TCB UART I/O is permanently forbidden for this boot.
#[cfg(feature = "kernel")]
#[must_use]
pub(crate) fn serial_root_uart_released_for_linked_runtime() -> bool {
    SERIAL_ROOT_UART_RELEASED_FOR_LINKED_RUNTIME.load(AtomicOrdering::Acquire) != 0
}

#[cfg(feature = "kernel")]
const fn serial_root_uart_direct_io_allowed(root_uart_released: bool) -> bool {
    !root_uart_released
}

/// Advance serial linked-runtime attachment by at most one child/HAL action.
#[cfg(feature = "kernel")]
pub(crate) fn service_serial_driver_task_runtime_after_prompt_turn() -> SerialRuntimeAttachTurn {
    if !crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active() {
        return SerialRuntimeAttachTurn::Complete;
    }
    let contract = driver_task_contract();
    let mut phase = SERIAL_RUNTIME_ATTACH_PHASE.load(AtomicOrdering::Acquire);
    if serial_driver_task_runtime_attached() && phase < SERIAL_RUNTIME_ATTACH_PROBE {
        let generation = SERIAL_SPSC_GENERATION.load(AtomicOrdering::Acquire);
        if !publish_serial_generation_owned_state(
            generation,
            &SERIAL_RUNTIME_ATTACH_PHASE,
            SERIAL_RUNTIME_ATTACH_PROBE,
            SERIAL_RUNTIME_ATTACH_FAILED,
        ) {
            return SerialRuntimeAttachTurn::Failed;
        }
        phase = SERIAL_RUNTIME_ATTACH_PROBE;
    }
    match phase {
        SERIAL_RUNTIME_ATTACH_DESCRIPTOR => {
            match crate::hal::driver_task::step_deferred_runtime_init_descriptor(
                contract,
                crate::hal::driver_task::DriverTaskHotPath::SerialConsole,
            ) {
                crate::hal::driver_task::DriverTaskDescriptorReplayTurn::Pending => {
                    SerialRuntimeAttachTurn::Pending
                }
                crate::hal::driver_task::DriverTaskDescriptorReplayTurn::Complete => {
                    if !initialize_serial_spsc_generation() {
                        SERIAL_RUNTIME_ATTACH_PHASE
                            .store(SERIAL_RUNTIME_ATTACH_FAILED, AtomicOrdering::Release);
                        return SerialRuntimeAttachTurn::Failed;
                    }
                    let generation = SERIAL_SPSC_GENERATION.load(AtomicOrdering::Acquire);
                    if !publish_serial_generation_owned_state(
                        generation,
                        &SERIAL_RUNTIME_ATTACH_PHASE,
                        SERIAL_RUNTIME_ATTACH_INIT,
                        SERIAL_RUNTIME_ATTACH_FAILED,
                    ) {
                        return SerialRuntimeAttachTurn::Failed;
                    }
                    SerialRuntimeAttachTurn::Pending
                }
                crate::hal::driver_task::DriverTaskDescriptorReplayTurn::Failed(_) => {
                    SERIAL_RUNTIME_ATTACH_PHASE
                        .store(SERIAL_RUNTIME_ATTACH_FAILED, AtomicOrdering::Release);
                    SerialRuntimeAttachTurn::Failed
                }
            }
        }
        SERIAL_RUNTIME_ATTACH_INIT => {
            if !serial_root_uart_released_for_linked_runtime() {
                SERIAL_RUNTIME_ATTACH_PHASE
                    .store(SERIAL_RUNTIME_ATTACH_FAILED, AtomicOrdering::Release);
                return SerialRuntimeAttachTurn::Failed;
            }
            SERIAL_DRIVER_TASK_CLIENT_SERVICE_PROVEN.store(0, AtomicOrdering::Release);
            crate::hal::driver_task::register_driver_task_pointer_free_ring_service(
                contract,
                crate::hal::driver_task::DriverTaskHotPath::SerialConsole.as_u32() as usize,
                serial_runtime_ring_service_driver_task,
            );
            let mut command = crate::hal::driver_task::DriverTaskCommandRecord::pi4_hot_path(
                0,
                crate::hal::driver_task::DriverTaskHotPath::SerialConsole,
                crate::hal::driver_task::DriverTaskBudgetGrant::from_contract(contract),
                crate::hal::driver_task::DriverFrameDescriptor {
                    offset: 0,
                    len: 0,
                    flags: 0,
                },
            );
            command.aux0 = SERIAL_RUNTIME_AUX_INIT;
            let Some(completion) =
                crate::hal::driver_task::run_driver_task_ring_service_retained_turn(
                    contract, command,
                )
            else {
                return SerialRuntimeAttachTurn::Pending;
            };
            let exact = completion.code
                == crate::hal::driver_task::DriverTaskCompletionCode::Progress.as_u16()
                && completion.detail == crate::hal::driver_task::DriverTaskFaultCode::None.as_u16()
                && completion.result == 1
                && completion.frame.offset == 0
                && completion.frame.len == 0
                && completion.frame.flags == 0;
            if !exact
                || !serial_owner_state_descriptor().is_some_and(|descriptor| {
                    crate::hal::driver_task::register_driver_task_owner_state_descriptor(
                        contract, descriptor,
                    )
                })
            {
                SERIAL_RUNTIME_ATTACH_PHASE
                    .store(SERIAL_RUNTIME_ATTACH_FAILED, AtomicOrdering::Release);
                SERIAL_LINKED_RUNTIME_ATTACHED.store(0, AtomicOrdering::Release);
                return SerialRuntimeAttachTurn::Failed;
            }
            let generation = SERIAL_SPSC_GENERATION.load(AtomicOrdering::Acquire);
            if !publish_serial_generation_owned_state(
                generation,
                &SERIAL_LINKED_RUNTIME_ATTACHED,
                1,
                0,
            ) {
                SERIAL_RUNTIME_ATTACH_PHASE
                    .store(SERIAL_RUNTIME_ATTACH_FAILED, AtomicOrdering::Release);
                return SerialRuntimeAttachTurn::Failed;
            }
            let _ = SERIAL_RUNTIME_INIT_LEASE.lock().take();
            if !publish_serial_generation_owned_state(
                generation,
                &SERIAL_RUNTIME_ATTACH_PHASE,
                SERIAL_RUNTIME_ATTACH_PROBE,
                SERIAL_RUNTIME_ATTACH_FAILED,
            ) {
                SERIAL_LINKED_RUNTIME_ATTACHED.store(0, AtomicOrdering::Release);
                return SerialRuntimeAttachTurn::Failed;
            }
            SerialRuntimeAttachTurn::Pending
        }
        SERIAL_RUNTIME_ATTACH_PROBE => {
            crate::hal::driver_task::register_driver_task_pointer_free_ring_service(
                contract,
                crate::hal::driver_task::DriverTaskHotPath::SerialConsole.as_u32() as usize,
                serial_runtime_ring_service_driver_task,
            );
            let mut command = crate::hal::driver_task::DriverTaskCommandRecord::pi4_hot_path(
                0,
                crate::hal::driver_task::DriverTaskHotPath::SerialConsole,
                crate::hal::driver_task::DriverTaskBudgetGrant::from_contract(contract),
                crate::hal::driver_task::DriverFrameDescriptor {
                    offset: 0,
                    len: 0,
                    flags: 0,
                },
            );
            command.aux0 = pi4_driver_abi::DRIVER_RUNTIME_SERIAL_SPSC_PROBE_AUX;
            let Some(completion) =
                crate::hal::driver_task::run_driver_task_ring_service_retained_turn(
                    contract, command,
                )
            else {
                return SerialRuntimeAttachTurn::Pending;
            };
            let generation = SERIAL_SPSC_GENERATION.load(AtomicOrdering::Acquire);
            let valid_probe = completion.code
                == crate::hal::driver_task::DriverTaskCompletionCode::Progress.as_u16()
                && completion.detail == crate::hal::driver_task::DriverTaskFaultCode::None.as_u16()
                && completion.result == generation
                && generation != 0
                && completion.frame.offset == 0
                && completion.frame.len == 0
                && completion.frame.flags == 0;
            if !valid_probe {
                SERIAL_RUNTIME_ATTACH_PHASE
                    .store(SERIAL_RUNTIME_ATTACH_FAILED, AtomicOrdering::Release);
                return SerialRuntimeAttachTurn::Failed;
            }
            if !publish_serial_generation_owned_state(
                generation,
                &SERIAL_DRIVER_TASK_CLIENT_SERVICE_PROVEN,
                1,
                0,
            ) || !publish_serial_generation_owned_state(
                generation,
                &SERIAL_RUNTIME_ATTACH_PHASE,
                SERIAL_RUNTIME_ATTACH_READY,
                SERIAL_RUNTIME_ATTACH_FAILED,
            ) {
                SERIAL_LINKED_RUNTIME_ATTACHED.store(0, AtomicOrdering::Release);
                SERIAL_DRIVER_TASK_CLIENT_SERVICE_PROVEN.store(0, AtomicOrdering::Release);
                return SerialRuntimeAttachTurn::Failed;
            }
            SerialRuntimeAttachTurn::Complete
        }
        SERIAL_RUNTIME_ATTACH_READY => SerialRuntimeAttachTurn::Complete,
        _ => SerialRuntimeAttachTurn::Failed,
    }
}

/// Returns whether the physical serial runtime is attached to its driver task.
#[cfg(feature = "kernel")]
#[must_use]
pub fn serial_driver_task_runtime_attached() -> bool {
    SERIAL_LINKED_RUNTIME_ATTACHED.load(AtomicOrdering::Acquire) != 0
        || SERIAL_DRIVER_RUNTIME.lock().is_some()
}

#[cfg(feature = "kernel")]
const fn serial_driver_task_transport_required(
    owner_state_active: bool,
    runtime_attached: bool,
    client_active: bool,
) -> bool {
    owner_state_active && runtime_attached && client_active
}

#[cfg(feature = "kernel")]
const fn serial_driver_task_interactive_cutover_policy(
    owner_state_active: bool,
    runtime_attached: bool,
    service_proven: bool,
) -> bool {
    runtime_attached && (!owner_state_active || service_proven)
}

#[cfg(feature = "kernel")]
pub(crate) fn serial_driver_task_interactive_cutover_allowed() -> bool {
    serial_driver_task_interactive_cutover_policy(
        crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active(),
        serial_driver_task_runtime_attached(),
        SERIAL_DRIVER_TASK_CLIENT_SERVICE_PROVEN.load(AtomicOrdering::Acquire) != 0,
    )
}

#[cfg(feature = "kernel")]
pub(crate) fn serial_linked_runtime_transport_active() -> bool {
    serial_driver_task_transport_active()
}

#[cfg(feature = "kernel")]
fn serial_driver_task_transport_active() -> bool {
    #[cfg(test)]
    if SERIAL_LINKED_RUNTIME_ONLY_TEST_ACTIVE.load(AtomicOrdering::Acquire) != 0 {
        return true;
    }
    serial_driver_task_transport_required(
        crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active(),
        serial_driver_task_runtime_attached(),
        SERIAL_DRIVER_TASK_CLIENT_ACTIVE.load(AtomicOrdering::Acquire) != 0,
    )
}

#[cfg(all(feature = "kernel", test))]
pub(crate) fn test_begin_linked_runtime_only_transport() {
    {
        let mut rx = SERIAL_LINKED_RUNTIME_ONLY_TEST_RX.lock();
        while rx.dequeue().is_some() {}
    }
    {
        let mut tx = SERIAL_LINKED_RUNTIME_ONLY_TEST_TX.lock();
        while tx.dequeue().is_some() {}
    }
    SERIAL_LINKED_RUNTIME_ONLY_TEST_TX_IDLE_MISSES.store(0, AtomicOrdering::Release);
    SERIAL_LINKED_RUNTIME_ONLY_TEST_ACTIVE.store(1, AtomicOrdering::Release);
}

#[cfg(all(feature = "kernel", test))]
pub(crate) fn test_end_linked_runtime_only_transport() {
    SERIAL_LINKED_RUNTIME_ONLY_TEST_ACTIVE.store(0, AtomicOrdering::Release);
    SERIAL_LINKED_RUNTIME_ONLY_TEST_TX_IDLE_MISSES.store(0, AtomicOrdering::Release);
    {
        let mut rx = SERIAL_LINKED_RUNTIME_ONLY_TEST_RX.lock();
        while rx.dequeue().is_some() {}
    }
    {
        let mut tx = SERIAL_LINKED_RUNTIME_ONLY_TEST_TX.lock();
        while tx.dequeue().is_some() {}
    }
}

#[cfg(all(feature = "kernel", test))]
pub(crate) fn test_set_linked_runtime_only_tx_idle_misses(misses: u32) {
    SERIAL_LINKED_RUNTIME_ONLY_TEST_TX_IDLE_MISSES.store(misses, AtomicOrdering::Release);
}

#[cfg(all(feature = "kernel", test))]
pub(crate) fn test_inject_linked_runtime_only_rx(bytes: &[u8]) -> usize {
    let mut queue = SERIAL_LINKED_RUNTIME_ONLY_TEST_RX.lock();
    let mut accepted = 0usize;
    for &byte in bytes {
        if queue.enqueue(byte).is_err() {
            break;
        }
        accepted = accepted.saturating_add(1);
    }
    accepted
}

#[cfg(all(feature = "kernel", test))]
pub(crate) fn test_take_linked_runtime_only_tx() -> heapless::Vec<u8, DEFAULT_TX_CAPACITY> {
    let mut queue = SERIAL_LINKED_RUNTIME_ONLY_TEST_TX.lock();
    let mut bytes = heapless::Vec::new();
    while let Some(byte) = queue.dequeue() {
        let _ = bytes.push(byte);
    }
    bytes
}

#[cfg(feature = "kernel")]
const fn serial_driver_task_rx_completion_proves_input(
    completion_code: u16,
    accepted: usize,
) -> bool {
    completion_code == crate::hal::driver_task::DriverTaskCompletionCode::FrameReady.as_u16()
        && accepted != 0
}

#[cfg(feature = "kernel")]
const fn serial_root_context_service_allowed_policy(owner_state_active: bool) -> bool {
    !owner_state_active
}

#[cfg(feature = "kernel")]
fn serial_root_context_service_allowed() -> bool {
    serial_root_context_service_allowed_policy(
        crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active(),
    )
}

#[cfg(feature = "kernel")]
fn serial_input_route_label() -> &'static str {
    if SERIAL_DRIVER_TASK_CLIENT_ACTIVE.load(AtomicOrdering::Acquire) != 0 {
        "driver-task-serial-client"
    } else if crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active() {
        "bcm2711-mini-uart"
    } else {
        "profile-serial-driver"
    }
}

#[cfg(feature = "kernel")]
const SERIAL_INPUT_RX_TRACE_LIMIT: u32 = 1;
#[cfg(feature = "kernel")]
const SERIAL_INPUT_LINE_TRACE_LIMIT: u32 = 1;

#[cfg(feature = "kernel")]
fn serial_input_trace_budget_take(counter: &AtomicU32, limit: u32) -> bool {
    counter.fetch_add(1, AtomicOrdering::AcqRel) < limit
}

#[cfg(feature = "kernel")]
const fn serial_input_poll_trace_allowed(
    accepted: usize,
    ordinary_root_control_turn_active: bool,
) -> bool {
    accepted != 0 && !ordinary_root_control_turn_active
}

#[cfg(all(feature = "kernel", target_arch = "aarch64", target_os = "none"))]
fn emit_serial_input_trace(line: &str) {
    crate::bootstrap::log::force_uart_line_raw(line);
}

#[cfg(all(
    feature = "kernel",
    not(all(target_arch = "aarch64", target_os = "none"))
))]
fn emit_serial_input_trace(_line: &str) {}

#[cfg(feature = "kernel")]
pub(crate) fn emit_serial_input_route_trace(stage: &str, reason: &str) {
    let mut line = HeaplessString::<224>::new();
    let _ = fmt::Write::write_fmt(
        &mut line,
        format_args!(
            "SERIAL_INPUT_TRACE stage={stage} route={} driver_runtime_attached={} client_active={} rx_proven={} root_context_service={} reason={reason}",
            serial_input_route_label(),
            serial_driver_task_runtime_attached() as u8,
            (SERIAL_DRIVER_TASK_CLIENT_ACTIVE.load(AtomicOrdering::Acquire) != 0) as u8,
            (SERIAL_DRIVER_TASK_CLIENT_RX_PROVEN.load(AtomicOrdering::Acquire) != 0) as u8,
            if serial_root_context_service_allowed() { "allowed" } else { "skipped" },
        ),
    );
    emit_serial_input_trace(line.as_str());
}

#[cfg(feature = "kernel")]
fn emit_serial_input_route_trace_once(stage: &str, reason: &str) {
    if SERIAL_INPUT_ROUTE_LOGGED
        .compare_exchange(0, 1, AtomicOrdering::AcqRel, AtomicOrdering::Acquire)
        .is_ok()
    {
        emit_serial_input_route_trace(stage, reason);
    }
}

#[cfg(feature = "kernel")]
fn emit_serial_input_poll_trace(
    stage: &str,
    count: usize,
    rx_depth: usize,
    line_len: usize,
    first: u8,
    last: u8,
) {
    if !serial_input_trace_budget_take(&SERIAL_INPUT_RX_TRACE_COUNT, SERIAL_INPUT_RX_TRACE_LIMIT) {
        return;
    }
    let mut line = HeaplessString::<192>::new();
    let _ = fmt::Write::write_fmt(
        &mut line,
        format_args!(
            "SERIAL_INPUT_TRACE stage={stage} route={} bytes={count} rx_depth={rx_depth} line_len={line_len} first=0x{first:02x} last=0x{last:02x}",
            serial_input_route_label(),
        ),
    );
    emit_serial_input_trace(line.as_str());
}

#[cfg(feature = "kernel")]
fn emit_serial_input_line_trace(stage: &str, len: usize, rx_depth: usize, line_len: usize) {
    if !serial_input_trace_budget_take(
        &SERIAL_INPUT_LINE_TRACE_COUNT,
        SERIAL_INPUT_LINE_TRACE_LIMIT,
    ) {
        return;
    }
    let mut line = HeaplessString::<160>::new();
    let _ = fmt::Write::write_fmt(
        &mut line,
        format_args!(
            "SERIAL_INPUT_TRACE stage={stage} route={} line_len={len} rx_depth={rx_depth} partial_len={line_len}",
            serial_input_route_label(),
        ),
    );
    emit_serial_input_trace(line.as_str());
}

#[cfg(feature = "kernel")]
pub(crate) fn emit_serial_input_consume_trace(len: usize) {
    emit_serial_input_line_trace("consume-line", len, 0, 0);
}

#[cfg(feature = "kernel")]
pub(crate) fn emit_serial_input_idle_trace(now_ms: u64, tx_pending: bool) {
    let mut line = HeaplessString::<192>::new();
    let _ = fmt::Write::write_fmt(
        &mut line,
        format_args!(
            "SERIAL_INPUT_TRACE stage=idle route={} now_ms={now_ms} driver_runtime_attached={} client_active={} rx_proven={} tx_pending={}",
            serial_input_route_label(),
            serial_driver_task_runtime_attached() as u8,
            (SERIAL_DRIVER_TASK_CLIENT_ACTIVE.load(AtomicOrdering::Acquire) != 0) as u8,
            (SERIAL_DRIVER_TASK_CLIENT_RX_PROVEN.load(AtomicOrdering::Acquire) != 0) as u8,
            tx_pending as u8,
        ),
    );
    emit_serial_input_trace(line.as_str());
}

#[cfg(feature = "kernel")]
pub(crate) fn driver_task_client_read_byte() -> nb::Result<u8, SerialError> {
    if let Some(byte) = SERIAL_CLIENT_RX.lock().dequeue() {
        return Ok(byte);
    }
    if driver_task_client_poll_rx_into_client_queue() == 0 {
        return Err(NbError::WouldBlock);
    }
    SERIAL_CLIENT_RX.lock().dequeue().ok_or(NbError::WouldBlock)
}

#[cfg(feature = "kernel")]
pub(crate) fn preserve_driver_task_rx_after_raw_uart() {
    if !serial_driver_task_transport_active() {
        return;
    }
    // Raw UART logging can run from diagnostic paths that must never block on a
    // driver-task reply. Prompt polling performs the real bounded RX service.
}

#[cfg(feature = "kernel")]
fn driver_task_client_poll_rx_into_client_queue() -> usize {
    // Keep the queue locked across SPSC consumption and local enqueue. A
    // `heapless::spsc::Queue<N>` deliberately reserves one sentinel slot, so
    // using `N - len` here would commit-consume one byte that cannot be queued
    // at the exact full boundary.
    let mut rx = SERIAL_CLIENT_RX.lock();
    let available_capacity = serial_rx_queue_available::<DEFAULT_RX_CAPACITY>(rx.len());
    if available_capacity == 0 {
        return 0;
    }
    let generation = SERIAL_SPSC_GENERATION.load(AtomicOrdering::Acquire);
    if generation == 0 {
        return 0;
    }
    let mut staged = [0u8; DEFAULT_RX_CAPACITY];
    let Some(transfer) = crate::hal::driver_task::driver_task_serial_spsc_dequeue_rx(
        generation,
        &mut staged[..available_capacity],
    ) else {
        return 0;
    };
    let mut accepted = 0usize;
    for &byte in &staged[..transfer.bytes] {
        if rx.enqueue(byte).is_err() {
            break;
        }
        accepted = accepted.saturating_add(1);
    }
    drop(rx);
    if accepted != transfer.bytes {
        // The locked sentinel-bounded capacity makes this unreachable unless
        // the local queue contract itself drifts. Fail the linked transport
        // closed rather than silently dropping an already-consumed byte.
        SERIAL_DRIVER_TASK_CLIENT_ACTIVE.store(0, AtomicOrdering::Release);
        SERIAL_DRIVER_TASK_CLIENT_SERVICE_PROVEN.store(0, AtomicOrdering::Release);
        SERIAL_DRIVER_TASK_CLIENT_RX_PROVEN.store(0, AtomicOrdering::Release);
        SERIAL_RUNTIME_ATTACH_PHASE.store(SERIAL_RUNTIME_ATTACH_FAILED, AtomicOrdering::Release);
        return 0;
    }
    if accepted != 0 {
        let _ = publish_serial_generation_owned_state(
            generation,
            &SERIAL_DRIVER_TASK_CLIENT_RX_PROVEN,
            1,
            0,
        );
    }
    accepted
}

#[cfg(feature = "kernel")]
const fn serial_driver_task_service_completion_proves_transport(code: u16) -> bool {
    code == crate::hal::driver_task::DriverTaskCompletionCode::Idle.as_u16()
        || code == crate::hal::driver_task::DriverTaskCompletionCode::Progress.as_u16()
        || code == crate::hal::driver_task::DriverTaskCompletionCode::FrameReady.as_u16()
}

#[cfg(feature = "kernel")]
pub(crate) fn driver_task_client_write_byte(byte: u8) -> nb::Result<(), SerialError> {
    let generation = SERIAL_SPSC_GENERATION.load(AtomicOrdering::Acquire);
    if generation == 0 {
        return Err(NbError::Other(SerialError::DeviceFault));
    }
    let Some(transfer) =
        crate::hal::driver_task::driver_task_serial_spsc_enqueue_tx(generation, &[byte])
    else {
        return Err(NbError::Other(SerialError::DeviceFault));
    };
    if transfer.bytes != 1 {
        return Err(NbError::WouldBlock);
    }
    Ok(())
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
        SERIAL_OWNER_DESCRIPTOR_FLAGS,
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
    ordinary_root_control_turn: OrdinaryRootControlSerialTurn,
    #[cfg(feature = "net-backend-virtio")]
    routine_audit_only_pending: bool,
    #[cfg(feature = "net-backend-virtio")]
    routine_audit_tx_attempts_left: Option<u8>,
    #[cfg(feature = "kernel")]
    root_context_rx_only_service: bool,
    #[cfg(feature = "kernel")]
    linked_tx: LinkedSerialTxCursor,
    #[cfg(feature = "kernel")]
    linked_tx_idle: LinkedSerialTxIdleCursor,
    #[cfg(feature = "kernel")]
    linked_rx_command: Option<crate::hal::driver_task::DriverTaskCommandRecord>,
    #[cfg(feature = "kernel")]
    linked_rx_ticket: u64,
    #[cfg(feature = "kernel")]
    linked_turn_id: u64,
    #[cfg(feature = "kernel")]
    linked_rx_due: bool,
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
            ordinary_root_control_turn: OrdinaryRootControlSerialTurn::default(),
            #[cfg(feature = "net-backend-virtio")]
            routine_audit_only_pending: false,
            #[cfg(feature = "net-backend-virtio")]
            routine_audit_tx_attempts_left: None,
            #[cfg(feature = "kernel")]
            root_context_rx_only_service: false,
            #[cfg(feature = "kernel")]
            linked_tx: LinkedSerialTxCursor::new(),
            #[cfg(feature = "kernel")]
            linked_tx_idle: LinkedSerialTxIdleCursor::new(),
            #[cfg(feature = "kernel")]
            linked_rx_command: None,
            #[cfg(feature = "kernel")]
            linked_rx_ticket: 0,
            #[cfg(feature = "kernel")]
            linked_turn_id: 0,
            #[cfg(feature = "kernel")]
            linked_rx_due: false,
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
            self.logical_tx_len(),
            self.line.len(),
            self.telemetry(),
        )
    }

    /// Whether bytes remain staged for the serial TX driver.
    #[must_use]
    pub fn tx_pending(&self) -> bool {
        self.logical_tx_len() != 0
    }

    /// Whether the complete ordinary TX backlog belongs only to one deferred
    /// QEMU routine-audit record.
    ///
    /// The isolated Operator may let network work preempt retries only while
    /// this provenance remains exact. Every ordinary TX admission clears it.
    #[cfg(feature = "net-backend-virtio")]
    #[must_use]
    pub(crate) fn routine_audit_only_pending(&self) -> bool {
        self.routine_audit_only_pending && self.tx_pending()
    }

    /// Atomically stage one low-priority QEMU routine-audit line.
    ///
    /// Admission requires the complete ordinary TX path to be empty so later
    /// retries can be identified without inspecting or reordering bytes.
    #[cfg(feature = "net-backend-virtio")]
    pub(crate) fn try_enqueue_routine_audit_line_record(&mut self, line: &str) -> bool {
        if self.tx_pending() {
            return false;
        }
        let total = line.len().saturating_add(2);
        if total > TX.saturating_sub(1) {
            self.telemetry.tx_overflow();
            return false;
        }
        let mut admitted = 0usize;
        for &byte in line.as_bytes().iter().chain(b"\r\n") {
            if self.tx.enqueue(byte).is_err() {
                self.telemetry.tx_overflow();
                while admitted != 0 {
                    let _ = self.tx.dequeue();
                    admitted = admitted.saturating_sub(1);
                }
                self.routine_audit_only_pending = false;
                return false;
            }
            admitted = admitted.saturating_add(1);
        }
        self.routine_audit_only_pending = true;
        true
    }

    #[cfg(feature = "net-backend-virtio")]
    fn promote_routine_audit_tx(&mut self) {
        self.routine_audit_only_pending = false;
    }

    #[cfg(feature = "net-backend-virtio")]
    fn clear_routine_audit_tx_if_drained(&mut self) {
        if !self.tx_pending() {
            self.routine_audit_only_pending = false;
        }
    }

    /// Attempt at most one physical TX byte from the tagged routine-audit record.
    ///
    /// The limit is carried through the synchronous QEMU root-context callback.
    /// `WouldBlock` keeps the exact byte and record tag for a later Operator
    /// visit. Ordinary flushes do not inherit this private diagnostic limit.
    #[cfg(feature = "net-backend-virtio")]
    pub(crate) fn flush_one_routine_audit_tx_byte(&mut self) {
        if !self.routine_audit_only_pending() {
            return;
        }
        debug_assert!(self.routine_audit_tx_attempts_left.is_none());
        self.routine_audit_tx_attempts_left = Some(1);
        self.flush_tx_locked();
        self.routine_audit_tx_attempts_left = None;
        self.clear_routine_audit_tx_if_drained();
    }

    #[cfg(feature = "net-backend-virtio")]
    fn routine_audit_tx_attempt_available(&self) -> bool {
        !matches!(self.routine_audit_tx_attempts_left, Some(0))
    }

    #[cfg(feature = "net-backend-virtio")]
    fn charge_routine_audit_tx_attempt(&mut self) {
        if let Some(attempts_left) = self.routine_audit_tx_attempts_left.as_mut() {
            *attempts_left = attempts_left.saturating_sub(1);
        }
    }

    /// Whether pre-cutover software queues and the selected UART are both idle.
    #[cfg(feature = "kernel")]
    pub(crate) fn root_uart_cutover_drained(&self) -> bool {
        !self.tx_pending() && self.driver.transmitter_idle()
    }

    /// Classify whether all accepted TX bytes have completed without confusing
    /// poison cleanup with successful drain.
    #[must_use]
    pub(crate) fn tx_drain_outcome(&self) -> SerialTxDrainOutcome {
        #[cfg(feature = "kernel")]
        {
            if self.linked_tx.poisoned || self.linked_tx_idle.poisoned {
                return SerialTxDrainOutcome::Failed;
            }
            if self.linked_tx_idle.required || self.linked_tx_idle.command.is_some() {
                return SerialTxDrainOutcome::Pending;
            }
        }
        if self.tx_pending() {
            SerialTxDrainOutcome::Pending
        } else {
            SerialTxDrainOutcome::Complete
        }
    }

    fn logical_tx_len(&self) -> usize {
        let pending = self
            .tx
            .len()
            .saturating_add(usize::from(self.driver_local.pending_tx_byte().is_some()));
        #[cfg(feature = "kernel")]
        {
            pending.saturating_add(self.linked_tx.bytes.len())
        }
        #[cfg(not(feature = "kernel"))]
        {
            pending
        }
    }

    #[cfg(feature = "kernel")]
    fn next_linked_turn_id(&mut self) -> u64 {
        self.linked_turn_id = self.linked_turn_id.wrapping_add(1).max(1);
        self.linked_turn_id
    }

    #[cfg(feature = "kernel")]
    fn poison_linked_tx(&mut self) {
        let newly_poisoned = !self.linked_tx.poisoned || !self.linked_tx_idle.poisoned;
        self.linked_tx.poison();
        self.linked_tx_idle.poison();
        let _ = self.driver_local.take_pending_tx();
        while self.tx.dequeue().is_some() {}
        #[cfg(feature = "net-backend-virtio")]
        self.promote_routine_audit_tx();
        if newly_poisoned {
            self.telemetry.driver_task_budget_overrun();
        }
    }

    #[cfg(all(feature = "kernel", test))]
    pub(crate) fn test_poison_linked_tx(&mut self) {
        self.poison_linked_tx();
    }

    /// Whether serial input is already waiting in the command path.
    #[must_use]
    pub fn interactive_input_active(&self) -> bool {
        !self.rx.is_empty() || !self.line.is_empty()
    }

    /// HAL scheduling contract consumed by this port.
    #[must_use]
    pub fn driver_task_contract(&self) -> DriverTaskContract {
        <D as SerialDriver>::driver_task_contract()
    }

    /// Begin one isolated VirtIO root-control Operator turn.
    ///
    /// Only the EventPump phase wrapper calls this. Linked physical serial and
    /// unsplit non-VirtIO profiles leave the cursor inactive and retain their
    /// existing service behavior.
    pub(crate) fn begin_ordinary_root_control_turn(&mut self) {
        self.begin_ordinary_root_control_turn_with_limit(
            crate::generated::ROOT_CONTROL_VIRTIO_OPERATOR_SERIAL_IO_BYTES_PER_TURN,
        );
    }

    fn begin_ordinary_root_control_turn_with_limit(&mut self, byte_limit: u32) {
        let tx_pending_at_turn_start = self.tx_pending();
        self.ordinary_root_control_turn
            .begin(byte_limit, tx_pending_at_turn_start);
    }

    /// Finish the isolated VirtIO root-control Operator turn.
    pub(crate) fn finish_ordinary_root_control_turn(&mut self) {
        self.ordinary_root_control_turn.finish();
    }

    /// Inject data that should be transmitted to the remote peer.
    pub fn enqueue_tx(&mut self, data: &[u8]) {
        #[cfg(feature = "kernel")]
        if self.linked_tx.poisoned {
            self.telemetry.tx_overflow();
            return;
        }
        let capacity = TX.saturating_sub(1);
        'bytes: for &byte in data {
            let mut attempts = 0usize;
            while self.logical_tx_len() >= capacity {
                self.telemetry.tx_overflow();
                let before = self.logical_tx_len();
                self.flush_tx();
                attempts = attempts.saturating_add(1);
                if attempts > TX || self.logical_tx_len() >= before {
                    break 'bytes;
                }
            }
            #[cfg(feature = "net-backend-virtio")]
            self.promote_routine_audit_tx();
            if self.tx.enqueue(byte).is_err() {
                self.telemetry.tx_overflow();
                break;
            }
        }
    }

    /// Attempt to stage TX bytes without flushing or retrying on saturation.
    ///
    /// This is for secondary mirrors where the caller must keep the event loop
    /// moving even if the serial peer is slow. It records one backpressure event
    /// and returns as soon as the queue is full.
    pub fn enqueue_tx_best_effort(&mut self, data: &[u8]) -> usize {
        #[cfg(feature = "kernel")]
        if self.linked_tx.poisoned {
            self.telemetry.tx_overflow();
            return 0;
        }
        let mut accepted = 0usize;
        for &byte in data {
            if self.logical_tx_len() >= TX.saturating_sub(1) {
                self.telemetry.tx_overflow();
                break;
            }
            #[cfg(feature = "net-backend-virtio")]
            self.promote_routine_audit_tx();
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
    /// Echoes and edit-control records must never synchronously flush the UART
    /// while input is being parsed. Under output pressure the input line still
    /// wins and the echo record is dropped whole.
    pub fn try_enqueue_tx_record(&mut self, parts: &[&[u8]]) -> bool {
        #[cfg(feature = "kernel")]
        if self.linked_tx.poisoned {
            self.telemetry.tx_overflow();
            return false;
        }
        let mut total = 0usize;
        for part in parts {
            total = total.saturating_add(part.len());
        }
        let pending = self.logical_tx_len();
        let capacity = TX.saturating_sub(1);
        if total > capacity.saturating_sub(pending) {
            self.telemetry.tx_overflow();
            return false;
        }
        #[cfg(feature = "net-backend-virtio")]
        if total != 0 {
            self.promote_routine_audit_tx();
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
        #[cfg(feature = "net-backend-virtio")]
        self.clear_routine_audit_tx_if_drained();
    }

    /// Emit bytes directly to the device while holding the shared UART TX lock.
    pub fn write_bytes_blocking(&mut self, data: &[u8]) {
        if self.ordinary_root_control_turn.active() {
            self.enqueue_tx(data);
            self.flush_tx_locked();
            return;
        }
        #[cfg(feature = "kernel")]
        if serial_driver_task_transport_active() {
            self.enqueue_tx(data);
            self.flush_tx_locked();
            return;
        }
        #[cfg(feature = "kernel")]
        if !serial_root_uart_direct_io_allowed(serial_root_uart_released_for_linked_runtime()) {
            self.enqueue_tx(data);
            return;
        }
        with_uart_tx_lock(|| {
            self.flush_tx_blocking_unlocked();
            for &byte in data {
                self.write_byte_blocking_unlocked(byte);
            }
        });
    }

    /// Emit a complete console line without allowing other UART producers to interleave.
    pub fn write_line_blocking(&mut self, line: &str) {
        if self.ordinary_root_control_turn.active() {
            self.enqueue_tx(line.as_bytes());
            self.enqueue_tx(b"\r\n");
            self.flush_tx_locked();
            return;
        }
        #[cfg(feature = "kernel")]
        if serial_driver_task_transport_active() {
            self.enqueue_tx(line.as_bytes());
            self.enqueue_tx(b"\r\n");
            self.flush_tx_locked();
            return;
        }
        #[cfg(feature = "kernel")]
        if !serial_root_uart_direct_io_allowed(serial_root_uart_released_for_linked_runtime()) {
            self.enqueue_tx(line.as_bytes());
            self.enqueue_tx(b"\r\n");
            return;
        }
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
            if serial_driver_task_transport_active() {
                return self.poll_driver_task_rx_into_queue(contract);
            }
            if !serial_root_uart_direct_io_allowed(serial_root_uart_released_for_linked_runtime()) {
                return false;
            }
            if serial_root_context_service_allowed() {
                if let Some(activity) = self.poll_root_context_io(contract, false) {
                    return activity;
                }
                if !crate::hal::driver_task::admit_root_task_compatibility_service(contract) {
                    self.telemetry.driver_task_budget_overrun();
                    return false;
                }
            } else {
                emit_serial_input_route_trace_once(
                    "poll-route",
                    "physical-root-mini-uart-fallback",
                );
            }
        }
        self.poll_io_current_tcb(contract)
    }

    /// Probe serial RX without admitting any TX transport or flush work.
    ///
    /// The isolated QEMU Operator uses this as its complete `SerialIo` unit;
    /// a later retained `SerialDispatch` unit owns parsing, echo, and TX. The
    /// public `poll_io` path remains the combined legacy/generic operation.
    pub(crate) fn poll_rx_only(&mut self) -> bool {
        let contract = <D as SerialDriver>::driver_task_contract();
        #[cfg(feature = "kernel")]
        {
            if serial_driver_task_transport_active() {
                return self.poll_driver_task_rx_into_queue(contract);
            }
            if !serial_root_uart_direct_io_allowed(serial_root_uart_released_for_linked_runtime()) {
                return false;
            }
            if serial_root_context_service_allowed() {
                if let Some(activity) = self.poll_root_context_io(contract, true) {
                    return activity;
                }
                if !crate::hal::driver_task::admit_root_task_compatibility_service(contract) {
                    self.telemetry.driver_task_budget_overrun();
                    return false;
                }
            } else {
                emit_serial_input_route_trace_once(
                    "poll-route",
                    "physical-root-mini-uart-fallback",
                );
            }
        }
        self.poll_rx_only_current_tcb(contract)
    }

    #[cfg(feature = "kernel")]
    fn poll_root_context_io(
        &mut self,
        contract: DriverTaskContract,
        rx_only: bool,
    ) -> Option<bool> {
        self.root_context_rx_only_service = rx_only;
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
                flags: crate::hal::driver_task::DRIVER_TASK_RING_FLAG_ROOT_CONTEXT_NON_ACCEPTANCE,
            },
        );
        let completion = crate::hal::driver_task::run_driver_task_ring_service(contract, command);
        self.root_context_rx_only_service = false;
        if let Some(completion) = completion {
            return Some(
                completion.code
                    == crate::hal::driver_task::DriverTaskCompletionCode::Progress.as_u16()
                    && completion.result != 0,
            );
        }

        self.root_context_rx_only_service = rx_only;
        // SAFETY: The HAL admits this compatibility callback only for QEMU/host
        // profiles. Physical Pi 4 builds return None without compiling callback
        // slot state, and the synchronous call returns before the mode is reset.
        let result = unsafe {
            crate::hal::driver_task::try_driver_task_compat_service(
                contract,
                self as *mut Self as usize,
                serial_poll_io_driver_task::<D, RX, TX, LINE>,
            )
        };
        self.root_context_rx_only_service = false;
        result.map(|activity| activity != 0)
    }

    /// Poll only the independently owned physical serial linked runtime.
    ///
    /// This is the narrow operator-liveness route used while another physical
    /// driver owns the root HAL. It fails closed unless the serial driver-task
    /// transport has completed cutover, and it never falls through to a
    /// root-context callback or the current-TCB UART driver.
    #[cfg(feature = "kernel")]
    pub fn poll_io_linked_runtime_only(&mut self) -> bool {
        if !serial_driver_task_transport_active() {
            return false;
        }
        #[cfg(test)]
        if SERIAL_LINKED_RUNTIME_ONLY_TEST_ACTIVE.load(AtomicOrdering::Acquire) != 0 {
            let mut staged = SERIAL_LINKED_RUNTIME_ONLY_TEST_RX.lock();
            let mut accepted = 0usize;
            while self.rx.len() < RX {
                let Some(byte) = staged.dequeue() else {
                    break;
                };
                if self.rx.enqueue(byte).is_err() {
                    self.telemetry.rx_overflow();
                    break;
                }
                accepted = accepted.saturating_add(1);
            }
            let activity = accepted != 0;
            // A frame that consumed its RX grant can leave the runtime's
            // combined mini-UART IRQ handler active. Preserve one explicit
            // owner turn to re-sample and ACK that level before TX is allowed
            // to occupy the reciprocal command ring.
            self.linked_rx_due = activity;
            return activity;
        }
        let contract = <D as SerialDriver>::driver_task_contract();
        match self.poll_driver_task_rx_turn(contract) {
            LinkedSerialTurnOutcome::Pending => false,
            LinkedSerialTurnOutcome::Complete { activity } => {
                // An active RX completion proves bytes were copied, not that
                // the runtime had grant left to rearm the shared UART IRQ.
                // Keep RX authoritative until one subsequent empty completion
                // closes that handshake; otherwise TX can strand after the
                // first hardware FIFO quantum behind the unacknowledged IRQ.
                self.linked_rx_due = activity;
                activity
            }
            LinkedSerialTurnOutcome::Failed => {
                self.linked_rx_due = false;
                false
            }
        }
    }

    /// Execute at most one linked-runtime serial operation for an outer turn.
    ///
    /// An issued request remains authoritative until its exact completion.
    /// Completed TX chunks force an RX turn before another chunk, preventing a
    /// startup transcript from starving serial commands or an authenticated
    /// reboot. A poisoned TX action remains fail-closed without blocking RX.
    #[cfg(feature = "kernel")]
    pub fn service_linked_runtime_only_turn(&mut self) -> bool {
        if self.linked_rx_command.is_some() {
            return self.poll_io_linked_runtime_only();
        }
        if self.linked_tx.command.is_some() {
            let _ = self.flush_tx_linked_runtime_only();
            return false;
        }
        if self.linked_tx.poisoned || self.linked_tx_idle.poisoned {
            return self.poll_io_linked_runtime_only();
        }
        if self.linked_tx_idle.command.is_some() {
            let _ = self.poll_linked_tx_idle_turn();
            return false;
        }
        if self.linked_rx_due || self.linked_tx.poisoned || !self.tx_pending() {
            if self.linked_rx_due || !self.linked_tx_idle.required {
                return self.poll_io_linked_runtime_only();
            }
            let _ = self.poll_linked_tx_idle_turn();
            return false;
        }
        let _ = self.flush_tx_linked_runtime_only();
        false
    }

    /// Execute one retained child-runtime poll of the physical UART idle bit.
    ///
    /// A valid busy sample completes only that sample. It schedules an RX turn
    /// before a fresh idle ticket so a slow transmitter cannot starve operator
    /// input. No call path polls privately or replays an issued-unknown command.
    #[cfg(feature = "kernel")]
    pub(crate) fn poll_linked_tx_idle_turn(&mut self) -> SerialTxIdleTurnOutcome {
        if self.linked_tx.poisoned || self.linked_tx_idle.poisoned {
            return SerialTxIdleTurnOutcome::Failed;
        }
        if self.linked_rx_command.is_some() || self.linked_tx.command.is_some() {
            return SerialTxIdleTurnOutcome::Pending;
        }
        if self.linked_tx_idle.command.is_none() && self.tx_pending() {
            return SerialTxIdleTurnOutcome::Pending;
        }
        if !self.linked_tx_idle.required {
            return SerialTxIdleTurnOutcome::Complete;
        }
        #[cfg(test)]
        if SERIAL_LINKED_RUNTIME_ONLY_TEST_ACTIVE.load(AtomicOrdering::Acquire) != 0 {
            let misses = SERIAL_LINKED_RUNTIME_ONLY_TEST_TX_IDLE_MISSES
                .fetch_update(AtomicOrdering::AcqRel, AtomicOrdering::Acquire, |current| {
                    if current == 0 {
                        None
                    } else {
                        Some(current - 1)
                    }
                })
                .unwrap_or(0);
            if misses != 0 {
                self.linked_rx_due = true;
                return SerialTxIdleTurnOutcome::Pending;
            }
            self.linked_tx_idle.required = self.tx_pending();
            return SerialTxIdleTurnOutcome::Complete;
        }
        let contract = <D as SerialDriver>::driver_task_contract();
        let command = if let Some(command) = self.linked_tx_idle.command {
            command
        } else {
            let mut command = crate::hal::driver_task::DriverTaskCommandRecord::pi4_hot_path(
                0,
                crate::hal::driver_task::DriverTaskHotPath::SerialConsole,
                crate::hal::driver_task::DriverTaskBudgetGrant::from_contract(contract),
                crate::hal::driver_task::DriverFrameDescriptor {
                    offset: 0,
                    len: 0,
                    flags: 0,
                },
            );
            command.aux0 = SERIAL_RUNTIME_AUX_TX_IDLE;
            self.linked_tx_idle.ticket = self.next_linked_turn_id();
            self.linked_tx_idle.command = Some(command);
            command
        };
        crate::hal::driver_task::register_driver_task_pointer_free_ring_service(
            contract,
            crate::hal::driver_task::DriverTaskHotPath::SerialConsole.as_u32() as usize,
            serial_runtime_ring_service_driver_task,
        );
        let turn = crate::hal::driver_task::run_driver_task_ring_service_retained_service_turn(
            contract, command,
        );
        self.finish_linked_tx_idle_turn(turn)
    }

    #[cfg(feature = "kernel")]
    fn finish_linked_tx_idle_turn(
        &mut self,
        turn: crate::hal::driver_task::DriverTaskRetainedServiceTurn,
    ) -> SerialTxIdleTurnOutcome {
        let completion = match turn {
            crate::hal::driver_task::DriverTaskRetainedServiceTurn::Pending => {
                return SerialTxIdleTurnOutcome::Pending;
            }
            crate::hal::driver_task::DriverTaskRetainedServiceTurn::Complete(completion) => {
                completion
            }
            crate::hal::driver_task::DriverTaskRetainedServiceTurn::Failed => {
                self.poison_linked_tx();
                return SerialTxIdleTurnOutcome::Failed;
            }
        };
        self.linked_tx_idle.command = None;
        self.linked_tx_idle.ticket = 0;
        let no_fault =
            completion.detail == crate::hal::driver_task::DriverTaskFaultCode::None.as_u16();
        let empty_frame = completion.frame.offset == 0
            && completion.frame.len == 0
            && completion.frame.flags == 0;
        if completion.code == crate::hal::driver_task::DriverTaskCompletionCode::Progress.as_u16()
            && no_fault
            && completion.result == 1
            && empty_frame
        {
            // This sample proves only bytes accepted before the immutable
            // probe. Output queued while it was in flight still needs its own
            // TX action and later wire-idle proof.
            self.linked_tx_idle.required = self.tx_pending();
            return SerialTxIdleTurnOutcome::Complete;
        }
        if completion.code == crate::hal::driver_task::DriverTaskCompletionCode::Idle.as_u16()
            && no_fault
            && completion.result == 0
            && empty_frame
        {
            self.linked_rx_due = true;
            return SerialTxIdleTurnOutcome::Pending;
        }
        self.poison_linked_tx();
        SerialTxIdleTurnOutcome::Failed
    }

    /// Flush only through the independently owned physical serial linked runtime.
    ///
    /// The helper is intentionally separate from [`Self::flush_tx`]: callers
    /// that temporarily cannot admit generic HAL work must not inherit its
    /// compatibility or current-TCB fallbacks.
    #[cfg(feature = "kernel")]
    pub fn flush_tx_linked_runtime_only(&mut self) -> bool {
        if !serial_driver_task_transport_active() {
            return false;
        }
        #[cfg(test)]
        if SERIAL_LINKED_RUNTIME_ONLY_TEST_ACTIVE.load(AtomicOrdering::Acquire) != 0 {
            if self.linked_tx.poisoned {
                return false;
            }
            let mut emitted = SERIAL_LINKED_RUNTIME_ONLY_TEST_TX.lock();
            let turn_limit = usize::from(
                <D as SerialDriver>::driver_task_contract()
                    .budget
                    .max_ops_per_turn,
            )
            .min(
                <D as SerialDriver>::driver_task_contract()
                    .budget
                    .max_bytes_per_turn as usize,
            )
            .min(SERIAL_LINKED_TX_TURN_BYTES)
            .min(DEFAULT_TX_CAPACITY.saturating_sub(emitted.len()));
            let mut written = 0usize;
            if let Some(byte) = self.driver_local.take_pending_tx() {
                if emitted.enqueue(byte).is_ok() {
                    written = written.saturating_add(1);
                } else {
                    self.driver_local.set_pending_tx(Some(byte));
                }
            }
            while written < turn_limit {
                let Some(byte) = self.tx.dequeue() else {
                    break;
                };
                if emitted.enqueue(byte).is_err() {
                    self.restore_staged_tx(&[byte]);
                    break;
                }
                written = written.saturating_add(1);
            }
            drop(emitted);
            if written != 0 {
                self.linked_tx_idle.required = true;
            }
            self.linked_rx_due = true;
            return true;
        }
        let contract = <D as SerialDriver>::driver_task_contract();
        crate::hal::driver_task::register_driver_task_pointer_free_ring_service(
            contract,
            crate::hal::driver_task::DriverTaskHotPath::SerialConsole.as_u32() as usize,
            serial_runtime_ring_service_driver_task,
        );
        match self.flush_tx_driver_task_ring_turn(contract) {
            LinkedSerialTurnOutcome::Pending => true,
            LinkedSerialTurnOutcome::Complete { .. } => {
                self.linked_rx_due = true;
                true
            }
            LinkedSerialTurnOutcome::Failed => {
                self.linked_rx_due = true;
                false
            }
        }
    }

    fn poll_rx_only_current_tcb(&mut self, contract: DriverTaskContract) -> bool {
        let mut budget = match DriverServiceBudget::new(contract) {
            Ok(budget) => budget,
            Err(_) => {
                self.telemetry.driver_task_budget_overrun();
                return false;
            }
        };
        let (rx_activity, budget_exhausted) = self.poll_rx_current_tcb(&mut budget);
        if budget_exhausted {
            self.telemetry.driver_task_budget_overrun();
        }
        rx_activity
    }

    #[cfg(feature = "kernel")]
    fn poll_root_context_callback_current_tcb(&mut self, contract: DriverTaskContract) -> bool {
        if self.root_context_rx_only_service {
            return self.poll_rx_only_current_tcb(contract);
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
        let (rx_activity, budget_exhausted) = self.poll_rx_current_tcb(&mut budget);
        if budget_exhausted {
            self.telemetry.driver_task_budget_overrun();
        } else {
            #[cfg(feature = "release-qemu")]
            if self.ordinary_root_control_turn.active() {
                if try_with_uart_tx_lock(|| self.flush_tx_unlocked(&mut budget)).is_none() {
                    record_qemu_uart_tx_lock_deferral();
                }
                return rx_activity;
            }
            with_uart_tx_lock(|| self.flush_tx_unlocked(&mut budget));
        }
        rx_activity
    }

    fn poll_rx_current_tcb(&mut self, budget: &mut DriverServiceBudget) -> (bool, bool) {
        let mut budget_exhausted = false;
        let mut rx_activity = false;
        let mut accepted = 0usize;
        #[cfg(feature = "kernel")]
        let mut first = 0u8;
        #[cfg(feature = "kernel")]
        let mut last = 0u8;
        // Drain RX side first so newly available bytes can be processed in the
        // same cycle.
        loop {
            match self.serial_rx_byte_budget_available(budget) {
                Ok(true) => {}
                Ok(false) => break,
                Err(_) => {
                    budget_exhausted = true;
                    break;
                }
            }
            match self.driver.read_byte() {
                Ok(byte) => {
                    if self.charge_serial_byte(budget).is_err() {
                        self.telemetry.driver_task_budget_overrun();
                        break;
                    }
                    #[cfg(feature = "kernel")]
                    {
                        if accepted == 0 {
                            first = byte;
                        }
                        last = byte;
                    }
                    accepted = accepted.saturating_add(1);
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

        #[cfg(feature = "kernel")]
        if serial_input_poll_trace_allowed(accepted, self.ordinary_root_control_turn.active()) {
            emit_serial_input_poll_trace(
                "uart-rx",
                accepted,
                self.rx.len(),
                self.line.len(),
                first,
                last,
            );
        }
        (rx_activity, budget_exhausted)
    }

    fn flush_tx_locked(&mut self) {
        let contract = <D as SerialDriver>::driver_task_contract();
        #[cfg(feature = "kernel")]
        {
            if serial_driver_task_transport_active() {
                crate::hal::driver_task::register_driver_task_pointer_free_ring_service(
                    contract,
                    crate::hal::driver_task::DriverTaskHotPath::SerialConsole.as_u32() as usize,
                    serial_runtime_ring_service_driver_task,
                );
                if self.flush_tx_driver_task_ring(contract) {
                    return;
                }
                self.telemetry.driver_task_budget_overrun();
                return;
            }
            if !serial_root_uart_direct_io_allowed(serial_root_uart_released_for_linked_runtime()) {
                return;
            }
            if serial_root_context_service_allowed() {
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
                if crate::hal::driver_task::run_driver_task_ring_service(contract, command)
                    .is_some()
                {
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
            } else {
                emit_serial_input_route_trace_once(
                    "flush-route",
                    "physical-root-mini-uart-fallback",
                );
            }
        }
        self.flush_tx_current_tcb(contract);
    }

    #[cfg(feature = "kernel")]
    fn poll_driver_task_rx_into_queue(&mut self, contract: DriverTaskContract) -> bool {
        match self.poll_driver_task_rx_turn(contract) {
            LinkedSerialTurnOutcome::Complete { activity } => {
                self.linked_rx_due = activity;
                activity
            }
            LinkedSerialTurnOutcome::Pending => false,
            LinkedSerialTurnOutcome::Failed => {
                self.linked_rx_due = false;
                false
            }
        }
    }

    #[cfg(feature = "kernel")]
    fn poll_driver_task_rx_turn(
        &mut self,
        _contract: DriverTaskContract,
    ) -> LinkedSerialTurnOutcome {
        if self.drain_driver_task_client_rx_queue() != 0 {
            return LinkedSerialTurnOutcome::Complete { activity: true };
        }
        let generation = SERIAL_SPSC_GENERATION.load(AtomicOrdering::Acquire);
        let available = serial_rx_queue_available::<RX>(self.rx.len())
            .min(crate::hal::driver_task::MAX_DRIVER_TASK_FRAME_BYTES);
        if generation == 0 || available == 0 {
            return LinkedSerialTurnOutcome::Complete { activity: false };
        }
        let mut staged = [0u8; crate::hal::driver_task::MAX_DRIVER_TASK_FRAME_BYTES];
        let Some(transfer) = crate::hal::driver_task::driver_task_serial_spsc_dequeue_rx(
            generation,
            &mut staged[..available],
        ) else {
            return LinkedSerialTurnOutcome::Failed;
        };
        let mut accepted = 0usize;
        for &byte in &staged[..transfer.bytes] {
            if self.rx.enqueue(byte).is_err() {
                self.telemetry.rx_overflow();
                return LinkedSerialTurnOutcome::Failed;
            }
            accepted = accepted.saturating_add(1);
        }
        if accepted != 0 {
            let _ = publish_serial_generation_owned_state(
                generation,
                &SERIAL_DRIVER_TASK_CLIENT_RX_PROVEN,
                1,
                0,
            );
        }
        LinkedSerialTurnOutcome::Complete {
            activity: accepted != 0,
        }
    }

    #[cfg(feature = "kernel")]
    fn finish_driver_task_rx_turn(
        &mut self,
        contract: DriverTaskContract,
        turn: crate::hal::driver_task::DriverTaskRetainedServiceTurn,
    ) -> LinkedSerialTurnOutcome {
        let completion = match turn {
            crate::hal::driver_task::DriverTaskRetainedServiceTurn::Pending => {
                return LinkedSerialTurnOutcome::Pending;
            }
            crate::hal::driver_task::DriverTaskRetainedServiceTurn::Complete(completion) => {
                completion
            }
            crate::hal::driver_task::DriverTaskRetainedServiceTurn::Failed => {
                self.linked_rx_command = None;
                self.linked_rx_ticket = 0;
                self.poison_linked_tx();
                return LinkedSerialTurnOutcome::Failed;
            }
        };
        self.linked_rx_command = None;
        self.linked_rx_ticket = 0;
        let no_fault =
            completion.detail == crate::hal::driver_task::DriverTaskFaultCode::None.as_u16();
        let empty_frame = completion.frame.offset == 0
            && completion.frame.len == 0
            && completion.frame.flags == 0;
        if completion.code == crate::hal::driver_task::DriverTaskCompletionCode::Idle.as_u16()
            && no_fault
            && completion.result == 0
            && empty_frame
        {
            return LinkedSerialTurnOutcome::Complete { activity: false };
        }
        if completion.code == crate::hal::driver_task::DriverTaskCompletionCode::FrameReady.as_u16()
            && no_fault
            && completion.frame.len != 0
            && completion.result == u32::from(completion.frame.len)
        {
            if let Some(bytes) =
                crate::hal::driver_task::driver_task_ring_frame_bytes(contract, completion.frame)
            {
                let mut accepted = 0usize;
                for &byte in bytes {
                    if self.rx.enqueue(byte).is_err() {
                        self.telemetry.rx_overflow();
                        break;
                    }
                    accepted = accepted.saturating_add(1);
                }
                return LinkedSerialTurnOutcome::Complete {
                    activity: accepted != 0,
                };
            }
        }
        self.telemetry.driver_task_budget_overrun();
        LinkedSerialTurnOutcome::Failed
    }

    #[cfg(feature = "kernel")]
    fn drain_driver_task_client_rx_queue(&mut self) -> usize {
        let mut rx = SERIAL_CLIENT_RX.lock();
        let Some(accepted) = drain_serial_rx_queue_exact(&mut rx, &mut self.rx) else {
            self.telemetry.rx_overflow();
            SERIAL_DRIVER_TASK_CLIENT_ACTIVE.store(0, AtomicOrdering::Release);
            SERIAL_DRIVER_TASK_CLIENT_SERVICE_PROVEN.store(0, AtomicOrdering::Release);
            SERIAL_DRIVER_TASK_CLIENT_RX_PROVEN.store(0, AtomicOrdering::Release);
            SERIAL_RUNTIME_ATTACH_PHASE
                .store(SERIAL_RUNTIME_ATTACH_FAILED, AtomicOrdering::Release);
            return 0;
        };
        accepted
    }

    /// Switch to the linked client without flushing through the prior backend.
    ///
    /// CYW43 supervision uses this only after its first physical operation, when
    /// re-entering the root/current-TCB UART would violate the fail-closed HAL
    /// boundary. Any queued output is retained and flushed through the linked
    /// runtime after the switch succeeds.
    #[cfg(feature = "kernel")]
    pub fn use_driver_task_client_after_attach_without_root_flush(&mut self) -> bool {
        self.activate_driver_task_client_after_attach()
    }

    #[cfg(feature = "kernel")]
    fn activate_driver_task_client_after_attach(&mut self) -> bool {
        if !serial_driver_task_transport_active() {
            if !serial_driver_task_interactive_cutover_allowed() {
                return false;
            }
        }
        let generation = SERIAL_SPSC_GENERATION.load(AtomicOrdering::Acquire);
        self.driver.try_use_driver_task_client_after_attach()
            && publish_serial_generation_owned_state(
                generation,
                &SERIAL_DRIVER_TASK_CLIENT_ACTIVE,
                1,
                0,
            )
    }

    #[cfg(feature = "kernel")]
    fn flush_tx_driver_task_ring(&mut self, contract: DriverTaskContract) -> bool {
        !matches!(
            self.flush_tx_driver_task_ring_turn(contract),
            LinkedSerialTurnOutcome::Failed
        )
    }

    #[cfg(feature = "kernel")]
    fn flush_tx_driver_task_ring_turn(
        &mut self,
        contract: DriverTaskContract,
    ) -> LinkedSerialTurnOutcome {
        if self.linked_tx_idle.command.is_some() || self.linked_tx.poisoned {
            return if self.linked_tx.poisoned {
                LinkedSerialTurnOutcome::Failed
            } else {
                LinkedSerialTurnOutcome::Pending
            };
        }
        let turn_limit = usize::from(contract.budget.max_ops_per_turn)
            .min(contract.budget.max_bytes_per_turn as usize)
            .min(crate::hal::driver_task::MAX_DRIVER_TASK_FRAME_BYTES)
            .min(SERIAL_LINKED_TX_TURN_BYTES);
        if self.linked_tx.bytes.is_empty() {
            if let Some(byte) = self.driver_local.take_pending_tx() {
                let _ = self.linked_tx.bytes.push(byte);
            }
            while self.linked_tx.bytes.len() < turn_limit {
                let Some(byte) = self.tx.dequeue() else {
                    break;
                };
                let _ = self.linked_tx.bytes.push(byte);
            }
        }
        if self.linked_tx.bytes.is_empty() {
            return LinkedSerialTurnOutcome::Complete { activity: false };
        }
        let generation = SERIAL_SPSC_GENERATION.load(AtomicOrdering::Acquire);
        let Some(transfer) = crate::hal::driver_task::driver_task_serial_spsc_enqueue_tx(
            generation,
            self.linked_tx.bytes.as_slice(),
        ) else {
            self.poison_linked_tx();
            return LinkedSerialTurnOutcome::Failed;
        };
        if transfer.bytes == 0 {
            return LinkedSerialTurnOutcome::Pending;
        }
        if !self.linked_tx.consume_prefix(transfer.bytes) {
            self.poison_linked_tx();
            return LinkedSerialTurnOutcome::Failed;
        }
        self.linked_tx_idle.required = true;
        if !publish_serial_generation_owned_state(
            generation,
            &SERIAL_DRIVER_TASK_CLIENT_SERVICE_PROVEN,
            1,
            0,
        ) {
            self.poison_linked_tx();
            return LinkedSerialTurnOutcome::Failed;
        }
        LinkedSerialTurnOutcome::Complete { activity: true }
    }

    #[cfg(feature = "kernel")]
    fn flush_tx_driver_task_ring_with(
        &mut self,
        contract: DriverTaskContract,
        execute: impl FnOnce(
            crate::hal::driver_task::DriverTaskCommandRecord,
            &[u8],
        ) -> Option<crate::hal::driver_task::DriverTaskCompletionRecord>,
    ) -> bool {
        !matches!(
            self.flush_tx_driver_task_ring_turn_with(contract, execute),
            LinkedSerialTurnOutcome::Failed
        )
    }

    #[cfg(feature = "kernel")]
    fn flush_tx_driver_task_ring_turn_with(
        &mut self,
        contract: DriverTaskContract,
        execute: impl FnOnce(
            crate::hal::driver_task::DriverTaskCommandRecord,
            &[u8],
        ) -> Option<crate::hal::driver_task::DriverTaskCompletionRecord>,
    ) -> LinkedSerialTurnOutcome {
        self.flush_tx_driver_task_ring_typed_turn_with(contract, |command, staged| {
            match execute(command, staged) {
                Some(completion) => {
                    crate::hal::driver_task::DriverTaskRetainedServiceTurn::Complete(completion)
                }
                None => crate::hal::driver_task::DriverTaskRetainedServiceTurn::Pending,
            }
        })
    }

    #[cfg(feature = "kernel")]
    fn flush_tx_driver_task_ring_typed_turn_with(
        &mut self,
        contract: DriverTaskContract,
        execute: impl FnOnce(
            crate::hal::driver_task::DriverTaskCommandRecord,
            &[u8],
        ) -> crate::hal::driver_task::DriverTaskRetainedServiceTurn,
    ) -> LinkedSerialTurnOutcome {
        // An outstanding RX poll owns the shared serial ring until its exact
        // completion. The next outer serial turn resumes it before TX stages a
        // different fingerprint.
        if self.linked_rx_command.is_some() || self.linked_tx_idle.command.is_some() {
            return LinkedSerialTurnOutcome::Pending;
        }
        if self.linked_tx.poisoned {
            return LinkedSerialTurnOutcome::Failed;
        }
        let turn_limit = usize::from(contract.budget.max_ops_per_turn)
            .min(contract.budget.max_bytes_per_turn as usize)
            .min(crate::hal::driver_task::MAX_DRIVER_TASK_FRAME_BYTES)
            .min(SERIAL_LINKED_TX_TURN_BYTES);
        #[cfg(feature = "net-backend-virtio")]
        let turn_limit = self
            .routine_audit_tx_attempts_left
            .map_or(turn_limit, |attempts_left| {
                turn_limit.min(usize::from(attempts_left))
            });
        if self.linked_tx.bytes.is_empty() {
            if let Some(byte) = self.driver_local.take_pending_tx() {
                let _ = self.linked_tx.bytes.push(byte);
            }
            while self.linked_tx.bytes.len() < turn_limit {
                let Some(byte) = self.tx.dequeue() else {
                    break;
                };
                let _ = self.linked_tx.bytes.push(byte);
            }
        }
        if self.linked_tx.bytes.is_empty() {
            return LinkedSerialTurnOutcome::Complete { activity: false };
        }
        if self.linked_tx.command.is_none() {
            let Some(frame) = crate::hal::driver_task::describe_driver_task_ring_frame(
                self.linked_tx.bytes.as_slice(),
                0,
            ) else {
                self.poison_linked_tx();
                return LinkedSerialTurnOutcome::Failed;
            };
            let command = crate::hal::driver_task::DriverTaskCommandRecord::pi4_hot_path(
                0,
                crate::hal::driver_task::DriverTaskHotPath::SerialConsole,
                crate::hal::driver_task::DriverTaskBudgetGrant::from_contract(contract),
                frame,
            );
            let ticket = self.next_linked_turn_id();
            self.linked_tx.begin_action(ticket, command);
        }
        let Some(command) = self.linked_tx.command else {
            self.poison_linked_tx();
            return LinkedSerialTurnOutcome::Failed;
        };
        let turn = execute(command, self.linked_tx.bytes.as_slice());
        let written = match turn {
            crate::hal::driver_task::DriverTaskRetainedServiceTurn::Pending => {
                return LinkedSerialTurnOutcome::Pending;
            }
            crate::hal::driver_task::DriverTaskRetainedServiceTurn::Failed => {
                self.poison_linked_tx();
                return LinkedSerialTurnOutcome::Failed;
            }
            crate::hal::driver_task::DriverTaskRetainedServiceTurn::Complete(completion)
                if completion.code
                    == crate::hal::driver_task::DriverTaskCompletionCode::Progress.as_u16()
                    && completion.detail
                        == crate::hal::driver_task::DriverTaskFaultCode::None.as_u16()
                    && completion.result != 0
                    && completion.frame.offset == 0
                    && completion.frame.len == 0
                    && completion.frame.flags == 0 =>
            {
                completion.result as usize
            }
            crate::hal::driver_task::DriverTaskRetainedServiceTurn::Complete(completion)
                if completion.code
                    == crate::hal::driver_task::DriverTaskCompletionCode::Idle.as_u16()
                    && completion.detail
                        == crate::hal::driver_task::DriverTaskFaultCode::None.as_u16()
                    && completion.result == 0
                    && completion.frame.offset == 0
                    && completion.frame.len == 0
                    && completion.frame.flags == 0 =>
            {
                0
            }
            _ => {
                self.poison_linked_tx();
                return LinkedSerialTurnOutcome::Failed;
            }
        };
        self.linked_tx.command = None;
        if !self.linked_tx.consume_prefix(written) {
            self.poison_linked_tx();
            return LinkedSerialTurnOutcome::Failed;
        }
        if written != 0 {
            self.linked_tx_idle.required = true;
        }
        LinkedSerialTurnOutcome::Complete {
            activity: written != 0,
        }
    }

    #[cfg(feature = "kernel")]
    fn restore_staged_tx(&mut self, staged: &[u8]) {
        for &byte in staged {
            if self.tx.enqueue(byte).is_err() {
                self.telemetry.tx_overflow();
                break;
            }
        }
    }

    fn flush_tx_current_tcb(&mut self, contract: DriverTaskContract) {
        let mut budget = match DriverServiceBudget::new(contract) {
            Ok(budget) => budget,
            Err(_) => {
                self.telemetry.driver_task_budget_overrun();
                return;
            }
        };
        #[cfg(feature = "release-qemu")]
        if self.ordinary_root_control_turn.active() {
            if try_with_uart_tx_lock(|| self.flush_tx_unlocked(&mut budget)).is_none() {
                record_qemu_uart_tx_lock_deferral();
            }
            return;
        }
        with_uart_tx_lock(|| self.flush_tx_unlocked(&mut budget));
    }

    fn flush_tx_unlocked(&mut self, budget: &mut DriverServiceBudget) {
        // Flush staged TX bytes to the device until it reports back-pressure.
        if let Some(byte) = self.driver_local.take_pending_tx() {
            #[cfg(feature = "net-backend-virtio")]
            if !self.routine_audit_tx_attempt_available() {
                self.driver_local.set_pending_tx(Some(byte));
                return;
            }
            match self.serial_byte_budget_available(budget) {
                Ok(true) => {}
                Ok(false) => {
                    self.driver_local.set_pending_tx(Some(byte));
                    return;
                }
                Err(_) => {
                    self.telemetry.driver_task_budget_overrun();
                    self.driver_local.set_pending_tx(Some(byte));
                    return;
                }
            }
            #[cfg(feature = "net-backend-virtio")]
            self.charge_routine_audit_tx_attempt();
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
            #[cfg(feature = "net-backend-virtio")]
            if !self.routine_audit_tx_attempt_available() {
                return;
            }
            match self.serial_byte_budget_available(budget) {
                Ok(true) => {}
                Ok(false) => return,
                Err(_) => {
                    self.telemetry.driver_task_budget_overrun();
                    return;
                }
            }
            let Some(byte) = self.tx.dequeue() else {
                break;
            };
            #[cfg(feature = "net-backend-virtio")]
            self.charge_routine_audit_tx_attempt();
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
    ) -> Result<bool, DriverServiceBudgetError> {
        if !self.ordinary_root_control_turn.byte_available() {
            return Ok(false);
        }
        if budget.ops_left() == 0 {
            return Err(DriverServiceBudgetError::OperationsExhausted);
        }
        if budget.bytes_left() == 0 {
            return Err(DriverServiceBudgetError::BytesExhausted);
        }
        if budget.frames_left() == 0 {
            return Err(DriverServiceBudgetError::FramesExhausted);
        }
        Ok(true)
    }

    fn serial_rx_byte_budget_available(
        &self,
        budget: &DriverServiceBudget,
    ) -> Result<bool, DriverServiceBudgetError> {
        if !self.ordinary_root_control_turn.rx_byte_available() {
            return Ok(false);
        }
        self.serial_byte_budget_available(budget)
    }

    fn charge_serial_byte(
        &mut self,
        budget: &mut DriverServiceBudget,
    ) -> Result<(), DriverServiceBudgetError> {
        if !self.serial_byte_budget_available(budget)? {
            return Err(DriverServiceBudgetError::BytesExhausted);
        }
        budget.charge_ops(1)?;
        budget.charge_bytes(1)?;
        budget.charge_frames(1)?;
        self.ordinary_root_control_turn.charge_byte();
        Ok(())
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
        self.next_line_with_observability(true)
    }

    /// Retrieve an already-buffered line without touching kernel observability.
    ///
    /// The CYW43 bootstrap operator callback uses this while the linked-runtime
    /// HAL is borrowed. Input echo remains staged in the bounded TX queue, but
    /// no debug UART trace or prompt-shadow lock is touched.
    #[cfg(feature = "kernel")]
    pub fn next_line_buffered_quiet(&mut self) -> Option<HeaplessString<LINE>> {
        self.next_line_with_observability(false)
    }

    fn next_line_with_observability(
        &mut self,
        observe_kernel_line: bool,
    ) -> Option<HeaplessString<LINE>> {
        #[cfg(not(feature = "kernel"))]
        let _ = observe_kernel_line;
        while let Some(byte) = self.rx.dequeue() {
            if self.driver_local.suppress_lf() && byte == b'\n' {
                self.driver_local.set_suppress_lf(false);
                continue;
            }
            match byte {
                b'\r' => {
                    self.driver_local.set_suppress_lf(true);
                    #[cfg(feature = "kernel")]
                    if observe_kernel_line {
                        prompt_shadow_clear();
                    }
                    self.emit_newline();
                    let mut completed = HeaplessString::new();
                    core::mem::swap(&mut completed, &mut self.line);
                    #[cfg(feature = "kernel")]
                    if observe_kernel_line {
                        emit_serial_input_line_trace(
                            "line-ready",
                            completed.len(),
                            self.rx.len(),
                            0,
                        );
                    }
                    return Some(completed);
                }
                b'\n' => {
                    #[cfg(feature = "kernel")]
                    if observe_kernel_line {
                        prompt_shadow_clear();
                    }
                    self.emit_newline();
                    let mut completed = HeaplessString::new();
                    core::mem::swap(&mut completed, &mut self.line);
                    #[cfg(feature = "kernel")]
                    if observe_kernel_line {
                        emit_serial_input_line_trace(
                            "line-ready",
                            completed.len(),
                            self.rx.len(),
                            0,
                        );
                    }
                    return Some(completed);
                }
                0x08 | 0x7f => {
                    if self.line.pop().is_some() && self.driver_local.echo_enabled() {
                        #[cfg(feature = "kernel")]
                        if observe_kernel_line {
                            prompt_shadow_pop();
                        }
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
                    #[cfg(feature = "kernel")]
                    if observe_kernel_line {
                        prompt_shadow_push(byte);
                    }
                    if self.driver_local.echo_enabled() {
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
        self.clear_partial_line_with_observability(true)
    }

    /// Drop a partial buffered line without touching kernel prompt state.
    #[cfg(feature = "kernel")]
    pub fn clear_partial_line_buffered_quiet(&mut self) -> bool {
        self.clear_partial_line_with_observability(false)
    }

    fn clear_partial_line_with_observability(&mut self, observe_kernel_line: bool) -> bool {
        #[cfg(not(feature = "kernel"))]
        let _ = observe_kernel_line;
        let had_partial = !self.line.is_empty();
        self.line.clear();
        #[cfg(feature = "kernel")]
        if observe_kernel_line {
            prompt_shadow_clear();
        }
        had_partial
    }

    fn emit_newline(&mut self) {
        if self.driver_local.echo_enabled() {
            let _ = self.try_enqueue_tx_record(&[b"\r\n"]);
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
    if command.aux0 == SERIAL_RUNTIME_AUX_INIT {
        let Some(mmio) = SERIAL_RUNTIME_INIT_LEASE.lock().take() else {
            return crate::hal::driver_task::DriverTaskCompletionRecord::fault(
                command.sequence,
                crate::hal::driver_task::DriverTaskFaultCode::DeviceUnavailable,
            );
        };
        let mut driver = kernel_uart::KernelSerialDriver::from_mmio(mmio);
        driver.init();
        SERIAL_LINKED_RUNTIME_ATTACHED.store(1, AtomicOrdering::Release);
        *SERIAL_DRIVER_RUNTIME.lock() = Some(SerialPort::new(driver));
        return crate::hal::driver_task::DriverTaskCompletionRecord::progress(command.sequence, 1);
    }
    if command.aux0 == pi4_driver_abi::DRIVER_RUNTIME_SERIAL_SPSC_PROBE_AUX
        && command.frame.offset == 0
        && command.frame.len == 0
        && command.frame.flags == 0
    {
        let generation = SERIAL_SPSC_GENERATION.load(AtomicOrdering::Acquire);
        return if generation == 0 {
            crate::hal::driver_task::DriverTaskCompletionRecord::fault(
                command.sequence,
                crate::hal::driver_task::DriverTaskFaultCode::DeviceUnavailable,
            )
        } else {
            crate::hal::driver_task::DriverTaskCompletionRecord::progress(
                command.sequence,
                generation,
            )
        };
    }
    if command.aux0 == SERIAL_RUNTIME_AUX_TX_IDLE
        && command.frame.offset == 0
        && command.frame.len == 0
        && command.frame.flags == 0
    {
        let runtime = SERIAL_DRIVER_RUNTIME.lock();
        let Some(port) = runtime.as_ref() else {
            return crate::hal::driver_task::DriverTaskCompletionRecord::fault(
                command.sequence,
                crate::hal::driver_task::DriverTaskFaultCode::DeviceUnavailable,
            );
        };
        return if port.driver.transmitter_idle() {
            crate::hal::driver_task::DriverTaskCompletionRecord::progress(command.sequence, 1)
        } else {
            crate::hal::driver_task::DriverTaskCompletionRecord::idle(command.sequence)
        };
    }
    if command.opcode == crate::hal::driver_task::DriverTaskOpcode::Service.as_u16()
        && command.arg0 == expected_hot_path.as_u32()
        && command.arg1 == expected_hot_path.role_bit() as u32
    {
        let mut runtime = SERIAL_DRIVER_RUNTIME.lock();
        let Some(port) = runtime.as_mut() else {
            return crate::hal::driver_task::DriverTaskCompletionRecord::fault(
                command.sequence,
                crate::hal::driver_task::DriverTaskFaultCode::DeviceUnavailable,
            );
        };
        return service_serial_runtime_port(port, command);
    }
    if command.opcode == crate::hal::driver_task::DriverTaskOpcode::Flush.as_u16() {
        let mut runtime = SERIAL_DRIVER_RUNTIME.lock();
        let Some(port) = runtime.as_mut() else {
            return crate::hal::driver_task::DriverTaskCompletionRecord::fault(
                command.sequence,
                crate::hal::driver_task::DriverTaskFaultCode::DeviceUnavailable,
            );
        };
        port.flush_tx_current_tcb(driver_task_contract());
        return crate::hal::driver_task::DriverTaskCompletionRecord::idle(command.sequence);
    }
    crate::hal::driver_task::DriverTaskCompletionRecord::fault(
        command.sequence,
        crate::hal::driver_task::DriverTaskFaultCode::RejectedCommand,
    )
}

#[cfg(feature = "kernel")]
fn service_serial_runtime_port<D, const RX: usize, const TX: usize, const LINE: usize>(
    port: &mut SerialPort<D, RX, TX, LINE>,
    command: crate::hal::driver_task::DriverTaskCommandRecord,
) -> crate::hal::driver_task::DriverTaskCompletionRecord
where
    D: SerialDriver,
{
    let contract = driver_task_contract();
    if command.frame.len != 0 {
        let Some(bytes) =
            crate::hal::driver_task::driver_task_ring_frame_bytes(contract, command.frame)
        else {
            return crate::hal::driver_task::DriverTaskCompletionRecord::fault(
                command.sequence,
                crate::hal::driver_task::DriverTaskFaultCode::RejectedCommand,
            );
        };
        let written = serial_runtime_write_bytes(port, bytes, contract);
        if written == 0 {
            return crate::hal::driver_task::DriverTaskCompletionRecord::idle(command.sequence);
        }
        return crate::hal::driver_task::DriverTaskCompletionRecord::progress(
            command.sequence,
            written as u32,
        );
    }

    let mut rx = [0u8; crate::hal::driver_task::MAX_DRIVER_TASK_FRAME_BYTES];
    let rx_limit = serial_runtime_rx_turn_limit(command.budget);
    if rx_limit == 0 {
        return crate::hal::driver_task::DriverTaskCompletionRecord::idle(command.sequence);
    }
    let read = serial_runtime_poll_bytes(port, &mut rx[..rx_limit], contract);
    if read == 0 {
        return crate::hal::driver_task::DriverTaskCompletionRecord::idle(command.sequence);
    }
    let Some(frame) =
        crate::hal::driver_task::stage_driver_task_ring_frame(contract, &rx[..read], 0)
    else {
        return crate::hal::driver_task::DriverTaskCompletionRecord::fault(
            command.sequence,
            crate::hal::driver_task::DriverTaskFaultCode::DeviceUnavailable,
        );
    };
    crate::hal::driver_task::DriverTaskCompletionRecord::frame_ready(command.sequence, frame)
}

#[cfg(feature = "kernel")]
fn serial_runtime_poll_bytes<D, const RX: usize, const TX: usize, const LINE: usize>(
    port: &mut SerialPort<D, RX, TX, LINE>,
    out: &mut [u8],
    contract: DriverTaskContract,
) -> usize
where
    D: SerialDriver,
{
    let _ = port.poll_io_current_tcb(contract);
    let mut read = 0usize;
    while read < out.len() {
        let Some(byte) = port.rx.dequeue() else {
            break;
        };
        out[read] = byte;
        read = read.saturating_add(1);
    }
    read
}

#[cfg(feature = "kernel")]
fn serial_runtime_drain_rx_budgeted<D, const RX: usize, const TX: usize, const LINE: usize>(
    port: &mut SerialPort<D, RX, TX, LINE>,
    budget: &mut DriverServiceBudget,
) where
    D: SerialDriver,
{
    loop {
        match port.serial_rx_byte_budget_available(budget) {
            Ok(true) => {}
            Ok(false) => break,
            Err(_) => {
                port.telemetry.driver_task_budget_overrun();
                break;
            }
        }
        match port.driver.read_byte() {
            Ok(byte) => {
                if port.charge_serial_byte(budget).is_err() {
                    port.telemetry.driver_task_budget_overrun();
                    break;
                }
                if port.rx.enqueue(byte).is_err() {
                    port.telemetry.rx_overflow();
                    break;
                }
            }
            Err(NbError::WouldBlock) => break,
            Err(NbError::Other(_)) => {
                port.telemetry.rx_overflow();
                break;
            }
        }
    }
}

#[cfg(feature = "kernel")]
fn serial_runtime_write_bytes<D, const RX: usize, const TX: usize, const LINE: usize>(
    port: &mut SerialPort<D, RX, TX, LINE>,
    bytes: &[u8],
    contract: DriverTaskContract,
) -> usize
where
    D: SerialDriver,
{
    let mut budget = match DriverServiceBudget::new(contract) {
        Ok(budget) => budget,
        Err(_) => {
            port.telemetry.driver_task_budget_overrun();
            return 0;
        }
    };
    let mut written = 0usize;
    serial_runtime_drain_rx_budgeted(port, &mut budget);
    with_uart_tx_lock(|| {
        for &byte in bytes {
            if written != 0 && written % SERIAL_RUNTIME_RX_DRAIN_DURING_TX_INTERVAL == 0 {
                serial_runtime_drain_rx_budgeted(port, &mut budget);
            }
            match port.serial_byte_budget_available(&budget) {
                Ok(true) => {}
                Ok(false) => break,
                Err(_) => {
                    port.telemetry.driver_task_budget_overrun();
                    break;
                }
            }
            match port.driver.write_byte(byte) {
                Ok(()) => {
                    if port.charge_serial_byte(&mut budget).is_err() {
                        port.telemetry.driver_task_budget_overrun();
                        break;
                    }
                    written = written.saturating_add(1);
                }
                Err(NbError::WouldBlock) => break,
                Err(NbError::Other(_)) => {
                    port.telemetry.tx_overflow();
                    break;
                }
            }
        }
    });
    serial_runtime_drain_rx_budgeted(port, &mut budget);
    written
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
        let progress = port.poll_root_context_callback_current_tcb(contract);
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
    port.poll_root_context_callback_current_tcb(contract) as usize
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
    use core::cell::{Cell, RefCell};

    /// In-memory serial stub backed by heapless queues.
    pub struct LoopbackSerial<const CAP: usize = 512> {
        pub(crate) rx: RefCell<Queue<u8, CAP>>,
        pub(crate) tx: RefCell<Queue<u8, CAP>>,
        read_calls: Cell<usize>,
        write_calls: Cell<usize>,
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
                read_calls: Cell::new(0),
                write_calls: Cell::new(0),
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

        /// Return underlying driver read/write calls made since construction or reset.
        pub fn io_call_counts(&self) -> (usize, usize) {
            (self.read_calls.get(), self.write_calls.get())
        }

        /// Reset underlying driver read/write call counters.
        pub fn reset_io_call_counts(&self) {
            self.read_calls.set(0);
            self.write_calls.set(0);
        }
    }

    impl<const CAP: usize> ErrorType for LoopbackSerial<CAP> {
        type Error = SerialError;
    }

    impl<const CAP: usize> SerialDriver for LoopbackSerial<CAP> {
        fn read_byte(&mut self) -> nb::Result<u8, Self::Error> {
            self.read_calls.set(self.read_calls.get().saturating_add(1));
            let mut guard = self.rx.borrow_mut();
            guard.dequeue().ok_or(NbError::WouldBlock)
        }

        fn write_byte(&mut self, byte: u8) -> nb::Result<(), Self::Error> {
            self.write_calls
                .set(self.write_calls.get().saturating_add(1));
            let mut guard = self.tx.borrow_mut();
            guard.enqueue(byte).map_err(|_| NbError::WouldBlock)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::LoopbackSerial;
    use super::*;

    #[cfg(feature = "kernel")]
    #[test]
    fn serial_descriptor_phase_preserves_root_uart_until_exact_handoff() {
        assert!(serial_runtime_attach_phase_is_descriptor(
            SERIAL_RUNTIME_ATTACH_DESCRIPTOR,
            false,
        ));
        assert!(!serial_runtime_attach_phase_is_descriptor(
            SERIAL_RUNTIME_ATTACH_INIT,
            false,
        ));
        assert!(!serial_runtime_attach_phase_is_descriptor(
            SERIAL_RUNTIME_ATTACH_DESCRIPTOR,
            true,
        ));
        assert!(serial_root_uart_direct_io_allowed(false));
        assert!(!serial_root_uart_direct_io_allowed(true));
    }

    #[cfg(feature = "kernel")]
    #[repr(C, align(64))]
    struct TestSerialRingPage([u8; crate::hal::driver_task::DRIVER_TASK_RING_PAGE_BYTES]);

    #[cfg(feature = "kernel")]
    static TEST_SERIAL_RING_PAGE: std::sync::Mutex<TestSerialRingPage> = std::sync::Mutex::new(
        TestSerialRingPage([0; crate::hal::driver_task::DRIVER_TASK_RING_PAGE_BYTES]),
    );
    #[cfg(feature = "kernel")]
    static TEST_SERIAL_RING_BYTES: SpinMutex<
        HeaplessVec<u8, { crate::hal::driver_task::MAX_DRIVER_TASK_FRAME_BYTES }>,
    > = SpinMutex::new(HeaplessVec::new());
    #[cfg(feature = "kernel")]
    static TEST_SERIAL_RING_CALLS: AtomicU32 = AtomicU32::new(0);
    #[cfg(feature = "kernel")]
    static TEST_SERIAL_RING_IDLE_MISSES: AtomicU32 = AtomicU32::new(0);

    #[cfg(feature = "kernel")]
    struct TestSerialRingGuard {
        _page: std::sync::MutexGuard<'static, TestSerialRingPage>,
    }

    #[cfg(feature = "kernel")]
    impl Drop for TestSerialRingGuard {
        fn drop(&mut self) {
            crate::hal::driver_task::clear_driver_task_transport(driver_task_contract());
            TEST_SERIAL_RING_BYTES.lock().clear();
            TEST_SERIAL_RING_CALLS.store(0, AtomicOrdering::Release);
            TEST_SERIAL_RING_IDLE_MISSES.store(0, AtomicOrdering::Release);
        }
    }

    #[cfg(feature = "kernel")]
    fn test_publish_serial_ring() -> TestSerialRingGuard {
        let mut page = TEST_SERIAL_RING_PAGE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        page.0.fill(0);
        crate::hal::driver_task::clear_driver_task_transport(driver_task_contract());
        crate::hal::driver_task::publish_driver_task_ring(
            driver_task_contract(),
            page.0.as_mut_ptr() as usize,
        );
        TEST_SERIAL_RING_BYTES.lock().clear();
        TEST_SERIAL_RING_CALLS.store(0, AtomicOrdering::Release);
        TEST_SERIAL_RING_IDLE_MISSES.store(0, AtomicOrdering::Release);
        TestSerialRingGuard { _page: page }
    }

    #[cfg(feature = "kernel")]
    unsafe fn test_serial_reciprocal_ring_service(
        context: usize,
        command: crate::hal::driver_task::DriverTaskCommandRecord,
    ) -> crate::hal::driver_task::DriverTaskCompletionRecord {
        if context != crate::hal::driver_task::DriverTaskHotPath::SerialConsole.as_u32() as usize {
            return crate::hal::driver_task::DriverTaskCompletionRecord::fault(
                command.sequence,
                crate::hal::driver_task::DriverTaskFaultCode::RejectedCommand,
            );
        }
        if command.aux0 == SERIAL_RUNTIME_AUX_TX_IDLE && command.frame.len == 0 {
            TEST_SERIAL_RING_CALLS.fetch_add(1, AtomicOrdering::AcqRel);
            let misses = TEST_SERIAL_RING_IDLE_MISSES
                .fetch_update(AtomicOrdering::AcqRel, AtomicOrdering::Acquire, |current| {
                    if current == 0 {
                        None
                    } else {
                        Some(current - 1)
                    }
                })
                .unwrap_or(0);
            return if misses == 0 {
                crate::hal::driver_task::DriverTaskCompletionRecord::progress(command.sequence, 1)
            } else {
                crate::hal::driver_task::DriverTaskCompletionRecord::idle(command.sequence)
            };
        }
        let Some(bytes) = crate::hal::driver_task::driver_task_ring_frame_bytes(
            driver_task_contract(),
            command.frame,
        ) else {
            return crate::hal::driver_task::DriverTaskCompletionRecord::fault(
                command.sequence,
                crate::hal::driver_task::DriverTaskFaultCode::RejectedCommand,
            );
        };
        let mut captured = TEST_SERIAL_RING_BYTES.lock();
        for byte in bytes.iter().copied() {
            if captured.push(byte).is_err() {
                return crate::hal::driver_task::DriverTaskCompletionRecord::fault(
                    command.sequence,
                    crate::hal::driver_task::DriverTaskFaultCode::DeviceUnavailable,
                );
            }
        }
        TEST_SERIAL_RING_CALLS.fetch_add(1, AtomicOrdering::AcqRel);
        crate::hal::driver_task::DriverTaskCompletionRecord::progress(
            command.sequence,
            bytes.len() as u32,
        )
    }

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
    fn driver_task_rx_budget_clamps_to_shell_queue_capacity() {
        let budget = serial_driver_task_rx_budget(driver_task_contract(), 17).unwrap();

        assert_eq!(budget.max_ops, 17);
        assert_eq!(budget.max_frames, 17);
        assert_eq!(budget.max_bytes, 17);
    }

    #[test]
    fn driver_task_rx_budget_refuses_full_shell_queue() {
        assert!(serial_driver_task_rx_budget(driver_task_contract(), 0).is_none());
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn linked_runtime_rx_budget_reserves_the_spsc_sentinel_slot() {
        assert_eq!(serial_rx_queue_available::<16>(0), 15);
        assert_eq!(serial_rx_queue_available::<16>(14), 1);
        assert_eq!(serial_rx_queue_available::<16>(15), 0);
        assert_eq!(serial_rx_queue_available::<16>(16), 0);

        let mut client_rx = Queue::<u8, DEFAULT_RX_CAPACITY>::new();
        for value in 0..DEFAULT_RX_CAPACITY.saturating_sub(1) {
            assert!(client_rx.enqueue(value as u8).is_ok());
        }
        assert_eq!(client_rx.len(), DEFAULT_RX_CAPACITY - 1);
        assert!(client_rx.enqueue(0).is_err());
        assert_eq!(
            serial_rx_queue_available::<DEFAULT_RX_CAPACITY>(client_rx.len()),
            0,
            "the client path must not consume a 256th byte into a 255-byte queue"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn linked_runtime_client_drain_never_dequeues_past_destination_sentinel() {
        let mut source = Queue::<u8, 4>::new();
        assert!(source.enqueue(0xa5).is_ok());
        let mut destination = Queue::<u8, 4>::new();
        for byte in [1, 2, 3] {
            assert!(destination.enqueue(byte).is_ok());
        }

        assert_eq!(
            drain_serial_rx_queue_exact(&mut source, &mut destination),
            Some(0)
        );
        assert_eq!(source.dequeue(), Some(0xa5));
        assert_eq!(destination.dequeue(), Some(1));
        assert_eq!(destination.dequeue(), Some(2));
        assert_eq!(destination.dequeue(), Some(3));
        assert_eq!(destination.dequeue(), None);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn linked_runtime_rx_turn_honors_the_root_command_budget() {
        assert_eq!(
            serial_runtime_rx_turn_limit(crate::hal::driver_task::DriverTaskBudgetGrant {
                max_ops: 9,
                max_frames: 7,
                max_bytes: 8,
            }),
            7
        );
    }

    #[test]
    fn serial_declares_valid_realtime_driver_task_contract() {
        let contract = driver_task_contract();

        assert_eq!(contract.name, "serial");
        assert!(contract.preempts_network_data());
        assert_eq!(contract.validate(), Ok(()));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn physical_pi_serial_uses_driver_task_only_after_runtime_attaches() {
        assert!(serial_driver_task_transport_required(true, true, true));
        assert!(!serial_driver_task_transport_required(true, true, false));
        assert!(!serial_driver_task_transport_required(true, false, true));
        assert!(!serial_driver_task_transport_required(false, true, true));
        assert!(!serial_driver_task_transport_required(false, false, false));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn physical_pi_serial_interactive_cutover_requires_service_proof() {
        assert!(serial_driver_task_interactive_cutover_policy(
            true, true, true
        ));
        assert!(!serial_driver_task_interactive_cutover_policy(
            true, true, false
        ));
        assert!(!serial_driver_task_interactive_cutover_policy(
            true, false, true
        ));
        assert!(serial_driver_task_interactive_cutover_policy(
            false, true, false
        ));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn serial_runtime_fault_fence_invalidates_generation_and_all_client_state() {
        let fault_fenced = AtomicU32::new(0);
        let generation = AtomicU32::new(7);
        let attached = AtomicU32::new(1);
        let client_active = AtomicU32::new(1);
        let service_proven = AtomicU32::new(1);
        let rx_proven = AtomicU32::new(1);
        let attach_phase = AtomicU32::new(SERIAL_RUNTIME_ATTACH_READY);

        fence_serial_driver_task_runtime_state_for(
            &fault_fenced,
            &generation,
            &[&attached, &client_active, &service_proven, &rx_proven],
            &attach_phase,
            SERIAL_RUNTIME_ATTACH_FAILED,
        );

        assert_eq!(fault_fenced.load(AtomicOrdering::Acquire), 1);
        assert!(!serial_spsc_initialization_allowed(
            fault_fenced.load(AtomicOrdering::Acquire)
        ));
        assert_eq!(generation.load(AtomicOrdering::Acquire), 0);
        for state in [&attached, &client_active, &service_proven, &rx_proven] {
            assert_eq!(state.load(AtomicOrdering::Acquire), 0);
        }
        assert_eq!(
            attach_phase.load(AtomicOrdering::Acquire),
            SERIAL_RUNTIME_ATTACH_FAILED
        );
        assert!(!publish_serial_generation_owned_state_for(
            &generation,
            7,
            &client_active,
            1,
            0,
        ));
        assert_eq!(client_active.load(AtomicOrdering::Acquire), 0);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn physical_pi_serial_idle_completion_is_not_rx_proof() {
        assert!(serial_driver_task_service_completion_proves_transport(
            crate::hal::driver_task::DriverTaskCompletionCode::Idle.as_u16(),
        ));
        assert!(!serial_driver_task_rx_completion_proves_input(
            crate::hal::driver_task::DriverTaskCompletionCode::Idle.as_u16(),
            0
        ));
        assert!(!serial_driver_task_rx_completion_proves_input(
            crate::hal::driver_task::DriverTaskCompletionCode::FrameReady.as_u16(),
            0
        ));
        assert!(serial_driver_task_rx_completion_proves_input(
            crate::hal::driver_task::DriverTaskCompletionCode::FrameReady.as_u16(),
            1
        ));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn physical_pi_serial_fallback_bypasses_root_context_service() {
        assert!(!serial_root_context_service_allowed_policy(true));
        assert!(serial_root_context_service_allowed_policy(false));
    }

    #[test]
    fn serial_owner_runtime_record_is_fixed_layout_and_acceptance_ready() {
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
        assert!(descriptor.has_required_runtime_flags());
        assert!(serial_owner_state_acceptance_ready());
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

    #[cfg(feature = "net-backend-virtio")]
    #[test]
    fn routine_audit_tx_tag_survives_empty_record_and_promotes_on_ordinary_admission() {
        let driver = LoopbackSerial::<128>::new();
        let mut port: SerialPort<_, 16, 64, 16> = SerialPort::new(driver);

        assert!(port.try_enqueue_routine_audit_line_record("audit"));
        assert!(port.routine_audit_only_pending());
        assert!(port.try_enqueue_tx_record(&[]));
        assert!(port.routine_audit_only_pending());

        assert!(port.try_enqueue_tx_record(&[b"ordinary"]));
        assert!(!port.routine_audit_only_pending());
        port.flush_tx();

        assert_eq!(
            port.driver_mut().drain_tx().as_slice(),
            b"audit\r\nordinary",
        );
        assert!(!port.tx_pending());

        assert!(port.try_enqueue_routine_audit_line_record("direct"));
        port.enqueue_tx(b"-ordinary");
        assert!(!port.routine_audit_only_pending());
        port.flush_tx();
        assert_eq!(
            port.driver_mut().drain_tx().as_slice(),
            b"direct\r\n-ordinary",
        );

        assert!(port.try_enqueue_routine_audit_line_record("best-effort"));
        assert_eq!(port.enqueue_tx_best_effort(b"-ordinary"), 9);
        assert!(!port.routine_audit_only_pending());
        port.flush_tx();
        assert_eq!(
            port.driver_mut().drain_tx().as_slice(),
            b"best-effort\r\n-ordinary",
        );
    }

    #[cfg(feature = "net-backend-virtio")]
    #[test]
    fn routine_audit_tx_admission_requires_empty_capacity_and_is_atomic() {
        let driver = LoopbackSerial::<64>::new();
        let mut port: SerialPort<_, 16, 8, 16> = SerialPort::new(driver);

        assert!(port.try_enqueue_tx_record(&[b"x"]));
        assert!(!port.try_enqueue_routine_audit_line_record("audit"));
        assert!(!port.routine_audit_only_pending());
        port.flush_tx();
        assert_eq!(port.driver_mut().drain_tx().as_slice(), b"x");

        assert!(!port.try_enqueue_routine_audit_line_record("123456"));
        assert!(!port.tx_pending());
        assert!(!port.routine_audit_only_pending());
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn prompt_refresh_shadow_tracks_partial_serial_input() {
        prompt_shadow_clear();
        let driver = LoopbackSerial::<64>::new();
        let mut port: SerialPort<_, 16, 32, 16> = SerialPort::new(driver);

        port.driver_mut().push_rx(b"wifi");
        assert!(port.poll_io());
        assert!(port.next_line().is_none());
        assert_eq!(SERIAL_PROMPT_INPUT_SHADOW.lock().as_str(), "wifi");

        port.driver_mut().push_rx(&[0x08, b'x', b'\n']);
        assert!(port.poll_io());
        let line = port
            .next_line()
            .expect("newline completes the edited input");

        assert_eq!(line.as_str(), "wifx");
        assert!(SERIAL_PROMPT_INPUT_SHADOW.lock().is_empty());

        prompt_shadow_push(b's');
        invalidate_prompt_input_shadow_quiet();
        assert_eq!(
            SERIAL_PROMPT_INPUT_SHADOW_VALID.load(AtomicOrdering::Acquire),
            0
        );
        assert_eq!(
            SERIAL_PROMPT_INPUT_SHADOW.lock().as_str(),
            "s",
            "Recovery invalidation must not acquire or mutate the shadow lock"
        );
        prompt_shadow_push(b'n');
        assert_eq!(SERIAL_PROMPT_INPUT_SHADOW.lock().as_str(), "n");
        assert_eq!(
            SERIAL_PROMPT_INPUT_SHADOW_VALID.load(AtomicOrdering::Acquire),
            1
        );
        prompt_shadow_clear();
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn serial_input_trace_budget_caps_diagnostic_bursts() {
        let counter = AtomicU32::new(0);

        for _ in 0..SERIAL_INPUT_RX_TRACE_LIMIT {
            assert!(serial_input_trace_budget_take(
                &counter,
                SERIAL_INPUT_RX_TRACE_LIMIT
            ));
        }

        assert!(!serial_input_trace_budget_take(
            &counter,
            SERIAL_INPUT_RX_TRACE_LIMIT
        ));
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn ordinary_root_control_rx_suppresses_raw_poll_trace() {
        assert!(serial_input_poll_trace_allowed(1, false));
        assert!(!serial_input_poll_trace_allowed(0, false));
        assert!(!serial_input_poll_trace_allowed(1, true));
    }

    #[test]
    fn poll_io_obeys_driver_task_budget() {
        let driver = LoopbackSerial::<2048>::new();
        let mut port: SerialPort<_, 2048, 2048, 16> = SerialPort::new(driver);
        let input = [b'a'; 1100];
        port.driver_mut().push_rx(&input);

        assert!(port.poll_io());

        assert_eq!(
            port.rx.len(),
            usize::from(driver_task_contract().budget.max_ops_per_turn)
        );
        assert_eq!(port.driver_mut().rx.borrow().len(), 76);
        assert!(port.telemetry().driver_task_budget_overruns > 0);
    }

    #[test]
    fn flush_tx_obeys_driver_task_budget() {
        let driver = LoopbackSerial::<2048>::new();
        let mut port: SerialPort<_, 2048, 2048, 16> = SerialPort::new(driver);
        let output = [b'x'; 1100];
        port.enqueue_tx(&output);

        port.flush_tx();

        let emitted = port.driver_mut().drain_tx();
        assert_eq!(
            emitted.len(),
            usize::from(driver_task_contract().budget.max_ops_per_turn)
        );
        assert!(port.telemetry().driver_task_budget_overruns > 0);
    }

    #[test]
    fn ordinary_root_control_turn_shares_exact_serial_cap_and_retains_tail() {
        const TEST_ROOT_CONTROL_SERIAL_BYTES: u32 = 64;

        let driver = LoopbackSerial::<2048>::new();
        let mut port: SerialPort<_, 2048, 2048, 16> = SerialPort::new(driver);
        let output = [b'x'; TEST_ROOT_CONTROL_SERIAL_BYTES as usize + 32];
        port.enqueue_tx(&output);

        port.begin_ordinary_root_control_turn_with_limit(TEST_ROOT_CONTROL_SERIAL_BYTES);
        assert!(!port.poll_io());
        assert_eq!(
            port.driver_mut().drain_tx().len(),
            TEST_ROOT_CONTROL_SERIAL_BYTES as usize
        );
        assert!(port.tx_pending());
        assert_eq!(port.telemetry().driver_task_budget_overruns, 0);

        // Later helpers in the same Operator body share the exhausted cursor.
        port.flush_tx();
        assert!(port.driver_mut().drain_tx().is_empty());
        port.write_bytes_blocking(&[b't'; TEST_ROOT_CONTROL_SERIAL_BYTES as usize]);
        assert!(port.driver_mut().drain_tx().is_empty());
        assert_eq!(port.telemetry().driver_task_budget_overruns, 0);
        port.finish_ordinary_root_control_turn();

        port.begin_ordinary_root_control_turn_with_limit(TEST_ROOT_CONTROL_SERIAL_BYTES);
        assert!(!port.poll_io());
        port.finish_ordinary_root_control_turn();
        let retained = port.driver_mut().drain_tx();
        assert_eq!(retained.len(), TEST_ROOT_CONTROL_SERIAL_BYTES as usize);
        assert_eq!(&retained[..32], &[b'x'; 32]);
        assert_eq!(
            &retained[32..],
            &[b't'; TEST_ROOT_CONTROL_SERIAL_BYTES as usize - 32]
        );
        assert!(port.tx_pending(), "the later blocking tail stays ordered");
        assert_eq!(port.telemetry().driver_task_budget_overruns, 0);

        port.begin_ordinary_root_control_turn_with_limit(TEST_ROOT_CONTROL_SERIAL_BYTES);
        assert!(!port.poll_io());
        port.finish_ordinary_root_control_turn();
        assert_eq!(port.driver_mut().drain_tx().as_slice(), &[b't'; 32]);
        assert!(!port.tx_pending());
        assert_eq!(port.telemetry().driver_task_budget_overruns, 0);
    }

    #[test]
    fn ordinary_root_control_turn_defers_rx_without_driver_budget_overrun() {
        const TEST_ROOT_CONTROL_SERIAL_BYTES: u32 = 64;

        let driver = LoopbackSerial::<2048>::new();
        let mut port: SerialPort<_, 2048, 2048, 16> = SerialPort::new(driver);
        port.driver_mut()
            .push_rx(&[b'r'; TEST_ROOT_CONTROL_SERIAL_BYTES as usize + 16]);

        port.begin_ordinary_root_control_turn_with_limit(TEST_ROOT_CONTROL_SERIAL_BYTES);
        assert!(port.poll_io());
        assert_eq!(port.rx.len(), TEST_ROOT_CONTROL_SERIAL_BYTES as usize);
        assert_eq!(port.telemetry().driver_task_budget_overruns, 0);
        assert!(
            !port.poll_io(),
            "the same cursor must defer a second RX drain"
        );
        assert_eq!(port.rx.len(), TEST_ROOT_CONTROL_SERIAL_BYTES as usize);
        assert_eq!(port.telemetry().driver_task_budget_overruns, 0);
        port.finish_ordinary_root_control_turn();

        port.begin_ordinary_root_control_turn_with_limit(TEST_ROOT_CONTROL_SERIAL_BYTES);
        assert!(port.poll_io());
        port.finish_ordinary_root_control_turn();
        assert_eq!(port.rx.len(), TEST_ROOT_CONTROL_SERIAL_BYTES as usize + 16);
        assert_eq!(port.telemetry().driver_task_budget_overruns, 0);
    }

    #[test]
    fn ordinary_root_control_turn_makes_bounded_rx_and_tx_progress_together() {
        const TEST_ROOT_CONTROL_SERIAL_BYTES: u32 = 64;
        const EXPECTED_LANE_BYTES: usize = TEST_ROOT_CONTROL_SERIAL_BYTES as usize / 2;

        let driver = LoopbackSerial::<2048>::new();
        let mut port: SerialPort<_, 2048, 2048, 16> = SerialPort::new(driver);
        port.enqueue_tx(&[b't'; 80]);
        port.driver_mut().push_rx(&[b'r'; 80]);

        port.begin_ordinary_root_control_turn_with_limit(TEST_ROOT_CONTROL_SERIAL_BYTES);
        assert!(port.poll_io());
        port.finish_ordinary_root_control_turn();

        assert_eq!(port.rx.len(), EXPECTED_LANE_BYTES);
        assert_eq!(port.driver_mut().drain_tx().len(), EXPECTED_LANE_BYTES);
        assert!(port.tx_pending());
        assert_eq!(port.telemetry().driver_task_budget_overruns, 0);
    }

    #[test]
    fn ordinary_root_control_turn_keeps_entry_tx_reserve_across_reentry() {
        const TEST_ROOT_CONTROL_SERIAL_BYTES: u32 = 64;

        let driver = LoopbackSerial::<2048>::new();
        let mut port: SerialPort<_, 2048, 2048, 16> = SerialPort::new(driver);
        port.enqueue_tx(&[b't'; 8]);
        port.driver_mut().push_rx(&[b'r'; 80]);

        port.begin_ordinary_root_control_turn_with_limit(TEST_ROOT_CONTROL_SERIAL_BYTES);
        assert!(port.poll_io());
        assert_eq!(port.rx.len(), 32);
        assert_eq!(port.driver_mut().drain_tx().as_slice(), &[b't'; 8]);
        assert!(
            !port.poll_io(),
            "later helpers must not reclaim the entry TX reservation for RX"
        );
        port.finish_ordinary_root_control_turn();

        assert_eq!(port.rx.len(), 32);
        assert!(!port.tx_pending());
        assert_eq!(port.telemetry().driver_task_budget_overruns, 0);
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

    #[test]
    fn echo_records_do_not_flush_or_block_when_tx_queue_is_full() {
        let driver = LoopbackSerial::<16>::new();
        let mut port: SerialPort<_, 16, 2, 16> = SerialPort::new(driver);
        assert!(port.tx.enqueue(b'x').is_ok());

        assert!(port.rx.enqueue(b'a').is_ok());
        assert!(port.rx.enqueue(b'\n').is_ok());
        let line = port.next_line().expect("newline completes input");

        assert_eq!(line.as_str(), "a");
        assert_eq!(port.tx.len(), 1);
        assert_eq!(port.driver_mut().drain_tx().len(), 0);
        assert!(port.telemetry().tx_backpressure > 0);
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
    fn runtime_ring_service_rejects_when_serial_runtime_missing() {
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
            crate::hal::driver_task::DriverTaskCompletionCode::Fault.as_u16()
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn runtime_serial_write_moves_bytes_without_root_port_pointer() {
        let driver = LoopbackSerial::<16>::new();
        let mut port: SerialPort<_, 16, 16, 16> = SerialPort::new(driver);

        let written = serial_runtime_write_bytes(&mut port, b"abc", driver_task_contract());

        assert_eq!(written, 3);
        assert_eq!(port.driver_mut().drain_tx().as_slice(), b"abc");
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn runtime_serial_write_preserves_rx_during_output() {
        let driver = LoopbackSerial::<128>::new();
        let mut port: SerialPort<_, 128, 128, 16> = SerialPort::new(driver);
        port.driver_mut().push_rx(b"wifi diag\n");

        let written = serial_runtime_write_bytes(
            &mut port,
            b"long serial output while operator is typing",
            driver_task_contract(),
        );

        assert!(written > 0);
        assert_eq!(port.driver_mut().rx.borrow().len(), 0);
        let mut out = [0u8; 16];
        let read = serial_runtime_poll_bytes(&mut port, &mut out, driver_task_contract());
        assert_eq!(&out[..read], b"wifi diag\n");
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn runtime_serial_poll_moves_rx_bytes_without_root_port_pointer() {
        let driver = LoopbackSerial::<16>::new();
        let mut port: SerialPort<_, 16, 16, 16> = SerialPort::new(driver);
        port.driver_mut().push_rx(b"xy");
        let mut out = [0u8; 8];

        let read = serial_runtime_poll_bytes(&mut port, &mut out, driver_task_contract());

        assert_eq!(read, 2);
        assert_eq!(&out[..read], b"xy");
        assert_eq!(port.rx.len(), 0);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn linked_runtime_tx_consumes_delayed_reciprocal_ring_completion_once() {
        let _ring_guard = test_publish_serial_ring();
        assert!(
            crate::hal::driver_task::test_publish_driver_task_ring_endpoint(driver_task_contract(),)
        );
        let driver = LoopbackSerial::<64>::new();
        let mut port: SerialPort<_, 16, 32, 16> = SerialPort::new(driver);
        port.enqueue_tx(b"abcdef");
        let ring_counters =
            || {
                crate::hal::driver_task::driver_task_counter_snapshot(driver_task_contract())
                    .map_or((0, 0, 0), |snapshot| {
                        (
                            snapshot.submitted_turns,
                            snapshot.completed_turns,
                            snapshot.send_attempts,
                        )
                    })
            };
        let counters_before = ring_counters();

        // Preparing the immutable payload and sequence-zero command is one root
        // turn. The autonomously polling reciprocal owner cannot observe it yet.
        assert_eq!(
            port.flush_tx_driver_task_ring_typed_turn_with(driver_task_contract(), |command, _| {
                crate::hal::driver_task::run_driver_task_ring_service_retained_service_turn(
                    driver_task_contract(),
                    command,
                )
            },),
            LinkedSerialTurnOutcome::Pending
        );
        let ticket = port.linked_tx.ticket;
        let command = port
            .linked_tx
            .command
            .expect("pending reciprocal-ring action retains its command");
        assert_ne!(ticket, 0);
        assert_eq!(TEST_SERIAL_RING_CALLS.load(AtomicOrdering::Acquire), 0);
        assert!(TEST_SERIAL_RING_BYTES.lock().is_empty());
        let prepared =
            crate::hal::driver_task::active_driver_task_retained_request(driver_task_contract())
                .expect("prepared serial request remains retained");
        assert!(!prepared.issued());
        assert_eq!(ring_counters().0, counters_before.0 + 1);
        assert_eq!(ring_counters().1, counters_before.1);
        assert_eq!(ring_counters().2, counters_before.2);

        assert!(
            crate::hal::driver_task::register_driver_task_pointer_free_ring_service(
                driver_task_contract(),
                crate::hal::driver_task::DriverTaskHotPath::SerialConsole.as_u32() as usize,
                test_serial_reciprocal_ring_service,
            )
        );

        // The serial contract needs no CYW43/SDIO priority boosts, so its next
        // root turn is the dedicated sequence-commit turn. It makes the exact
        // request visible but does not also notify or poll the child.
        assert_eq!(
            port.flush_tx_driver_task_ring_typed_turn_with(driver_task_contract(), |command, _| {
                crate::hal::driver_task::run_driver_task_ring_service_retained_service_turn(
                    driver_task_contract(),
                    command,
                )
            },),
            LinkedSerialTurnOutcome::Pending
        );
        let committed =
            crate::hal::driver_task::active_driver_task_retained_request(driver_task_contract())
                .expect("committed serial request remains retained");
        assert!(committed.issued());
        assert_eq!(committed.request(), prepared.request());
        assert_eq!(committed.command(), prepared.command());
        assert_eq!(TEST_SERIAL_RING_CALLS.load(AtomicOrdering::Acquire), 0);
        assert!(TEST_SERIAL_RING_BYTES.lock().is_empty());
        assert_eq!(ring_counters().0, counters_before.0 + 1);
        assert_eq!(ring_counters().1, counters_before.1);
        assert_eq!(ring_counters().2, counters_before.2);

        // Ordinary EventPump polling used to install a distinct shared-command
        // RX fingerprint here. The occupied TX slot then rejected RX while the
        // TX path refused to advance behind that RX cursor, deterministically
        // freezing the prompt after GENET had already reached DHCP. The direct
        // RX SPSC consumer is independent of that command slot: an empty ring
        // completes without activity and must not disturb the retained TX.
        assert_eq!(
            port.poll_driver_task_rx_turn(driver_task_contract()),
            LinkedSerialTurnOutcome::Complete { activity: false }
        );
        assert!(port.linked_rx_command.is_none());
        assert_eq!(port.linked_rx_ticket, 0);
        assert_eq!(port.linked_tx.command, Some(command));
        assert_eq!(port.linked_tx.ticket, ticket);
        assert_eq!(ring_counters().0, counters_before.0 + 1);
        assert_eq!(ring_counters().1, counters_before.1);
        assert_eq!(ring_counters().2, counters_before.2);

        // Notification is a separate root turn. The test then schedules the
        // reciprocal controller between root turns, exactly as the child TCB
        // runs independently in production.
        assert_eq!(
            port.flush_tx_driver_task_ring_typed_turn_with(driver_task_contract(), |command, _| {
                crate::hal::driver_task::run_driver_task_ring_service_retained_service_turn(
                    driver_task_contract(),
                    command,
                )
            },),
            LinkedSerialTurnOutcome::Pending
        );
        let notified =
            crate::hal::driver_task::active_driver_task_retained_request(driver_task_contract())
                .expect("notified serial request remains retained");
        assert!(notified.issued());
        assert_eq!(notified.request(), prepared.request());
        assert_eq!(notified.command(), prepared.command());
        assert_eq!(TEST_SERIAL_RING_CALLS.load(AtomicOrdering::Acquire), 0);
        assert!(TEST_SERIAL_RING_BYTES.lock().is_empty());
        assert_eq!(ring_counters().0, counters_before.0 + 1);
        assert_eq!(ring_counters().1, counters_before.1);
        assert_eq!(ring_counters().2, counters_before.2 + 1);

        assert!(
            crate::hal::driver_task::test_service_pending_driver_task_ring_command(
                driver_task_contract(),
            )
        );
        assert_eq!(TEST_SERIAL_RING_CALLS.load(AtomicOrdering::Acquire), 1);
        // Payload integrity now belongs to the independent, generation-bound
        // SPSC producer/consumer regressions. This retained-command test proves
        // only that a delayed reciprocal completion is consumed exactly once;
        // the legacy command-frame storage is no longer the serial data path.
        assert_eq!(port.linked_tx.command, Some(command));

        // The following root turn performs only the retained completion poll
        // and local lease finalisation. It must not notify or execute the child
        // a second time.
        assert_eq!(
            port.flush_tx_driver_task_ring_typed_turn_with(driver_task_contract(), |command, _| {
                crate::hal::driver_task::run_driver_task_ring_service_retained_service_turn(
                    driver_task_contract(),
                    command,
                )
            },),
            LinkedSerialTurnOutcome::Complete { activity: true }
        );
        assert_eq!(port.linked_tx.ticket, ticket);
        assert!(port.linked_tx.command.is_none());
        assert!(!port.tx_pending());
        assert_eq!(TEST_SERIAL_RING_CALLS.load(AtomicOrdering::Acquire), 1);
        assert_eq!(ring_counters().0, counters_before.0 + 1);
        assert_eq!(ring_counters().1, counters_before.1 + 1);
        assert_eq!(ring_counters().2, counters_before.2 + 1);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn linked_runtime_resumes_issued_idle_probe_before_later_tx() {
        let _ring_guard = test_publish_serial_ring();
        assert!(
            crate::hal::driver_task::test_publish_driver_task_ring_endpoint(driver_task_contract(),)
        );
        assert!(
            crate::hal::driver_task::register_driver_task_pointer_free_ring_service(
                driver_task_contract(),
                crate::hal::driver_task::DriverTaskHotPath::SerialConsole.as_u32() as usize,
                test_serial_reciprocal_ring_service,
            )
        );
        let driver = LoopbackSerial::<64>::new();
        let mut port: SerialPort<_, 16, 32, 16> = SerialPort::new(driver);
        port.linked_tx_idle.required = true;

        assert_eq!(
            port.poll_linked_tx_idle_turn(),
            SerialTxIdleTurnOutcome::Pending
        );
        let command = port
            .linked_tx_idle
            .command
            .expect("idle probe retains its immutable command");
        assert_eq!(
            port.poll_linked_tx_idle_turn(),
            SerialTxIdleTurnOutcome::Pending
        );
        assert_eq!(
            port.poll_linked_tx_idle_turn(),
            SerialTxIdleTurnOutcome::Pending
        );
        assert_eq!(port.linked_tx_idle.command, Some(command));

        port.enqueue_tx(b"later");
        assert!(port.tx_pending());
        assert!(
            crate::hal::driver_task::register_driver_task_pointer_free_ring_service(
                driver_task_contract(),
                crate::hal::driver_task::DriverTaskHotPath::SerialConsole.as_u32() as usize,
                test_serial_reciprocal_ring_service,
            )
        );
        assert!(
            crate::hal::driver_task::test_service_pending_driver_task_ring_command(
                driver_task_contract(),
            )
        );
        assert!(!port.service_linked_runtime_only_turn());
        assert!(port.linked_tx_idle.command.is_none());
        assert!(port.linked_tx_idle.required);
        assert!(port.tx_pending());

        assert_eq!(
            port.flush_tx_driver_task_ring_typed_turn_with(driver_task_contract(), |command, _| {
                crate::hal::driver_task::run_driver_task_ring_service_retained_service_turn(
                    driver_task_contract(),
                    command,
                )
            },),
            LinkedSerialTurnOutcome::Pending
        );
        assert!(
            port.linked_tx.command.is_some(),
            "later TX must begin after the exact idle completion, not deadlock behind it"
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn poisoned_linked_tx_keeps_polling_successive_operator_rx() {
        struct LinkedRuntimeTestReset;
        impl Drop for LinkedRuntimeTestReset {
            fn drop(&mut self) {
                test_end_linked_runtime_only_transport();
            }
        }

        test_begin_linked_runtime_only_transport();
        let _reset = LinkedRuntimeTestReset;
        let driver = LoopbackSerial::<64>::new();
        let mut port: SerialPort<_, 16, 32, 16> = SerialPort::new(driver);
        port.test_poison_linked_tx();

        assert_eq!(test_inject_linked_runtime_only_rx(b"a"), 1);
        assert!(port.service_linked_runtime_only_turn());
        assert_eq!(test_inject_linked_runtime_only_rx(b"b"), 1);
        assert!(port.service_linked_runtime_only_turn());
        assert_eq!(port.rx.len(), 2);
        assert_eq!(port.tx_drain_outcome(), SerialTxDrainOutcome::Failed);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn retained_rx_terminal_transport_failure_is_not_reported_as_pending() {
        let driver = LoopbackSerial::<64>::new();
        let mut port: SerialPort<_, 16, 32, 16> = SerialPort::new(driver);
        let budget = serial_driver_task_rx_budget(driver_task_contract(), 8).unwrap();
        let command = crate::hal::driver_task::DriverTaskCommandRecord::pi4_hot_path(
            0,
            crate::hal::driver_task::DriverTaskHotPath::SerialConsole,
            budget,
            crate::hal::driver_task::DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        );
        port.linked_rx_command = Some(command);
        port.linked_rx_ticket = 17;
        port.enqueue_tx(b"must-not-replay");

        assert_eq!(
            port.finish_driver_task_rx_turn(
                driver_task_contract(),
                crate::hal::driver_task::DriverTaskRetainedServiceTurn::Failed,
            ),
            LinkedSerialTurnOutcome::Failed,
        );
        assert!(port.linked_rx_command.is_none());
        assert_eq!(port.linked_rx_ticket, 0);
        assert!(port.linked_tx.poisoned);
        assert!(port.linked_tx_idle.poisoned);
        assert!(!port.tx_pending());
        assert_eq!(port.tx_drain_outcome(), SerialTxDrainOutcome::Failed);
        assert_eq!(port.telemetry().driver_task_budget_overruns, 1);

        let backpressure_before = port.telemetry().tx_backpressure;
        port.enqueue_tx(b"blocked-after-terminal-failure");
        assert!(!port.tx_pending());
        assert_eq!(
            port.telemetry().tx_backpressure,
            backpressure_before.saturating_add(1),
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn retained_tx_idle_terminal_transport_failure_poison_is_idempotent() {
        let driver = LoopbackSerial::<64>::new();
        let mut port: SerialPort<_, 16, 32, 16> = SerialPort::new(driver);
        let mut command = crate::hal::driver_task::DriverTaskCommandRecord::pi4_hot_path(
            0,
            crate::hal::driver_task::DriverTaskHotPath::SerialConsole,
            crate::hal::driver_task::DriverTaskBudgetGrant::from_contract(driver_task_contract()),
            crate::hal::driver_task::DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        );
        command.aux0 = SERIAL_RUNTIME_AUX_TX_IDLE;
        port.linked_tx_idle.required = true;
        port.linked_tx_idle.command = Some(command);
        port.linked_tx_idle.ticket = 23;

        assert_eq!(
            port.finish_linked_tx_idle_turn(
                crate::hal::driver_task::DriverTaskRetainedServiceTurn::Failed,
            ),
            SerialTxIdleTurnOutcome::Failed,
        );
        assert!(port.linked_tx_idle.command.is_none());
        assert_eq!(port.linked_tx_idle.ticket, 0);
        assert_eq!(port.tx_drain_outcome(), SerialTxDrainOutcome::Failed);
        assert_eq!(port.telemetry().driver_task_budget_overruns, 1);

        assert_eq!(
            port.finish_linked_tx_idle_turn(
                crate::hal::driver_task::DriverTaskRetainedServiceTurn::Failed,
            ),
            SerialTxIdleTurnOutcome::Failed,
        );
        assert_eq!(
            port.telemetry().driver_task_budget_overruns,
            1,
            "a terminal poisoned generation must not inflate telemetry every outer turn",
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn linked_runtime_tx_retains_pending_and_partial_suffix_in_fifo_order() {
        let driver = LoopbackSerial::<64>::new();
        let mut port: SerialPort<_, 16, 32, 16> = SerialPort::new(driver);
        port.enqueue_tx(b"abcdef");

        assert!(
            port.flush_tx_driver_task_ring_with(driver_task_contract(), |_command, staged| {
                assert_eq!(staged, b"abcdef");
                None
            },)
        );
        let ticket = port.linked_tx.ticket;
        let command = port
            .linked_tx
            .command
            .expect("pending TX action must retain its exact command");
        assert_ne!(ticket, 0);
        assert_eq!(port.linked_tx.bytes.as_slice(), b"abcdef");
        assert!(port.tx.is_empty());

        port.enqueue_tx(b"XYZ");
        assert!(
            port.flush_tx_driver_task_ring_with(driver_task_contract(), |resumed, staged| {
                assert_eq!(resumed, command);
                assert_eq!(staged, b"abcdef");
                Some(crate::hal::driver_task::DriverTaskCompletionRecord::progress(7, 2))
            },)
        );
        assert_eq!(port.linked_tx.ticket, ticket);
        assert_eq!(port.linked_tx.bytes.as_slice(), b"cdef");
        assert!(port.linked_tx.command.is_none());

        assert!(
            port.flush_tx_driver_task_ring_with(driver_task_contract(), |_command, staged| {
                assert_eq!(staged, b"cdef");
                Some(crate::hal::driver_task::DriverTaskCompletionRecord::progress(7, 4))
            },)
        );
        assert_eq!(port.linked_tx.ticket, ticket.wrapping_add(1).max(1));
        assert!(port.linked_tx.bytes.is_empty());

        assert!(
            port.flush_tx_driver_task_ring_with(driver_task_contract(), |_command, staged| {
                assert_eq!(staged, b"XYZ");
                Some(crate::hal::driver_task::DriverTaskCompletionRecord::progress(8, 3))
            },)
        );
        assert_eq!(port.linked_tx.ticket, ticket.wrapping_add(2).max(1));
        assert!(!port.tx_pending());
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn linked_runtime_tx_poison_rejects_impossible_completion_without_replay() {
        struct LinkedRuntimeTestReset;

        impl Drop for LinkedRuntimeTestReset {
            fn drop(&mut self) {
                test_end_linked_runtime_only_transport();
            }
        }

        let driver = LoopbackSerial::<64>::new();
        let mut port: SerialPort<_, 16, 32, 16> = SerialPort::new(driver);
        port.enqueue_tx(b"abc");

        assert!(!port.flush_tx_driver_task_ring_with(
            driver_task_contract(),
            |_command, staged| {
                assert_eq!(staged, b"abc");
                Some(crate::hal::driver_task::DriverTaskCompletionRecord::progress(9, 4))
            },
        ));
        assert!(port.linked_tx.poisoned);
        assert!(port.linked_tx.bytes.is_empty());
        assert!(!port.tx_pending());
        assert_eq!(port.tx_drain_outcome(), SerialTxDrainOutcome::Failed);

        let mut executed = false;
        assert!(!port.flush_tx_driver_task_ring_with(
            driver_task_contract(),
            |_command, _staged| {
                executed = true;
                None
            },
        ));
        assert!(!executed);
        assert!(port.linked_tx.bytes.is_empty());
        port.enqueue_tx(b"must-not-queue");
        assert!(!port.tx_pending());

        test_begin_linked_runtime_only_transport();
        let _reset = LinkedRuntimeTestReset;
        assert_eq!(test_inject_linked_runtime_only_rx(b"reboot\n"), 7);
        assert!(port.service_linked_runtime_only_turn());
        assert_eq!(port.next_line().unwrap().as_str(), "reboot");
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn retained_staged_tx_terminal_transport_failure_is_not_backpressure() {
        let driver = LoopbackSerial::<64>::new();
        let mut port: SerialPort<_, 16, 32, 16> = SerialPort::new(driver);
        port.enqueue_tx(b"issued-unknown");

        assert_eq!(
            port.flush_tx_driver_task_ring_typed_turn_with(
                driver_task_contract(),
                |_command, staged| {
                    assert_eq!(staged, b"issued-unknown");
                    crate::hal::driver_task::DriverTaskRetainedServiceTurn::Failed
                },
            ),
            LinkedSerialTurnOutcome::Failed,
        );
        assert!(port.linked_tx.poisoned);
        assert!(port.linked_tx.command.is_none());
        assert!(port.linked_tx.bytes.is_empty());
        assert!(!port.tx_pending());
        assert_eq!(port.tx_drain_outcome(), SerialTxDrainOutcome::Failed);
        assert_eq!(port.telemetry().driver_task_budget_overruns, 1);

        let mut replayed = false;
        assert_eq!(
            port.flush_tx_driver_task_ring_typed_turn_with(
                driver_task_contract(),
                |_command, _staged| {
                    replayed = true;
                    crate::hal::driver_task::DriverTaskRetainedServiceTurn::Pending
                },
            ),
            LinkedSerialTurnOutcome::Failed,
        );
        assert!(!replayed, "terminal staged TX must never be replayed");
        assert_eq!(
            port.telemetry().driver_task_budget_overruns,
            1,
            "terminal poison remains one bounded telemetry event",
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn linked_runtime_rx_activity_forces_rearm_before_the_next_tx_chunk() {
        struct LinkedRuntimeTestReset;

        impl Drop for LinkedRuntimeTestReset {
            fn drop(&mut self) {
                test_end_linked_runtime_only_transport();
            }
        }

        test_begin_linked_runtime_only_transport();
        let _reset = LinkedRuntimeTestReset;
        let driver = LoopbackSerial::<512>::new();
        let mut port: SerialPort<_, 64, 512, 32> = SerialPort::new(driver);
        port.enqueue_tx(&[b'x'; 300]);

        assert!(!port.service_linked_runtime_only_turn());
        assert_eq!(test_take_linked_runtime_only_tx().len(), 128);
        assert!(port.tx_pending());

        assert_eq!(test_inject_linked_runtime_only_rx(b"wifi diag\n"), 10);
        assert!(port.service_linked_runtime_only_turn());
        assert!(test_take_linked_runtime_only_tx().is_empty());
        assert_eq!(port.next_line().unwrap().as_str(), "wifi diag");

        assert!(!port.service_linked_runtime_only_turn());
        assert!(
            test_take_linked_runtime_only_tx().is_empty(),
            "RX activity must retain one empty owner turn to rearm the shared UART IRQ",
        );

        assert!(!port.service_linked_runtime_only_turn());
        assert_eq!(test_take_linked_runtime_only_tx().len(), 128);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn linked_runtime_rx_ticket_fences_new_tx_payload() {
        let driver = LoopbackSerial::<64>::new();
        let mut port: SerialPort<_, 16, 32, 16> = SerialPort::new(driver);
        let budget = serial_driver_task_rx_budget(driver_task_contract(), 8).unwrap();
        port.linked_rx_command = Some(
            crate::hal::driver_task::DriverTaskCommandRecord::pi4_hot_path(
                0,
                crate::hal::driver_task::DriverTaskHotPath::SerialConsole,
                budget,
                crate::hal::driver_task::DriverFrameDescriptor {
                    offset: 0,
                    len: 0,
                    flags: 0,
                },
            ),
        );
        port.enqueue_tx(b"abc");

        let mut executed = false;
        assert!(port.flush_tx_driver_task_ring_with(
            driver_task_contract(),
            |_command, _staged| {
                executed = true;
                None
            },
        ));
        assert!(!executed);
        assert!(port.linked_tx.bytes.is_empty());
        assert_eq!(port.tx.len(), 3);
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
