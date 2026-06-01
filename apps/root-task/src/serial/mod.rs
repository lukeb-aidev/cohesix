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
const SERIAL_OWNER_DESCRIPTOR_FLAGS: u16 =
    crate::hal::driver_task::DRIVER_TASK_OWNER_STATE_REQUIRED_FLAGS;
#[cfg(feature = "kernel")]
const SERIAL_RUNTIME_AUX_INIT: u32 = 0x5345_5249;

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
static SERIAL_DRIVER_TASK_CLIENT_RX_PROVEN: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "kernel")]
static SERIAL_PROMPT_INPUT_SHADOW: SpinMutex<HeaplessString<DEFAULT_LINE_CAPACITY>> =
    SpinMutex::new(HeaplessString::new());

#[cfg(feature = "kernel")]
fn prompt_shadow_push(byte: u8) {
    let mut shadow = SERIAL_PROMPT_INPUT_SHADOW.lock();
    let _ = shadow.push(byte as char);
}

#[cfg(feature = "kernel")]
fn prompt_shadow_pop() {
    let _ = SERIAL_PROMPT_INPUT_SHADOW.lock().pop();
}

#[cfg(feature = "kernel")]
fn prompt_shadow_clear() {
    SERIAL_PROMPT_INPUT_SHADOW.lock().clear();
}

