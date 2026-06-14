// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Enforce manifest-gated local diagnostics seat policy and bounds.
// Author: Lukas Bower

//! Local diagnostics seat policy helpers (Milestone 26).

#![allow(unsafe_code)]

extern crate alloc;

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
use crate::bootstrap::log as boot_log;
use crate::console::{Command, CommandParser, ConsoleError};
use crate::generated::{self, HardwareDeviceKind};
use crate::hal::driver_task::{
    DriverServiceBudget, DriverTaskContract, USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT,
};
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
#[cfg(all(feature = "kernel", feature = "usb"))]
use pi4_driver_abi::{DRIVER_RUNTIME_LOCAL_SEAT_INIT_AUX, DRIVER_RUNTIME_USB_ENUMERATE_AUX};
#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
use pi4_driver_abi::{
    DRIVER_RUNTIME_USB_INIT_DETAIL_ADDRESS_DEVICE_FAILED,
    DRIVER_RUNTIME_USB_INIT_DETAIL_COMMAND_RING_PENDING,
    DRIVER_RUNTIME_USB_INIT_DETAIL_COMMAND_RING_READY,
    DRIVER_RUNTIME_USB_INIT_DETAIL_CONFIG_DESCRIPTOR,
    DRIVER_RUNTIME_USB_INIT_DETAIL_CONFIG_DESCRIPTOR_FAILED,
    DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_ADDRESSED,
    DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_DESCRIPTOR,
    DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_DESCRIPTOR_FAILED,
    DRIVER_RUNTIME_USB_INIT_DETAIL_ENABLE_SLOT_FAILED,
    DRIVER_RUNTIME_USB_INIT_DETAIL_HID_ATTACH_FAILED,
    DRIVER_RUNTIME_USB_INIT_DETAIL_HID_ENDPOINT_SEEN,
    DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_ATTACH_FAILED,
    DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_CONTEXT_FAILED,
    DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_DESCRIPTOR_FAILED,
    DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_SET_CONFIG_FAILED,
    DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_TOPOLOGY_SEEN,
    DRIVER_RUNTIME_USB_INIT_DETAIL_KEYBOARD_READY,
    DRIVER_RUNTIME_USB_INIT_DETAIL_ROOT_PORT_CONNECTED, DRIVER_RUNTIME_USB_INIT_DETAIL_XHCI_READY,
    DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_PENDING,
    DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_READY,
};
#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
use spin::Mutex;

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
const USB_ENUM_RESULT_ROOT_PORT_MASK: u32 = 0x0000_00ff;
#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
const USB_ENUM_RESULT_SLOT_SHIFT: u32 = 8;
#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
const USB_ENUM_RESULT_ENDPOINT_SHIFT: u32 = 16;
#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
const USB_ENUM_RESULT_SCAN_PASS_SHIFT: u32 = 21;
#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
const USB_ENUM_RESULT_SCAN_PASS_MASK: u32 = 0x3;
#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
const USB_ENUM_RESULT_ROOT_POWERED: u32 = 1 << 25;
#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
const USB_ENUM_RESULT_COMMAND_PATH: u32 = 1 << 26;
#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
const USB_ENUM_RESULT_PORT_EVENT: u32 = 1 << 27;
#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
const USB_ENUM_RESULT_HID_ENDPOINT: u32 = 1 << 28;
#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
const USB_ENUM_RESULT_PRESERVED_EVENT: u32 = 1 << 29;
#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
const USB_ENUM_RESULT_TRANSFER_EVENT: u32 = 1 << 30;
#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
const USB_ENUM_RESULT_ENDPOINT_READY: u32 = 1 << 31;

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LOCAL_SEAT_POLL_LOGGED: AtomicBool = AtomicBool::new(false);
#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LOCAL_SEAT_RUNTIME_INIT_LEASE: Mutex<Option<LocalSeatRuntimeInitLease>> = Mutex::new(None);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_RUNTIME_ATTACHED: AtomicBool = AtomicBool::new(false);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_PCIE_HAL_PREP_ATTEMPTS: AtomicUsize = AtomicUsize::new(0);
#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
const LINKED_LOCAL_SEAT_PCIE_HAL_PREP_MAX_ATTEMPTS: usize = 8;
const LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES: usize = 64;

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_PCIE_HAL_PREP_BEGIN_LOGGED: AtomicBool = AtomicBool::new(false);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_PCIE_HAL_PREP_READY_LOGGED: AtomicBool = AtomicBool::new(false);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_PCIE_HAL_PREP_BLOCKED_LOGGED: AtomicBool = AtomicBool::new(false);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_DISPLAY_ATTACHED: AtomicBool = AtomicBool::new(false);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_DISPLAY_FAILED: AtomicBool = AtomicBool::new(false);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_DISPLAY_DEFERRED_LOGGED: AtomicBool = AtomicBool::new(false);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_DISPLAY_NO_REPLY_LOGGED: AtomicBool = AtomicBool::new(false);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_DISPLAY_INIT_ATTEMPTED: AtomicBool = AtomicBool::new(false);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_DISPLAY_INIT_ATTEMPTS: AtomicUsize = AtomicUsize::new(0);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_DISPLAY_FIRST_DRAW_READY_LOGGED: AtomicBool = AtomicBool::new(false);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_DISPLAY_FIRST_DRAW_FAILED_LOGGED: AtomicBool = AtomicBool::new(false);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_DISPLAY_INIT_DEFERRED_LOGGED: AtomicBool = AtomicBool::new(false);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_DISPLAY_ADOPTED_LOGGED: AtomicBool = AtomicBool::new(false);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_DISPLAY_FIRST_FRAME_OWNER_LOGGED: AtomicBool = AtomicBool::new(false);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_PCIE_REPLAY_DEFERRED_LOGGED: AtomicBool = AtomicBool::new(false);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_PCIE_REPLAY_BEGIN_LOGGED: AtomicBool = AtomicBool::new(false);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_PCIE_REPLAY_READY_LOGGED: AtomicBool = AtomicBool::new(false);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_PCIE_ENGINE_BEGIN_LOGGED: AtomicBool = AtomicBool::new(false);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_PCIE_ENGINE_DEFERRED_LOGGED: AtomicBool = AtomicBool::new(false);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_USB_REPLAY_BEGIN_LOGGED: AtomicBool = AtomicBool::new(false);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_USB_REPLAY_DEFERRED_LOGGED: AtomicBool = AtomicBool::new(false);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_USB_REPLAY_READY_LOGGED: AtomicBool = AtomicBool::new(false);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_USB_INIT_DEFERRED_LOGGED: AtomicBool = AtomicBool::new(false);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_USB_ENGINE_BEGIN_LOGGED: AtomicBool = AtomicBool::new(false);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_USB_KEYBOARD_READY: AtomicBool = AtomicBool::new(false);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_USB_ENUMERATION_PENDING: AtomicBool = AtomicBool::new(false);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_USB_LAST_DETAIL: AtomicUsize = AtomicUsize::new(0);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_USB_LAST_RESULT: AtomicUsize = AtomicUsize::new(0);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_USB_ENUM_LOGGED: AtomicBool = AtomicBool::new(false);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_USB_ENUM_PROGRESS_LOGGED: AtomicBool = AtomicBool::new(false);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_USB_ENUM_READY_LOGGED: AtomicBool = AtomicBool::new(false);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_USB_OWNER_READY_LOGGED: AtomicBool = AtomicBool::new(false);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_USB_FIRST_REPORT_PENDING_LOGGED: AtomicBool = AtomicBool::new(false);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_USB_FIRST_REPORT_READY_LOGGED: AtomicBool = AtomicBool::new(false);

#[cfg(all(feature = "kernel", feature = "usb"))]
// A controller-ready completion can precede root-port and HID endpoint events by
// a few linked-runtime turns; keep prompt settling bounded and non-blocking.
const LINKED_LOCAL_SEAT_USB_ENUM_RESUME_ATTEMPTS: usize = 3;
#[cfg(all(feature = "kernel", feature = "usb"))]
const LINKED_LOCAL_SEAT_USB_PROBE_STABLE_PROGRESS_BURST_ATTEMPTS: usize = 8;

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
#[derive(Clone, Copy)]
struct LocalSeatRuntimeInitLease {
    hal_ptr: usize,
    _hints: LocalSeatPlatformHints,
}

/// Maximum number of queued keyboard bytes retained by the local-seat runtime.
pub const KEYBOARD_QUEUE_MAX_BYTES: usize = 4_096;

/// Maximum keyboard bytes drained from the runtime in one event-pump cycle.
pub const KEYBOARD_POLL_CHUNK_BYTES: usize = 128;

/// HAL-enforced scheduling contract for USB local-seat input service.
#[must_use]
pub const fn driver_task_contract() -> DriverTaskContract {
    USB_LOCAL_SEAT_DRIVER_TASK_CONTRACT
}

/// Return whether a USB ownership status line may report replayed COMMAND as
/// fresh runtime authority.
pub(crate) fn usb_runtime_command_replay_ready(
    cfg_replay_ready: bool,
    command_ready: bool,
    command_source: &'static str,
) -> bool {
    cfg_replay_ready && command_ready && matches!(command_source, "hal-ext-cfg-proof")
}

/// Return whether pre-root local-seat runtime init may submit USB/xHCI service
/// turns without risking the serial shell.
#[must_use]
pub(crate) const fn local_seat_pre_root_runtime_init_allowed(
    physical_pi_owner_state: bool,
    pointer_free_ipc_proof: bool,
) -> bool {
    !physical_pi_owner_state || pointer_free_ipc_proof
}

/// Return whether display mirroring may submit a linked-runtime service turn.
#[must_use]
pub(crate) const fn local_seat_linked_display_service_allowed(
    physical_pi_owner_state: bool,
    display_attached: bool,
    display_failed: bool,
) -> bool {
    !physical_pi_owner_state || display_attached || !display_failed
}

/// Return whether linked local-seat service may use the steady driver-task path.
#[must_use]
pub(crate) const fn local_seat_prompt_steady_service_allowed(
    physical_pi_owner_state: bool,
    root_console_ready: bool,
    pointer_free_ipc_proof: bool,
) -> bool {
    !physical_pi_owner_state || root_console_ready || pointer_free_ipc_proof
}

/// Return whether prompt-side HDMI attach should retry after a transient miss.
#[must_use]
pub(crate) const fn local_seat_display_attach_retry_allowed(
    root_console_ready: bool,
    display_attached: bool,
    attempts: usize,
) -> bool {
    const DISPLAY_ATTACH_RETRY_LIMIT: usize = 4;

    root_console_ready && !display_attached && attempts < DISPLAY_ATTACH_RETRY_LIMIT
}

/// Return whether a post-prompt USB poll/attach miss should suspend background
/// keyboard polling instead of holding the serial shell behind another turn.
#[must_use]
pub(crate) const fn local_seat_keyboard_poll_suspends_on_missing_reply(
    physical_pi_owner_state: bool,
    root_console_ready: bool,
    keyboard_ready: bool,
) -> bool {
    physical_pi_owner_state && root_console_ready && !keyboard_ready
}

/// Return whether a display mirror miss should preserve the serial shell.
#[must_use]
pub(crate) const fn local_seat_display_mirror_suspends_on_missing_reply(
    physical_pi_owner_state: bool,
    root_console_ready: bool,
) -> bool {
    physical_pi_owner_state && root_console_ready
}

#[cfg(all(feature = "kernel", feature = "usb"))]
const fn local_seat_keyboard_poll_aux(keyboard_ready: bool) -> u32 {
    let _ = keyboard_ready;
    0
}

#[cfg(all(feature = "kernel", feature = "usb"))]
const fn linked_local_seat_usb_attach_probe_required(
    controller_attached: bool,
    keyboard_ready: bool,
    enumeration_pending: bool,
) -> bool {
    !controller_attached || (enumeration_pending && !keyboard_ready)
}

#[cfg(all(feature = "kernel", feature = "usb"))]
const fn linked_local_seat_usb_pre_prompt_retry_deferred(
    controller_attached: bool,
    keyboard_ready: bool,
    enumeration_pending: bool,
    root_console_ready: bool,
) -> bool {
    controller_attached && enumeration_pending && !keyboard_ready && !root_console_ready
}

/// Deterministic local-seat initialisation outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalSeatInit {
    /// Local seat not requested by manifest.
    Disabled,
    /// Local seat is active and can mirror I/O.
    Active(LocalSeatStatus),
    /// Manifest allowed degradation to serial-only diagnostics.
    Degraded(LocalSeatDegradedReason),
}

/// Local-seat readiness details when active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalSeatStatus {
    /// Declared keyboard device identifier.
    pub keyboard_device: &'static str,
    /// Declared display device identifier.
    pub display_device: &'static str,
    /// Maximum mirrored line width in bytes.
    pub line_bytes: u16,
    /// Ring depth for mirrored lines.
    pub buffer_lines: u16,
}

/// Result of one bounded physical keyboard probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalSeatKeyboardProbeResult {
    /// A backend keyboard was attached during the probe.
    Attached,
    /// A platform backend exists, but no keyboard became usable.
    KeyboardUnavailable,
    /// Driver-task local-seat service is intentionally deferred until prompt.
    DeferredUntilRootConsole,
    /// No platform backend is attached.
    BackendUnavailable,
}

impl LocalSeatKeyboardProbeResult {
    /// Returns whether the physical keyboard path is usable.
    #[must_use]
    pub const fn attached(self) -> bool {
        matches!(self, Self::Attached)
    }

    /// Stable diagnostic token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Attached => "attached",
            Self::KeyboardUnavailable => "keyboard-unavailable",
            Self::DeferredUntilRootConsole => "deferred-until-root-console",
            Self::BackendUnavailable => "backend-unavailable",
        }
    }
}

/// Keyboard ingress counters for isolating local-seat stalls without hot logs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LocalSeatKeyboardTrace {
    /// Bytes currently waiting in the local-seat queue.
    pub queued_bytes: usize,
    /// Event-pump attempts to poll the platform keyboard backend.
    pub backend_poll_calls: u64,
    /// Bytes returned by the platform keyboard backend.
    pub backend_read_bytes: u64,
    /// Bytes accepted into the bounded local-seat queue.
    pub accepted_bytes: u64,
    /// Bytes drained from the queue into the console parser path.
    pub drained_bytes: u64,
    /// Bytes echoed to HDMI after entering the parser path.
    pub echoed_bytes: u64,
    /// Bytes dropped because the queue was full.
    pub dropped_bytes: u64,
    /// Service turns stopped by the HAL driver-task budget.
    pub driver_task_budget_overruns: u64,
}

/// Runtime state for local-seat keyboard ingress and mirrored line egress.
///
/// This state is bounded by manifest values (`line_bytes`, `buffer_lines`) and
/// is transport-agnostic so HAL-owned keyboard/display backends can wire bytes
/// in/out without affecting parser semantics.
#[derive(Debug)]
pub struct LocalSeatRuntime {
    status: LocalSeatStatus,
    keyboard_queue: VecDeque<u8>,
    input_echo_preview: String,
    mirrored_lines: VecDeque<String>,
    hdmi_scrollback_offset: usize,
    hdmi_input_escape_state: u8,
    dropped_keyboard_bytes: u64,
    dropped_mirrored_lines: u64,
    backend_keyboard_poll_calls: u64,
    backend_keyboard_read_bytes: u64,
    accepted_keyboard_bytes: u64,
    drained_keyboard_bytes: u64,
    echoed_keyboard_bytes: u64,
    driver_task_budget_overruns: u64,
    backend_keyboard_polling_enabled: bool,
    backend_keyboard_poll_deferred_logged: bool,
    root_console_ready: bool,
}

/// Optional DT/firmware display mapping hint for local-seat HDMI output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalSeatDisplayHint {
    /// Physical base of the framebuffer allocation.
    pub paddr: usize,
    /// Visible width in pixels.
    pub width: usize,
    /// Visible height in pixels.
    pub height: usize,
    /// Bytes per rendered scanline.
    pub pitch: usize,
}

/// Optional bootloader-provided xHCI capability snapshot for local-seat handoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalSeatXhciCapabilitySnapshot {
    /// Capability register length.
    pub cap_length: u8,
    /// xHCI interface version.
    pub hci_version: u16,
    /// Structural Parameters 1.
    pub hcs1: u32,
    /// Structural Parameters 2.
    pub hcs2: u32,
    /// Capability Parameters 1.
    pub hccparams1: u32,
    /// Doorbell offset.
    pub db_offset: u32,
    /// Runtime space offset.
    pub rts_offset: u32,
}

/// Optional bootloader-provided xHCI stop-state snapshot for local-seat handoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalSeatXhciStopStateSnapshot {
    /// Operational `USBCMD` captured before handoff.
    pub usbcmd: Option<u32>,
    /// Operational `USBSTS` captured before handoff.
    pub usbsts: Option<u32>,
    /// Interrupter 0 `IMAN` captured before handoff.
    pub iman0: Option<u32>,
}

/// Optional platform-specific hints for local-seat backend attachment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LocalSeatPlatformHints {
    /// Whether local-seat keyboard/display proof is a boot requirement.
    pub required: bool,
    /// Optional MMIO base for Pi4 xHCI.
    pub xhci_mmio_hint: Option<usize>,
    /// Legacy diagnostic PCI command hint from older Pi4 boot scripts.
    pub xhci_pci_cmd: Option<u16>,
    /// Legacy diagnostic flag from older Pi4 xHCI handoff scripts.
    pub xhci_handoff_ready: bool,
    /// Legacy diagnostic flag from older Pi4 xHCI handoff scripts.
    pub xhci_irq_quiesced: bool,
    /// Legacy diagnostic flag from older Pi4 xHCI handoff scripts.
    pub xhci_bootloader_reset_authorized: bool,
    /// Optional diagnostic capability snapshot from older Pi4 boot scripts.
    pub xhci_capability_snapshot: Option<LocalSeatXhciCapabilitySnapshot>,
    /// Optional stop-state snapshot exported by the bootloader.
    pub xhci_stop_state_snapshot: Option<LocalSeatXhciStopStateSnapshot>,
    /// Optional DT/firmware framebuffer hint for HDMI rendering.
    pub display_hint: Option<LocalSeatDisplayHint>,
}

impl LocalSeatRuntime {
    /// Create a new runtime buffer set for the active local-seat manifest.
    #[must_use]
    pub fn new(status: LocalSeatStatus) -> Self {
        Self {
            status,
            keyboard_queue: VecDeque::new(),
            input_echo_preview: String::new(),
            mirrored_lines: VecDeque::new(),
            hdmi_scrollback_offset: 0,
            hdmi_input_escape_state: 0,
            dropped_keyboard_bytes: 0,
            dropped_mirrored_lines: 0,
            backend_keyboard_poll_calls: 0,
            backend_keyboard_read_bytes: 0,
            accepted_keyboard_bytes: 0,
            drained_keyboard_bytes: 0,
            echoed_keyboard_bytes: 0,
            driver_task_budget_overruns: 0,
            // Keep boot fail-open: the root shell must stay reachable even if
            // a platform keyboard backend can still wedge during first probe.
            backend_keyboard_polling_enabled: false,
            backend_keyboard_poll_deferred_logged: false,
            root_console_ready: false,
        }
    }

    /// Return manifest-derived runtime limits.
    #[must_use]
    pub const fn status(&self) -> LocalSeatStatus {
        self.status
    }

