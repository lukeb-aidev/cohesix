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
use pi4_driver_abi::{
    DRIVER_RUNTIME_LOCAL_SEAT_INIT_AUX, DRIVER_RUNTIME_USB_ENUMERATE_AUX,
    DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_AUX,
};
#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
use pi4_driver_abi::{
    DRIVER_RUNTIME_USB_HUB_PORT_STATUS_FRAME_LEN, DRIVER_RUNTIME_USB_HUB_PORT_STATUS_FRAME_MAGIC,
    DRIVER_RUNTIME_USB_HUB_PORT_STATUS_FRAME_VERSION,
    DRIVER_RUNTIME_USB_HUB_PORT_STATUS_STAGE_INITIAL,
    DRIVER_RUNTIME_USB_HUB_PORT_STATUS_STAGE_READY,
    DRIVER_RUNTIME_USB_HUB_PORT_STATUS_STAGE_RECOVERY_POWER,
    DRIVER_RUNTIME_USB_HUB_PORT_STATUS_STAGE_RECOVERY_RESET,
    DRIVER_RUNTIME_USB_HUB_PORT_STATUS_STAGE_RESET_POLL,
    DRIVER_RUNTIME_USB_HUB_PORT_STATUS_STAGE_SKIP_DISCONNECTED,
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
    DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_IDLE,
    DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_MASK,
    DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_SHIFT,
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
const USB_HUB_STATUS_CONNECTION: u16 = 1 << 0;
#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
const USB_HUB_STATUS_ENABLE: u16 = 1 << 1;
#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
const USB_HUB_STATUS_RESET: u16 = 1 << 4;
#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
const USB_HUB_STATUS_LOW_SPEED: u16 = 1 << 9;
#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
const USB_HUB_STATUS_HIGH_SPEED: u16 = 1 << 10;
#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
const USB_HUB_CHANGE_CONNECTION: u16 = 1 << 0;
#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
const USB_HUB_CHANGE_ENABLE: u16 = 1 << 1;
#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
const USB_HUB_CHANGE_RESET: u16 = 1 << 4;

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
static LINKED_LOCAL_SEAT_DISPLAY_NO_REPLY_LOG_COUNT: AtomicUsize = AtomicUsize::new(0);

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

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_USB_FIRST_REPORT_EVENT_LOGGED: AtomicBool = AtomicBool::new(false);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_USB_FIRST_BYTE_READY_LOGGED: AtomicBool = AtomicBool::new(false);

#[cfg(all(feature = "kernel", feature = "usb"))]
// A controller-ready completion can precede root-port and HID endpoint events by
// a few linked-runtime turns; keep prompt settling bounded and non-blocking.
const LINKED_LOCAL_SEAT_USB_ENUM_RESUME_ATTEMPTS: usize = 3;
#[cfg(all(feature = "kernel", feature = "usb"))]
const LINKED_LOCAL_SEAT_USB_COLD_BOOT_ENUM_RESUME_ATTEMPTS: usize = 128;
#[cfg(all(feature = "kernel", feature = "usb"))]
// Explicit prompt-side probes must remain responsive; long hub/control waits
// continue through the pre-root cold-boot budget and cached progress evidence.
const LINKED_LOCAL_SEAT_USB_PROBE_STABLE_PROGRESS_BURST_ATTEMPTS: usize = 16;

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

/// Maximum display bytes retained while HDMI is busy or output is deferred.
const DISPLAY_QUEUE_MAX_BYTES: usize = 4_096;

/// Maximum bytes submitted to the HDMI linked runtime in one local-seat pump.
const LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES: usize = 4_096;

/// Text cell width used by the linked HDMI runtime.
const LINKED_LOCAL_SEAT_HDMI_CHAR_WIDTH: usize = 8;

/// Text cell height used by the linked HDMI runtime.
const LINKED_LOCAL_SEAT_HDMI_CHAR_HEIGHT: usize = 16;

/// Overscan clipping divisor used by the linked HDMI runtime.
const LINKED_LOCAL_SEAT_HDMI_SAFE_AREA_MARGIN_DIVISOR: usize = 50;

/// Fallback Pi 4 HDMI text width after linked-runtime overscan clipping.
const LINKED_LOCAL_SEAT_HDMI_FALLBACK_SNAPSHOT_COLS: usize = 77;

/// Fallback Pi 4 HDMI text height after linked-runtime overscan clipping.
const LINKED_LOCAL_SEAT_HDMI_FALLBACK_SNAPSHOT_ROWS: usize = 28;

/// Bytes in `ESC[H`, used to re-home redraws without a full-frame blink.
const LINKED_LOCAL_SEAT_HDMI_CURSOR_HOME_BYTES: usize = 3;

/// Bytes in `ESC[K`, used to clear stale text to the physical right edge.
const LINKED_LOCAL_SEAT_HDMI_CLEAR_EOL_BYTES: usize = 3;

/// Bytes in `ESC[J`, used to clear stale text below the rendered viewport.
const LINKED_LOCAL_SEAT_HDMI_CLEAR_TO_END_BYTES: usize = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LinkedHdmiSnapshotGeometry {
    cols: usize,
    rows: usize,
}

impl LinkedHdmiSnapshotGeometry {
    const fn fallback() -> Self {
        Self {
            cols: LINKED_LOCAL_SEAT_HDMI_FALLBACK_SNAPSHOT_COLS,
            rows: LINKED_LOCAL_SEAT_HDMI_FALLBACK_SNAPSHOT_ROWS,
        }
    }
}

/// First cooldown after a steady USB keyboard poll misses its driver reply.
const LINKED_LOCAL_SEAT_USB_POLL_NO_REPLY_INITIAL_COOLDOWN: u8 = 4;

/// Maximum adaptive cooldown for repeated steady USB keyboard poll misses.
const LINKED_LOCAL_SEAT_USB_POLL_NO_REPLY_MAX_COOLDOWN: u8 = 32;

/// First cooldown after a post-first-byte steady poll misses while the runtime
/// still reports a fully armed, idle interrupt-IN queue.
const LINKED_LOCAL_SEAT_USB_POLL_READY_IDLE_COOLDOWN: u8 = 16;

/// A healthy steady interrupt-IN queue should stay near the runtime queue
/// target. Below this floor, keep the faster recovery cadence.
const LINKED_LOCAL_SEAT_USB_READY_IDLE_MIN_QUEUED_REPORTS: u32 = 16;

/// Consecutive post-first-byte no-reply polls before asking the runtime to
/// recover the already accepted interrupt-IN endpoint.
const LINKED_LOCAL_SEAT_USB_POST_FIRST_BYTE_RECOVERY_NO_REPLY_THRESHOLD: u64 = 64;

/// Repeated raw HDMI frame no-reply logs are throttled after this many misses.
const LINKED_LOCAL_SEAT_HDMI_NO_REPLY_VERBOSE_LIMIT: usize = 4;

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

/// Return the next bounded USB keyboard poll backoff after a missing reply.
#[must_use]
pub(crate) const fn local_seat_keyboard_poll_next_no_reply_backoff(
    keyboard_ready: bool,
    previous: u8,
) -> u8 {
    local_seat_keyboard_poll_next_no_reply_backoff_for_queue(keyboard_ready, false, previous)
}

/// Return the next USB keyboard no-reply backoff with post-first-byte queue
/// health as an input.
#[must_use]
pub(crate) const fn local_seat_keyboard_poll_next_no_reply_backoff_for_queue(
    keyboard_ready: bool,
    steady_queue_idle: bool,
    previous: u8,
) -> u8 {
    if keyboard_ready && !steady_queue_idle {
        1
    } else if steady_queue_idle && previous < LINKED_LOCAL_SEAT_USB_POLL_READY_IDLE_COOLDOWN {
        LINKED_LOCAL_SEAT_USB_POLL_READY_IDLE_COOLDOWN
    } else if previous == 0 {
        LINKED_LOCAL_SEAT_USB_POLL_NO_REPLY_INITIAL_COOLDOWN
    } else {
        let doubled = previous.saturating_mul(2);
        if doubled > LINKED_LOCAL_SEAT_USB_POLL_NO_REPLY_MAX_COOLDOWN {
            LINKED_LOCAL_SEAT_USB_POLL_NO_REPLY_MAX_COOLDOWN
        } else {
            doubled
        }
    }
}

/// Return whether the `count`th repeat of a no-reply diagnostic should still be
/// logged to the raw UART.
#[must_use]
const fn repeated_no_reply_log_visible(count: usize) -> bool {
    count < LINKED_LOCAL_SEAT_HDMI_NO_REPLY_VERBOSE_LIMIT
        || (count != 0 && (count & count.saturating_sub(1)) == 0)
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
const fn local_seat_keyboard_poll_aux(keyboard_ready: bool, request_recovery: bool) -> u32 {
    if keyboard_ready && request_recovery {
        DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_AUX
    } else {
        0
    }
}

const fn local_seat_keyboard_recovery_aux_allowed(queue_empty: bool, no_reply_streak: u64) -> bool {
    queue_empty
        && no_reply_streak == LINKED_LOCAL_SEAT_USB_POST_FIRST_BYTE_RECOVERY_NO_REPLY_THRESHOLD
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
    /// Steady USB keyboard polls that did not receive a driver-task reply.
    pub driver_task_no_replies: u64,
    /// Consecutive no-reply polls since the last driver-task completion.
    pub driver_task_no_reply_streak: u64,
    /// Current adaptive cooldown before the next steady USB keyboard poll.
    pub poll_cooldown_turns: u8,
    /// Poll turns intentionally skipped while the no-reply cooldown was active.
    pub poll_cooldown_skips: u64,
}

/// HDMI display pressure counters for isolating mirror stalls from USB input.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LocalSeatDisplayTrace {
    /// Queued display bytes waiting for an idle HDMI pump turn.
    pub pending_bytes: usize,
    /// Whether a coalesced scrollback redraw is waiting.
    pub pending_redraw: bool,
    /// HDMI frames submitted by the deferred display pump.
    pub submitted_frames: u64,
    /// Display frames intentionally deferred because HDMI was not idle.
    pub deferred_frames: u64,
    /// Pump attempts blocked by an active HDMI driver-task command.
    pub busy_frames: u64,
    /// Pump attempts that did not receive a driver-task reply.
    pub no_reply_frames: u64,
    /// Scrollback redraw requests collapsed into an already pending redraw.
    pub coalesced_redraws: u64,
    /// Display bytes dropped because the bounded mirror queue was full.
    pub backpressure_bytes: u64,
    /// Stale queued bytes discarded because a redraw superseded them.
    pub superseded_bytes: u64,
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
    input_echo_escape_state: u8,
    mirrored_lines: VecDeque<String>,
    hdmi_pending_bytes: VecDeque<u8>,
    hdmi_redraw_bytes: VecDeque<u8>,
    hdmi_pending_redraw: bool,
    hdmi_scrollback_offset: usize,
    hdmi_input_escape_state: u8,
    hdmi_open_line: bool,
    hdmi_open_line_floor_bytes: usize,
    hdmi_open_line_mirrors_input: bool,
    dropped_keyboard_bytes: u64,
    dropped_mirrored_lines: u64,
    hdmi_submitted_frames: u64,
    hdmi_deferred_frames: u64,
    hdmi_busy_frames: u64,
    hdmi_no_reply_frames: u64,
    hdmi_coalesced_redraws: u64,
    hdmi_backpressure_bytes: u64,
    hdmi_superseded_bytes: u64,
    backend_keyboard_poll_calls: u64,
    backend_keyboard_read_bytes: u64,
    accepted_keyboard_bytes: u64,
    drained_keyboard_bytes: u64,
    echoed_keyboard_bytes: u64,
    driver_task_budget_overruns: u64,
    driver_task_no_replies: u64,
    keyboard_poll_no_reply_streak: u64,
    keyboard_poll_no_reply_cooldown: u8,
    keyboard_poll_no_reply_backoff: u8,
    keyboard_poll_cooldown_skips: u64,
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
            input_echo_escape_state: 0,
            mirrored_lines: VecDeque::new(),
            hdmi_pending_bytes: VecDeque::new(),
            hdmi_redraw_bytes: VecDeque::new(),
            hdmi_pending_redraw: false,
            hdmi_scrollback_offset: 0,
            hdmi_input_escape_state: 0,
            hdmi_open_line: false,
            hdmi_open_line_floor_bytes: 0,
            hdmi_open_line_mirrors_input: false,
            dropped_keyboard_bytes: 0,
            dropped_mirrored_lines: 0,
            hdmi_submitted_frames: 0,
            hdmi_deferred_frames: 0,
            hdmi_busy_frames: 0,
            hdmi_no_reply_frames: 0,
            hdmi_coalesced_redraws: 0,
            hdmi_backpressure_bytes: 0,
            hdmi_superseded_bytes: 0,
            backend_keyboard_poll_calls: 0,
            backend_keyboard_read_bytes: 0,
            accepted_keyboard_bytes: 0,
            drained_keyboard_bytes: 0,
            echoed_keyboard_bytes: 0,
            driver_task_budget_overruns: 0,
            driver_task_no_replies: 0,
            keyboard_poll_no_reply_streak: 0,
            keyboard_poll_no_reply_cooldown: 0,
            keyboard_poll_no_reply_backoff: 0,
            keyboard_poll_cooldown_skips: 0,
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

    /// Drain complete display-control arrow sequences without entering commands.
    ///
    /// This keeps HDMI scrollback responsive while the console is emitting output,
    /// but preserves ordinary typed bytes for the canonical parser path.
    pub fn drain_display_control_bytes_during_output(&mut self, budget: usize) -> usize {
        let mut drained = 0usize;
        let mut bytes = [0u8; KEYBOARD_POLL_CHUNK_BYTES];
        let limit = budget.min(KEYBOARD_POLL_CHUNK_BYTES);
        while drained.saturating_add(3) <= limit && self.keyboard_queue.len() >= 3 {
            if self.keyboard_queue.get(0) != Some(&0x1b)
                || self.keyboard_queue.get(1) != Some(&b'[')
                || !matches!(self.keyboard_queue.get(2), Some(b'A' | b'B'))
            {
                break;
            }
            bytes[drained] = self.keyboard_queue.pop_front().unwrap_or(0);
            bytes[drained + 1] = self.keyboard_queue.pop_front().unwrap_or(0);
            bytes[drained + 2] = self.keyboard_queue.pop_front().unwrap_or(0);
            drained = drained.saturating_add(3);
        }
        if drained != 0 {
            self.drained_keyboard_bytes =
                self.drained_keyboard_bytes.saturating_add(drained as u64);
            self.echo_input_bytes(&bytes[..drained]);
        }
        drained
    }

    /// Mirror a console line into the bounded local-seat output ring.
    pub fn mirror_line(&mut self, line: &str) {
        #[cfg(feature = "kernel")]
        {
            let contract = crate::hal::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT;
            #[cfg(all(feature = "usb", target_arch = "aarch64", target_os = "none"))]
            {
                if crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active() {
                    let queue_tail = !self.root_console_ready
                        || (self.hdmi_scrollback_offset == 0 && !self.linked_hdmi_redraw_pending());
                    self.close_linked_hdmi_open_line(queue_tail);
                    self.mirror_line_current_tcb(line);
                    if queue_tail && !self.queue_linked_hdmi_line(line) {
                        self.request_linked_hdmi_snapshot_redraw();
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
    /// On physical Pi 4 this queues HDMI text for the linked `hdmi-text`
    /// runtime. The event pump submits it later on an idle turn.
    pub fn mirror_high_impact_line(&mut self, line: &str) -> bool {
        #[cfg(all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        ))]
        {
            if crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active() {
                let queue_tail = !self.root_console_ready
                    || (self.hdmi_scrollback_offset == 0 && !self.linked_hdmi_redraw_pending());
                self.close_linked_hdmi_open_line(queue_tail);
                self.mirror_line_current_tcb(line);
                if queue_tail {
                    if self.queue_linked_hdmi_line(line) {
                        return true;
                    }
                    self.request_linked_hdmi_snapshot_redraw();
                }
                return true;
            }
        }
        self.mirror_line(line);
        true
    }

    fn mirror_line_current_tcb(&mut self, line: &str) {
        let truncated = truncate_for_display(line, self.status.line_bytes);
        let mut mirrored = String::new();
        mirrored.push_str(truncated);
        let was_scrolled_back = self.hdmi_scrollback_offset != 0;

        while self.mirrored_lines.len() >= usize::from(self.status.buffer_lines) {
            if self.mirrored_lines.pop_front().is_none() {
                break;
            }
            self.dropped_mirrored_lines = self.dropped_mirrored_lines.saturating_add(1);
        }
        self.mirrored_lines.push_back(mirrored);
        if was_scrolled_back {
            self.hdmi_scrollback_offset = self
                .hdmi_scrollback_offset
                .saturating_add(1)
                .min(self.max_linked_hdmi_scrollback_offset());
        }

        #[cfg(all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        ))]
        let _ = truncated;
    }

    fn queue_linked_hdmi_payload(&mut self, bytes: &[u8]) -> bool {
        if bytes.is_empty() {
            return true;
        }
        let available = DISPLAY_QUEUE_MAX_BYTES.saturating_sub(self.hdmi_pending_bytes.len());
        if bytes.len() > available {
            self.hdmi_backpressure_bytes = self
                .hdmi_backpressure_bytes
                .saturating_add(bytes.len() as u64);
            return false;
        }
        for &byte in bytes {
            self.hdmi_pending_bytes.push_back(byte);
        }
        true
    }

    fn queue_linked_hdmi_line(&mut self, line: &str) -> bool {
        let truncated = truncate_for_display(line, self.status.line_bytes);
        let mut payload = Vec::new();
        payload.extend_from_slice(truncated.as_bytes());
        payload.push(b'\n');
        self.queue_linked_hdmi_payload(payload.as_slice())
    }

    fn queue_linked_hdmi_prompt(&mut self, prompt: &str) -> bool {
        let truncated = truncate_for_display(prompt, self.status.line_bytes);
        self.queue_linked_hdmi_payload(truncated.as_bytes())
    }

    fn close_linked_hdmi_open_line(&mut self, queue_visible_newline: bool) -> bool {
        if !self.hdmi_open_line {
            return true;
        }
        self.hdmi_open_line = false;
        self.hdmi_open_line_floor_bytes = 0;
        self.hdmi_open_line_mirrors_input = false;
        if queue_visible_newline {
            self.queue_linked_hdmi_payload(b"\n")
        } else {
            true
        }
    }

    fn open_linked_hdmi_prompt_line(&mut self, prompt: &str) {
        self.hdmi_open_line = true;
        self.hdmi_open_line_floor_bytes =
            truncate_for_display(prompt, self.status.line_bytes).len();
        self.hdmi_open_line_mirrors_input = false;
    }

    fn linked_hdmi_open_line_matches(&self, line: &str) -> bool {
        if !self.hdmi_open_line {
            return false;
        }
        let expected = truncate_for_display(line, self.status.line_bytes);
        self.mirrored_lines
            .back()
            .is_some_and(|current| current.as_str() == expected)
    }

    /// Mirror a prompt without forcing a newline, so USB input echoes on the
    /// same terminal row.
    pub fn mirror_prompt(&mut self, prompt: &str) {
        #[cfg(all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        ))]
        {
            if crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active() {
                let queue_tail = self.root_console_ready
                    && self.hdmi_scrollback_offset == 0
                    && !self.linked_hdmi_redraw_pending();
                if self.linked_hdmi_open_line_matches(prompt) {
                    return;
                }
                self.close_linked_hdmi_open_line(queue_tail);
                self.mirror_line_current_tcb(prompt);
                self.open_linked_hdmi_prompt_line(prompt);
                if queue_tail && !self.queue_linked_hdmi_prompt(prompt) {
                    self.request_linked_hdmi_snapshot_redraw();
                }
                return;
            }
        }
        self.mirror_line(prompt);
    }

    fn request_linked_hdmi_snapshot_redraw(&mut self) {
        if self.linked_hdmi_redraw_pending() {
            self.hdmi_coalesced_redraws = self.hdmi_coalesced_redraws.saturating_add(1);
        }
        let superseded = self
            .hdmi_pending_bytes
            .len()
            .saturating_add(self.hdmi_redraw_bytes.len());
        if superseded != 0 {
            self.hdmi_superseded_bytes =
                self.hdmi_superseded_bytes.saturating_add(superseded as u64);
            self.hdmi_pending_bytes.clear();
            self.hdmi_redraw_bytes.clear();
        }
        self.hdmi_pending_redraw = true;
    }

    fn append_linked_hdmi_snapshot_line(
        &self,
        payload: &mut Vec<u8>,
        line: &str,
        input_preview: Option<&str>,
        geometry: LinkedHdmiSnapshotGeometry,
        append_newline: bool,
    ) -> bool {
        let mut live_line = String::new();
        let line_width = linked_hdmi_snapshot_line_width(self.status.line_bytes, geometry);
        let display_line = if let Some(preview) = input_preview {
            live_line.push_str(line);
            live_line.push_str(preview);
            truncate_for_display(
                live_line.as_str(),
                linked_hdmi_snapshot_width_u16(line_width),
            )
        } else {
            truncate_for_display(line, linked_hdmi_snapshot_width_u16(line_width))
        };
        let line_bytes = display_line.as_bytes();
        let take = line_bytes.len().min(line_width);
        payload.extend_from_slice(&line_bytes[..take]);
        payload.extend_from_slice(b"\x1b[K");
        if append_newline {
            payload.push(b'\n');
        }
        true
    }

    fn linked_hdmi_snapshot_start(
        &self,
        end: usize,
        geometry: LinkedHdmiSnapshotGeometry,
    ) -> usize {
        if end == 0 {
            return 0;
        }
        let visible = linked_hdmi_scrollback_visible_lines_for_geometry(
            self.status.buffer_lines,
            self.status.line_bytes,
            geometry,
        );
        end.saturating_sub(visible)
    }

    fn build_linked_hdmi_scrollback_payload(&self) -> Vec<u8> {
        self.build_linked_hdmi_scrollback_payload_for_geometry(linked_hdmi_snapshot_geometry())
    }

    fn build_linked_hdmi_scrollback_payload_for_geometry(
        &self,
        geometry: LinkedHdmiSnapshotGeometry,
    ) -> Vec<u8> {
        let total = self.mirrored_lines.len();
        let max_offset = self.max_linked_hdmi_scrollback_offset();
        let offset = self.hdmi_scrollback_offset.min(max_offset);
        let end = total.saturating_sub(offset);
        let start = self.linked_hdmi_snapshot_start(end, geometry);
        let mut payload = Vec::new();
        payload.extend_from_slice(b"\x1b[H");
        if start == end {
            let input_preview = if offset == 0
                && total == 0
                && !self.input_echo_preview.is_empty()
                && !self.hdmi_open_line_mirrors_input
            {
                Some(self.input_echo_preview.as_str())
            } else {
                None
            };
            if input_preview.is_some() {
                if !self.append_linked_hdmi_snapshot_line(
                    &mut payload,
                    "",
                    input_preview,
                    geometry,
                    false,
                ) {
                    return payload;
                }
            }
        } else {
            for index in start..end {
                let append_newline = index.saturating_add(1) < end;
                let Some(line) = self.mirrored_lines.get(index) else {
                    continue;
                };
                let input_preview = if offset == 0
                    && index.saturating_add(1) == end
                    && !self.input_echo_preview.is_empty()
                    && !self.hdmi_open_line_mirrors_input
                {
                    Some(self.input_echo_preview.as_str())
                } else {
                    None
                };
                if !self.append_linked_hdmi_snapshot_line(
                    &mut payload,
                    line.as_str(),
                    input_preview,
                    geometry,
                    append_newline,
                ) {
                    break;
                }
            }
        }
        payload.extend_from_slice(b"\x1b[J");
        payload
    }

    fn pop_linked_hdmi_chunk(queue: &mut VecDeque<u8>) -> Vec<u8> {
        let mut payload = Vec::new();
        while payload.len() < LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES {
            let Some(byte) = queue.pop_front() else {
                break;
            };
            payload.push(byte);
        }
        payload
    }

    fn next_linked_hdmi_payload(&mut self) -> Option<(Vec<u8>, &'static str, bool)> {
        if self.hdmi_pending_redraw {
            self.hdmi_pending_redraw = false;
            self.hdmi_redraw_bytes.clear();
            for byte in self.build_linked_hdmi_scrollback_payload() {
                self.hdmi_redraw_bytes.push_back(byte);
            }
        }
        if !self.hdmi_redraw_bytes.is_empty() {
            return Some((
                Self::pop_linked_hdmi_chunk(&mut self.hdmi_redraw_bytes),
                "keyboard-scrollback",
                true,
            ));
        }
        if self.hdmi_pending_bytes.is_empty() {
            return None;
        }
        Some((
            Self::pop_linked_hdmi_chunk(&mut self.hdmi_pending_bytes),
            "queued-output",
            false,
        ))
    }

    fn record_linked_hdmi_submit_miss(&mut self, payload_len: usize, redraw: bool) {
        self.hdmi_no_reply_frames = self.hdmi_no_reply_frames.saturating_add(1);
        self.hdmi_deferred_frames = self.hdmi_deferred_frames.saturating_add(1);
        if !redraw {
            self.hdmi_superseded_bytes = self
                .hdmi_superseded_bytes
                .saturating_add(payload_len as u64);
        }
        self.request_linked_hdmi_snapshot_redraw();
    }

    /// Return whether local-seat HDMI has queued work waiting for an idle turn.
    #[must_use]
    pub fn linked_hdmi_pending_work(&self) -> bool {
        self.linked_hdmi_redraw_pending() || !self.hdmi_pending_bytes.is_empty()
    }

    fn linked_hdmi_redraw_pending(&self) -> bool {
        self.hdmi_pending_redraw || !self.hdmi_redraw_bytes.is_empty()
    }

    /// Submit at most one queued HDMI frame on a quiet event-loop turn.
    #[must_use]
    pub fn pump_linked_hdmi_once(&mut self) -> bool {
        if !self.linked_hdmi_pending_work() {
            return false;
        }
        #[cfg(all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        ))]
        {
            if !self.root_console_ready
                || !crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active()
            {
                self.hdmi_deferred_frames = self.hdmi_deferred_frames.saturating_add(1);
                return false;
            }
            let contract = crate::hal::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT;
            let _ = adopt_linked_display_runtime_owner_state("queued-display-pump");
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
                self.hdmi_deferred_frames = self.hdmi_deferred_frames.saturating_add(1);
                return false;
            }
            crate::hal::driver_task::register_driver_task_pointer_free_ring_service(
                contract,
                crate::hal::driver_task::DriverTaskHotPath::HdmiText.as_u32() as usize,
                display_runtime_ring_service_driver_task,
            );
            if crate::hal::driver_task::driver_task_ring_command_active(contract) {
                self.hdmi_busy_frames = self.hdmi_busy_frames.saturating_add(1);
                self.hdmi_deferred_frames = self.hdmi_deferred_frames.saturating_add(1);
                return false;
            }
            let Some((payload, reason, redraw)) = self.next_linked_hdmi_payload() else {
                return false;
            };
            let submitted = submit_linked_hdmi_payload_via_linked_hdmi(
                payload.as_slice(),
                self.root_console_ready,
                reason,
            );
            if submitted {
                self.hdmi_submitted_frames = self.hdmi_submitted_frames.saturating_add(1);
                true
            } else {
                self.record_linked_hdmi_submit_miss(payload.len(), redraw);
                false
            }
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
            driver_task_no_replies: self.driver_task_no_replies,
            driver_task_no_reply_streak: self.keyboard_poll_no_reply_streak,
            poll_cooldown_turns: self.keyboard_poll_no_reply_cooldown,
            poll_cooldown_skips: self.keyboard_poll_cooldown_skips,
        }
    }

    /// Return HDMI mirror pressure counters for diagnostics.
    #[must_use]
    pub fn display_trace(&self) -> LocalSeatDisplayTrace {
        LocalSeatDisplayTrace {
            pending_bytes: self
                .hdmi_pending_bytes
                .len()
                .saturating_add(self.hdmi_redraw_bytes.len()),
            pending_redraw: self.linked_hdmi_redraw_pending(),
            submitted_frames: self.hdmi_submitted_frames,
            deferred_frames: self.hdmi_deferred_frames,
            busy_frames: self.hdmi_busy_frames,
            no_reply_frames: self.hdmi_no_reply_frames,
            coalesced_redraws: self.hdmi_coalesced_redraws,
            backpressure_bytes: self.hdmi_backpressure_bytes,
            superseded_bytes: self.hdmi_superseded_bytes,
        }
    }

    /// Enable backend keyboard polling after boot has reached a safe manual
    /// control point.
    pub fn enable_backend_keyboard_polling(&mut self) {
        self.backend_keyboard_polling_enabled = true;
        self.backend_keyboard_poll_deferred_logged = false;
        self.clear_keyboard_poll_no_reply_backoff();
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
                self.clear_keyboard_poll_no_reply_backoff();
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
            self.clear_keyboard_poll_no_reply_backoff();
            self.poll_backend_keyboard();
            for _ in 0..LINKED_LOCAL_SEAT_USB_PROBE_STABLE_PROGRESS_BURST_ATTEMPTS {
                let current_progress = latest_usb_enumeration_progress_token();
                if !usb_enumeration_progress_token_allows_probe_burst(
                    previous_progress,
                    current_progress,
                    crate::hal::driver_task::driver_task_ring_command_active(driver_task_contract()),
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
                self.clear_keyboard_poll_no_reply_backoff();
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
        let mut preview_changed = false;
        for &byte in bytes {
            preview_changed |= update_input_echo_preview(
                &mut self.input_echo_preview,
                &mut self.input_echo_escape_state,
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
        self.mirror_input_bytes_to_display(bytes, preview_changed);

        #[cfg(not(all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        )))]
        let _ = preview_changed;
    }

    fn max_linked_hdmi_scrollback_offset(&self) -> usize {
        self.mirrored_lines
            .len()
            .saturating_sub(linked_hdmi_scrollback_visible_lines(
                self.status.buffer_lines,
                self.status.line_bytes,
            ))
    }

    fn apply_linked_hdmi_scroll_delta(&mut self, delta: i32) -> bool {
        if delta == 0 {
            return false;
        }
        let previous = self.hdmi_scrollback_offset;
        let max_offset = self.max_linked_hdmi_scrollback_offset();
        self.hdmi_scrollback_offset = if delta > 0 {
            previous.saturating_add(delta as usize).min(max_offset)
        } else {
            previous.saturating_sub(delta.saturating_neg() as usize)
        };
        self.hdmi_scrollback_offset != previous
    }

    fn ensure_linked_hdmi_open_input_line(&mut self) {
        if self.hdmi_open_line {
            return;
        }
        self.mirror_line_current_tcb("");
        self.hdmi_open_line = true;
        self.hdmi_open_line_floor_bytes = 0;
        self.hdmi_open_line_mirrors_input = true;
    }

    fn record_linked_hdmi_input_echo_byte(&mut self, byte: u8) {
        match byte {
            b'\r' | b'\n' => {
                self.close_linked_hdmi_open_line(false);
            }
            0x08 | 0x7f => {
                if !self.hdmi_open_line {
                    return;
                }
                if let Some(line) = self.mirrored_lines.back_mut() {
                    if line.len() > self.hdmi_open_line_floor_bytes {
                        line.pop();
                        self.hdmi_open_line_mirrors_input = true;
                    }
                }
            }
            b'\t' => {
                for _ in 0..4 {
                    self.record_linked_hdmi_input_echo_byte(b' ');
                }
            }
            byte if byte.is_ascii_control() => {}
            byte => {
                self.ensure_linked_hdmi_open_input_line();
                if let Some(line) = self.mirrored_lines.back_mut() {
                    if line.len() < usize::from(self.status.line_bytes) {
                        line.push(byte as char);
                        self.hdmi_open_line_mirrors_input = true;
                    }
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
    fn mirror_input_bytes_to_display(&mut self, bytes: &[u8], preview_changed: bool) {
        if bytes.is_empty() || !self.root_console_ready {
            return;
        }
        if crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active() {
            let mut scroll_delta = 0i32;
            let mut redraw = false;
            let mut terminal_echo = Vec::new();
            for &byte in bytes {
                match self.hdmi_input_escape_state {
                    0 if byte == 0x1b => {
                        self.hdmi_input_escape_state = 1;
                    }
                    0 => {
                        if local_seat_hdmi_input_echo_byte(byte) {
                            self.record_linked_hdmi_input_echo_byte(byte);
                            terminal_echo.push(byte);
                        }
                    }
                    1 if byte == b'[' => {
                        self.hdmi_input_escape_state = 2;
                    }
                    2 if byte == b'A' => {
                        self.hdmi_input_escape_state = 0;
                        scroll_delta = scroll_delta.saturating_add(1);
                    }
                    2 if byte == b'B' => {
                        self.hdmi_input_escape_state = 0;
                        scroll_delta = scroll_delta.saturating_sub(1);
                    }
                    2 if byte.is_ascii_digit() || byte == b';' => {}
                    2 if (0x40..=0x7e).contains(&byte) => {
                        self.hdmi_input_escape_state = 0;
                    }
                    _ => {
                        self.hdmi_input_escape_state = 0;
                    }
                }
            }
            if !terminal_echo.is_empty()
                && self.hdmi_scrollback_offset == 0
                && !self.linked_hdmi_redraw_pending()
            {
                if !self.queue_linked_hdmi_payload(terminal_echo.as_slice()) {
                    self.request_linked_hdmi_snapshot_redraw();
                }
            } else if preview_changed && self.hdmi_scrollback_offset != 0 {
                self.hdmi_scrollback_offset = 0;
                redraw = true;
            }
            if self.apply_linked_hdmi_scroll_delta(scroll_delta) {
                redraw = true;
            }
            if redraw {
                self.request_linked_hdmi_snapshot_redraw();
            }
        }
    }

    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    fn scroll_linked_hdmi_back(&mut self) {
        if self.apply_linked_hdmi_scroll_delta(1) {
            self.request_linked_hdmi_snapshot_redraw();
        }
    }

    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    fn scroll_linked_hdmi_forward(&mut self) {
        if self.apply_linked_hdmi_scroll_delta(-1) {
            self.request_linked_hdmi_snapshot_redraw();
        }
    }

    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    fn render_linked_hdmi_scrollback(&mut self) {
        let max_offset = self.max_linked_hdmi_scrollback_offset();
        self.hdmi_scrollback_offset = self.hdmi_scrollback_offset.min(max_offset);
        self.request_linked_hdmi_snapshot_redraw();
    }

    fn keyboard_poll_no_reply_cooldown_active(&mut self) -> bool {
        if self.keyboard_poll_no_reply_cooldown == 0 {
            return false;
        }
        self.keyboard_poll_no_reply_cooldown =
            self.keyboard_poll_no_reply_cooldown.saturating_sub(1);
        self.keyboard_poll_cooldown_skips = self.keyboard_poll_cooldown_skips.saturating_add(1);
        true
    }

    fn clear_keyboard_poll_no_reply_backoff(&mut self) {
        self.keyboard_poll_no_reply_cooldown = 0;
        self.keyboard_poll_no_reply_backoff = 0;
    }

    fn record_keyboard_poll_completion(&mut self) {
        self.keyboard_poll_no_reply_streak = 0;
        self.clear_keyboard_poll_no_reply_backoff();
    }

    fn keyboard_poll_fast_recovery_active(&self) -> bool {
        #[cfg(all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        ))]
        {
            crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active()
                && LINKED_LOCAL_SEAT_USB_KEYBOARD_READY.load(Ordering::Acquire)
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

    fn keyboard_poll_steady_queue_idle(&self) -> bool {
        #[cfg(all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        ))]
        {
            if !crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active()
                || !LINKED_LOCAL_SEAT_USB_FIRST_BYTE_READY_LOGGED.load(Ordering::Acquire)
            {
                return false;
            }
            let result = LINKED_LOCAL_SEAT_USB_LAST_RESULT.load(Ordering::Acquire) as u32;
            let queued_reports = result & 0xff;
            let report_status = (result >> DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_SHIFT)
                & DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_MASK;
            queued_reports >= LINKED_LOCAL_SEAT_USB_READY_IDLE_MIN_QUEUED_REPORTS
                && report_status == u32::from(DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_IDLE)
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

    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    fn keyboard_poll_recovery_aux_requested(&self) -> bool {
        local_seat_keyboard_recovery_aux_allowed(
            self.keyboard_queue.is_empty(),
            self.keyboard_poll_no_reply_streak,
        ) && crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active()
            && LINKED_LOCAL_SEAT_USB_KEYBOARD_READY.load(Ordering::Acquire)
            && LINKED_LOCAL_SEAT_USB_FIRST_REPORT_READY_LOGGED.load(Ordering::Acquire)
            && LINKED_LOCAL_SEAT_USB_FIRST_BYTE_READY_LOGGED.load(Ordering::Acquire)
    }

    fn record_keyboard_poll_no_reply(&mut self) {
        self.driver_task_no_replies = self.driver_task_no_replies.saturating_add(1);
        self.keyboard_poll_no_reply_streak = self.keyboard_poll_no_reply_streak.saturating_add(1);
        let steady_queue_idle = self.keyboard_poll_steady_queue_idle();
        let next = local_seat_keyboard_poll_next_no_reply_backoff_for_queue(
            self.keyboard_poll_fast_recovery_active(),
            steady_queue_idle,
            self.keyboard_poll_no_reply_backoff,
        );
        self.keyboard_poll_no_reply_backoff = next;
        self.keyboard_poll_no_reply_cooldown = next;
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
        if self.keyboard_poll_no_reply_cooldown_active() {
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
                    self.record_keyboard_poll_no_reply();
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
                    self.keyboard_poll_recovery_aux_requested(),
                );
                if let Some(completion) = run_local_seat_driver_task_ring_service(contract, command)
                {
                    self.record_keyboard_poll_completion();
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
                            let accepted = self.enqueue_keyboard_bytes(bytes);
                            publish_linked_local_seat_usb_first_report_event(
                                contract, completion, bytes, accepted,
                            );
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
                self.record_keyboard_poll_no_reply();
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
                self.record_keyboard_poll_completion();
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
                self.record_keyboard_poll_completion();
                return;
            }
            self.record_keyboard_poll_no_reply();
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
fn publish_linked_local_seat_usb_first_report_event(
    contract: crate::hal::driver_task::DriverTaskContract,
    completion: crate::hal::driver_task::DriverTaskCompletionRecord,
    bytes: &[u8],
    accepted: usize,
) {
    use core::fmt::Write;

    if bytes.is_empty() {
        return;
    }
    if !LINKED_LOCAL_SEAT_USB_FIRST_REPORT_EVENT_LOGGED.swap(true, Ordering::AcqRel) {
        let mut line = heapless::String::<256>::new();
        let _ = write!(
            line,
            "[local-seat] usb hid first report contract={} source=linked-runtime-hid tag=usb-hid-report-event len={} accepted={} detail=0x{:04x} result=0x{:08x} transfer_event=yes",
            contract.name,
            bytes.len(),
            accepted,
            completion.detail,
            completion.result,
        );
        boot_log::force_uart_line_raw(line.as_str());
    }
    if accepted == 0 {
        return;
    }
    if !LINKED_LOCAL_SEAT_USB_FIRST_BYTE_READY_LOGGED.swap(true, Ordering::AcqRel) {
        let byte = bytes.first().copied().unwrap_or(0);
        let mut line = heapless::String::<192>::new();
        let _ = write!(
            line,
            "[local-seat] runtime keyboard first-byte source=linked-runtime-hid read=1 ascii=0x{:02x} detail=0x{:04x} result=0x{:08x}",
            byte,
            completion.detail,
            completion.result,
        );
        boot_log::force_uart_line_raw(line.as_str());
    }
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
const fn usb_hub_port_trace_stage_label(stage: u8) -> &'static str {
    match stage {
        DRIVER_RUNTIME_USB_HUB_PORT_STATUS_STAGE_INITIAL => "initial",
        DRIVER_RUNTIME_USB_HUB_PORT_STATUS_STAGE_RESET_POLL => "reset-poll",
        DRIVER_RUNTIME_USB_HUB_PORT_STATUS_STAGE_RECOVERY_POWER => "recovery-power",
        DRIVER_RUNTIME_USB_HUB_PORT_STATUS_STAGE_RECOVERY_RESET => "recovery-reset",
        DRIVER_RUNTIME_USB_HUB_PORT_STATUS_STAGE_READY => "ready",
        DRIVER_RUNTIME_USB_HUB_PORT_STATUS_STAGE_SKIP_DISCONNECTED => "skip-disconnected",
        _ => "unknown",
    }
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
const fn usb_hub_port_trace_speed(status: u16) -> &'static str {
    if status & USB_HUB_STATUS_LOW_SPEED != 0 {
        "low"
    } else if status & USB_HUB_STATUS_HIGH_SPEED != 0 {
        "high"
    } else {
        "full"
    }
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn usb_trace_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from(bytes[offset]) | (u16::from(bytes[offset + 1]) << 8)
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn usb_trace_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from(bytes[offset])
        | (u32::from(bytes[offset + 1]) << 8)
        | (u32::from(bytes[offset + 2]) << 16)
        | (u32::from(bytes[offset + 3]) << 24)
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn emit_linked_local_seat_usb_hub_port_trace(
    contract: crate::hal::driver_task::DriverTaskContract,
    completion: crate::hal::driver_task::DriverTaskCompletionRecord,
) {
    use core::fmt::Write;

    if completion.frame.len != DRIVER_RUNTIME_USB_HUB_PORT_STATUS_FRAME_LEN {
        return;
    }
    let Some(bytes) =
        crate::hal::driver_task::driver_task_ring_frame_bytes(contract, completion.frame)
    else {
        return;
    };
    if bytes.len() < usize::from(DRIVER_RUNTIME_USB_HUB_PORT_STATUS_FRAME_LEN)
        || usb_trace_u32(bytes, 0) != DRIVER_RUNTIME_USB_HUB_PORT_STATUS_FRAME_MAGIC
        || bytes[4] != DRIVER_RUNTIME_USB_HUB_PORT_STATUS_FRAME_VERSION
    {
        return;
    }

    let stage = bytes[5];
    let hub_slot = bytes[6];
    let hub_port = bytes[7];
    let w_index = usb_trace_u16(bytes, 8);
    let depth = bytes[10];
    let settle_ms = bytes[11];
    let status = usb_trace_u16(bytes, 12);
    let change = usb_trace_u16(bytes, 14);
    let clear_mask = usb_trace_u16(bytes, 16);
    let flags = usb_trace_u16(bytes, 18);
    let route = usb_trace_u32(bytes, 20);
    let progress = crate::hal::driver_task::latest_driver_task_ring_progress(contract);
    let mut line = heapless::String::<640>::new();
    let _ = write!(
        line,
        "USB_HUB_PORT_TRACE contract={} detail=0x{:04x} result=0x{:08x} stage={} stage_code={} hub_slot={} hub_port={} wIndex=0x{:04x} depth={} route=0x{:05x} settle_ms={} wPortStatus=0x{:04x} wPortChange=0x{:04x} clear_mask=0x{:04x} flags=0x{:04x} connected={} enabled={} reset={} speed={} c_connection={} c_enable={} c_reset={} marker_sequence={} marker_phase={} marker_phase_name={} marker_aux0=0x{:08x} source=linked-runtime",
        contract.name,
        completion.detail,
        completion.result,
        usb_hub_port_trace_stage_label(stage),
        stage,
        hub_slot,
        hub_port,
        w_index,
        depth,
        route,
        settle_ms,
        status,
        change,
        clear_mask,
        flags,
        local_seat_yes_no(status & USB_HUB_STATUS_CONNECTION != 0),
        local_seat_yes_no(status & USB_HUB_STATUS_ENABLE != 0),
        local_seat_yes_no(status & USB_HUB_STATUS_RESET != 0),
        usb_hub_port_trace_speed(status),
        local_seat_yes_no(change & USB_HUB_CHANGE_CONNECTION != 0),
        local_seat_yes_no(change & USB_HUB_CHANGE_ENABLE != 0),
        local_seat_yes_no(change & USB_HUB_CHANGE_RESET != 0),
        progress.map_or(0, |progress| progress.sequence),
        progress.map_or(0, |progress| progress.phase),
        progress.map_or("none", |progress| progress.phase_name),
        progress.map_or(0, |progress| progress.aux0),
    );
    boot_log::force_uart_line(line.as_str());
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
    active_request: bool,
) -> bool {
    usb_enumeration_progress_token_advanced(previous, current)
        || (active_request && current.is_some())
}

#[cfg(all(feature = "kernel", feature = "usb"))]
const fn linked_local_seat_usb_enum_resume_attempts(root_console_ready: bool) -> usize {
    if root_console_ready {
        LINKED_LOCAL_SEAT_USB_ENUM_RESUME_ATTEMPTS
    } else {
        LINKED_LOCAL_SEAT_USB_COLD_BOOT_ENUM_RESUME_ATTEMPTS
    }
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn linked_local_seat_usb_enum_no_reply_should_continue(
    contract: crate::hal::driver_task::DriverTaskContract,
    root_console_ready: bool,
) -> bool {
    !root_console_ready && crate::hal::driver_task::driver_task_ring_command_active(contract)
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
    emit_linked_local_seat_usb_hub_port_trace(contract, completion);
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
    let no_reply = completion.is_none() && !ready;
    if no_reply {
        let repeat = LINKED_LOCAL_SEAT_DISPLAY_NO_REPLY_LOG_COUNT.fetch_add(1, Ordering::AcqRel);
        if !fatal && !repeated_no_reply_log_visible(repeat) {
            return;
        }
    } else {
        LINKED_LOCAL_SEAT_DISPLAY_NO_REPLY_LOG_COUNT.store(0, Ordering::Release);
    }
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
            for _ in 0..linked_local_seat_usb_enum_resume_attempts(root_console_ready) {
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
                    if linked_local_seat_usb_enum_no_reply_should_continue(
                        usb_contract,
                        root_console_ready,
                    ) {
                        continue;
                    }
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
#[must_use]
pub(crate) fn linked_local_seat_usb_first_byte_ready() -> bool {
    LINKED_LOCAL_SEAT_USB_FIRST_BYTE_READY_LOGGED.load(Ordering::Acquire)
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

fn linked_hdmi_snapshot_geometry() -> LinkedHdmiSnapshotGeometry {
    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    {
        if let Some(framebuffer) = crate::hal::driver_task::hdmi_runtime_framebuffer_hint() {
            return linked_hdmi_snapshot_geometry_for_framebuffer(
                framebuffer.width as usize,
                framebuffer.height as usize,
            );
        }
    }
    LinkedHdmiSnapshotGeometry::fallback()
}

fn linked_hdmi_snapshot_geometry_for_framebuffer(
    width: usize,
    height: usize,
) -> LinkedHdmiSnapshotGeometry {
    let (_, safe_width) = linked_hdmi_safe_axis(width, LINKED_LOCAL_SEAT_HDMI_CHAR_WIDTH);
    let (_, safe_height) = linked_hdmi_safe_axis(height, LINKED_LOCAL_SEAT_HDMI_CHAR_HEIGHT);
    LinkedHdmiSnapshotGeometry {
        cols: safe_width
            .checked_div(LINKED_LOCAL_SEAT_HDMI_CHAR_WIDTH)
            .unwrap_or(0)
            .max(1),
        rows: safe_height
            .checked_div(LINKED_LOCAL_SEAT_HDMI_CHAR_HEIGHT)
            .unwrap_or(0)
            .max(1),
    }
}

fn linked_hdmi_safe_axis(total: usize, cell: usize) -> (usize, usize) {
    if total == 0 {
        return (0, 0);
    }
    if total <= cell {
        return (0, total);
    }
    let min_visible = cell.min(total);
    let desired_margin = (total / LINKED_LOCAL_SEAT_HDMI_SAFE_AREA_MARGIN_DIVISOR).max(1);
    let max_margin = total.saturating_sub(min_visible) / 2;
    let margin = desired_margin.min(max_margin);
    let available = total.saturating_sub(margin.saturating_mul(2));
    let cells = (available / cell).max(1);
    let aligned = cells.saturating_mul(cell).min(available);
    let offset = margin.saturating_add(available.saturating_sub(aligned) / 2);
    (offset, aligned)
}

fn linked_hdmi_snapshot_width_u16(width: usize) -> u16 {
    width.min(u16::MAX as usize) as u16
}

fn linked_hdmi_snapshot_line_width(line_bytes: u16, geometry: LinkedHdmiSnapshotGeometry) -> usize {
    usize::from(line_bytes).min(geometry.cols).max(1)
}

fn linked_hdmi_scrollback_visible_lines(buffer_lines: u16, line_bytes: u16) -> usize {
    linked_hdmi_scrollback_visible_lines_for_geometry(
        buffer_lines,
        line_bytes,
        linked_hdmi_snapshot_geometry(),
    )
}

fn linked_hdmi_scrollback_visible_lines_for_geometry(
    buffer_lines: u16,
    _line_bytes: u16,
    geometry: LinkedHdmiSnapshotGeometry,
) -> usize {
    usize::from(buffer_lines).min(geometry.rows).max(1)
}

const fn local_seat_hdmi_input_echo_byte(byte: u8) -> bool {
    matches!(byte, b'\n' | b'\r' | b'\t' | 0x08 | 0x7f) || !byte.is_ascii_control()
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

fn update_input_echo_preview(
    preview: &mut String,
    escape_state: &mut u8,
    byte: u8,
    max_bytes: usize,
) -> bool {
    match *escape_state {
        0 if byte == 0x1b => {
            *escape_state = 1;
            false
        }
        0 => match byte {
            b'\r' | b'\n' => {
                let changed = !preview.is_empty();
                preview.clear();
                changed
            }
            0x08 | 0x7f => preview.pop().is_some(),
            byte if byte.is_ascii_control() => false,
            byte => {
                if preview.len() < max_bytes {
                    preview.push(byte as char);
                    true
                } else {
                    false
                }
            }
        },
        1 if byte == b'[' => {
            *escape_state = 2;
            false
        }
        1 => {
            *escape_state = 0;
            false
        }
        2 if byte.is_ascii_digit() || byte == b';' => false,
        2 if (0x40..=0x7e).contains(&byte) => {
            *escape_state = 0;
            false
        }
        2 => false,
        _ => {
            *escape_state = 0;
            false
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

    #[test]
    fn runtime_output_drain_consumes_only_complete_scrollback_arrows() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 4,
        });

        assert_eq!(runtime.enqueue_keyboard_bytes(b"\x1b[A\x1b[B\x1b[Bx"), 10);
        assert_eq!(runtime.drain_display_control_bytes_during_output(9), 9);
        let trace = runtime.keyboard_trace();
        assert_eq!(trace.queued_bytes, 1);
        assert_eq!(trace.drained_bytes, 9);
        assert_eq!(trace.echoed_bytes, 9);

        let mut remaining = [0u8; 4];
        assert_eq!(runtime.drain_keyboard_bytes(&mut remaining), 1);
        assert_eq!(remaining[0], b'x');

        assert_eq!(runtime.enqueue_keyboard_bytes(b"\x1b[Bx"), 4);
        assert_eq!(runtime.drain_display_control_bytes_during_output(3), 3);
        let trace = runtime.keyboard_trace();
        assert_eq!(trace.queued_bytes, 1);
        assert_eq!(trace.drained_bytes, 13);
        assert_eq!(trace.echoed_bytes, 12);

        assert_eq!(runtime.drain_keyboard_bytes(&mut remaining), 1);
        assert_eq!(remaining[0], b'x');

        assert_eq!(runtime.enqueue_keyboard_bytes(b"\x1b["), 2);
        assert_eq!(runtime.drain_display_control_bytes_during_output(3), 0);
        assert_eq!(runtime.keyboard_trace().queued_bytes, 2);
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

    #[cfg(feature = "kernel")]
    #[test]
    fn linked_hdmi_frame_chunks_stay_within_display_contract() {
        let contract = crate::hal::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT;
        let submit_chunk = LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES
            .min(crate::hal::driver_task::MAX_DRIVER_TASK_FRAME_BYTES);

        assert_eq!(contract.name, "hdmi-text");
        assert!(submit_chunk <= contract.budget.max_bytes_per_turn as usize);
        assert!(submit_chunk <= crate::hal::driver_task::MAX_DRIVER_TASK_FRAME_BYTES);
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
    fn runtime_hdmi_scrollback_delta_clamps_to_history_window() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 40,
        });
        for _ in 0..40 {
            runtime.mirror_line("line");
        }

        assert_eq!(runtime.max_linked_hdmi_scrollback_offset(), 12);
        assert!(runtime.apply_linked_hdmi_scroll_delta(3));
        assert_eq!(runtime.hdmi_scrollback_offset, 3);
        assert!(runtime.apply_linked_hdmi_scroll_delta(99));
        assert_eq!(runtime.hdmi_scrollback_offset, 12);
        assert!(!runtime.apply_linked_hdmi_scroll_delta(1));
        assert_eq!(runtime.hdmi_scrollback_offset, 12);
        assert!(runtime.apply_linked_hdmi_scroll_delta(-2));
        assert_eq!(runtime.hdmi_scrollback_offset, 10);
        assert!(runtime.apply_linked_hdmi_scroll_delta(-99));
        assert_eq!(runtime.hdmi_scrollback_offset, 0);
        assert!(!runtime.apply_linked_hdmi_scroll_delta(-1));
    }

    #[test]
    fn linked_hdmi_scrollback_viewport_uses_safe_area_rows() {
        let fallback = LinkedHdmiSnapshotGeometry::fallback();
        assert_eq!(linked_hdmi_snapshot_line_width(160, fallback), 77);
        assert_eq!(linked_hdmi_scrollback_visible_lines(128, 160), 28);
        assert!(
            linked_hdmi_scrollback_visible_lines(128, 160)
                .saturating_mul(
                    linked_hdmi_snapshot_line_width(160, fallback)
                        .saturating_add(LINKED_LOCAL_SEAT_HDMI_CLEAR_EOL_BYTES)
                        .saturating_add(1),
                )
                .saturating_add(
                    LINKED_LOCAL_SEAT_HDMI_CURSOR_HOME_BYTES
                        .saturating_add(LINKED_LOCAL_SEAT_HDMI_CLEAR_TO_END_BYTES),
                )
                <= LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES
        );
    }

    #[test]
    fn linked_hdmi_snapshot_geometry_matches_wide_framebuffer_hint() {
        let geometry = linked_hdmi_snapshot_geometry_for_framebuffer(1824, 984);
        assert_eq!(
            geometry,
            LinkedHdmiSnapshotGeometry {
                cols: 219,
                rows: 59
            }
        );
        assert_eq!(linked_hdmi_snapshot_line_width(160, geometry), 160);
        assert_eq!(
            linked_hdmi_scrollback_visible_lines_for_geometry(128, 160, geometry),
            59
        );
    }

    #[test]
    fn runtime_hdmi_wide_snapshot_redraw_chunks_full_viewport() {
        let geometry = linked_hdmi_snapshot_geometry_for_framebuffer(1824, 984);
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 160,
            buffer_lines: 128,
        });
        let long_line = "x".repeat(160);
        for _ in 0..80 {
            runtime.mirror_line_current_tcb(long_line.as_str());
        }

        let payload = runtime.build_linked_hdmi_scrollback_payload_for_geometry(geometry);

        assert!(payload.starts_with(b"\x1b[H"));
        assert!(payload.ends_with(b"\x1b[J"));
        assert_eq!(payload.iter().filter(|&&byte| byte == b'\n').count(), 58);
        assert!(payload.len() > LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES);
        assert!(!payload.contains(&0x0c));

        for byte in payload.iter().copied() {
            runtime.hdmi_redraw_bytes.push_back(byte);
        }
        let mut chunk_count = 0usize;
        let mut submitted_bytes = 0usize;
        while let Some((chunk, reason, redraw)) = runtime.next_linked_hdmi_payload() {
            assert_eq!(reason, "keyboard-scrollback");
            assert!(redraw);
            assert!(chunk.len() <= LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES);
            chunk_count = chunk_count.saturating_add(1);
            submitted_bytes = submitted_bytes.saturating_add(chunk.len());
        }

        assert!(chunk_count > 1);
        assert_eq!(submitted_bytes, payload.len());
        assert!(!runtime.linked_hdmi_pending_work());
    }

    #[test]
    fn runtime_hdmi_snapshot_redraw_rehomes_without_form_feed_clear() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 2,
        });

        runtime.mirror_line_current_tcb("cohesix> ");
        assert!(runtime.queue_linked_hdmi_payload(b"stale echo"));
        runtime.request_linked_hdmi_snapshot_redraw();
        runtime.request_linked_hdmi_snapshot_redraw();

        let display = runtime.display_trace();
        assert_eq!(display.pending_bytes, 0);
        assert!(display.pending_redraw);
        assert_eq!(display.coalesced_redraws, 1);
        assert_eq!(display.superseded_bytes, b"stale echo".len() as u64);
        assert_eq!(display.backpressure_bytes, 0);
        assert_eq!(runtime.keyboard_trace().driver_task_budget_overruns, 0);

        let Some((payload, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected snapshot payload");
        };
        assert_eq!(reason, "keyboard-scrollback");
        assert!(redraw);
        assert!(!payload.contains(&0x0c));
        assert_eq!(payload.as_slice(), b"\x1b[Hcohesix> \x1b[K\x1b[J");
    }

    #[test]
    fn runtime_hdmi_snapshot_includes_input_preview_on_tail_line() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 1,
        });

        runtime.mirror_line_current_tcb("cohesix> ");
        runtime.echo_input_bytes(b"hel");
        runtime.request_linked_hdmi_snapshot_redraw();

        let Some((payload, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected snapshot payload");
        };
        assert_eq!(reason, "keyboard-scrollback");
        assert!(redraw);
        assert_eq!(payload.as_slice(), b"\x1b[Hcohesix> hel\x1b[K\x1b[J");
    }

    #[test]
    fn runtime_prompt_payload_does_not_force_newline() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 4,
        });

        assert!(runtime.queue_linked_hdmi_prompt("cohesix> "));

        let Some((payload, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected prompt payload");
        };
        assert_eq!(payload.as_slice(), b"cohesix> ");
        assert_eq!(reason, "queued-output");
        assert!(!redraw);
    }

    #[test]
    fn runtime_output_closes_open_prompt_before_line() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 4,
        });

        runtime.mirror_line_current_tcb("cohesix> ");
        runtime.open_linked_hdmi_prompt_line("cohesix> ");
        assert!(runtime.close_linked_hdmi_open_line(true));
        assert!(runtime.queue_linked_hdmi_line("PONG"));

        let Some((payload, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected queued prompt-close payload");
        };
        assert_eq!(payload.as_slice(), b"\nPONG\n");
        assert_eq!(reason, "queued-output");
        assert!(!redraw);
    }

    #[test]
    fn runtime_input_echo_commits_command_line_for_snapshots() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 4,
        });

        runtime.mirror_line_current_tcb("cohesix> ");
        runtime.open_linked_hdmi_prompt_line("cohesix> ");
        for &byte in b"help\n" {
            runtime.record_linked_hdmi_input_echo_byte(byte);
        }
        runtime.request_linked_hdmi_snapshot_redraw();

        let Some((payload, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected command-line snapshot");
        };
        assert_eq!(reason, "keyboard-scrollback");
        assert!(redraw);
        assert_eq!(payload.as_slice(), b"\x1b[Hcohesix> help\x1b[K\x1b[J");
    }

    #[test]
    fn runtime_hdmi_snapshot_redraw_uses_full_physical_viewport() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 160,
            buffer_lines: 128,
        });
        for index in 0..64 {
            let mut line = String::from("line-");
            let _ = core::fmt::Write::write_fmt(&mut line, format_args!("{index:02}"));
            runtime.mirror_line_current_tcb(line.as_str());
        }

        runtime.request_linked_hdmi_snapshot_redraw();

        let Some((payload, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected snapshot payload");
        };
        assert_eq!(reason, "keyboard-scrollback");
        assert!(redraw);
        assert!(payload.starts_with(b"\x1b[H"));
        assert!(payload.ends_with(b"\x1b[J"));
        assert_eq!(payload.iter().filter(|&&byte| byte == b'\n').count(), 27);
        assert!(payload.windows(3).any(|window| window == b"\x1b[K"));
        assert!(!payload.contains(&0x0c));
        assert!(payload.len() <= LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES);
    }

    #[test]
    fn runtime_scrollback_offset_stays_anchored_when_output_arrives() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 40,
        });
        for _ in 0..40 {
            runtime.mirror_line_current_tcb("line");
        }
        assert!(runtime.apply_linked_hdmi_scroll_delta(3));

        runtime.mirror_line_current_tcb("new-tail");

        assert_eq!(runtime.hdmi_scrollback_offset, 4);
    }

    #[test]
    fn runtime_input_preview_ignores_split_arrow_sequences() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 32,
            buffer_lines: 40,
        });

        runtime.echo_input_bytes(b"\x1b");
        runtime.echo_input_bytes(b"[Ahe");
        runtime.echo_input_bytes(b"\x1b[B");
        runtime.echo_input_bytes(b"\x1b[1~");
        runtime.echo_input_bytes(b"lp");

        assert_eq!(runtime.input_echo_preview, "help");
    }

    #[test]
    fn runtime_keyboard_burst_drains_and_echoes_without_drops() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 64,
            buffer_lines: 40,
        });
        let mut burst = Vec::new();
        for index in 0..256 {
            if index % 16 == 0 {
                burst.extend_from_slice(b"\x1b[A");
            }
            burst.push(b'a' + (index % 26) as u8);
        }

        assert_eq!(
            runtime.enqueue_keyboard_bytes(burst.as_slice()),
            burst.len()
        );
        let mut drained_total = 0usize;
        let mut chunk = [0u8; KEYBOARD_POLL_CHUNK_BYTES];
        loop {
            let read = runtime.drain_keyboard_bytes(&mut chunk);
            if read == 0 {
                break;
            }
            runtime.echo_input_bytes(&chunk[..read]);
            drained_total = drained_total.saturating_add(read);
        }

        let trace = runtime.keyboard_trace();
        assert_eq!(drained_total, burst.len());
        assert_eq!(trace.accepted_bytes, burst.len() as u64);
        assert_eq!(trace.drained_bytes, burst.len() as u64);
        assert_eq!(trace.echoed_bytes, burst.len() as u64);
        assert_eq!(trace.dropped_bytes, 0);
        assert!(!runtime.input_echo_preview.contains('['));
    }

    #[test]
    fn runtime_display_control_drain_handles_repeated_arrows_without_text_preview() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 64,
            buffer_lines: 40,
        });
        let mut controls = Vec::new();
        for _ in 0..32 {
            controls.extend_from_slice(b"\x1b[A");
            controls.extend_from_slice(b"\x1b[B");
        }
        controls.push(b'x');

        assert_eq!(
            runtime.enqueue_keyboard_bytes(controls.as_slice()),
            controls.len()
        );
        assert_eq!(
            runtime.drain_display_control_bytes_during_output(KEYBOARD_POLL_CHUNK_BYTES),
            126
        );
        assert_eq!(
            runtime.drain_display_control_bytes_during_output(KEYBOARD_POLL_CHUNK_BYTES),
            66
        );

        let trace = runtime.keyboard_trace();
        assert_eq!(trace.drained_bytes, 192);
        assert_eq!(trace.echoed_bytes, 192);
        assert_eq!(trace.queued_bytes, 1);
        assert!(runtime.input_echo_preview.is_empty());

        let mut remaining = [0u8; 1];
        assert_eq!(runtime.drain_keyboard_bytes(&mut remaining), 1);
        runtime.echo_input_bytes(&remaining);
        assert_eq!(runtime.input_echo_preview, "x");
    }

    #[test]
    fn runtime_hdmi_no_reply_coalesces_queued_output_to_snapshot_redraw() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 4,
        });
        let payload = vec![b'x'; LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES];

        runtime.mirror_line_current_tcb("cohesix> ");
        assert!(runtime.queue_linked_hdmi_payload(payload.as_slice()));
        let Some((submitted, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected queued payload");
        };
        assert_eq!(submitted.len(), LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES);
        assert_eq!(reason, "queued-output");
        assert!(!redraw);
        assert_eq!(runtime.display_trace().pending_bytes, 0);

        runtime.record_linked_hdmi_submit_miss(submitted.len(), redraw);

        let display = runtime.display_trace();
        assert_eq!(display.pending_bytes, 0);
        assert!(display.pending_redraw);
        assert_eq!(display.no_reply_frames, 1);
        assert_eq!(display.deferred_frames, 1);
        assert_eq!(
            display.superseded_bytes,
            LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES as u64
        );
        let Some((snapshot, snapshot_reason, snapshot_redraw)) = runtime.next_linked_hdmi_payload()
        else {
            panic!("expected recovery snapshot");
        };
        assert_eq!(snapshot_reason, "keyboard-scrollback");
        assert!(snapshot_redraw);
        assert!(snapshot.starts_with(b"\x1b[H"));
    }

    #[test]
    fn runtime_hdmi_line_queue_is_bounded_and_nonblocking() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 4,
        });

        assert!(runtime.queue_linked_hdmi_line("driver frontier"));

        let display = runtime.display_trace();
        assert_eq!(display.pending_bytes, "driver frontier\n".len());
        assert_eq!(runtime.keyboard_trace().driver_task_budget_overruns, 0);
    }

    #[test]
    fn runtime_hdmi_backpressure_does_not_count_as_keyboard_budget() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 4,
        });
        let payload = vec![b'x'; DISPLAY_QUEUE_MAX_BYTES + 32];

        assert!(!runtime.queue_linked_hdmi_payload(&payload));

        let display = runtime.display_trace();
        assert_eq!(display.pending_bytes, 0);
        assert_eq!(display.backpressure_bytes, payload.len() as u64);
        assert_eq!(runtime.keyboard_trace().driver_task_budget_overruns, 0);
    }

    #[test]
    fn runtime_keyboard_no_reply_backoff_is_bounded() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 4,
        });

        runtime.record_keyboard_poll_no_reply();
        assert_eq!(runtime.keyboard_trace().driver_task_no_replies, 1);
        assert_eq!(runtime.keyboard_trace().driver_task_no_reply_streak, 1);
        assert_eq!(
            runtime.keyboard_trace().poll_cooldown_turns,
            LINKED_LOCAL_SEAT_USB_POLL_NO_REPLY_INITIAL_COOLDOWN
        );

        for _ in 0..8 {
            runtime.record_keyboard_poll_no_reply();
        }

        assert_eq!(
            runtime.keyboard_trace().poll_cooldown_turns,
            LINKED_LOCAL_SEAT_USB_POLL_NO_REPLY_MAX_COOLDOWN
        );
        assert_eq!(runtime.keyboard_trace().driver_task_no_replies, 9);
        assert_eq!(runtime.keyboard_trace().driver_task_no_reply_streak, 9);
        runtime.record_keyboard_poll_completion();
        assert_eq!(runtime.keyboard_trace().driver_task_no_reply_streak, 0);

        assert_eq!(local_seat_keyboard_poll_next_no_reply_backoff(true, 0), 1);
        assert_eq!(local_seat_keyboard_poll_next_no_reply_backoff(true, 32), 1);
        assert_eq!(
            local_seat_keyboard_poll_next_no_reply_backoff_for_queue(true, true, 0),
            LINKED_LOCAL_SEAT_USB_POLL_READY_IDLE_COOLDOWN
        );
        assert_eq!(
            local_seat_keyboard_poll_next_no_reply_backoff_for_queue(
                true,
                true,
                LINKED_LOCAL_SEAT_USB_POLL_READY_IDLE_COOLDOWN
            ),
            LINKED_LOCAL_SEAT_USB_POLL_NO_REPLY_MAX_COOLDOWN
        );
    }

    #[test]
    fn runtime_keyboard_recovery_aux_waits_for_root_queue_drain() {
        assert!(!local_seat_keyboard_recovery_aux_allowed(
            false,
            LINKED_LOCAL_SEAT_USB_POST_FIRST_BYTE_RECOVERY_NO_REPLY_THRESHOLD
        ));
        assert!(!local_seat_keyboard_recovery_aux_allowed(
            true,
            LINKED_LOCAL_SEAT_USB_POST_FIRST_BYTE_RECOVERY_NO_REPLY_THRESHOLD - 1
        ));
        assert!(local_seat_keyboard_recovery_aux_allowed(
            true,
            LINKED_LOCAL_SEAT_USB_POST_FIRST_BYTE_RECOVERY_NO_REPLY_THRESHOLD
        ));
        assert!(!local_seat_keyboard_recovery_aux_allowed(
            true,
            LINKED_LOCAL_SEAT_USB_POST_FIRST_BYTE_RECOVERY_NO_REPLY_THRESHOLD + 1
        ));
        assert!(!local_seat_keyboard_recovery_aux_allowed(
            true,
            LINKED_LOCAL_SEAT_USB_POST_FIRST_BYTE_RECOVERY_NO_REPLY_THRESHOLD * 2
        ));
    }

    #[test]
    fn repeated_no_reply_logging_keeps_first_and_power_of_two_samples() {
        assert!(repeated_no_reply_log_visible(0));
        assert!(repeated_no_reply_log_visible(1));
        assert!(repeated_no_reply_log_visible(2));
        assert!(repeated_no_reply_log_visible(3));
        assert!(repeated_no_reply_log_visible(4));
        assert!(!repeated_no_reply_log_visible(5));
        assert!(!repeated_no_reply_log_visible(6));
        assert!(!repeated_no_reply_log_visible(7));
        assert!(repeated_no_reply_log_visible(8));
    }

    #[test]
    fn runtime_keyboard_no_reply_cooldown_skips_backend_submit() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 4,
        });

        runtime.enable_backend_keyboard_polling();
        runtime.record_keyboard_poll_no_reply();
        runtime.poll_backend_keyboard();

        let trace = runtime.keyboard_trace();
        assert_eq!(trace.backend_poll_calls, 0);
        assert_eq!(trace.poll_cooldown_skips, 1);
        assert_eq!(
            trace.poll_cooldown_turns,
            LINKED_LOCAL_SEAT_USB_POLL_NO_REPLY_INITIAL_COOLDOWN - 1
        );
    }

    #[test]
    fn runtime_keyboard_enable_clears_no_reply_cooldown() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 4,
        });

        runtime.record_keyboard_poll_no_reply();
        runtime.enable_backend_keyboard_polling();

        let trace = runtime.keyboard_trace();
        assert_eq!(trace.driver_task_no_replies, 1);
        assert_eq!(trace.poll_cooldown_turns, 0);
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
        assert_eq!(local_seat_keyboard_poll_aux(false, false), 0);
        assert_eq!(local_seat_keyboard_poll_aux(true, false), 0);
        assert_eq!(local_seat_keyboard_poll_aux(false, true), 0);
        assert_eq!(
            local_seat_keyboard_poll_aux(true, true),
            DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_AUX
        );
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
        assert!(LINKED_LOCAL_SEAT_USB_COLD_BOOT_ENUM_RESUME_ATTEMPTS >= 16);
        assert!(LINKED_LOCAL_SEAT_USB_COLD_BOOT_ENUM_RESUME_ATTEMPTS <= 128);
        assert_eq!(linked_local_seat_usb_enum_resume_attempts(true), 3);
        assert_eq!(
            linked_local_seat_usb_enum_resume_attempts(false),
            LINKED_LOCAL_SEAT_USB_COLD_BOOT_ENUM_RESUME_ATTEMPTS
        );
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    #[test]
    fn linked_usb_probe_progress_burst_is_progress_bounded() {
        assert!((1..=16).contains(&LINKED_LOCAL_SEAT_USB_PROBE_STABLE_PROGRESS_BURST_ATTEMPTS));
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
            None, None, true
        ));
        assert!(!usb_enumeration_progress_token_allows_probe_burst(
            Some((8, 190, DRIVER_RUNTIME_USB_ENUMERATE_AUX)),
            Some((8, 190, DRIVER_RUNTIME_USB_ENUMERATE_AUX)),
            false
        ));
        assert!(usb_enumeration_progress_token_allows_probe_burst(
            Some((8, 190, DRIVER_RUNTIME_USB_ENUMERATE_AUX)),
            Some((8, 190, DRIVER_RUNTIME_USB_ENUMERATE_AUX)),
            true
        ));
        assert!(usb_enumeration_progress_token_allows_probe_burst(
            Some((8, 190, DRIVER_RUNTIME_USB_ENUMERATE_AUX)),
            Some((8, 236, DRIVER_RUNTIME_USB_ENUMERATE_AUX)),
            false
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