#[cfg(feature = "kernel")]
pub(crate) fn emit_prompt_refresh_with_input_shadow_unlocked(prompt: &[u8]) {
    crate::sel4::debug_put_bytes_unlocked(prompt);
    let shadow = SERIAL_PROMPT_INPUT_SHADOW.lock();
    if !shadow.is_empty() {
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

#[cfg(feature = "kernel")]
pub(crate) fn emit_serial_runtime_cutover_deferred(reason: &str) {
    let mut line = HeaplessString::<192>::new();
    let _ = fmt::Write::write_fmt(
        &mut line,
        format_args!(
            "SERIAL_RUNTIME_STATE owner=root stage=serial-runtime-init status=cutover-deferred acceptance=red reason={reason}",
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
    let contract = driver_task_contract();
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
            emit_serial_runtime_state("driver", "owner-state-rejected", "red");
            return false;
        }
        crate::hal::driver_task::emit_driver_task_resource_init_status(
            contract,
            crate::hal::driver_task::DriverTaskHotPath::SerialConsole,
            "serial-owner-state",
            "ready",
            completion,
        );
        SERIAL_LINKED_RUNTIME_ATTACHED.store(1, AtomicOrdering::Release);
        emit_serial_runtime_state("driver", "ready", "green");
    } else {
        SERIAL_LINKED_RUNTIME_ATTACHED.store(0, AtomicOrdering::Release);
        emit_serial_runtime_state("driver", status, "red");
    }
    if ok || completion.is_some() {
        let _ = SERIAL_RUNTIME_INIT_LEASE.lock().take();
    }
    ok
}

/// Replay deferred serial runtime topology and attach the linked mini-UART image.
///
/// This is intentionally prompt-side on physical Pi 4 owner-state boots: the
/// direct HAL-owned mini-UART fallback must publish a usable shell before any
/// linked-runtime no-reply can affect operator input.
#[cfg(feature = "kernel")]
pub fn resume_serial_driver_task_runtime_after_prompt() -> bool {
    if !crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active() {
        return true;
    }
    if serial_driver_task_runtime_attached() {
        return true;
    }
    let contract = driver_task_contract();
    let descriptor_ready = crate::hal::driver_task::ensure_deferred_runtime_init_descriptor(
        contract,
        crate::hal::driver_task::DriverTaskHotPath::SerialConsole,
    );
    if !descriptor_ready {
        emit_serial_runtime_state("driver", "post-prompt-descriptor-pending", "red");
        return false;
    }
    init_serial_driver_task_runtime()
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
    rx_proven: bool,
) -> bool {
    runtime_attached && (!owner_state_active || rx_proven)
}

#[cfg(feature = "kernel")]
pub(crate) fn serial_driver_task_interactive_cutover_allowed() -> bool {
    serial_driver_task_interactive_cutover_policy(
        crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active(),
        serial_driver_task_runtime_attached(),
        SERIAL_DRIVER_TASK_CLIENT_RX_PROVEN.load(AtomicOrdering::Acquire) != 0,
    )
}

#[cfg(feature = "kernel")]
fn serial_driver_task_transport_active() -> bool {
    serial_driver_task_transport_required(
        crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active(),
        serial_driver_task_runtime_attached(),
        SERIAL_DRIVER_TASK_CLIENT_ACTIVE.load(AtomicOrdering::Acquire) != 0,
    )
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
    let _ = driver_task_client_poll_rx_into_client_queue();
}

#[cfg(feature = "kernel")]
fn driver_task_client_poll_rx_into_client_queue() -> usize {
    let contract = driver_task_contract();
    let available_capacity = {
        let rx = SERIAL_CLIENT_RX.lock();
        DEFAULT_RX_CAPACITY.saturating_sub(rx.len())
    };
    let Some(budget) = serial_driver_task_rx_budget(contract, available_capacity) else {
        return 0;
    };
    crate::hal::driver_task::register_driver_task_pointer_free_ring_service(
        contract,
        crate::hal::driver_task::DriverTaskHotPath::SerialConsole.as_u32() as usize,
        serial_runtime_ring_service_driver_task,
    );
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
    let Some(completion) = crate::hal::driver_task::run_driver_task_ring_service(contract, command)
    else {
        return 0;
    };
    if completion.code != crate::hal::driver_task::DriverTaskCompletionCode::FrameReady.as_u16() {
        return 0;
    }
    let Some(bytes) =
        crate::hal::driver_task::driver_task_ring_frame_bytes(contract, completion.frame)
    else {
        return 0;
    };
    let mut rx = SERIAL_CLIENT_RX.lock();
    let mut accepted = 0usize;
    for &byte in bytes {
        if rx.enqueue(byte).is_err() {
            break;
        }
        accepted = accepted.saturating_add(1);
    }
    if accepted != 0 {
        SERIAL_DRIVER_TASK_CLIENT_RX_PROVEN.store(1, AtomicOrdering::Release);
    }
    accepted
}

#[cfg(feature = "kernel")]
pub(crate) fn driver_task_client_write_byte(byte: u8) -> nb::Result<(), SerialError> {
    let contract = driver_task_contract();
    crate::hal::driver_task::register_driver_task_pointer_free_ring_service(
        contract,
        crate::hal::driver_task::DriverTaskHotPath::SerialConsole.as_u32() as usize,
        serial_runtime_ring_service_driver_task,
    );
    let Some(frame) = crate::hal::driver_task::stage_driver_task_ring_frame(contract, &[byte], 0)
    else {
        return Err(NbError::WouldBlock);
    };
    let command = crate::hal::driver_task::DriverTaskCommandRecord::pi4_hot_path(
        0,
        crate::hal::driver_task::DriverTaskHotPath::SerialConsole,
        crate::hal::driver_task::DriverTaskBudgetGrant::from_contract(contract),
        frame,
    );
    match crate::hal::driver_task::run_driver_task_ring_service(contract, command) {
        Some(completion)
            if completion.code
                == crate::hal::driver_task::DriverTaskCompletionCode::Progress.as_u16()
                && completion.result == 1 =>
        {
            Ok(())
        }
        Some(completion)
            if completion.code
                == crate::hal::driver_task::DriverTaskCompletionCode::Fault.as_u16() =>
        {
            Err(NbError::Other(SerialError::DeviceFault))
        }
        _ => Err(NbError::WouldBlock),
    }
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
    /// Echoes and edit-control records must never synchronously flush the UART
    /// while input is being parsed. Under output pressure the input line still
    /// wins and the echo record is dropped whole.
    pub fn try_enqueue_tx_record(&mut self, parts: &[&[u8]]) -> bool {
        let mut total = 0usize;
        for part in parts {
            total = total.saturating_add(part.len());
        }
        let pending = self
            .tx
            .len()
            .saturating_add(usize::from(self.driver_local.pending_tx_byte().is_some()));
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
        #[cfg(feature = "kernel")]
        if serial_driver_task_transport_active() {
            self.enqueue_tx(data);
            self.flush_tx_locked();
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
        #[cfg(feature = "kernel")]
        if serial_driver_task_transport_active() {
            self.enqueue_tx(line.as_bytes());
            self.enqueue_tx(b"\r\n");
            self.flush_tx_locked();
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
            if serial_driver_task_transport_active() {
                crate::hal::driver_task::register_driver_task_pointer_free_ring_service(
                    contract,
                    crate::hal::driver_task::DriverTaskHotPath::SerialConsole.as_u32() as usize,
                    serial_runtime_ring_service_driver_task,
                );
                if self.flush_tx_driver_task_ring(contract) {
                    let _ = self.poll_driver_task_rx_into_queue(contract);
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

    #[cfg(feature = "kernel")]
    fn poll_driver_task_rx_into_queue(&mut self, contract: DriverTaskContract) -> bool {
        if self.drain_driver_task_client_rx_queue() != 0 {
            return true;
        }
        let Some(budget) = serial_driver_task_rx_budget(contract, RX.saturating_sub(self.rx.len()))
        else {
            return false;
        };
        crate::hal::driver_task::register_driver_task_pointer_free_ring_service(
            contract,
            crate::hal::driver_task::DriverTaskHotPath::SerialConsole.as_u32() as usize,
            serial_runtime_ring_service_driver_task,
        );
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
        if let Some(completion) =
            crate::hal::driver_task::run_driver_task_ring_service(contract, command)
        {
            if completion.code
                == crate::hal::driver_task::DriverTaskCompletionCode::FrameReady.as_u16()
            {
                if let Some(bytes) = crate::hal::driver_task::driver_task_ring_frame_bytes(
                    contract,
                    completion.frame,
                ) {
                    let mut accepted = 0usize;
                    for &byte in bytes {
                        if self.rx.enqueue(byte).is_err() {
                            self.telemetry.rx_overflow();
                            break;
                        }
                        accepted = accepted.saturating_add(1);
                    }
                    return accepted != 0;
                }
                self.telemetry.driver_task_budget_overrun();
                return false;
            }
            return completion.code
                == crate::hal::driver_task::DriverTaskCompletionCode::Progress.as_u16()
                && completion.result != 0;
        }
        self.telemetry.driver_task_budget_overrun();
        false
    }

    #[cfg(feature = "kernel")]
    fn drain_driver_task_client_rx_queue(&mut self) -> usize {
        let mut rx = SERIAL_CLIENT_RX.lock();
        let mut accepted = 0usize;
        while self.rx.len() < RX {
            let Some(byte) = rx.dequeue() else {
                break;
            };
            if self.rx.enqueue(byte).is_err() {
                self.telemetry.rx_overflow();
                break;
            }
            accepted = accepted.saturating_add(1);
        }
        accepted
    }

    /// Switch prompt-side service from the root mini-UART fallback to the driver task.
    #[cfg(feature = "kernel")]
    pub fn use_driver_task_client_after_attach(&mut self) -> bool {
        if !serial_driver_task_transport_active() {
            if !serial_driver_task_interactive_cutover_allowed() {
                return false;
            }
        }
        self.flush_tx_locked();
        let attached = self.driver.try_use_driver_task_client_after_attach();
        if attached {
            SERIAL_DRIVER_TASK_CLIENT_ACTIVE.store(1, AtomicOrdering::Release);
        }
        attached
    }

    #[cfg(feature = "kernel")]
    fn flush_tx_driver_task_ring(&mut self, contract: DriverTaskContract) -> bool {
        let turn_limit = usize::from(contract.budget.max_ops_per_turn)
            .min(contract.budget.max_bytes_per_turn as usize)
            .min(crate::hal::driver_task::MAX_DRIVER_TASK_FRAME_BYTES);
        let mut staged =
            heapless::Vec::<u8, { crate::hal::driver_task::MAX_DRIVER_TASK_FRAME_BYTES }>::new();
        if let Some(byte) = self.driver_local.take_pending_tx() {
            let _ = staged.push(byte);
        }
        while staged.len() < turn_limit {
            let Some(byte) = self.tx.dequeue() else {
                break;
            };
            let _ = staged.push(byte);
        }

        let frame = if staged.is_empty() {
            crate::hal::driver_task::DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            }
        } else {
            match crate::hal::driver_task::stage_driver_task_ring_frame(
                contract,
                staged.as_slice(),
                0,
            ) {
                Some(frame) => frame,
                None => {
                    self.restore_staged_tx(staged.as_slice());
                    return false;
                }
            }
        };
        let command = crate::hal::driver_task::DriverTaskCommandRecord::pi4_hot_path(
            0,
            crate::hal::driver_task::DriverTaskHotPath::SerialConsole,
            crate::hal::driver_task::DriverTaskBudgetGrant::from_contract(contract),
            frame,
        );
        let completion = crate::hal::driver_task::run_driver_task_ring_service(contract, command);
        let written = match completion {
            Some(completion)
                if completion.code
                    == crate::hal::driver_task::DriverTaskCompletionCode::Progress.as_u16() =>
            {
                completion.result as usize
            }
            Some(completion)
                if completion.code
                    == crate::hal::driver_task::DriverTaskCompletionCode::Idle.as_u16()
                    && staged.is_empty() =>
            {
                0
            }
            _ => {
                self.restore_staged_tx(staged.as_slice());
                return false;
            }
        };
        if written < staged.len() {
            self.restore_staged_tx(&staged[written..]);
        }
        true
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
                    #[cfg(feature = "kernel")]
                    prompt_shadow_clear();
                    self.emit_newline();
                    let mut completed = HeaplessString::new();
                    core::mem::swap(&mut completed, &mut self.line);
                    return Some(completed);
                }
                b'\n' => {
                    #[cfg(feature = "kernel")]
                    prompt_shadow_clear();
                    self.emit_newline();
                    let mut completed = HeaplessString::new();
                    core::mem::swap(&mut completed, &mut self.line);
                    return Some(completed);
                }
                0x08 | 0x7f => {
                    if self.line.pop().is_some() && self.driver_local.echo_enabled() {
                        #[cfg(feature = "kernel")]
                        prompt_shadow_pop();
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
                    prompt_shadow_push(byte);
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
        let had_partial = !self.line.is_empty();
        self.line.clear();
        #[cfg(feature = "kernel")]
        prompt_shadow_clear();
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
    let read = serial_runtime_poll_bytes(port, &mut rx, contract);
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
        if port.serial_byte_budget_available(budget).is_err() {
            break;
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
            if port.serial_byte_budget_available(&budget).is_err() {
                port.telemetry.driver_task_budget_overrun();
                break;
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
    fn physical_pi_serial_interactive_cutover_requires_rx_proof() {
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