    /// Queue keyboard bytes received from a HAL-owned input backend.
    ///
    /// Returns the number of bytes accepted into the bounded queue.
    pub fn enqueue_keyboard_bytes(&mut self, bytes: &[u8]) -> usize {
        let mut accepted = 0usize;
        for &byte in bytes {
            if self.keyboard_queue.len() >= KEYBOARD_QUEUE_MAX_BYTES {
                self.dropped_keyboard_bytes = self.dropped_keyboard_bytes.saturating_add(1);
                continue;
            }
            self.keyboard_queue.push_back(byte);
            accepted = accepted.saturating_add(1);
        }
        self.accepted_keyboard_bytes = self.accepted_keyboard_bytes.saturating_add(accepted as u64);
        accepted
    }

    /// Drain queued keyboard bytes into `out` and return bytes written.
    pub fn drain_keyboard_bytes(&mut self, out: &mut [u8]) -> usize {
        let mut written = 0usize;
        for slot in out.iter_mut() {
            match self.keyboard_queue.pop_front() {
                Some(byte) => {
                    *slot = byte;
                    written = written.saturating_add(1);
                }
                None => break,
            }
        }
        self.drained_keyboard_bytes = self.drained_keyboard_bytes.saturating_add(written as u64);
        written
    }

    /// Mirror a console line into the bounded local-seat output ring.
    pub fn mirror_line(&mut self, line: &str) {
        #[cfg(feature = "kernel")]
        {
            let contract = crate::hal::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT;
            #[cfg(all(feature = "usb", target_arch = "aarch64", target_os = "none"))]
            {
                if crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active() {
                    let _ = adopt_linked_display_runtime_owner_state("mirror-line");
                    if !LINKED_LOCAL_SEAT_DISPLAY_ATTACHED.load(Ordering::Acquire)
                        && !LINKED_LOCAL_SEAT_DISPLAY_FAILED.load(Ordering::Acquire)
                    {
                        let _ = try_attach_linked_display_runtime(self.root_console_ready);
                    }
                    if !local_seat_linked_display_service_allowed(
                        true,
                        LINKED_LOCAL_SEAT_DISPLAY_ATTACHED.load(Ordering::Acquire),
                        LINKED_LOCAL_SEAT_DISPLAY_FAILED.load(Ordering::Acquire),
                    ) {
                        if !LINKED_LOCAL_SEAT_DISPLAY_DEFERRED_LOGGED.swap(true, Ordering::AcqRel) {
                            boot_log::force_uart_line(
                                "[local-seat] runtime display mirror deferred action=serial-shell-first",
                            );
                        }
                        return;
                    }
                    crate::hal::driver_task::register_driver_task_pointer_free_ring_service(
                        contract,
                        crate::hal::driver_task::DriverTaskHotPath::HdmiText.as_u32() as usize,
                        display_runtime_ring_service_driver_task,
                    );
                    if crate::hal::driver_task::driver_task_ring_command_active(contract) {
                        self.driver_task_budget_overruns =
                            self.driver_task_budget_overruns.saturating_add(1);
                        return;
                    }
                    self.mirror_line_current_tcb(line);
                    let line_bytes = line.as_bytes();
                    let chunk_limit = LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES
                        .min(crate::hal::driver_task::MAX_DRIVER_TASK_FRAME_BYTES)
                        .max(1);
                    let mut offset = 0usize;
                    let mut newline_pending = true;
                    loop {
                        let mut payload = heapless::Vec::<
                            u8,
                            { crate::hal::driver_task::MAX_DRIVER_TASK_FRAME_BYTES },
                        >::new();
                        while payload.len() < chunk_limit && offset < line_bytes.len() {
                            let _ = payload.push(line_bytes[offset]);
                            offset = offset.saturating_add(1);
                        }
                        if offset == line_bytes.len()
                            && newline_pending
                            && payload.len() < chunk_limit
                        {
                            let _ = payload.push(b'\n');
                            newline_pending = false;
                        }
                        if payload.is_empty() {
                            break;
                        }
                        let Some(frame) = crate::hal::driver_task::describe_driver_task_ring_frame(
                            payload.as_slice(),
                            0,
                        ) else {
                            break;
                        };
                        let command =
                            crate::hal::driver_task::DriverTaskCommandRecord::pi4_hot_path(
                                0,
                                crate::hal::driver_task::DriverTaskHotPath::HdmiText,
                                crate::hal::driver_task::DriverTaskBudgetGrant::from_contract(
                                    contract,
                                ),
                                frame,
                            );
                        let staging_segments = [
                            crate::hal::driver_task::DriverTaskStagingSegment::ring_frame(
                                payload.as_slice(),
                                0,
                            ),
                        ];
                        let completion = run_local_seat_driver_task_ring_service_staged(
                            contract,
                            command,
                            &staging_segments,
                        );
                        let ready = completion.is_some_and(|completion| {
                            completion.code
                                == crate::hal::driver_task::DriverTaskCompletionCode::Progress
                                    .as_u16()
                                && completion.result != 0
                        });
                        emit_hdmi_frame_submit_state(
                            "console-line",
                            payload.len(),
                            self.root_console_ready,
                            completion,
                            ready,
                            false,
                        );
                        if ready {
                            let Some(completion) = completion else {
                                return;
                            };
                            if completion.code
                                == crate::hal::driver_task::DriverTaskCompletionCode::Progress
                                    .as_u16()
                                && completion.result != 0
                            {
                                let _ = credit_linked_display_runtime_frame_owner_state(
                                    "console-line",
                                    self.root_console_ready,
                                    completion,
                                );
                                if !LINKED_LOCAL_SEAT_DISPLAY_FIRST_DRAW_READY_LOGGED
                                    .swap(true, Ordering::AcqRel)
                                {
                                    crate::hal::driver_task::emit_driver_task_resource_init_status(
                                        contract,
                                        crate::hal::driver_task::DriverTaskHotPath::HdmiText,
                                        "hdmi-first-draw",
                                        "ready",
                                        Some(completion),
                                    );
                                    emit_hdmi_text_final_state(
                                        true,
                                        "first-draw",
                                        "framebuffer-write",
                                        self.root_console_ready,
                                        LINKED_LOCAL_SEAT_DISPLAY_INIT_ATTEMPTS
                                            .load(Ordering::Acquire),
                                        Some(completion),
                                    );
                                }
                                if offset == line_bytes.len() && !newline_pending {
                                    return;
                                }
                                continue;
                            }
                        }
                        if !LINKED_LOCAL_SEAT_DISPLAY_FIRST_DRAW_FAILED_LOGGED
                            .swap(true, Ordering::AcqRel)
                        {
                            let status = local_seat_completion_status(completion, false);
                            crate::hal::driver_task::emit_driver_task_resource_init_status(
                                contract,
                                crate::hal::driver_task::DriverTaskHotPath::HdmiText,
                                "hdmi-first-draw",
                                status,
                                completion,
                            );
                        }
                        self.driver_task_budget_overruns =
                            self.driver_task_budget_overruns.saturating_add(1);
                        if completion.is_none()
                            && local_seat_display_mirror_suspends_on_missing_reply(
                                true,
                                self.root_console_ready,
                            )
                        {
                            if !LINKED_LOCAL_SEAT_DISPLAY_NO_REPLY_LOGGED
                                .swap(true, Ordering::AcqRel)
                            {
                                boot_log::force_uart_line(
                                    "[local-seat] runtime display mirror deferred reason=driver-task-no-reply action=retry-next-frame",
                                );
                            }
                        }
                        return;
                    }
                    self.driver_task_budget_overruns =
                        self.driver_task_budget_overruns.saturating_add(1);
                    if local_seat_display_mirror_suspends_on_missing_reply(
                        true,
                        self.root_console_ready,
                    ) && !LINKED_LOCAL_SEAT_DISPLAY_NO_REPLY_LOGGED.swap(true, Ordering::AcqRel)
                    {
                        boot_log::force_uart_line(
                            "[local-seat] runtime display mirror deferred reason=driver-task-no-reply action=retry-next-frame",
                        );
                    }
                    return;
                }
            }
            if !crate::hal::driver_task::steady_state_root_compatibility_service_allowed(contract) {
                self.driver_task_budget_overruns =
                    self.driver_task_budget_overruns.saturating_add(1);
                return;
            }
            crate::hal::driver_task::register_driver_task_root_context_ring_service(
                contract,
                self as *mut Self as usize,
                display_ring_service_driver_task,
            );
            if let Some(frame) = crate::hal::driver_task::describe_driver_task_ring_frame(
                line.as_bytes(),
                crate::hal::driver_task::DRIVER_TASK_RING_FLAG_ROOT_CONTEXT_NON_ACCEPTANCE,
            ) {
                let command = crate::hal::driver_task::DriverTaskCommandRecord::pi4_hot_path(
                    0,
                    crate::hal::driver_task::DriverTaskHotPath::HdmiText,
                    crate::hal::driver_task::DriverTaskBudgetGrant::from_contract(contract),
                    frame,
                );
                let staging_segments = [
                    crate::hal::driver_task::DriverTaskStagingSegment::ring_frame(
                        line.as_bytes(),
                        crate::hal::driver_task::DRIVER_TASK_RING_FLAG_ROOT_CONTEXT_NON_ACCEPTANCE,
                    ),
                ];
                if run_local_seat_driver_task_ring_service_staged(
                    contract,
                    command,
                    &staging_segments,
                )
                .is_some()
                {
                    return;
                }
            }
            let mut context = DisplayMirrorTaskContext {
                runtime: self as *mut Self as usize,
                line_ptr: line.as_ptr() as usize,
                line_len: line.len(),
            };
            // SAFETY: The HAL admits this compatibility callback only for
            // QEMU/host profiles. Physical Pi 4 builds return None without
            // compiling callback slot state.
            if unsafe {
                crate::hal::driver_task::try_driver_task_compat_service(
                    contract,
                    &mut context as *mut DisplayMirrorTaskContext as usize,
                    display_mirror_driver_task,
                )
            }
            .is_some()
            {
                return;
            }
            if !crate::hal::driver_task::admit_root_task_compatibility_service(contract) {
                self.driver_task_budget_overruns =
                    self.driver_task_budget_overruns.saturating_add(1);
                return;
            }
        }
        self.mirror_line_current_tcb(line);
    }

    /// Mirror an explicit high-impact progress line.
    ///
    /// On physical Pi 4 this is the only path that may submit HDMI text, and
    /// it uses the linked `hdmi-text` driver runtime. Routine console/log lines
    /// remain on UART so serial captures stay complete.
    pub fn mirror_high_impact_line(&mut self, line: &str) -> bool {
        #[cfg(all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        ))]
        {
            if crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active() {
                self.mirror_line_current_tcb(line);
                return mirror_high_impact_line_via_linked_hdmi(
                    line,
                    self.root_console_ready,
                    "local-seat-high-impact",
                );
            }
        }
        self.mirror_line(line);
        true
    }

    fn mirror_line_current_tcb(&mut self, line: &str) {
        let truncated = truncate_for_display(line, self.status.line_bytes);
        let mut mirrored = String::new();
        mirrored.push_str(truncated);

        while self.mirrored_lines.len() >= usize::from(self.status.buffer_lines) {
            if self.mirrored_lines.pop_front().is_none() {
                break;
            }
            self.dropped_mirrored_lines = self.dropped_mirrored_lines.saturating_add(1);
        }
        self.mirrored_lines.push_back(mirrored);
        self.hdmi_scrollback_offset = 0;

        #[cfg(all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        ))]
        let _ = truncated;
    }

    /// Snapshot mirrored lines for diagnostics/tests.
    #[must_use]
    pub fn mirrored_lines_snapshot(&self) -> Vec<String> {
        self.mirrored_lines.iter().cloned().collect()
    }

    /// Count of keyboard bytes dropped due to queue saturation.
    #[must_use]
    pub const fn dropped_keyboard_bytes(&self) -> u64 {
        self.dropped_keyboard_bytes
    }

    /// Count of mirrored lines dropped due to ring saturation.
    #[must_use]
    pub const fn dropped_mirrored_lines(&self) -> u64 {
        self.dropped_mirrored_lines
    }

    /// Returns whether backend keyboard polling is currently enabled.
    #[must_use]
    pub const fn backend_keyboard_polling_enabled(&self) -> bool {
        self.backend_keyboard_polling_enabled
    }

    /// Returns whether the serial root console may settle local-seat work.
    #[must_use]
    pub const fn root_console_ready(&self) -> bool {
        self.root_console_ready
    }

    /// Return keyboard ingress counters for `usb status` diagnostics.
    #[must_use]
    pub fn keyboard_trace(&self) -> LocalSeatKeyboardTrace {
        LocalSeatKeyboardTrace {
            queued_bytes: self.keyboard_queue.len(),
            backend_poll_calls: self.backend_keyboard_poll_calls,
            backend_read_bytes: self.backend_keyboard_read_bytes,
            accepted_bytes: self.accepted_keyboard_bytes,
            drained_bytes: self.drained_keyboard_bytes,
            echoed_bytes: self.echoed_keyboard_bytes,
            dropped_bytes: self.dropped_keyboard_bytes,
            driver_task_budget_overruns: self.driver_task_budget_overruns,
        }
    }

    /// Enable backend keyboard polling after boot has reached a safe manual
    /// control point.
    pub fn enable_backend_keyboard_polling(&mut self) {
        self.backend_keyboard_polling_enabled = true;
        self.backend_keyboard_poll_deferred_logged = false;
    }

    /// Mark that the serial root console may settle local-seat work.
    pub fn mark_root_console_ready(&mut self) {
        self.root_console_ready = true;
        self.backend_keyboard_poll_deferred_logged = false;
    }

    /// Try the linked HDMI runtime after the serial prompt.
    #[must_use]
    pub fn ensure_prompt_linked_display_ready(&mut self) -> bool {
        #[cfg(all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        ))]
        {
            if !self.root_console_ready {
                return false;
            }
            let ready = try_attach_linked_display_runtime(self.root_console_ready);
            if ready {
                self.render_linked_hdmi_scrollback();
            }
            return ready;
        }
        #[cfg(not(all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        )))]
        {
            false
        }
    }

    /// Run one bounded backend keyboard probe pass without permanently arming
    /// background polling unless the caller had already enabled it or the
    /// keyboard comes online during the probe.
    pub fn probe_backend_keyboard_once(&mut self) -> LocalSeatKeyboardProbeResult {
        #[cfg(all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        ))]
        {
            if crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active()
                && !self.root_console_ready
                && !local_seat_pre_root_runtime_init_allowed(
                    true,
                    crate::hal::driver_task::driver_task_runtime_proof().pointer_free_ipc_proof,
                )
            {
                boot_log::force_uart_line(
                    "[local-seat] linked USB runtime keyboard probe deferred contract=usb-local-seat source=linked-runtime reason=driver-task-runtime-unproved action=root-prompt-first",
                );
                self.backend_keyboard_polling_enabled = false;
                self.backend_keyboard_poll_deferred_logged = false;
                return LocalSeatKeyboardProbeResult::DeferredUntilRootConsole;
            }
            if LINKED_LOCAL_SEAT_USB_KEYBOARD_READY.load(Ordering::Acquire) {
                self.backend_keyboard_polling_enabled = true;
                self.poll_backend_keyboard();
                return LocalSeatKeyboardProbeResult::Attached;
            }
            let mut result = if local_seat_usb_controller_runtime_attached() {
                LocalSeatKeyboardProbeResult::KeyboardUnavailable
            } else {
                LocalSeatKeyboardProbeResult::BackendUnavailable
            };
            let was_enabled = self.backend_keyboard_polling_enabled;
            let mut previous_progress = latest_usb_enumeration_progress_token();
            local_seat_driver_runtime_arm_prompt_safe_probe(self.root_console_ready);
            self.backend_keyboard_polling_enabled = true;
            self.poll_backend_keyboard();
            for _ in 0..LINKED_LOCAL_SEAT_USB_PROBE_STABLE_PROGRESS_BURST_ATTEMPTS {
                let current_progress = latest_usb_enumeration_progress_token();
                if !usb_enumeration_progress_token_allows_probe_burst(
                    previous_progress,
                    current_progress,
                ) {
                    break;
                }
                if !LINKED_LOCAL_SEAT_USB_ENUMERATION_PENDING.load(Ordering::Acquire)
                    || LINKED_LOCAL_SEAT_USB_KEYBOARD_READY.load(Ordering::Acquire)
                {
                    break;
                }
                previous_progress = current_progress;
                local_seat_driver_runtime_arm_prompt_safe_probe(self.root_console_ready);
                self.backend_keyboard_polling_enabled = true;
                self.poll_backend_keyboard();
            }
            let keep_polling = was_enabled || local_seat_driver_runtime_keyboard_attached();
            self.backend_keyboard_polling_enabled = keep_polling;
            if !keep_polling {
                self.backend_keyboard_poll_deferred_logged = false;
            }
            if local_seat_driver_runtime_keyboard_attached() {
                result = LocalSeatKeyboardProbeResult::Attached;
            } else if local_seat_usb_controller_runtime_attached() {
                result = LocalSeatKeyboardProbeResult::KeyboardUnavailable;
            }
            return result;
        }
        #[cfg(not(all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        )))]
        {
            LocalSeatKeyboardProbeResult::BackendUnavailable
        }
    }

    /// Echo keyboard bytes at the point they enter the canonical console parser.
    pub(crate) fn echo_input_bytes(&mut self, bytes: &[u8]) {
        self.echoed_keyboard_bytes = self
            .echoed_keyboard_bytes
            .saturating_add(bytes.len() as u64);
        for &byte in bytes {
            update_input_echo_preview(
                &mut self.input_echo_preview,
                byte,
                usize::from(self.status.line_bytes),
            );
        }

        #[cfg(all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        ))]
        self.mirror_input_bytes_to_display(bytes);
    }

    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    fn mirror_input_bytes_to_display(&mut self, bytes: &[u8]) {
        if bytes.is_empty() || !self.root_console_ready {
            return;
        }
        if crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active() {
            let mut plain =
                heapless::Vec::<u8, { crate::hal::driver_task::MAX_DRIVER_TASK_FRAME_BYTES }>::new(
                );
            for &byte in bytes {
                match self.hdmi_input_escape_state {
                    0 if byte == 0x1b => {
                        let _ = submit_linked_hdmi_payload_via_linked_hdmi(
                            plain.as_slice(),
                            self.root_console_ready,
                            "keyboard-input",
                        );
                        plain.clear();
                        self.hdmi_input_escape_state = 1;
                    }
                    0 => {
                        if plain.push(byte).is_err() {
                            let _ = submit_linked_hdmi_payload_via_linked_hdmi(
                                plain.as_slice(),
                                self.root_console_ready,
                                "keyboard-input",
                            );
                            plain.clear();
                            let _ = plain.push(byte);
                        }
                    }
                    1 if byte == b'[' => {
                        self.hdmi_input_escape_state = 2;
                    }
                    2 if byte == b'A' => {
                        self.hdmi_input_escape_state = 0;
                        self.scroll_linked_hdmi_back();
                    }
                    2 if byte == b'B' => {
                        self.hdmi_input_escape_state = 0;
                        self.scroll_linked_hdmi_forward();
                    }
                    _ => {
                        self.hdmi_input_escape_state = 0;
                    }
                }
            }
            let _ = submit_linked_hdmi_payload_via_linked_hdmi(
                plain.as_slice(),
                self.root_console_ready,
                "keyboard-input",
            );
        }
    }

    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    fn scroll_linked_hdmi_back(&mut self) {
        let visible = linked_hdmi_scrollback_visible_lines(self.status.buffer_lines);
        let max_offset = self.mirrored_lines.len().saturating_sub(visible);
        self.hdmi_scrollback_offset = self
            .hdmi_scrollback_offset
            .saturating_add(1)
            .min(max_offset);
        self.render_linked_hdmi_scrollback();
    }

    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    fn scroll_linked_hdmi_forward(&mut self) {
        self.hdmi_scrollback_offset = self.hdmi_scrollback_offset.saturating_sub(1);
        self.render_linked_hdmi_scrollback();
    }

    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    fn render_linked_hdmi_scrollback(&mut self) {
        let visible = linked_hdmi_scrollback_visible_lines(self.status.buffer_lines);
        let total = self.mirrored_lines.len();
        let max_offset = total.saturating_sub(visible);
        self.hdmi_scrollback_offset = self.hdmi_scrollback_offset.min(max_offset);
        let end = total.saturating_sub(self.hdmi_scrollback_offset);
        let start = end.saturating_sub(visible);
        let mut payload =
            heapless::Vec::<u8, { crate::hal::driver_task::MAX_DRIVER_TASK_FRAME_BYTES }>::new();
        let _ = payload.push(0x0c);
        let mut index = start;
        while index < end {
            if let Some(line) = self.mirrored_lines.get(index) {
                let needed = line.len().saturating_add(1);
                if payload.len().saturating_add(needed)
                    > crate::hal::driver_task::MAX_DRIVER_TASK_FRAME_BYTES
                {
                    let _ = submit_linked_hdmi_payload_via_linked_hdmi(
                        payload.as_slice(),
                        self.root_console_ready,
                        "keyboard-scrollback",
                    );
                    payload.clear();
                }
                for &byte in line.as_bytes() {
                    if payload.push(byte).is_err() {
                        let _ = submit_linked_hdmi_payload_via_linked_hdmi(
                            payload.as_slice(),
                            self.root_console_ready,
                            "keyboard-scrollback",
                        );
                        payload.clear();
                        let _ = payload.push(byte);
                    }
                }
                let _ = payload.push(b'\n');
            }
            index = index.saturating_add(1);
        }
        let _ = submit_linked_hdmi_payload_via_linked_hdmi(
            payload.as_slice(),
            self.root_console_ready,
            "keyboard-scrollback",
        );
    }

    /// Poll the platform local-seat input backend and enqueue discovered bytes.
    pub fn poll_backend_keyboard(&mut self) {
        if !self.backend_keyboard_polling_enabled {
            #[cfg(all(
                feature = "kernel",
                feature = "usb",
                target_arch = "aarch64",
                target_os = "none"
            ))]
            {
                if crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active()
                    && LINKED_LOCAL_SEAT_USB_KEYBOARD_READY.load(Ordering::Acquire)
                {
                    self.backend_keyboard_polling_enabled = true;
                    self.backend_keyboard_poll_deferred_logged = false;
                }
            }
        }
        if !self.backend_keyboard_polling_enabled {
            if !self.backend_keyboard_poll_deferred_logged {
                #[cfg(all(
                    feature = "kernel",
                    feature = "usb",
                    target_arch = "aarch64",
                    target_os = "none"
                ))]
                boot_log::force_uart_line(
                    "[local-seat] linked USB runtime keyboard poll deferred contract=usb-local-seat source=linked-runtime action=serial-shell-first",
                );
                self.backend_keyboard_poll_deferred_logged = true;
            }
            return;
        }

        #[cfg(feature = "kernel")]
        if crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active()
            && !self.root_console_ready
            && !local_seat_pre_root_runtime_init_allowed(
                true,
                crate::hal::driver_task::driver_task_runtime_proof().pointer_free_ipc_proof,
            )
        {
            if !self.backend_keyboard_poll_deferred_logged {
                #[cfg(all(
                    feature = "kernel",
                    feature = "usb",
                    target_arch = "aarch64",
                    target_os = "none"
                ))]
                boot_log::force_uart_line(
                    "[local-seat] linked USB runtime keyboard poll deferred contract=usb-local-seat source=linked-runtime reason=driver-task-runtime-unproved action=serial-shell-first",
                );
                self.backend_keyboard_poll_deferred_logged = true;
            }
            return;
        }

        let contract = driver_task_contract();
        #[cfg(feature = "kernel")]
        {
            #[cfg(all(feature = "usb", target_arch = "aarch64", target_os = "none"))]
            if crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active() {
                let controller_attached =
                    LINKED_LOCAL_SEAT_RUNTIME_ATTACHED.load(Ordering::Acquire);
                let keyboard_ready = LINKED_LOCAL_SEAT_USB_KEYBOARD_READY.load(Ordering::Acquire);
                let enumeration_pending =
                    LINKED_LOCAL_SEAT_USB_ENUMERATION_PENDING.load(Ordering::Acquire);
                if linked_local_seat_usb_pre_prompt_retry_deferred(
                    controller_attached,
                    keyboard_ready,
                    enumeration_pending,
                    self.root_console_ready,
                ) {
                    return;
                }
                if linked_local_seat_usb_attach_probe_required(
                    controller_attached,
                    keyboard_ready,
                    enumeration_pending,
                ) && !try_attach_linked_local_seat_runtime(self.root_console_ready)
                {
                    self.driver_task_budget_overruns =
                        self.driver_task_budget_overruns.saturating_add(1);
                    if local_seat_keyboard_poll_suspends_on_missing_reply(
                        true,
                        self.root_console_ready,
                        keyboard_ready,
                    ) {
                        self.backend_keyboard_polling_enabled = false;
                        if !self.backend_keyboard_poll_deferred_logged {
                            boot_log::force_uart_line(
                                "[local-seat] linked USB runtime keyboard poll suspended contract=usb-local-seat source=linked-runtime reason=driver-task-no-reply action=serial-shell",
                            );
                            self.backend_keyboard_poll_deferred_logged = true;
                        }
                    }
                    return;
                }
                if LINKED_LOCAL_SEAT_USB_ENUMERATION_PENDING.load(Ordering::Acquire)
                    && !LINKED_LOCAL_SEAT_USB_KEYBOARD_READY.load(Ordering::Acquire)
                {
                    return;
                }
                crate::hal::driver_task::register_driver_task_pointer_free_ring_service(
                    contract,
                    crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard.as_u32() as usize,
                    usb_keyboard_runtime_ring_service_driver_task,
                );
                self.backend_keyboard_poll_calls =
                    self.backend_keyboard_poll_calls.saturating_add(1);
                let mut command = crate::hal::driver_task::DriverTaskCommandRecord::pi4_hot_path(
                    0,
                    crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
                    crate::hal::driver_task::DriverTaskBudgetGrant::from_contract(contract),
                    crate::hal::driver_task::DriverFrameDescriptor {
                        offset: 0,
                        len: 0,
                        flags: 0,
                    },
                );
                command.aux0 = local_seat_keyboard_poll_aux(
                    LINKED_LOCAL_SEAT_USB_KEYBOARD_READY.load(Ordering::Acquire),
                );
                if let Some(completion) = run_local_seat_driver_task_ring_service(contract, command)
                {
                    if completion.code
                        == crate::hal::driver_task::DriverTaskCompletionCode::FrameReady.as_u16()
                    {
                        publish_local_seat_usb_keyboard_owner_ready(contract, completion);
                        if !LINKED_LOCAL_SEAT_USB_FIRST_REPORT_READY_LOGGED
                            .swap(true, Ordering::AcqRel)
                        {
                            crate::hal::driver_task::emit_driver_task_resource_init_status(
                                contract,
                                crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
                                "usb-keyboard-first-report",
                                "ready",
                                Some(completion),
                            );
                        }
                        if let Some(bytes) = crate::hal::driver_task::driver_task_ring_frame_bytes(
                            contract,
                            completion.frame,
                        ) {
                            self.backend_keyboard_read_bytes = self
                                .backend_keyboard_read_bytes
                                .saturating_add(bytes.len() as u64);
                            LINKED_LOCAL_SEAT_USB_KEYBOARD_READY.store(true, Ordering::Release);
                            let _ = self.enqueue_keyboard_bytes(bytes);
                        }
                        return;
                    }
                    if completion.code
                        == crate::hal::driver_task::DriverTaskCompletionCode::Progress.as_u16()
                        && completion.result != 0
                    {
                        if completion.detail == DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_READY
                        {
                            record_linked_local_seat_usb_detail(Some(completion));
                            LINKED_LOCAL_SEAT_USB_KEYBOARD_READY.store(true, Ordering::Release);
                            LINKED_LOCAL_SEAT_USB_ENUMERATION_PENDING
                                .store(false, Ordering::Release);
                            if !LINKED_LOCAL_SEAT_USB_FIRST_REPORT_READY_LOGGED
                                .swap(true, Ordering::AcqRel)
                            {
                                crate::hal::driver_task::emit_driver_task_resource_init_status(
                                    contract,
                                    crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
                                    "usb-keyboard-first-report",
                                    "ready",
                                    Some(completion),
                                );
                            }
                            let usb_owner =
                                crate::hal::driver_task::register_driver_task_runtime_owner_state(
                                    crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
                                );
                            if usb_owner
                                && !LINKED_LOCAL_SEAT_USB_OWNER_READY_LOGGED
                                    .swap(true, Ordering::AcqRel)
                            {
                                crate::hal::driver_task::emit_driver_task_resource_init_status(
                                    contract,
                                    crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
                                    "usb-owner-state",
                                    "ready",
                                    Some(completion),
                                );
                            }
                            return;
                        }
                        if local_seat_usb_keyboard_enumeration_progress(completion) {
                            publish_local_seat_usb_enumeration_progress(contract, completion);
                            return;
                        }
                        record_linked_local_seat_usb_detail(Some(completion));
                        if !LINKED_LOCAL_SEAT_USB_FIRST_REPORT_PENDING_LOGGED
                            .swap(true, Ordering::AcqRel)
                        {
                            let status = if completion.detail
                                == DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_PENDING
                            {
                                "pending"
                            } else {
                                "progress"
                            };
                            crate::hal::driver_task::emit_driver_task_resource_init_status(
                                contract,
                                crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
                                "usb-keyboard-first-report",
                                status,
                                Some(completion),
                            );
                        }
                        return;
                    }
                    if completion.code
                        == crate::hal::driver_task::DriverTaskCompletionCode::Idle.as_u16()
                    {
                        if !LINKED_LOCAL_SEAT_USB_FIRST_REPORT_PENDING_LOGGED
                            .swap(true, Ordering::AcqRel)
                        {
                            let status =
                                if LINKED_LOCAL_SEAT_USB_KEYBOARD_READY.load(Ordering::Acquire) {
                                    "pending"
                                } else {
                                    "blocked-keyboard-enumeration"
                                };
                            crate::hal::driver_task::emit_driver_task_resource_init_status(
                                contract,
                                crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
                                "usb-keyboard-first-report",
                                status,
                                Some(completion),
                            );
                        }
                        return;
                    }
                }
                self.driver_task_budget_overruns =
                    self.driver_task_budget_overruns.saturating_add(1);
                if local_seat_keyboard_poll_suspends_on_missing_reply(
                    true,
                    self.root_console_ready,
                    LINKED_LOCAL_SEAT_USB_KEYBOARD_READY.load(Ordering::Acquire),
                ) {
                    self.backend_keyboard_polling_enabled = false;
                    if !self.backend_keyboard_poll_deferred_logged {
                        boot_log::force_uart_line(
                            "[local-seat] linked USB runtime keyboard poll suspended contract=usb-local-seat source=linked-runtime reason=driver-task-no-reply action=serial-shell",
                        );
                        self.backend_keyboard_poll_deferred_logged = true;
                    }
                }
                return;
            }
            if !crate::hal::driver_task::steady_state_root_compatibility_service_allowed(contract) {
                self.driver_task_budget_overruns =
                    self.driver_task_budget_overruns.saturating_add(1);
                return;
            }
            crate::hal::driver_task::register_driver_task_root_context_ring_service(
                contract,
                self as *mut Self as usize,
                usb_keyboard_ring_service_driver_task,
            );
            let command = crate::hal::driver_task::DriverTaskCommandRecord::pi4_hot_path(
                0,
                crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
                crate::hal::driver_task::DriverTaskBudgetGrant::from_contract(contract),
                crate::hal::driver_task::DriverFrameDescriptor {
                    offset: 0,
                    len: 0,
                    flags:
                        crate::hal::driver_task::DRIVER_TASK_RING_FLAG_ROOT_CONTEXT_NON_ACCEPTANCE,
                },
            );
            if run_local_seat_driver_task_ring_service(contract, command).is_some() {
                return;
            }
            // SAFETY: The HAL admits this compatibility callback only for
            // QEMU/host profiles. Physical Pi 4 builds return None without
            // compiling callback slot state.
            if unsafe {
                crate::hal::driver_task::try_driver_task_compat_service(
                    contract,
                    self as *mut Self as usize,
                    usb_keyboard_poll_driver_task,
                )
            }
            .is_some()
            {
                return;
            }
            if !crate::hal::driver_task::admit_root_task_compatibility_service(contract) {
                self.driver_task_budget_overruns =
                    self.driver_task_budget_overruns.saturating_add(1);
                return;
            }
        }
        self.poll_backend_keyboard_current_tcb(contract);
    }

    fn poll_backend_keyboard_current_tcb(&mut self, contract: DriverTaskContract) {
        self.backend_keyboard_poll_calls = self.backend_keyboard_poll_calls.saturating_add(1);
        let mut budget = match DriverServiceBudget::new(contract) {
            Ok(budget) => budget,
            Err(_) => {
                self.driver_task_budget_overruns =
                    self.driver_task_budget_overruns.saturating_add(1);
                return;
            }
        };
        if budget.charge_ops(1).is_err() {
            self.driver_task_budget_overruns = self.driver_task_budget_overruns.saturating_add(1);
            return;
        }
        {
            #[cfg(all(
                feature = "kernel",
                feature = "usb",
                target_arch = "aarch64",
                target_os = "none"
            ))]
            {
                if !LOCAL_SEAT_POLL_LOGGED.swap(true, Ordering::AcqRel) {
                    boot_log::force_uart_line(
                    "[local-seat] linked USB runtime keyboard poll routed contract=usb-local-seat source=linked-runtime path=driver-task-ring",
                );
                }
            }
        }
    }

    /// Return the current local input echo preview for diagnostics and tests.
    #[must_use]
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn input_echo_preview(&self) -> &str {
        self.input_echo_preview.as_str()
    }

    /// Returns whether a physical backend is attached to this runtime.
    #[must_use]
    pub fn backend_attached(&self) -> bool {
        #[cfg(all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        ))]
        {
            return local_seat_usb_driver_runtime_attached();
        }
        #[cfg(not(all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        )))]
        {
            false
        }
    }

    /// Publish the attached HDMI sink for boot-progress banners once runtime
    /// storage is stable.
    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    pub fn register_boot_progress_backend(&mut self) {}

    /// Host-test no-op for boot-progress backend publication.
    #[cfg(not(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    )))]
    pub fn register_boot_progress_backend(&mut self) {}

    /// Preseed platform keyboard MMIO windows after core boot mappings settle.
    pub fn preseed_backend_keyboard_mmio(&mut self) {}
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn local_seat_driver_runtime_attached() -> bool {
    local_seat_usb_driver_runtime_attached()
        || LINKED_LOCAL_SEAT_DISPLAY_ATTACHED.load(Ordering::Acquire)
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn local_seat_usb_driver_runtime_attached() -> bool {
    local_seat_driver_runtime_keyboard_attached()
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn local_seat_usb_controller_runtime_attached() -> bool {
    LINKED_LOCAL_SEAT_RUNTIME_ATTACHED.load(Ordering::Acquire)
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn adopt_linked_display_runtime_owner_state(reason: &'static str) -> bool {
    if LINKED_LOCAL_SEAT_DISPLAY_ATTACHED.load(Ordering::Acquire) {
        return true;
    }
    if !crate::hal::driver_task::driver_task_runtime_owner_state_registered(
        crate::hal::driver_task::DriverTaskHotPath::HdmiText,
    ) {
        return false;
    }
    LINKED_LOCAL_SEAT_DISPLAY_ATTACHED.store(true, Ordering::Release);
    LINKED_LOCAL_SEAT_DISPLAY_FAILED.store(false, Ordering::Release);
    if !LINKED_LOCAL_SEAT_DISPLAY_ADOPTED_LOGGED.swap(true, Ordering::AcqRel) {
        let mut line = heapless::String::<160>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] linked HDMI runtime adopted source=boot-owner-state reason={reason}"
            ),
        );
        boot_log::force_uart_line(line.as_str());
    }
    true
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn credit_linked_display_runtime_frame_owner_state(
    reason: &'static str,
    root_console_ready: bool,
    completion: crate::hal::driver_task::DriverTaskCompletionRecord,
) -> bool {
    let owner_registered = crate::hal::driver_task::register_driver_task_runtime_owner_state(
        crate::hal::driver_task::DriverTaskHotPath::HdmiText,
    );
    if owner_registered {
        LINKED_LOCAL_SEAT_DISPLAY_ATTACHED.store(true, Ordering::Release);
        LINKED_LOCAL_SEAT_DISPLAY_FAILED.store(false, Ordering::Release);
        if !LINKED_LOCAL_SEAT_DISPLAY_FIRST_FRAME_OWNER_LOGGED.swap(true, Ordering::AcqRel) {
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                crate::hal::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT,
                crate::hal::driver_task::DriverTaskHotPath::HdmiText,
                "hdmi-owner-state",
                "first-frame-ready",
                Some(completion),
            );
            emit_hdmi_text_final_state(
                true,
                "first-frame-owner",
                reason,
                root_console_ready,
                LINKED_LOCAL_SEAT_DISPLAY_INIT_ATTEMPTS.load(Ordering::Acquire),
                Some(completion),
            );
        }
        true
    } else {
        crate::hal::driver_task::emit_driver_task_resource_init_status(
            crate::hal::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT,
            crate::hal::driver_task::DriverTaskHotPath::HdmiText,
            "hdmi-owner-state",
            "first-frame-descriptor-rejected",
            Some(completion),
        );
        false
    }
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn local_seat_usb_engine_init_ready(
    completion: Option<crate::hal::driver_task::DriverTaskCompletionRecord>,
) -> bool {
    completion.is_some_and(local_seat_usb_engine_progress)
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn local_seat_usb_keyboard_init_ready(
    completion: Option<crate::hal::driver_task::DriverTaskCompletionRecord>,
) -> bool {
    completion.is_some_and(|completion| {
        local_seat_usb_completion_progress(completion)
            && completion.detail == DRIVER_RUNTIME_USB_INIT_DETAIL_KEYBOARD_READY
    })
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn local_seat_usb_engine_init_status(
    completion: Option<crate::hal::driver_task::DriverTaskCompletionRecord>,
) -> &'static str {
    let ready = completion.is_some_and(local_seat_usb_engine_progress);
    local_seat_completion_status(completion, ready)
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn local_seat_usb_completion_progress(
    completion: crate::hal::driver_task::DriverTaskCompletionRecord,
) -> bool {
    completion.code == crate::hal::driver_task::DriverTaskCompletionCode::Progress.as_u16()
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn local_seat_usb_engine_progress(
    completion: crate::hal::driver_task::DriverTaskCompletionRecord,
) -> bool {
    local_seat_usb_completion_progress(completion)
        && matches!(
            completion.detail,
            DRIVER_RUNTIME_USB_INIT_DETAIL_XHCI_READY
                | DRIVER_RUNTIME_USB_INIT_DETAIL_COMMAND_RING_PENDING
                | DRIVER_RUNTIME_USB_INIT_DETAIL_COMMAND_RING_READY
                | DRIVER_RUNTIME_USB_INIT_DETAIL_ROOT_PORT_CONNECTED
                | DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_ADDRESSED
                | DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_DESCRIPTOR
                | DRIVER_RUNTIME_USB_INIT_DETAIL_CONFIG_DESCRIPTOR
                | DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_TOPOLOGY_SEEN
                | DRIVER_RUNTIME_USB_INIT_DETAIL_HID_ENDPOINT_SEEN
                | DRIVER_RUNTIME_USB_INIT_DETAIL_ENABLE_SLOT_FAILED
                | DRIVER_RUNTIME_USB_INIT_DETAIL_ADDRESS_DEVICE_FAILED
                | DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_DESCRIPTOR_FAILED
                | DRIVER_RUNTIME_USB_INIT_DETAIL_CONFIG_DESCRIPTOR_FAILED
                | DRIVER_RUNTIME_USB_INIT_DETAIL_HID_ATTACH_FAILED
                | DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_ATTACH_FAILED
                | DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_SET_CONFIG_FAILED
                | DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_DESCRIPTOR_FAILED
                | DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_CONTEXT_FAILED
                | DRIVER_RUNTIME_USB_INIT_DETAIL_KEYBOARD_READY
        )
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn local_seat_usb_keyboard_enum_status(
    completion: Option<crate::hal::driver_task::DriverTaskCompletionRecord>,
) -> &'static str {
    match completion {
        Some(completion)
            if local_seat_usb_completion_progress(completion)
                && completion.detail == DRIVER_RUNTIME_USB_INIT_DETAIL_KEYBOARD_READY =>
        {
            "ready"
        }
        Some(completion)
            if local_seat_usb_completion_progress(completion)
                && completion.detail == DRIVER_RUNTIME_USB_INIT_DETAIL_ENABLE_SLOT_FAILED =>
        {
            "enable-slot-failed"
        }
        Some(completion)
            if local_seat_usb_completion_progress(completion)
                && completion.detail == DRIVER_RUNTIME_USB_INIT_DETAIL_ADDRESS_DEVICE_FAILED =>
        {
            "address-device-failed"
        }
        Some(completion)
            if local_seat_usb_completion_progress(completion)
                && completion.detail == DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_DESCRIPTOR_FAILED =>
        {
            "device-descriptor-failed"
        }
        Some(completion)
            if local_seat_usb_completion_progress(completion)
                && completion.detail == DRIVER_RUNTIME_USB_INIT_DETAIL_CONFIG_DESCRIPTOR_FAILED =>
        {
            "config-descriptor-failed"
        }
        Some(completion)
            if local_seat_usb_completion_progress(completion)
                && completion.detail == DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_ATTACH_FAILED =>
        {
            "hub-attach-failed"
        }
        Some(completion)
            if local_seat_usb_completion_progress(completion)
                && completion.detail == DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_SET_CONFIG_FAILED =>
        {
            "hub-set-configuration-failed"
        }
        Some(completion)
            if local_seat_usb_completion_progress(completion)
                && completion.detail == DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_DESCRIPTOR_FAILED =>
        {
            "hub-descriptor-failed"
        }
        Some(completion)
            if local_seat_usb_completion_progress(completion)
                && completion.detail == DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_CONTEXT_FAILED =>
        {
            "hub-context-failed"
        }
        Some(completion)
            if local_seat_usb_completion_progress(completion)
                && completion.detail == DRIVER_RUNTIME_USB_INIT_DETAIL_HID_ATTACH_FAILED =>
        {
            "hid-attach-failed"
        }
        Some(completion)
            if local_seat_usb_completion_progress(completion)
                && completion.detail == DRIVER_RUNTIME_USB_INIT_DETAIL_HID_ENDPOINT_SEEN =>
        {
            "hid-endpoint-not-ready"
        }
        Some(completion)
            if local_seat_usb_completion_progress(completion)
                && completion.detail == DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_TOPOLOGY_SEEN =>
        {
            "hub-topology-no-keyboard"
        }
        Some(completion)
            if local_seat_usb_completion_progress(completion)
                && completion.detail == DRIVER_RUNTIME_USB_INIT_DETAIL_XHCI_READY =>
        {
            "not-enumerated"
        }
        Some(completion)
            if local_seat_usb_completion_progress(completion)
                && completion.detail == DRIVER_RUNTIME_USB_INIT_DETAIL_COMMAND_RING_PENDING =>
        {
            "command-ring-pending"
        }
        Some(completion)
            if local_seat_usb_completion_progress(completion)
                && completion.detail == DRIVER_RUNTIME_USB_INIT_DETAIL_COMMAND_RING_READY =>
        {
            "command-ring-ready"
        }
        Some(completion)
            if local_seat_usb_completion_progress(completion)
                && completion.detail == DRIVER_RUNTIME_USB_INIT_DETAIL_ROOT_PORT_CONNECTED =>
        {
            "root-port-connected"
        }
        Some(completion)
            if local_seat_usb_completion_progress(completion)
                && completion.detail == DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_ADDRESSED =>
        {
            "device-addressed"
        }
        Some(completion)
            if local_seat_usb_completion_progress(completion)
                && completion.detail == DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_DESCRIPTOR =>
        {
            "device-descriptor"
        }
        Some(completion)
            if local_seat_usb_completion_progress(completion)
                && completion.detail == DRIVER_RUNTIME_USB_INIT_DETAIL_CONFIG_DESCRIPTOR =>
        {
            "config-descriptor"
        }
        Some(completion)
            if completion.code
                == crate::hal::driver_task::DriverTaskCompletionCode::Fault.as_u16() =>
        {
            "fault"
        }
        Some(_) => "unexpected-completion",
        None => "no-reply",
    }
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn local_seat_usb_keyboard_enumeration_progress(
    completion: crate::hal::driver_task::DriverTaskCompletionRecord,
) -> bool {
    local_seat_usb_completion_progress(completion)
        && matches!(
            completion.detail,
            DRIVER_RUNTIME_USB_INIT_DETAIL_XHCI_READY
                | DRIVER_RUNTIME_USB_INIT_DETAIL_COMMAND_RING_PENDING
                | DRIVER_RUNTIME_USB_INIT_DETAIL_COMMAND_RING_READY
                | DRIVER_RUNTIME_USB_INIT_DETAIL_ROOT_PORT_CONNECTED
                | DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_ADDRESSED
                | DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_DESCRIPTOR
                | DRIVER_RUNTIME_USB_INIT_DETAIL_CONFIG_DESCRIPTOR
                | DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_TOPOLOGY_SEEN
                | DRIVER_RUNTIME_USB_INIT_DETAIL_HID_ENDPOINT_SEEN
                | DRIVER_RUNTIME_USB_INIT_DETAIL_ENABLE_SLOT_FAILED
                | DRIVER_RUNTIME_USB_INIT_DETAIL_ADDRESS_DEVICE_FAILED
                | DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_DESCRIPTOR_FAILED
                | DRIVER_RUNTIME_USB_INIT_DETAIL_CONFIG_DESCRIPTOR_FAILED
                | DRIVER_RUNTIME_USB_INIT_DETAIL_HID_ATTACH_FAILED
                | DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_ATTACH_FAILED
                | DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_SET_CONFIG_FAILED
                | DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_DESCRIPTOR_FAILED
                | DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_CONTEXT_FAILED
                | DRIVER_RUNTIME_USB_INIT_DETAIL_KEYBOARD_READY
        )
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
const fn linked_local_seat_usb_detail_rank(detail: u16) -> u8 {
    match detail {
        DRIVER_RUNTIME_USB_INIT_DETAIL_XHCI_READY => 3,
        DRIVER_RUNTIME_USB_INIT_DETAIL_COMMAND_RING_PENDING => 4,
        DRIVER_RUNTIME_USB_INIT_DETAIL_COMMAND_RING_READY => 4,
        DRIVER_RUNTIME_USB_INIT_DETAIL_ROOT_PORT_CONNECTED
        | DRIVER_RUNTIME_USB_INIT_DETAIL_ENABLE_SLOT_FAILED => 5,
        DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_ADDRESSED
        | DRIVER_RUNTIME_USB_INIT_DETAIL_ADDRESS_DEVICE_FAILED => 6,
        DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_DESCRIPTOR
        | DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_DESCRIPTOR_FAILED
        | DRIVER_RUNTIME_USB_INIT_DETAIL_CONFIG_DESCRIPTOR
        | DRIVER_RUNTIME_USB_INIT_DETAIL_CONFIG_DESCRIPTOR_FAILED
        | DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_TOPOLOGY_SEEN
        | DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_ATTACH_FAILED
        | DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_SET_CONFIG_FAILED
        | DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_DESCRIPTOR_FAILED
        | DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_CONTEXT_FAILED
        | DRIVER_RUNTIME_USB_INIT_DETAIL_HID_ENDPOINT_SEEN
        | DRIVER_RUNTIME_USB_INIT_DETAIL_HID_ATTACH_FAILED => 7,
        DRIVER_RUNTIME_USB_INIT_DETAIL_KEYBOARD_READY => 8,
        DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_PENDING
        | DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_READY => 9,
        _ => 0,
    }
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
const fn linked_local_seat_usb_detail_warrants_recovery(detail: u16) -> bool {
    matches!(
        detail,
        DRIVER_RUNTIME_USB_INIT_DETAIL_ENABLE_SLOT_FAILED
            | DRIVER_RUNTIME_USB_INIT_DETAIL_ADDRESS_DEVICE_FAILED
            | DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_DESCRIPTOR_FAILED
            | DRIVER_RUNTIME_USB_INIT_DETAIL_CONFIG_DESCRIPTOR_FAILED
            | DRIVER_RUNTIME_USB_INIT_DETAIL_HID_ATTACH_FAILED
            | DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_ATTACH_FAILED
            | DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_SET_CONFIG_FAILED
            | DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_DESCRIPTOR_FAILED
            | DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_CONTEXT_FAILED
    )
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn record_linked_local_seat_usb_detail(
    completion: Option<crate::hal::driver_task::DriverTaskCompletionRecord>,
) {
    if let Some(completion) = completion {
        if completion.detail != 0 {
            let old = LINKED_LOCAL_SEAT_USB_LAST_DETAIL.load(Ordering::Acquire) as u16;
            let old_rank = linked_local_seat_usb_detail_rank(old);
            let new_rank = linked_local_seat_usb_detail_rank(completion.detail);
            if new_rank >= old_rank
                || (old_rank
                    < linked_local_seat_usb_detail_rank(
                        DRIVER_RUNTIME_USB_INIT_DETAIL_KEYBOARD_READY,
                    )
                    && linked_local_seat_usb_detail_warrants_recovery(completion.detail))
            {
                LINKED_LOCAL_SEAT_USB_LAST_DETAIL
                    .store(completion.detail as usize, Ordering::Release);
                LINKED_LOCAL_SEAT_USB_LAST_RESULT
                    .store(completion.result as usize, Ordering::Release);
            }
        }
    }
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn publish_local_seat_usb_keyboard_ready(
    contract: crate::hal::driver_task::DriverTaskContract,
    completion: crate::hal::driver_task::DriverTaskCompletionRecord,
) {
    LINKED_LOCAL_SEAT_USB_KEYBOARD_READY.store(true, Ordering::Release);
    LINKED_LOCAL_SEAT_USB_LAST_DETAIL.store(completion.detail as usize, Ordering::Release);
    LINKED_LOCAL_SEAT_USB_LAST_RESULT.store(completion.result as usize, Ordering::Release);
    if !LINKED_LOCAL_SEAT_USB_ENUM_READY_LOGGED.swap(true, Ordering::AcqRel) {
        crate::hal::driver_task::emit_driver_task_resource_init_status(
            contract,
            crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
            "usb-keyboard-enumeration",
            "ready",
            Some(completion),
        );
    }
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn publish_local_seat_usb_keyboard_owner_ready(
    contract: crate::hal::driver_task::DriverTaskContract,
    completion: crate::hal::driver_task::DriverTaskCompletionRecord,
) {
    publish_local_seat_usb_keyboard_ready(contract, completion);
    LINKED_LOCAL_SEAT_USB_ENUMERATION_PENDING.store(false, Ordering::Release);
    let usb_owner = crate::hal::driver_task::register_driver_task_runtime_owner_state(
        crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
    );
    if usb_owner {
        if !LINKED_LOCAL_SEAT_USB_OWNER_READY_LOGGED.swap(true, Ordering::AcqRel) {
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                contract,
                crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
                "usb-owner-state",
                "ready",
                Some(completion),
            );
        }
    } else {
        crate::hal::driver_task::emit_driver_task_resource_init_status(
            contract,
            crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
            "usb-owner-state",
            "descriptor-rejected",
            Some(completion),
        );
    }
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn emit_linked_local_seat_usb_enumeration_snapshot(
    contract: crate::hal::driver_task::DriverTaskContract,
    completion: crate::hal::driver_task::DriverTaskCompletionRecord,
) {
    use core::fmt::Write;

    let result = completion.result;
    let root_or_events = result & USB_ENUM_RESULT_ROOT_PORT_MASK;
    let slot = (result >> USB_ENUM_RESULT_SLOT_SHIFT) & 0xff;
    let endpoint = (result >> USB_ENUM_RESULT_ENDPOINT_SHIFT) & 0x1f;
    let scan_pass = (result >> USB_ENUM_RESULT_SCAN_PASS_SHIFT) & USB_ENUM_RESULT_SCAN_PASS_MASK;
    let command_proof = matches!(
        completion.detail,
        DRIVER_RUNTIME_USB_INIT_DETAIL_COMMAND_RING_PENDING
            | DRIVER_RUNTIME_USB_INIT_DETAIL_COMMAND_RING_READY
    );
    let root_port_mask = if command_proof { 0 } else { root_or_events };
    let command_events_seen = if command_proof { root_or_events } else { 0 };
    let progress = crate::hal::driver_task::latest_driver_task_ring_progress(contract);
    let active_request = crate::hal::driver_task::current_driver_task_ring_request(contract);
    let request_match = match (active_request, progress) {
        (Some(request), Some(progress))
            if progress.marker_valid && progress.sequence == request as u32 =>
        {
            "yes"
        }
        (Some(_), Some(_)) => "no",
        (Some(_), None) => "no-progress",
        (None, Some(_)) => "no-active-request",
        (None, None) => "none",
    };
    let mut line = heapless::String::<768>::new();
    let _ = write!(
        line,
        "USB_RUNTIME_ENUM_SNAPSHOT contract={} detail=0x{:04x} result=0x{:08x} root_port_mask=0x{:02x} slot={} ep_id={} scan_pass={} root_port_power={} cmd_path={} port_event={} hid_ep={} preserved_event={} transfer_event={} endpoint_ready={} cmd_proof={} cmd_events_seen={} cmd_slot_or_polls={} cmd_event_type={} cmd_ack_failures={} marker_valid={} marker_sequence={} marker_phase={} marker_phase_name={} marker_aux0=0x{:08x} active_request_valid={} active_request={} request_match={} marker_aux_match={}",
        contract.name,
        completion.detail,
        result,
        root_port_mask,
        slot,
        endpoint,
        scan_pass,
        local_seat_yes_no(result & USB_ENUM_RESULT_ROOT_POWERED != 0),
        local_seat_yes_no(result & USB_ENUM_RESULT_COMMAND_PATH != 0),
        local_seat_yes_no(result & USB_ENUM_RESULT_PORT_EVENT != 0),
        local_seat_yes_no(result & USB_ENUM_RESULT_HID_ENDPOINT != 0),
        local_seat_yes_no(result & USB_ENUM_RESULT_PRESERVED_EVENT != 0),
        local_seat_yes_no(result & USB_ENUM_RESULT_TRANSFER_EVENT != 0),
        local_seat_yes_no(result & USB_ENUM_RESULT_ENDPOINT_READY != 0),
        local_seat_yes_no(command_proof),
        command_events_seen,
        if command_proof { slot } else { 0 },
        if command_proof { endpoint } else { 0 },
        if command_proof { scan_pass } else { 0 },
        progress.map_or("no", |progress| local_seat_yes_no(progress.marker_valid)),
        progress.map_or(0, |progress| progress.sequence),
        progress.map_or(0, |progress| progress.phase),
        progress.map_or("none", |progress| progress.phase_name),
        progress.map_or(0, |progress| progress.aux0),
        active_request.map_or("no", |_| "yes"),
        active_request.unwrap_or(0),
        request_match,
        progress.map_or("none", |progress| {
            local_seat_yes_no(progress.aux0 == DRIVER_RUNTIME_USB_ENUMERATE_AUX)
        }),
    );
    boot_log::force_uart_line(line.as_str());
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
const fn local_seat_yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

#[cfg(all(feature = "kernel", feature = "usb"))]
fn latest_usb_enumeration_progress_token() -> Option<(u32, u32, u32)> {
    crate::hal::driver_task::latest_driver_task_ring_progress(driver_task_contract()).and_then(
        |progress| {
            (progress.marker_valid && progress.aux0 == DRIVER_RUNTIME_USB_ENUMERATE_AUX)
                .then_some((progress.sequence, progress.phase, progress.aux0))
        },
    )
}

#[cfg(all(feature = "kernel", feature = "usb"))]
fn usb_enumeration_progress_token_advanced(
    previous: Option<(u32, u32, u32)>,
    current: Option<(u32, u32, u32)>,
) -> bool {
    matches!(current, Some(current) if Some(current) != previous)
}

#[cfg(all(feature = "kernel", feature = "usb"))]
fn usb_enumeration_progress_token_allows_probe_burst(
    previous: Option<(u32, u32, u32)>,
    current: Option<(u32, u32, u32)>,
) -> bool {
    current.is_some() || usb_enumeration_progress_token_advanced(previous, current)
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn publish_local_seat_usb_enumeration_progress(
    contract: crate::hal::driver_task::DriverTaskContract,
    completion: crate::hal::driver_task::DriverTaskCompletionRecord,
) {
    let previous_detail = LINKED_LOCAL_SEAT_USB_LAST_DETAIL.load(Ordering::Acquire);
    let previous_result = LINKED_LOCAL_SEAT_USB_LAST_RESULT.load(Ordering::Acquire);
    record_linked_local_seat_usb_detail(Some(completion));
    if previous_detail != completion.detail as usize
        || previous_result != completion.result as usize
    {
        emit_linked_local_seat_usb_enumeration_snapshot(contract, completion);
    }
    if completion.detail == DRIVER_RUNTIME_USB_INIT_DETAIL_KEYBOARD_READY {
        publish_local_seat_usb_keyboard_ready(contract, completion);
        return;
    }
    if !LINKED_LOCAL_SEAT_USB_ENUM_PROGRESS_LOGGED.swap(true, Ordering::AcqRel) {
        crate::hal::driver_task::emit_driver_task_resource_init_status(
            contract,
            crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
            "usb-keyboard-enumeration-retry",
            local_seat_usb_keyboard_enum_status(Some(completion)),
            Some(completion),
        );
    }
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn emit_hdmi_text_final_state(
    ready: bool,
    stage: &'static str,
    reason: &'static str,
    root_console_ready: bool,
    attempt: usize,
    completion: Option<crate::hal::driver_task::DriverTaskCompletionRecord>,
) {
    use core::fmt::Write;

    let mut line = heapless::String::<224>::new();
    let event = if ready {
        "HDMI_TEXT_READY"
    } else {
        "HDMI_TEXT_BLOCKED"
    };
    if let Some(completion) = completion {
        let _ = write!(
            line,
            "{event} stage={stage} reason={reason} root_console_ready={} attempt={} code={} detail={} result={} frame_len={}",
            if root_console_ready { "yes" } else { "no" },
            attempt,
            completion.code,
            completion.detail,
            completion.result,
            completion.frame.len,
        );
    } else {
        let _ = write!(
            line,
            "{event} stage={stage} reason={reason} root_console_ready={} attempt={} code=none detail=none result=none frame_len=0",
            if root_console_ready { "yes" } else { "no" },
            attempt,
        );
    }
    boot_log::force_uart_line(line.as_str());
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn emit_hdmi_frame_submit_state(
    reason: &'static str,
    bytes_len: usize,
    root_console_ready: bool,
    completion: Option<crate::hal::driver_task::DriverTaskCompletionRecord>,
    ready: bool,
    fatal: bool,
) {
    use core::fmt::Write;

    let status = local_seat_completion_status(completion, ready);
    let mut line = heapless::String::<256>::new();
    if let Some(completion) = completion {
        let _ = write!(
            line,
            "HDMI_FRAME_SUBMIT reason={reason} status={status} root_console_ready={} attached={} failed={} fatal={} bytes={} code={} detail={} result={} frame_len={}",
            if root_console_ready { "yes" } else { "no" },
            if LINKED_LOCAL_SEAT_DISPLAY_ATTACHED.load(Ordering::Acquire) {
                "yes"
            } else {
                "no"
            },
            if LINKED_LOCAL_SEAT_DISPLAY_FAILED.load(Ordering::Acquire) {
                "yes"
            } else {
                "no"
            },
            if fatal { "yes" } else { "no" },
            bytes_len,
            completion.code,
            completion.detail,
            completion.result,
            completion.frame.len,
        );
    } else {
        let _ = write!(
            line,
            "HDMI_FRAME_SUBMIT reason={reason} status={status} root_console_ready={} attached={} failed={} fatal={} bytes={} code=none detail=none result=none frame_len=0",
            if root_console_ready { "yes" } else { "no" },
            if LINKED_LOCAL_SEAT_DISPLAY_ATTACHED.load(Ordering::Acquire) {
                "yes"
            } else {
                "no"
            },
            if LINKED_LOCAL_SEAT_DISPLAY_FAILED.load(Ordering::Acquire) {
                "yes"
            } else {
                "no"
            },
            if fatal { "yes" } else { "no" },
            bytes_len,
        );
    }
    boot_log::force_uart_line(line.as_str());

    if let Some(progress) = crate::hal::driver_task::latest_driver_task_ring_progress(
        crate::hal::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT,
    ) {
        let mut progress_line = heapless::String::<224>::new();
        let _ = write!(
            progress_line,
            "HDMI_FRAME_PROGRESS reason={reason} marker_valid={} sequence={} phase={} phase_name={} aux0=0x{:08x}",
            if progress.marker_valid { "yes" } else { "no" },
            progress.sequence,
            progress.phase,
            progress.phase_name,
            progress.aux0,
        );
        boot_log::force_uart_line(progress_line.as_str());
    }
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn local_seat_completion_status(
    completion: Option<crate::hal::driver_task::DriverTaskCompletionRecord>,
    ready: bool,
) -> &'static str {
    if ready {
        "ready"
    } else {
        match completion {
            Some(completion)
                if completion.code
                    == crate::hal::driver_task::DriverTaskCompletionCode::Fault.as_u16() =>
            {
                "fault"
            }
            Some(_) => "unexpected-completion",
            None => "no-reply",
        }
    }
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn try_attach_linked_display_runtime(root_console_ready: bool) -> bool {
    if LINKED_LOCAL_SEAT_DISPLAY_ATTACHED.load(Ordering::Acquire) {
        return true;
    }
    if adopt_linked_display_runtime_owner_state("attach-fast-path") {
        return true;
    }
    if LINKED_LOCAL_SEAT_DISPLAY_FAILED.load(Ordering::Acquire) {
        return false;
    }
    crate::hal::driver_task::register_driver_task_pointer_free_ring_service(
        crate::hal::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT,
        crate::hal::driver_task::DriverTaskHotPath::HdmiText.as_u32() as usize,
        display_runtime_ring_service_driver_task,
    );
    if !local_seat_prompt_steady_service_allowed(
        crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active(),
        root_console_ready,
        crate::hal::driver_task::driver_task_runtime_proof().pointer_free_ipc_proof,
    ) {
        return false;
    }
    if LINKED_LOCAL_SEAT_DISPLAY_INIT_ATTEMPTED.swap(true, Ordering::AcqRel) {
        let attempts = LINKED_LOCAL_SEAT_DISPLAY_INIT_ATTEMPTS.load(Ordering::Acquire);
        if !local_seat_display_attach_retry_allowed(root_console_ready, false, attempts) {
            return false;
        }
    }
    let attempt = LINKED_LOCAL_SEAT_DISPLAY_INIT_ATTEMPTS
        .fetch_add(1, Ordering::AcqRel)
        .saturating_add(1);
    if !local_seat_display_attach_retry_allowed(
        root_console_ready,
        LINKED_LOCAL_SEAT_DISPLAY_ATTACHED.load(Ordering::Acquire),
        attempt.saturating_sub(1),
    ) {
        return false;
    }
    crate::hal::driver_task::emit_driver_task_resource_init_status(
        crate::hal::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT,
        crate::hal::driver_task::DriverTaskHotPath::HdmiText,
        "hdmi-engine-attach-begin",
        if root_console_ready {
            "root-console-ready"
        } else {
            "serial-shell-first"
        },
        None,
    );
    let hdmi_command = crate::hal::driver_task::runtime_engine_init_command(
        crate::hal::driver_task::DriverTaskHotPath::HdmiText,
        crate::hal::driver_task::DriverTaskBudgetGrant::from_contract(
            crate::hal::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT,
        ),
    );
    let hdmi_completion = run_local_seat_driver_task_ring_service_with_prompt_state(
        crate::hal::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT,
        hdmi_command,
        root_console_ready,
    );
    let hdmi_ok = hdmi_completion.is_some_and(|completion| {
        completion.code == crate::hal::driver_task::DriverTaskCompletionCode::Progress.as_u16()
            && completion.result == 1
    });
    let hdmi_status = local_seat_completion_status(hdmi_completion, hdmi_ok);
    crate::hal::driver_task::emit_driver_task_resource_init_status(
        crate::hal::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT,
        crate::hal::driver_task::DriverTaskHotPath::HdmiText,
        "hdmi-engine-init",
        hdmi_status,
        hdmi_completion,
    );
    let hdmi_owner = hdmi_ok
        && crate::hal::driver_task::register_driver_task_runtime_owner_state(
            crate::hal::driver_task::DriverTaskHotPath::HdmiText,
        );
    if hdmi_ok && !hdmi_owner {
        crate::hal::driver_task::emit_driver_task_resource_init_status(
            crate::hal::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT,
            crate::hal::driver_task::DriverTaskHotPath::HdmiText,
            "hdmi-owner-state",
            "descriptor-rejected",
            None,
        );
        emit_hdmi_text_final_state(
            false,
            "engine-init",
            "owner-state-descriptor-rejected",
            root_console_ready,
            attempt,
            hdmi_completion,
        );
    }
    if hdmi_owner {
        LINKED_LOCAL_SEAT_DISPLAY_ATTACHED.store(true, Ordering::Release);
        LINKED_LOCAL_SEAT_DISPLAY_FAILED.store(false, Ordering::Release);
        emit_hdmi_text_final_state(
            true,
            "engine-init",
            "driver-task-owner-state",
            root_console_ready,
            attempt,
            hdmi_completion,
        );
        boot_log::force_uart_line(
            "[local-seat] linked HDMI runtime active action=serial-safe-mirror",
        );
        return true;
    }
    if !hdmi_ok {
        emit_hdmi_text_final_state(
            false,
            "engine-init",
            hdmi_status,
            root_console_ready,
            attempt,
            hdmi_completion,
        );
    }
    if !LINKED_LOCAL_SEAT_DISPLAY_INIT_DEFERRED_LOGGED.swap(true, Ordering::AcqRel) {
        boot_log::force_uart_line(
            "[local-seat] linked HDMI runtime pending reason=driver-task-init-no-reply action=serial-shell",
        );
    }
    LINKED_LOCAL_SEAT_DISPLAY_INIT_ATTEMPTED.store(false, Ordering::Release);
    false
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn try_attach_linked_local_seat_runtime(root_console_ready: bool) -> bool {
    if !local_seat_prompt_steady_service_allowed(
        crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active(),
        root_console_ready,
        crate::hal::driver_task::driver_task_runtime_proof().pointer_free_ipc_proof,
    ) {
        return false;
    }
    let _ = try_attach_linked_display_runtime(root_console_ready);
    let usb_contract = driver_task_contract();
    let hdmi_contract = crate::hal::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT;
    crate::hal::driver_task::register_driver_task_pointer_free_ring_service(
        usb_contract,
        crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard.as_u32() as usize,
        usb_keyboard_runtime_ring_service_driver_task,
    );
    crate::hal::driver_task::register_driver_task_pointer_free_ring_service(
        hdmi_contract,
        crate::hal::driver_task::DriverTaskHotPath::HdmiText.as_u32() as usize,
        display_runtime_ring_service_driver_task,
    );
    let mut usb_enumeration_no_reply = false;
    if !LINKED_LOCAL_SEAT_RUNTIME_ATTACHED.load(Ordering::Acquire) {
        let _ = crate::hal::driver_task::register_pi4_bus_ring_service(
            crate::hal::driver_task::PCIE_ROOT_DRIVER_TASK_CONTRACT,
        );
        if !LINKED_LOCAL_SEAT_PCIE_REPLAY_BEGIN_LOGGED.swap(true, Ordering::AcqRel) {
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                crate::hal::driver_task::PCIE_ROOT_DRIVER_TASK_CONTRACT,
                crate::hal::driver_task::DriverTaskHotPath::PcieRoot,
                "usb-prereq-pcie-replay",
                "begin",
                None,
            );
        }
        if !crate::hal::driver_task::ensure_deferred_runtime_init_descriptor(
            crate::hal::driver_task::PCIE_ROOT_DRIVER_TASK_CONTRACT,
            crate::hal::driver_task::DriverTaskHotPath::PcieRoot,
        ) {
            if !LINKED_LOCAL_SEAT_PCIE_REPLAY_DEFERRED_LOGGED.swap(true, Ordering::AcqRel) {
                crate::hal::driver_task::emit_driver_task_resource_init_status(
                    crate::hal::driver_task::PCIE_ROOT_DRIVER_TASK_CONTRACT,
                    crate::hal::driver_task::DriverTaskHotPath::PcieRoot,
                    "usb-prereq-pcie-replay",
                    "blocked",
                    None,
                );
                crate::hal::driver_task::emit_driver_task_resource_init_status(
                    usb_contract,
                    crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
                    "usb-xhci-init",
                    "blocked-pcie-runtime",
                    None,
                );
            }
            return false;
        }
        if !LINKED_LOCAL_SEAT_PCIE_REPLAY_READY_LOGGED.swap(true, Ordering::AcqRel) {
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                crate::hal::driver_task::PCIE_ROOT_DRIVER_TASK_CONTRACT,
                crate::hal::driver_task::DriverTaskHotPath::PcieRoot,
                "usb-prereq-pcie-replay",
                "ready",
                None,
            );
        }
        let pcie_contract = crate::hal::driver_task::PCIE_ROOT_DRIVER_TASK_CONTRACT;
        if !linked_local_seat_pcie_hal_prep_ready()
            && !linked_local_seat_prepare_pcie_hal_from_lease()
        {
            if !LINKED_LOCAL_SEAT_PCIE_ENGINE_DEFERRED_LOGGED.swap(true, Ordering::AcqRel) {
                crate::hal::driver_task::emit_driver_task_resource_init_status(
                    pcie_contract,
                    crate::hal::driver_task::DriverTaskHotPath::PcieRoot,
                    "usb-prereq-pcie-engine-init",
                    "blocked-hal-prep-required",
                    None,
                );
                crate::hal::driver_task::emit_driver_task_resource_init_status(
                    usb_contract,
                    crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
                    "usb-xhci-init",
                    "blocked-pcie-hal-prep",
                    None,
                );
            }
            return false;
        }
        if !LINKED_LOCAL_SEAT_PCIE_ENGINE_BEGIN_LOGGED.swap(true, Ordering::AcqRel) {
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                pcie_contract,
                crate::hal::driver_task::DriverTaskHotPath::PcieRoot,
                "usb-prereq-pcie-engine-init",
                "adopt-hal-prepared-descriptor",
                None,
            );
        }
        crate::hal::driver_task::emit_driver_task_resource_init_status(
            pcie_contract,
            crate::hal::driver_task::DriverTaskHotPath::PcieRoot,
            "usb-prereq-pcie-engine-init",
            "ready-adopted",
            None,
        );
        let pcie_owner = crate::hal::driver_task::register_driver_task_runtime_owner_state(
            crate::hal::driver_task::DriverTaskHotPath::PcieRoot,
        );
        if pcie_owner {
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                pcie_contract,
                crate::hal::driver_task::DriverTaskHotPath::PcieRoot,
                "pcie-owner-state",
                "ready",
                None,
            );
        } else {
            if !LINKED_LOCAL_SEAT_PCIE_ENGINE_DEFERRED_LOGGED.swap(true, Ordering::AcqRel) {
                crate::hal::driver_task::emit_driver_task_resource_init_status(
                    pcie_contract,
                    crate::hal::driver_task::DriverTaskHotPath::PcieRoot,
                    "pcie-owner-state",
                    "descriptor-rejected",
                    None,
                );
                crate::hal::driver_task::emit_driver_task_resource_init_status(
                    usb_contract,
                    crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
                    "usb-xhci-init",
                    "blocked-pcie-owner-state",
                    None,
                );
            }
            return false;
        }
        if !LINKED_LOCAL_SEAT_USB_REPLAY_BEGIN_LOGGED.swap(true, Ordering::AcqRel) {
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                usb_contract,
                crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
                "usb-runtime-descriptor-replay",
                "begin",
                None,
            );
        }
        if !crate::hal::driver_task::ensure_deferred_runtime_init_descriptor(
            usb_contract,
            crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
        ) {
            if !LINKED_LOCAL_SEAT_USB_REPLAY_DEFERRED_LOGGED.swap(true, Ordering::AcqRel) {
                crate::hal::driver_task::emit_driver_task_resource_init_status(
                    usb_contract,
                    crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
                    "usb-runtime-descriptor-replay",
                    "blocked",
                    None,
                );
                crate::hal::driver_task::emit_driver_task_resource_init_status(
                    usb_contract,
                    crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
                    "usb-xhci-init",
                    "blocked-usb-runtime",
                    None,
                );
            }
            return false;
        }
        if !LINKED_LOCAL_SEAT_USB_REPLAY_READY_LOGGED.swap(true, Ordering::AcqRel) {
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                usb_contract,
                crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
                "usb-runtime-descriptor-replay",
                "ready",
                None,
            );
        }
        let mut usb_command = crate::hal::driver_task::DriverTaskCommandRecord::pi4_hot_path(
            0,
            crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
            crate::hal::driver_task::DriverTaskBudgetGrant::from_contract(usb_contract),
            crate::hal::driver_task::DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        );
        usb_command.aux0 = DRIVER_RUNTIME_LOCAL_SEAT_INIT_AUX;
        if !LINKED_LOCAL_SEAT_USB_ENGINE_BEGIN_LOGGED.swap(true, Ordering::AcqRel) {
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                usb_contract,
                crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
                "usb-engine-init",
                "begin",
                None,
            );
        }
        let mut usb_completion = run_local_seat_driver_task_ring_service_with_prompt_state(
            usb_contract,
            usb_command,
            root_console_ready,
        );
        record_linked_local_seat_usb_detail(usb_completion);
        let mut usb_controller_ready = local_seat_usb_engine_init_ready(usb_completion);
        let mut usb_keyboard_ready = local_seat_usb_keyboard_init_ready(usb_completion);
        crate::hal::driver_task::emit_driver_task_resource_init_status(
            usb_contract,
            crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
            "usb-engine-init",
            local_seat_usb_engine_init_status(usb_completion),
            usb_completion,
        );
        if usb_controller_ready && !usb_keyboard_ready {
            usb_command.aux0 = DRIVER_RUNTIME_USB_ENUMERATE_AUX;
            for _ in 0..LINKED_LOCAL_SEAT_USB_ENUM_RESUME_ATTEMPTS {
                crate::hal::driver_task::emit_driver_task_resource_init_status(
                    usb_contract,
                    crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
                    "usb-keyboard-enumeration-resume",
                    "begin",
                    None,
                );
                let resume_completion = run_local_seat_driver_task_ring_service_with_prompt_state(
                    usb_contract,
                    usb_command,
                    root_console_ready,
                );
                let resume_replied = resume_completion.is_some();
                if let Some(completion) = resume_completion {
                    if local_seat_usb_keyboard_init_ready(Some(completion)) {
                        publish_local_seat_usb_keyboard_ready(usb_contract, completion);
                    } else if local_seat_usb_engine_init_ready(Some(completion)) {
                        publish_local_seat_usb_enumeration_progress(usb_contract, completion);
                    } else {
                        record_linked_local_seat_usb_detail(Some(completion));
                    }
                }
                crate::hal::driver_task::emit_driver_task_resource_init_status(
                    usb_contract,
                    crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
                    "usb-keyboard-enumeration-resume",
                    local_seat_usb_keyboard_enum_status(resume_completion),
                    resume_completion,
                );
                if !resume_replied {
                    usb_enumeration_no_reply = true;
                    break;
                }
                if local_seat_usb_engine_init_ready(resume_completion) {
                    usb_completion = resume_completion;
                    usb_controller_ready = true;
                    usb_keyboard_ready = local_seat_usb_keyboard_init_ready(resume_completion);
                }
                if usb_keyboard_ready {
                    break;
                }
            }
        }
        if usb_controller_ready
            || !LINKED_LOCAL_SEAT_USB_INIT_DEFERRED_LOGGED.swap(true, Ordering::AcqRel)
        {
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                usb_contract,
                crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
                "usb-xhci-init",
                local_seat_usb_engine_init_status(usb_completion),
                usb_completion,
            );
        }
        if usb_controller_ready || !LINKED_LOCAL_SEAT_USB_ENUM_LOGGED.swap(true, Ordering::AcqRel) {
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                usb_contract,
                crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
                "usb-keyboard-enumeration",
                local_seat_usb_keyboard_enum_status(usb_completion),
                usb_completion,
            );
        }
        if usb_controller_ready {
            LINKED_LOCAL_SEAT_RUNTIME_ATTACHED.store(true, Ordering::Release);
        }
        if usb_keyboard_ready {
            LINKED_LOCAL_SEAT_USB_KEYBOARD_READY.store(true, Ordering::Release);
        } else if usb_controller_ready {
            LINKED_LOCAL_SEAT_USB_ENUMERATION_PENDING.store(true, Ordering::Release);
        }
        let first_report_ready =
            LINKED_LOCAL_SEAT_USB_FIRST_REPORT_READY_LOGGED.load(Ordering::Acquire);
        let usb_owner = first_report_ready
            && crate::hal::driver_task::register_driver_task_runtime_owner_state(
                crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
            );
        if usb_controller_ready && !usb_owner {
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                usb_contract,
                crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
                "usb-owner-state",
                if first_report_ready {
                    "descriptor-rejected"
                } else if usb_keyboard_ready {
                    "blocked-first-report"
                } else {
                    "blocked-keyboard-enumeration"
                },
                usb_completion,
            );
        }
        if usb_controller_ready {
            if usb_keyboard_ready {
                boot_log::force_uart_line(
                    "[local-seat] linked USB runtime active keyboard=ready action=serial-safe-keyboard",
                );
            } else {
                boot_log::force_uart_line(
                    "[local-seat] linked USB runtime active keyboard=not-enumerated action=keep-diagnostics-open",
                );
            }
        } else {
            return false;
        }
    }
    if usb_enumeration_no_reply && !root_console_ready {
        return LINKED_LOCAL_SEAT_USB_KEYBOARD_READY.load(Ordering::Acquire);
    }
    if LINKED_LOCAL_SEAT_RUNTIME_ATTACHED.load(Ordering::Acquire)
        && !LINKED_LOCAL_SEAT_USB_KEYBOARD_READY.load(Ordering::Acquire)
        && LINKED_LOCAL_SEAT_USB_ENUMERATION_PENDING.load(Ordering::Acquire)
    {
        let mut usb_command = crate::hal::driver_task::DriverTaskCommandRecord::pi4_hot_path(
            0,
            crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
            crate::hal::driver_task::DriverTaskBudgetGrant::from_contract(usb_contract),
            crate::hal::driver_task::DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        );
        usb_command.aux0 = DRIVER_RUNTIME_USB_ENUMERATE_AUX;
        let verbose_retry = !LINKED_LOCAL_SEAT_USB_ENUM_PROGRESS_LOGGED.load(Ordering::Acquire);
        if verbose_retry {
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                usb_contract,
                crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
                "usb-keyboard-enumeration-retry",
                "begin",
                None,
            );
        }
        let resume_completion = run_local_seat_driver_task_ring_service_with_prompt_state(
            usb_contract,
            usb_command,
            root_console_ready,
        );
        record_linked_local_seat_usb_detail(resume_completion);
        if let Some(completion) = resume_completion {
            if local_seat_usb_keyboard_init_ready(Some(completion)) {
                LINKED_LOCAL_SEAT_USB_ENUMERATION_PENDING.store(false, Ordering::Release);
                publish_local_seat_usb_keyboard_ready(usb_contract, completion);
            } else if local_seat_usb_engine_init_ready(Some(completion)) {
                LINKED_LOCAL_SEAT_USB_ENUMERATION_PENDING.store(true, Ordering::Release);
                publish_local_seat_usb_enumeration_progress(usb_contract, completion);
            } else if verbose_retry {
                crate::hal::driver_task::emit_driver_task_resource_init_status(
                    usb_contract,
                    crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
                    "usb-keyboard-enumeration-retry",
                    local_seat_usb_keyboard_enum_status(Some(completion)),
                    Some(completion),
                );
            }
        } else if verbose_retry {
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                usb_contract,
                crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
                "usb-keyboard-enumeration-retry",
                "no-reply",
                None,
            );
        }
    }
    LINKED_LOCAL_SEAT_USB_KEYBOARD_READY.load(Ordering::Acquire)
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn local_seat_driver_runtime_keyboard_attached() -> bool {
    LINKED_LOCAL_SEAT_USB_KEYBOARD_READY.load(Ordering::Acquire)
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
pub(crate) fn linked_local_seat_usb_keyboard_ready() -> bool {
    LINKED_LOCAL_SEAT_USB_KEYBOARD_READY.load(Ordering::Acquire)
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
pub(crate) fn linked_local_seat_usb_controller_ready() -> bool {
    LINKED_LOCAL_SEAT_RUNTIME_ATTACHED.load(Ordering::Acquire)
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
pub(crate) fn linked_local_seat_usb_first_report_ready() -> bool {
    LINKED_LOCAL_SEAT_USB_FIRST_REPORT_READY_LOGGED.load(Ordering::Acquire)
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
pub(crate) fn linked_local_seat_usb_runtime_detail() -> u16 {
    LINKED_LOCAL_SEAT_USB_LAST_DETAIL.load(Ordering::Acquire) as u16
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
pub(crate) fn linked_local_seat_usb_runtime_result() -> u32 {
    LINKED_LOCAL_SEAT_USB_LAST_RESULT.load(Ordering::Acquire) as u32
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
pub(crate) fn linked_local_seat_usb_frontier_label() -> &'static str {
    if LINKED_LOCAL_SEAT_USB_KEYBOARD_READY.load(Ordering::Acquire) {
        return "usb-keyboard-ready";
    }
    if LINKED_LOCAL_SEAT_USB_ENUMERATION_PENDING.load(Ordering::Acquire) {
        return "usb-keyboard-enumeration-pending";
    }
    if LINKED_LOCAL_SEAT_RUNTIME_ATTACHED.load(Ordering::Acquire) {
        return "usb-xhci-ready";
    }
    if LINKED_LOCAL_SEAT_USB_ENGINE_BEGIN_LOGGED.load(Ordering::Acquire) {
        return "usb-engine-init-no-reply";
    }
    if LINKED_LOCAL_SEAT_USB_REPLAY_READY_LOGGED.load(Ordering::Acquire) {
        return "usb-runtime-descriptor-replay-ready";
    }
    if LINKED_LOCAL_SEAT_USB_REPLAY_BEGIN_LOGGED.load(Ordering::Acquire) {
        return "usb-runtime-descriptor-replay-begin";
    }
    "usb-runtime-not-started"
}

#[cfg(not(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
)))]
pub(crate) fn linked_local_seat_usb_frontier_label() -> &'static str {
    "usb-runtime-unavailable"
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
pub(crate) fn mirror_driver_start_progress_line(line: &str) -> bool {
    mirror_high_impact_line_via_linked_hdmi(line, false, "driver-resource-progress")
}

#[cfg(not(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
)))]
pub(crate) fn mirror_driver_start_progress_line(_line: &str) -> bool {
    false
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn mirror_high_impact_line_via_linked_hdmi(
    line: &str,
    root_console_ready: bool,
    reason: &'static str,
) -> bool {
    if !crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active() {
        return false;
    }
    let contract = crate::hal::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT;
    let _ = adopt_linked_display_runtime_owner_state(reason);
    if !LINKED_LOCAL_SEAT_DISPLAY_ATTACHED.load(Ordering::Acquire)
        && !LINKED_LOCAL_SEAT_DISPLAY_FAILED.load(Ordering::Acquire)
    {
        let _ = try_attach_linked_display_runtime(root_console_ready);
    }
    if !local_seat_linked_display_service_allowed(
        true,
        LINKED_LOCAL_SEAT_DISPLAY_ATTACHED.load(Ordering::Acquire),
        LINKED_LOCAL_SEAT_DISPLAY_FAILED.load(Ordering::Acquire),
    ) {
        if !LINKED_LOCAL_SEAT_DISPLAY_DEFERRED_LOGGED.swap(true, Ordering::AcqRel) {
            boot_log::force_uart_line(
                "[local-seat] high-impact HDMI route deferred reason=linked-hdmi-not-ready action=serial-log-only",
            );
        }
        return false;
    }
    crate::hal::driver_task::register_driver_task_pointer_free_ring_service(
        contract,
        crate::hal::driver_task::DriverTaskHotPath::HdmiText.as_u32() as usize,
        display_runtime_ring_service_driver_task,
    );
    if crate::hal::driver_task::driver_task_ring_command_active(contract) {
        return false;
    }
    let mut payload =
        heapless::Vec::<u8, { crate::hal::driver_task::MAX_DRIVER_TASK_FRAME_BYTES }>::new();
    let max_line_bytes = crate::hal::driver_task::MAX_DRIVER_TASK_FRAME_BYTES.saturating_sub(1);
    for &byte in line.as_bytes().iter().take(max_line_bytes) {
        let _ = payload.push(byte);
    }
    let _ = payload.push(b'\n');
    let Some(frame) =
        crate::hal::driver_task::describe_driver_task_ring_frame(payload.as_slice(), 0)
    else {
        return false;
    };
    let command = crate::hal::driver_task::DriverTaskCommandRecord::pi4_hot_path(
        0,
        crate::hal::driver_task::DriverTaskHotPath::HdmiText,
        crate::hal::driver_task::DriverTaskBudgetGrant::from_contract(contract),
        frame,
    );
    let staging_segments =
        [crate::hal::driver_task::DriverTaskStagingSegment::ring_frame(payload.as_slice(), 0)];
    let completion = run_local_seat_driver_task_ring_service_with_prompt_state_and_staging(
        contract,
        command,
        false,
        &staging_segments,
    );
    let ready = completion.is_some_and(|completion| {
        completion.code == crate::hal::driver_task::DriverTaskCompletionCode::Progress.as_u16()
            && completion.result != 0
    });
    let status = local_seat_completion_status(completion, ready);
    emit_hdmi_frame_submit_state(
        reason,
        payload.len(),
        root_console_ready,
        completion,
        ready,
        false,
    );
    if ready {
        if let Some(completion) = completion {
            let _ = credit_linked_display_runtime_frame_owner_state(
                reason,
                root_console_ready,
                completion,
            );
        }
        if !LINKED_LOCAL_SEAT_DISPLAY_FIRST_DRAW_READY_LOGGED.swap(true, Ordering::AcqRel) {
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                contract,
                crate::hal::driver_task::DriverTaskHotPath::HdmiText,
                "hdmi-high-impact-progress",
                "ready",
                completion,
            );
            emit_hdmi_text_final_state(
                true,
                "high-impact-progress",
                "linked-hdmi-text",
                root_console_ready,
                LINKED_LOCAL_SEAT_DISPLAY_INIT_ATTEMPTS.load(Ordering::Acquire),
                completion,
            );
        }
        return true;
    }
    if !LINKED_LOCAL_SEAT_DISPLAY_FIRST_DRAW_FAILED_LOGGED.swap(true, Ordering::AcqRel) {
        crate::hal::driver_task::emit_driver_task_resource_init_status(
            contract,
            crate::hal::driver_task::DriverTaskHotPath::HdmiText,
            "hdmi-high-impact-progress",
            status,
            completion,
        );
        emit_hdmi_text_final_state(
            false,
            "high-impact-progress",
            status,
            root_console_ready,
            LINKED_LOCAL_SEAT_DISPLAY_INIT_ATTEMPTS.load(Ordering::Acquire),
            completion,
        );
    }
    false
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn linked_hdmi_scrollback_visible_lines(buffer_lines: u16) -> usize {
    usize::from(buffer_lines).min(32).max(1)
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn submit_linked_hdmi_payload_via_linked_hdmi(
    bytes: &[u8],
    root_console_ready: bool,
    reason: &'static str,
) -> bool {
    if bytes.is_empty()
        || !crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active()
    {
        return false;
    }
    let contract = crate::hal::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT;
    let _ = adopt_linked_display_runtime_owner_state(reason);
    if !LINKED_LOCAL_SEAT_DISPLAY_ATTACHED.load(Ordering::Acquire)
        && !LINKED_LOCAL_SEAT_DISPLAY_FAILED.load(Ordering::Acquire)
    {
        let _ = try_attach_linked_display_runtime(root_console_ready);
    }
    if !local_seat_linked_display_service_allowed(
        true,
        LINKED_LOCAL_SEAT_DISPLAY_ATTACHED.load(Ordering::Acquire),
        LINKED_LOCAL_SEAT_DISPLAY_FAILED.load(Ordering::Acquire),
    ) {
        return false;
    }
    crate::hal::driver_task::register_driver_task_pointer_free_ring_service(
        contract,
        crate::hal::driver_task::DriverTaskHotPath::HdmiText.as_u32() as usize,
        display_runtime_ring_service_driver_task,
    );
    if crate::hal::driver_task::driver_task_ring_command_active(contract) {
        return false;
    }
    let chunk_limit = LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES
        .min(crate::hal::driver_task::MAX_DRIVER_TASK_FRAME_BYTES)
        .max(1);
    let mut offset = 0usize;
    let mut any_ready = false;
    while offset < bytes.len() {
        let end = offset.saturating_add(chunk_limit).min(bytes.len());
        let chunk = &bytes[offset..end];
        let Some(frame) = crate::hal::driver_task::describe_driver_task_ring_frame(chunk, 0) else {
            return any_ready;
        };
        let command = crate::hal::driver_task::DriverTaskCommandRecord::pi4_hot_path(
            0,
            crate::hal::driver_task::DriverTaskHotPath::HdmiText,
            crate::hal::driver_task::DriverTaskBudgetGrant::from_contract(contract),
            frame,
        );
        let staging_segments =
            [crate::hal::driver_task::DriverTaskStagingSegment::ring_frame(chunk, 0)];
        let completion = run_local_seat_driver_task_ring_service_with_prompt_state_and_staging(
            contract,
            command,
            false,
            &staging_segments,
        );
        let ready = completion.is_some_and(|completion| {
            completion.code == crate::hal::driver_task::DriverTaskCompletionCode::Progress.as_u16()
                && completion.result != 0
        });
        emit_hdmi_frame_submit_state(
            reason,
            end.saturating_sub(offset),
            root_console_ready,
            completion,
            ready,
            false,
        );
        if !ready {
            if completion.is_none()
                && local_seat_display_mirror_suspends_on_missing_reply(true, root_console_ready)
            {
                if !LINKED_LOCAL_SEAT_DISPLAY_NO_REPLY_LOGGED.swap(true, Ordering::AcqRel) {
                    boot_log::force_uart_line(
                        "[local-seat] runtime display mirror deferred reason=driver-task-no-reply action=retry-next-frame",
                    );
                }
            }
            return any_ready;
        }
        if let Some(completion) = completion {
            let _ = credit_linked_display_runtime_frame_owner_state(
                reason,
                root_console_ready,
                completion,
            );
        }
        any_ready = true;
        offset = end;
    }
    any_ready
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn local_seat_driver_runtime_arm_prompt_safe_probe(root_console_ready: bool) {
    let _ = try_attach_linked_local_seat_runtime(root_console_ready);
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn linked_local_seat_pcie_hal_prep_ready() -> bool {
    crate::hal::pi4_pcie::pi4_pcie_link_and_rc_ready_proven()
        && crate::hal::pi4_pcie::pi4_pcie_irq_sources_masked_proven()
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn linked_local_seat_prepare_pcie_hal_from_lease() -> bool {
    if linked_local_seat_pcie_hal_prep_ready() {
        return true;
    }
    let Some(lease) = LOCAL_SEAT_RUNTIME_INIT_LEASE.lock().as_ref().copied() else {
        if !LINKED_LOCAL_SEAT_PCIE_HAL_PREP_BLOCKED_LOGGED.swap(true, Ordering::AcqRel) {
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                crate::hal::driver_task::PCIE_ROOT_DRIVER_TASK_CONTRACT,
                crate::hal::driver_task::DriverTaskHotPath::PcieRoot,
                "usb-prereq-pcie-hal-prep",
                "blocked-lease-missing",
                None,
            );
        }
        return false;
    };
    let attempt = LINKED_LOCAL_SEAT_PCIE_HAL_PREP_ATTEMPTS.fetch_add(1, Ordering::AcqRel);
    if attempt >= LINKED_LOCAL_SEAT_PCIE_HAL_PREP_MAX_ATTEMPTS {
        return linked_local_seat_pcie_hal_prep_ready();
    }
    if !LINKED_LOCAL_SEAT_PCIE_HAL_PREP_BEGIN_LOGGED.swap(true, Ordering::AcqRel) {
        crate::hal::driver_task::emit_driver_task_resource_init_status(
            crate::hal::driver_task::PCIE_ROOT_DRIVER_TASK_CONTRACT,
            crate::hal::driver_task::DriverTaskHotPath::PcieRoot,
            "usb-prereq-pcie-hal-prep",
            "begin",
            None,
        );
    } else {
        crate::hal::driver_task::emit_driver_task_resource_init_status(
            crate::hal::driver_task::PCIE_ROOT_DRIVER_TASK_CONTRACT,
            crate::hal::driver_task::DriverTaskHotPath::PcieRoot,
            "usb-prereq-pcie-hal-prep",
            "retry",
            None,
        );
    }
    // SAFETY: `init_local_seat_driver_runtime_on_service` stores the long-lived
    // root HAL pointer only so HAL can prepare the PCIe owner-link prerequisite
    // before the linked USB runtime is admitted. This helper does not construct
    // or poll a root-owned USB backend.
    let hal = unsafe { &mut *(lease.hal_ptr as *mut crate::hal::KernelHal<'static>) };
    let prepared = hal
        .prove_pi4_vl805_pcie_ownership()
        .or_else(|_| hal.prove_pi4_vl805_pcie_ownership_after_mailbox_reset())
        .is_ok_and(|proof| proof.interrupt_modes_quiesced() && proof.pcie_device_control_ready());
    if prepared || linked_local_seat_pcie_hal_prep_ready() {
        if !LINKED_LOCAL_SEAT_PCIE_HAL_PREP_READY_LOGGED.swap(true, Ordering::AcqRel) {
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                crate::hal::driver_task::PCIE_ROOT_DRIVER_TASK_CONTRACT,
                crate::hal::driver_task::DriverTaskHotPath::PcieRoot,
                "usb-prereq-pcie-hal-prep",
                "ready",
                None,
            );
        }
        true
    } else {
        if !LINKED_LOCAL_SEAT_PCIE_HAL_PREP_BLOCKED_LOGGED.swap(true, Ordering::AcqRel) {
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                crate::hal::driver_task::PCIE_ROOT_DRIVER_TASK_CONTRACT,
                crate::hal::driver_task::DriverTaskHotPath::PcieRoot,
                "usb-prereq-pcie-hal-prep",
                "blocked-live-proof-missing",
                None,
            );
        }
        false
    }
}

#[cfg(feature = "kernel")]
fn run_local_seat_driver_task_ring_service(
    contract: crate::hal::driver_task::DriverTaskContract,
    command: crate::hal::driver_task::DriverTaskCommandRecord,
) -> Option<crate::hal::driver_task::DriverTaskCompletionRecord> {
    run_local_seat_driver_task_ring_service_with_prompt_state(contract, command, true)
}

#[cfg(feature = "kernel")]
fn run_local_seat_driver_task_ring_service_staged(
    contract: crate::hal::driver_task::DriverTaskContract,
    command: crate::hal::driver_task::DriverTaskCommandRecord,
    staging_segments: &[crate::hal::driver_task::DriverTaskStagingSegment<'_>],
) -> Option<crate::hal::driver_task::DriverTaskCompletionRecord> {
    run_local_seat_driver_task_ring_service_with_prompt_state_and_staging(
        contract,
        command,
        true,
        staging_segments,
    )
}

#[cfg(feature = "kernel")]
fn run_local_seat_driver_task_ring_service_with_prompt_state(
    contract: crate::hal::driver_task::DriverTaskContract,
    command: crate::hal::driver_task::DriverTaskCommandRecord,
    root_console_ready: bool,
) -> Option<crate::hal::driver_task::DriverTaskCompletionRecord> {
    run_local_seat_driver_task_ring_service_with_prompt_state_and_staging(
        contract,
        command,
        root_console_ready,
        &[],
    )
}

#[cfg(feature = "kernel")]
fn run_local_seat_driver_task_ring_service_with_prompt_state_and_staging(
    contract: crate::hal::driver_task::DriverTaskContract,
    command: crate::hal::driver_task::DriverTaskCommandRecord,
    root_console_ready: bool,
    staging_segments: &[crate::hal::driver_task::DriverTaskStagingSegment<'_>],
) -> Option<crate::hal::driver_task::DriverTaskCompletionRecord> {
    if crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active() {
        if local_seat_driver_task_prompt_slice_required(command.aux0, root_console_ready) {
            crate::hal::driver_task::run_driver_task_ring_service_prompt_slice_staged(
                contract,
                command,
                staging_segments,
            )
        } else {
            crate::hal::driver_task::run_driver_task_ring_service_nonblocking_staged(
                contract,
                command,
                staging_segments,
            )
        }
    } else {
        crate::hal::driver_task::run_driver_task_ring_service_staged(
            contract,
            command,
            staging_segments,
        )
    }
}

#[cfg(feature = "kernel")]
const fn local_seat_driver_task_prompt_slice_required(aux0: u32, root_console_ready: bool) -> bool {
    if !root_console_ready {
        return false;
    }
    #[cfg(feature = "usb")]
    {
        matches!(
            aux0,
            DRIVER_RUNTIME_LOCAL_SEAT_INIT_AUX | DRIVER_RUNTIME_USB_ENUMERATE_AUX
        )
    }
    #[cfg(not(feature = "usb"))]
    {
        aux0 == pi4_driver_abi::DRIVER_RUNTIME_LOCAL_SEAT_INIT_AUX
    }
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn init_local_seat_driver_runtime_on_service(
    hal: &mut crate::hal::KernelHal<'_>,
    hints: LocalSeatPlatformHints,
) -> Result<(), LocalSeatBackendError> {
    crate::hal::driver_task::register_driver_task_pointer_free_ring_service(
        driver_task_contract(),
        crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard.as_u32() as usize,
        usb_keyboard_runtime_ring_service_driver_task,
    );
    crate::hal::driver_task::register_driver_task_pointer_free_ring_service(
        crate::hal::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT,
        crate::hal::driver_task::DriverTaskHotPath::HdmiText.as_u32() as usize,
        display_runtime_ring_service_driver_task,
    );
    if crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active() {
        *LOCAL_SEAT_RUNTIME_INIT_LEASE.lock() = Some(LocalSeatRuntimeInitLease {
            hal_ptr: hal as *mut crate::hal::KernelHal<'_> as usize,
            _hints: hints,
        });
        boot_log::force_uart_line(
            "[local-seat] linked HDMI runtime init deferred reason=serial-shell-first action=steady-call-after-root",
        );
        boot_log::force_uart_line(
            "[local-seat] linked USB runtime init deferred reason=serial-shell-first action=steady-call-after-root",
        );
        return Ok(());
    }
    *LOCAL_SEAT_RUNTIME_INIT_LEASE.lock() = Some(LocalSeatRuntimeInitLease {
        hal_ptr: hal as *mut crate::hal::KernelHal<'_> as usize,
        _hints: hints,
    });
    let mut command = crate::hal::driver_task::DriverTaskCommandRecord::pi4_hot_path(
        0,
        crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
        crate::hal::driver_task::DriverTaskBudgetGrant::from_contract(driver_task_contract()),
        crate::hal::driver_task::DriverFrameDescriptor {
            offset: 0,
            len: 0,
            flags: 0,
        },
    );
    command.aux0 = DRIVER_RUNTIME_LOCAL_SEAT_INIT_AUX;
    let ok = run_local_seat_driver_task_ring_service(driver_task_contract(), command).is_some_and(
        |completion| {
            completion.code == crate::hal::driver_task::DriverTaskCompletionCode::Progress.as_u16()
                && completion.result == 1
        },
    );
    if ok {
        Ok(())
    } else {
        let _ = LOCAL_SEAT_RUNTIME_INIT_LEASE.lock().take();
        Err(LocalSeatBackendError::RuntimeInit(
            "local-seat-driver-task-init",
        ))
    }
}

#[cfg(feature = "kernel")]
unsafe fn display_runtime_ring_service_driver_task(
    context: usize,
    command: crate::hal::driver_task::DriverTaskCommandRecord,
) -> crate::hal::driver_task::DriverTaskCompletionRecord {
    let expected_hot_path = crate::hal::driver_task::DriverTaskHotPath::HdmiText;
    if context != expected_hot_path.as_u32() as usize
        || command.opcode != expected_hot_path.opcode().as_u16()
        || command.arg0 != expected_hot_path.as_u32()
        || command.arg1 != expected_hot_path.role_bit() as u32
    {
        return crate::hal::driver_task::DriverTaskCompletionRecord::fault(
            command.sequence,
            crate::hal::driver_task::DriverTaskFaultCode::RejectedCommand,
        );
    }
    let Some(bytes) = crate::hal::driver_task::driver_task_ring_frame_bytes(
        crate::hal::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT,
        command.frame,
    ) else {
        return crate::hal::driver_task::DriverTaskCompletionRecord::fault(
            command.sequence,
            crate::hal::driver_task::DriverTaskFaultCode::DeviceUnavailable,
        );
    };
    let Ok(line) = core::str::from_utf8(bytes) else {
        return crate::hal::driver_task::DriverTaskCompletionRecord::fault(
            command.sequence,
            crate::hal::driver_task::DriverTaskFaultCode::RejectedCommand,
        );
    };
    #[cfg(not(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    )))]
    let _ = line;
    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    {
        let _ = (line, command.aux0);
    }
    crate::hal::driver_task::DriverTaskCompletionRecord::fault(
        command.sequence,
        crate::hal::driver_task::DriverTaskFaultCode::DeviceUnavailable,
    )
}

#[cfg(feature = "kernel")]
unsafe fn usb_keyboard_runtime_ring_service_driver_task(
    context: usize,
    command: crate::hal::driver_task::DriverTaskCommandRecord,
) -> crate::hal::driver_task::DriverTaskCompletionRecord {
    let expected_hot_path = crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard;
    if context != expected_hot_path.as_u32() as usize
        || command.opcode != expected_hot_path.opcode().as_u16()
        || command.arg0 != expected_hot_path.as_u32()
        || command.arg1 != expected_hot_path.role_bit() as u32
        || command.frame.len != 0
    {
        return crate::hal::driver_task::DriverTaskCompletionRecord::fault(
            command.sequence,
            crate::hal::driver_task::DriverTaskFaultCode::RejectedCommand,
        );
    }
    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    {
        let _ = command.aux0;
        crate::hal::driver_task::DriverTaskCompletionRecord::fault(
            command.sequence,
            crate::hal::driver_task::DriverTaskFaultCode::DeviceUnavailable,
        )
    }
    #[cfg(not(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    )))]
    {
        crate::hal::driver_task::DriverTaskCompletionRecord::idle(command.sequence)
    }
}

#[cfg(feature = "kernel")]
unsafe fn display_ring_service_driver_task(
    context: usize,
    command: crate::hal::driver_task::DriverTaskCommandRecord,
) -> crate::hal::driver_task::DriverTaskCompletionRecord {
    let expected_hot_path = crate::hal::driver_task::DriverTaskHotPath::HdmiText;
    if command.opcode != expected_hot_path.opcode().as_u16()
        || command.arg0 != expected_hot_path.as_u32()
        || command.arg1 != expected_hot_path.role_bit() as u32
    {
        return crate::hal::driver_task::DriverTaskCompletionRecord::fault(
            command.sequence,
            crate::hal::driver_task::DriverTaskFaultCode::RejectedCommand,
        );
    }
    let Some(bytes) = crate::hal::driver_task::driver_task_ring_frame_bytes(
        crate::hal::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT,
        command.frame,
    ) else {
        return crate::hal::driver_task::DriverTaskCompletionRecord::fault(
            command.sequence,
            crate::hal::driver_task::DriverTaskFaultCode::DeviceUnavailable,
        );
    };
    let Ok(line) = core::str::from_utf8(bytes) else {
        return crate::hal::driver_task::DriverTaskCompletionRecord::fault(
            command.sequence,
            crate::hal::driver_task::DriverTaskFaultCode::RejectedCommand,
        );
    };
    // SAFETY: `context` is registered by `LocalSeatRuntime` before submitting a
    // synchronous ring command, and root waits for completion before mutating it.
    // This root pointer is transitional service context and is not owner-state
    // acceptance proof.
    let runtime = unsafe { &mut *(context as *mut LocalSeatRuntime) };
    runtime.mirror_line_current_tcb(line);
    crate::hal::driver_task::DriverTaskCompletionRecord::idle(command.sequence)
}

#[cfg(feature = "kernel")]
unsafe fn usb_keyboard_ring_service_driver_task(
    context: usize,
    command: crate::hal::driver_task::DriverTaskCommandRecord,
) -> crate::hal::driver_task::DriverTaskCompletionRecord {
    let expected_hot_path = crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard;
    if command.opcode != expected_hot_path.opcode().as_u16()
        || command.arg0 != expected_hot_path.as_u32()
        || command.arg1 != expected_hot_path.role_bit() as u32
        || command.frame.len != 0
    {
        return crate::hal::driver_task::DriverTaskCompletionRecord::fault(
            command.sequence,
            crate::hal::driver_task::DriverTaskFaultCode::RejectedCommand,
        );
    }
    // SAFETY: `context` is registered by `LocalSeatRuntime` before submitting a
    // synchronous ring command, and root waits for completion before mutating it.
    // This root pointer is transitional service context and is not owner-state
    // acceptance proof.
    let runtime = unsafe { &mut *(context as *mut LocalSeatRuntime) };
    runtime.poll_backend_keyboard_current_tcb(driver_task_contract());
    crate::hal::driver_task::DriverTaskCompletionRecord::idle(command.sequence)
}

#[cfg(feature = "kernel")]
struct DisplayMirrorTaskContext {
    runtime: usize,
    line_ptr: usize,
    line_len: usize,
}

#[cfg(feature = "kernel")]
unsafe fn display_mirror_driver_task(context: usize) -> usize {
    // SAFETY: `context` is built by `LocalSeatRuntime::mirror_line`; root waits
    // synchronously while this callback borrows the runtime and line slice. This
    // callback-pointer path is compatibility-only, not owner-state proof.
    let task = unsafe { &mut *(context as *mut DisplayMirrorTaskContext) };
    // SAFETY: `line_ptr/line_len` describe the borrowed `&str` passed to
    // `mirror_line`, which remains live until the synchronous dispatch returns.
    let bytes = unsafe { core::slice::from_raw_parts(task.line_ptr as *const u8, task.line_len) };
    // SAFETY: The original input was `&str`, so the byte slice is valid UTF-8.
    let line = unsafe { core::str::from_utf8_unchecked(bytes) };
    // SAFETY: `runtime` is the `self` pointer from `mirror_line`, exclusively
    // borrowed while root waits for this driver TCB callback to complete.
    let runtime = unsafe { &mut *(task.runtime as *mut LocalSeatRuntime) };
    runtime.mirror_line_current_tcb(line);
    0
}

#[cfg(feature = "kernel")]
unsafe fn usb_keyboard_poll_driver_task(context: usize) -> usize {
    // SAFETY: `context` is the `self` pointer from `poll_backend_keyboard`;
    // root waits synchronously while the USB/local-seat TCB polls the backend.
    // This callback-pointer path is compatibility-only, not owner-state proof.
    let runtime = unsafe { &mut *(context as *mut LocalSeatRuntime) };
    runtime.poll_backend_keyboard_current_tcb(driver_task_contract());
    0
}

/// Runtime local-seat backend initialisation error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalSeatBackendError {
    /// Driver-task runtime initialisation failed before steady-state service.
    RuntimeInit(&'static str),
    /// No local-seat backend is available on this profile/target.
    Unsupported,
}

impl LocalSeatBackendError {
    /// Stable diagnostic token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RuntimeInit(reason) => reason,
            Self::Unsupported => "unsupported",
        }
    }
}

/// Try to attach a concrete platform backend to a local-seat runtime.
#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
pub fn attach_platform_backend(
    _runtime: &mut LocalSeatRuntime,
    hal: &mut crate::hal::KernelHal<'_>,
    hints: LocalSeatPlatformHints,
) -> Result<(), LocalSeatBackendError> {
    init_local_seat_driver_runtime_on_service(hal, hints)?;
    if local_seat_driver_runtime_attached() {
        boot_log::force_uart_line("[local-seat] platform backend attached owner=linked-runtime");
    } else {
        boot_log::force_uart_line(
            "[local-seat] platform backend deferred owner=linked-runtime action=serial-shell-first",
        );
    }
    Ok(())
}

/// Host/test profile backend attach path (always unavailable).
#[cfg(not(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
)))]
#[cfg(feature = "kernel")]
pub fn attach_platform_backend(
    _runtime: &mut LocalSeatRuntime,
    _hal: &mut crate::hal::KernelHal<'_>,
    _hints: LocalSeatPlatformHints,
) -> Result<(), LocalSeatBackendError> {
    Err(LocalSeatBackendError::Unsupported)
}

/// Non-fatal degraded modes when `required=false`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalSeatDegradedReason {
    /// Manifest keyboard declaration is missing or mismatched.
    MissingKeyboard,
    /// Manifest display declaration is missing or mismatched.
    MissingDisplay,
    /// Runtime backend for USB keyboard/HDMI text is unavailable.
    BackendUnavailable,
}

impl LocalSeatDegradedReason {
    /// Stable diagnostic token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MissingKeyboard => "missing-keyboard",
            Self::MissingDisplay => "missing-display",
            Self::BackendUnavailable => "backend-unavailable",
        }
    }
}

/// Fatal local-seat error for `required=true`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalSeatError {
    /// Manifest keyboard declaration is missing or mismatched.
    MissingKeyboard,
    /// Manifest display declaration is missing or mismatched.
    MissingDisplay,
    /// Runtime backend for USB keyboard/HDMI text is unavailable.
    BackendUnavailable,
}

impl LocalSeatError {
    /// Stable diagnostic token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MissingKeyboard => "missing-keyboard",
            Self::MissingDisplay => "missing-display",
            Self::BackendUnavailable => "backend-unavailable",
        }
    }
}

/// Returns whether the local-seat runtime backend is available.
///
/// The current backend provides bounded keyboard buffering and mirrored output
/// routing while HAL-owned physical USB/HDMI device transports are attached.
#[must_use]
pub const fn runtime_backend_available() -> bool {
    cfg!(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))
}

/// Evaluate local-seat initialisation policy.
///
/// `backend_available` should reflect whether the runtime has a HAL-backed local
/// seat implementation for this platform profile.
pub fn evaluate(
    hardware: generated::HardwareConfig,
    backend_available: bool,
) -> Result<LocalSeatInit, LocalSeatError> {
    let config = hardware.local_seat;
    if !config.enabled {
        return Ok(LocalSeatInit::Disabled);
    }

    let has_keyboard = hardware.devices.iter().any(|device| {
        device.kind == HardwareDeviceKind::Keyboard && device.id == config.keyboard_device
    });
    if !has_keyboard {
        return missing(config.required, LocalSeatError::MissingKeyboard);
    }

    let has_display = hardware.devices.iter().any(|device| {
        device.kind == HardwareDeviceKind::Display && device.id == config.display_device
    });
    if !has_display {
        return missing(config.required, LocalSeatError::MissingDisplay);
    }

    if !backend_available {
        return missing(config.required, LocalSeatError::BackendUnavailable);
    }

    Ok(LocalSeatInit::Active(LocalSeatStatus {
        keyboard_device: config.keyboard_device,
        display_device: config.display_device,
        line_bytes: config.line_bytes,
        buffer_lines: config.buffer_lines,
    }))
}

fn missing(required: bool, error: LocalSeatError) -> Result<LocalSeatInit, LocalSeatError> {
    if required {
        Err(error)
    } else {
        let reason = match error {
            LocalSeatError::MissingKeyboard => LocalSeatDegradedReason::MissingKeyboard,
            LocalSeatError::MissingDisplay => LocalSeatDegradedReason::MissingDisplay,
            LocalSeatError::BackendUnavailable => LocalSeatDegradedReason::BackendUnavailable,
        };
        Ok(LocalSeatInit::Degraded(reason))
    }
}

/// Feed keyboard bytes through the canonical root-console parser.
///
/// This intentionally shares the same parser implementation used by serial/TCP
/// paths so local-seat input does not introduce a new grammar surface.
pub fn feed_keyboard_bytes(
    parser: &mut CommandParser,
    bytes: &[u8],
) -> Result<Option<Command>, ConsoleError> {
    for &byte in bytes {
        if let Some(command) = parser.push_byte(byte)? {
            return Ok(Some(command));
        }
    }
    Ok(None)
}

/// Truncate a mirrored display line to the manifest-declared byte bound.
#[must_use]
pub fn truncate_for_display(line: &str, line_bytes: u16) -> &str {
    let limit = usize::from(line_bytes);
    if line.len() <= limit {
        return line;
    }
    let mut idx = limit;
    while idx > 0 && !line.is_char_boundary(idx) {
        idx -= 1;
    }
    &line[..idx]
}

fn update_input_echo_preview(preview: &mut String, byte: u8, max_bytes: usize) {
    match byte {
        b'\r' | b'\n' => preview.clear(),
        0x08 | 0x7f => {
            let _ = preview.pop();
        }
        byte if byte.is_ascii_control() => {}
        byte => {
            if preview.len() < max_bytes {
                preview.push(byte as char);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::{
        AttestationConfig, AttestationPolicy, DhcpPolicyConfig, HardwareConfig, HardwareDevice,
        HardwareNetworkConfig, LocalSeatConfig, NetworkBackendKind, NetworkInterfacePolicy,
        NetworkMode, StaticIpv4Config,
    };

    #[cfg(feature = "kernel")]
    static LOCAL_SEAT_RING_TEST_LOCK: spin::Mutex<()> = spin::Mutex::new(());

    const KEYBOARD: HardwareDevice = HardwareDevice {
        kind: HardwareDeviceKind::Keyboard,
        id: "usb-kbd0",
        required: true,
    };
    const DISPLAY: HardwareDevice = HardwareDevice {
        kind: HardwareDeviceKind::Display,
        id: "hdmi0",
        required: true,
    };
    const DEVICES_KEYBOARD_DISPLAY: [HardwareDevice; 2] = [KEYBOARD, DISPLAY];

    fn local_seat_hw(required: bool, devices: &'static [HardwareDevice]) -> HardwareConfig {
        HardwareConfig {
            secure_boot: false,
            no_nic: false,
            network: HardwareNetworkConfig {
                enabled: false,
                backend: NetworkBackendKind::Auto,
                mode: NetworkMode::Off,
                interface: NetworkInterfacePolicy::Wired,
                static_ipv4: StaticIpv4Config {
                    ip: [0, 0, 0, 0],
                    prefix_len: 0,
                    gateway: None,
                },
                dhcp: DhcpPolicyConfig {
                    discover_timeout_ms: 1_000,
                    request_timeout_ms: 1_000,
                    max_retries: 4,
                },
            },
            attestation: AttestationConfig {
                enabled: false,
                policy: AttestationPolicy::TpmOrDice,
                evidence_max_bytes: 256,
            },
            local_seat: LocalSeatConfig {
                enabled: true,
                required,
                keyboard_device: "usb-kbd0",
                display_device: "hdmi0",
                line_bytes: 16,
                buffer_lines: 8,
            },
            devices,
        }
    }

    #[test]
    fn usb_runtime_command_replay_requires_hal_ext_cfg_proof() {
        assert!(usb_runtime_command_replay_ready(
            true,
            true,
            "hal-ext-cfg-proof",
        ));
        assert!(!usb_runtime_command_replay_ready(
            true,
            true,
            "linux-capture-replay",
        ));
        assert!(!usb_runtime_command_replay_ready(
            true,
            true,
            "linux-capture-static",
        ));
        assert!(!usb_runtime_command_replay_ready(
            true,
            true,
            "firmware-command-snapshot",
        ));
        assert!(!usb_runtime_command_replay_ready(
            false,
            true,
            "hal-ext-cfg-proof",
        ));
        assert!(!usb_runtime_command_replay_ready(
            true,
            false,
            "hal-ext-cfg-proof",
        ));
    }

    #[test]
    fn required_local_seat_fails_without_backend() {
        let err = evaluate(local_seat_hw(true, &DEVICES_KEYBOARD_DISPLAY), false)
            .expect_err("required local seat must fail when backend is unavailable");
        assert_eq!(err, LocalSeatError::BackendUnavailable);
    }

    #[test]
    fn optional_local_seat_degrades_without_backend() {
        let state = evaluate(local_seat_hw(false, &DEVICES_KEYBOARD_DISPLAY), false)
            .expect("optional local seat should degrade");
        assert_eq!(
            state,
            LocalSeatInit::Degraded(LocalSeatDegradedReason::BackendUnavailable)
        );
    }

    #[test]
    fn keyboard_probe_result_labels_required_policy_boundary() {
        assert!(LocalSeatKeyboardProbeResult::Attached.attached());
        assert!(!LocalSeatKeyboardProbeResult::KeyboardUnavailable.attached());
        assert!(!LocalSeatKeyboardProbeResult::DeferredUntilRootConsole.attached());
        assert!(!LocalSeatKeyboardProbeResult::BackendUnavailable.attached());
        assert_eq!(
            LocalSeatKeyboardProbeResult::KeyboardUnavailable.as_str(),
            "keyboard-unavailable"
        );
        assert_eq!(
            LocalSeatKeyboardProbeResult::DeferredUntilRootConsole.as_str(),
            "deferred-until-root-console"
        );
    }

    #[test]
    fn keyboard_input_uses_canonical_parser() {
        let mut parser = CommandParser::new();
        let command = feed_keyboard_bytes(&mut parser, b"help\n")
            .expect("help command must parse")
            .expect("help should yield a command");
        assert_eq!(command, Command::Help);
    }

    #[test]
    fn mirror_truncation_respects_configured_bound() {
        let truncated = truncate_for_display("0123456789abcdef", 8);
        assert_eq!(truncated, "01234567");
    }

    #[test]
    fn runtime_queues_keyboard_bytes_with_bounded_capacity() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 4,
        });

        let payload = [b'a'; KEYBOARD_QUEUE_MAX_BYTES + 64];
        let accepted = runtime.enqueue_keyboard_bytes(&payload);
        assert_eq!(accepted, KEYBOARD_QUEUE_MAX_BYTES);
        assert_eq!(runtime.dropped_keyboard_bytes(), 64);

        let mut drained = vec![0u8; 32];
        let read = runtime.drain_keyboard_bytes(&mut drained);
        assert_eq!(read, 32);
        assert!(drained.iter().all(|byte| *byte == b'a'));
    }

    #[test]
    fn runtime_keyboard_trace_records_ingress_boundaries() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 4,
        });

        runtime.enable_backend_keyboard_polling();
        runtime.poll_backend_keyboard();
        assert_eq!(runtime.enqueue_keyboard_bytes(b"abc"), 3);
        let mut drained = [0u8; 2];
        assert_eq!(runtime.drain_keyboard_bytes(&mut drained), 2);
        runtime.echo_input_bytes(&drained);

        let trace = runtime.keyboard_trace();
        assert_eq!(trace.queued_bytes, 1);
        assert_eq!(trace.backend_poll_calls, 1);
        assert_eq!(trace.backend_read_bytes, 0);
        assert_eq!(trace.accepted_bytes, 3);
        assert_eq!(trace.drained_bytes, 2);
        assert_eq!(trace.echoed_bytes, 2);
        assert_eq!(trace.dropped_bytes, 0);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn usb_keyboard_ring_service_uses_fixed_hot_path_command() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 4,
        });
        let command = crate::hal::driver_task::DriverTaskCommandRecord::pi4_hot_path(
            11,
            crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
            crate::hal::driver_task::DriverTaskBudgetGrant::from_contract(driver_task_contract()),
            crate::hal::driver_task::DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        );

        let completion = unsafe {
            usb_keyboard_ring_service_driver_task(
                &mut runtime as *mut LocalSeatRuntime as usize,
                command,
            )
        };

        assert_eq!(completion.sequence, 11);
        assert_eq!(
            completion.code,
            crate::hal::driver_task::DriverTaskCompletionCode::Idle.as_u16()
        );
        assert_eq!(runtime.keyboard_trace().backend_poll_calls, 1);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn usb_keyboard_runtime_ring_service_uses_selector_without_runtime_pointer() {
        let command = crate::hal::driver_task::DriverTaskCommandRecord::pi4_hot_path(
            14,
            crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
            crate::hal::driver_task::DriverTaskBudgetGrant::from_contract(driver_task_contract()),
            crate::hal::driver_task::DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        );

        let completion = unsafe {
            usb_keyboard_runtime_ring_service_driver_task(
                crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard.as_u32() as usize,
                command,
            )
        };

        assert_eq!(completion.sequence, 14);
        assert_eq!(
            completion.code,
            crate::hal::driver_task::DriverTaskCompletionCode::Idle.as_u16()
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn usb_keyboard_ring_service_rejects_non_hot_path_commands() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 4,
        });
        let command = crate::hal::driver_task::DriverTaskCommandRecord::service(
            13,
            crate::hal::driver_task::DriverTaskBudgetGrant::from_contract(driver_task_contract()),
        );

        let completion = unsafe {
            usb_keyboard_ring_service_driver_task(
                &mut runtime as *mut LocalSeatRuntime as usize,
                command,
            )
        };

        assert_eq!(completion.sequence, 13);
        assert_eq!(
            completion.code,
            crate::hal::driver_task::DriverTaskCompletionCode::Fault.as_u16()
        );
        assert_eq!(
            completion.detail,
            crate::hal::driver_task::DriverTaskFaultCode::RejectedCommand.as_u16()
        );
        assert_eq!(runtime.keyboard_trace().backend_poll_calls, 0);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn display_ring_service_uses_hot_path_submit_frame_command() {
        let _guard = LOCAL_SEAT_RING_TEST_LOCK.lock();
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 4,
            buffer_lines: 2,
        });
        let contract = crate::hal::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT;
        let mut ring_page = [0u8; crate::hal::driver_task::DRIVER_TASK_RING_PAGE_BYTES];
        crate::hal::driver_task::publish_driver_task_ring(
            contract,
            ring_page.as_mut_ptr() as usize,
        );
        let frame = crate::hal::driver_task::stage_driver_task_ring_frame(contract, b"abcdef", 0)
            .expect("test ring has room for one display line");
        let command = crate::hal::driver_task::DriverTaskCommandRecord::pi4_hot_path(
            12,
            crate::hal::driver_task::DriverTaskHotPath::HdmiText,
            crate::hal::driver_task::DriverTaskBudgetGrant::from_contract(contract),
            frame,
        );

        let completion = unsafe {
            display_ring_service_driver_task(
                &mut runtime as *mut LocalSeatRuntime as usize,
                command,
            )
        };
        crate::hal::driver_task::publish_driver_task_ring(contract, 0);

        assert_eq!(completion.sequence, 12);
        assert_eq!(
            completion.code,
            crate::hal::driver_task::DriverTaskCompletionCode::Idle.as_u16()
        );
        assert_eq!(
            command.arg0,
            crate::hal::driver_task::DriverTaskHotPath::HdmiText.as_u32()
        );
        let lines = runtime.mirrored_lines_snapshot();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "abcd");
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn display_runtime_ring_service_uses_selector_without_runtime_pointer() {
        let _guard = LOCAL_SEAT_RING_TEST_LOCK.lock();
        let contract = crate::hal::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT;
        let mut ring_page = [0u8; crate::hal::driver_task::DRIVER_TASK_RING_PAGE_BYTES];
        crate::hal::driver_task::publish_driver_task_ring(
            contract,
            ring_page.as_mut_ptr() as usize,
        );
        let frame =
            crate::hal::driver_task::stage_driver_task_ring_frame(contract, b"driver-task", 0)
                .expect("test ring has room for one display line");
        let command = crate::hal::driver_task::DriverTaskCommandRecord::pi4_hot_path(
            15,
            crate::hal::driver_task::DriverTaskHotPath::HdmiText,
            crate::hal::driver_task::DriverTaskBudgetGrant::from_contract(contract),
            frame,
        );

        let completion = unsafe {
            display_runtime_ring_service_driver_task(
                crate::hal::driver_task::DriverTaskHotPath::HdmiText.as_u32() as usize,
                command,
            )
        };
        crate::hal::driver_task::publish_driver_task_ring(contract, 0);

        assert_eq!(completion.sequence, 15);
        assert_eq!(
            completion.code,
            crate::hal::driver_task::DriverTaskCompletionCode::Fault.as_u16()
        );
        assert_eq!(
            completion.detail,
            crate::hal::driver_task::DriverTaskFaultCode::DeviceUnavailable.as_u16()
        );
    }

    #[test]
    fn local_seat_declares_valid_realtime_driver_task_contract() {
        let contract = driver_task_contract();

        assert_eq!(contract.name, "usb-local-seat");
        assert!(contract.preempts_network_data());
        assert_eq!(contract.validate(), Ok(()));
        assert!(contract.budget.max_ops_per_turn as usize >= KEYBOARD_POLL_CHUNK_BYTES);
        assert!(contract.budget.max_bytes_per_turn as usize >= KEYBOARD_POLL_CHUNK_BYTES);
        assert!(contract.budget.max_frames_per_turn as usize >= KEYBOARD_POLL_CHUNK_BYTES);
    }

    #[test]
    fn local_seat_pre_root_runtime_init_requires_runtime_proof_on_physical_pi() {
        assert!(!local_seat_pre_root_runtime_init_allowed(true, false));
        assert!(local_seat_pre_root_runtime_init_allowed(true, true));
        assert!(local_seat_pre_root_runtime_init_allowed(false, false));
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    #[test]
    fn local_seat_usb_init_and_enum_use_prompt_slice_only_after_prompt() {
        assert!(!local_seat_driver_task_prompt_slice_required(
            DRIVER_RUNTIME_LOCAL_SEAT_INIT_AUX,
            false
        ));
        assert!(!local_seat_driver_task_prompt_slice_required(
            DRIVER_RUNTIME_USB_ENUMERATE_AUX,
            false
        ));
        assert!(local_seat_driver_task_prompt_slice_required(
            DRIVER_RUNTIME_LOCAL_SEAT_INIT_AUX,
            true
        ));
        assert!(local_seat_driver_task_prompt_slice_required(
            DRIVER_RUNTIME_USB_ENUMERATE_AUX,
            true
        ));
        assert!(!local_seat_driver_task_prompt_slice_required(0, true));
    }

    #[test]
    fn prompt_side_keyboard_poll_suspends_after_missing_driver_reply() {
        assert!(!local_seat_keyboard_poll_suspends_on_missing_reply(
            true, false, false
        ));
        assert!(local_seat_keyboard_poll_suspends_on_missing_reply(
            true, true, false
        ));
        assert!(!local_seat_keyboard_poll_suspends_on_missing_reply(
            true, true, true
        ));
    }

    #[test]
    fn local_seat_linked_display_service_allows_first_frame_owner_proof() {
        assert!(local_seat_linked_display_service_allowed(
            true, false, false
        ));
        assert!(local_seat_linked_display_service_allowed(true, true, false));
        assert!(local_seat_linked_display_service_allowed(true, true, true));
        assert!(local_seat_linked_display_service_allowed(
            false, false, true
        ));
    }

    #[test]
    fn prompt_side_display_attach_retries_transient_misses() {
        assert!(!local_seat_display_attach_retry_allowed(false, false, 0));
        assert!(!local_seat_display_attach_retry_allowed(true, true, 0));
        assert!(local_seat_display_attach_retry_allowed(true, false, 0));
        assert!(local_seat_display_attach_retry_allowed(true, false, 3));
        assert!(!local_seat_display_attach_retry_allowed(true, false, 4));
    }

    #[test]
    fn runtime_mirrors_lines_with_manifest_bounds() {
        #[cfg(feature = "kernel")]
        let _guard = LOCAL_SEAT_RING_TEST_LOCK.lock();
        #[cfg(feature = "kernel")]
        crate::hal::driver_task::publish_driver_task_ring(
            crate::hal::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT,
            0,
        );
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 5,
            buffer_lines: 2,
        });

        runtime.mirror_line("123456");
        runtime.mirror_line("abcdef");
        runtime.mirror_line("xyz");

        let lines = runtime.mirrored_lines_snapshot();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "abcde");
        assert_eq!(lines[1], "xyz");
        assert_eq!(runtime.dropped_mirrored_lines(), 1);
    }

    #[test]
    fn input_echo_preview_tracks_typing_backspace_and_enter() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 4,
            buffer_lines: 2,
        });

        runtime.echo_input_bytes(b"ab");
        assert_eq!(runtime.input_echo_preview, "ab");

        runtime.echo_input_bytes(b"\x08c");
        assert_eq!(runtime.input_echo_preview, "ac");

        runtime.echo_input_bytes(b"def");
        assert_eq!(runtime.input_echo_preview, "acde");

        runtime.echo_input_bytes(b"\n");
        assert!(runtime.input_echo_preview.is_empty());
    }

    #[test]
    fn runtime_backend_keyboard_poll_is_manual_by_default() {
        let runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 4,
        });

        assert!(!runtime.backend_keyboard_polling_enabled());
    }

    #[test]
    fn runtime_keyboard_poll_does_not_run_while_deferred() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 4,
        });

        runtime.poll_backend_keyboard();
        assert_eq!(runtime.keyboard_trace().backend_poll_calls, 0);

        runtime.enable_backend_keyboard_polling();
        runtime.poll_backend_keyboard();
        assert_eq!(runtime.keyboard_trace().backend_poll_calls, 1);
    }

    #[test]
    fn runtime_root_console_ready_is_explicit_post_prompt_state() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 4,
        });

        assert!(!runtime.root_console_ready());
        runtime.mark_root_console_ready();
        assert!(runtime.root_console_ready());
    }

    #[test]
    fn physical_pi_local_seat_steady_service_accepts_prompt_or_runtime_proof() {
        assert!(!local_seat_prompt_steady_service_allowed(
            true, false, false
        ));
        assert!(local_seat_prompt_steady_service_allowed(true, true, false));
        assert!(local_seat_prompt_steady_service_allowed(true, false, true));
        assert!(local_seat_prompt_steady_service_allowed(
            false, false, false
        ));
    }

    #[test]
    fn physical_pi_keyboard_poll_suspends_after_missing_reply_once_shell_is_live() {
        assert!(!local_seat_keyboard_poll_suspends_on_missing_reply(
            true, false, false
        ));
        assert!(local_seat_keyboard_poll_suspends_on_missing_reply(
            true, true, false
        ));
        assert!(!local_seat_keyboard_poll_suspends_on_missing_reply(
            true, true, true
        ));
        assert!(!local_seat_keyboard_poll_suspends_on_missing_reply(
            false, true, false
        ));
    }

    #[test]
    fn physical_pi_display_mirror_miss_preserves_live_serial_prompt() {
        assert!(!local_seat_display_mirror_suspends_on_missing_reply(
            true, false
        ));
        assert!(local_seat_display_mirror_suspends_on_missing_reply(
            true, true
        ));
        assert!(!local_seat_display_mirror_suspends_on_missing_reply(
            false, true
        ));
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    #[test]
    fn linked_usb_keyboard_poll_stays_nonblocking_until_keyboard_ready() {
        assert_eq!(local_seat_keyboard_poll_aux(false), 0);
        assert_eq!(local_seat_keyboard_poll_aux(true), 0);
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    #[test]
    fn linked_usb_attach_probe_continues_while_enumeration_is_pending() {
        assert!(linked_local_seat_usb_attach_probe_required(
            false, false, false
        ));
        assert!(linked_local_seat_usb_attach_probe_required(
            true, false, true
        ));
        assert!(!linked_local_seat_usb_attach_probe_required(
            true, false, false
        ));
        assert!(!linked_local_seat_usb_attach_probe_required(
            true, true, true
        ));
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    #[test]
    fn linked_usb_enumeration_resume_remains_bounded_per_retry() {
        assert!((1..=3).contains(&LINKED_LOCAL_SEAT_USB_ENUM_RESUME_ATTEMPTS));
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    #[test]
    fn linked_usb_probe_progress_burst_is_progress_bounded() {
        assert!((1..=8).contains(&LINKED_LOCAL_SEAT_USB_PROBE_STABLE_PROGRESS_BURST_ATTEMPTS));
        assert!(!usb_enumeration_progress_token_advanced(None, None));
        assert!(usb_enumeration_progress_token_advanced(
            None,
            Some((8, 190, DRIVER_RUNTIME_USB_ENUMERATE_AUX))
        ));
        assert!(!usb_enumeration_progress_token_advanced(
            Some((8, 190, DRIVER_RUNTIME_USB_ENUMERATE_AUX)),
            Some((8, 190, DRIVER_RUNTIME_USB_ENUMERATE_AUX))
        ));
        assert!(usb_enumeration_progress_token_advanced(
            Some((8, 190, DRIVER_RUNTIME_USB_ENUMERATE_AUX)),
            Some((8, 236, DRIVER_RUNTIME_USB_ENUMERATE_AUX))
        ));
        assert!(!usb_enumeration_progress_token_allows_probe_burst(
            None, None
        ));
        assert!(usb_enumeration_progress_token_allows_probe_burst(
            Some((8, 190, DRIVER_RUNTIME_USB_ENUMERATE_AUX)),
            Some((8, 190, DRIVER_RUNTIME_USB_ENUMERATE_AUX))
        ));
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    #[test]
    fn linked_usb_pending_enumeration_defers_retry_until_prompt() {
        assert!(linked_local_seat_usb_pre_prompt_retry_deferred(
            true, false, true, false
        ));
        assert!(!linked_local_seat_usb_pre_prompt_retry_deferred(
            true, false, true, true
        ));
        assert!(!linked_local_seat_usb_pre_prompt_retry_deferred(
            false, false, true, false
        ));
        assert!(!linked_local_seat_usb_pre_prompt_retry_deferred(
            true, true, true, false
        ));
    }

    #[test]
    fn runtime_backend_keyboard_poll_can_be_enabled_explicitly() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 4,
        });

        runtime.enable_backend_keyboard_polling();

        assert!(runtime.backend_keyboard_polling_enabled());
    }

    #[test]
    fn runtime_probe_backend_keyboard_once_restores_deferred_state_by_default() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 4,
        });

        runtime.probe_backend_keyboard_once();

        assert!(!runtime.backend_keyboard_polling_enabled());
    }
}
