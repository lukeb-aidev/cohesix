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
use core::sync::atomic::AtomicBool;
#[cfg(any(
    test,
    all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    )
))]
use core::sync::atomic::{AtomicUsize, Ordering};
#[cfg(all(feature = "kernel", feature = "usb"))]
use pi4_driver_abi::{
    DRIVER_RUNTIME_LOCAL_SEAT_INIT_AUX, DRIVER_RUNTIME_USB_ENUMERATE_AUX,
    DRIVER_RUNTIME_USB_KEYBOARD_FRAME_FLAG_INPUT, DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_AUX,
    DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_DECODED_EMPTY,
    DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_IDLE, DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_NONE,
    DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_NO_IDLE_REPORT,
    DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_QUEUE_COLLAPSE,
    DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_RECOVERY_FAILED,
    DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_RECOVERY_SUCCESS,
    DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_UNMATCHED_TRANSFER,
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
const LOCAL_SEAT_HDMI_PENDING_PROMPT_BYTES: usize = 32;
#[cfg(any(
    test,
    all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    )
))]
const LOCAL_SEAT_HDMI_KEYBOARD_STARTING_LINE: &str = "USB controller starting...";
#[cfg(any(test, all(feature = "kernel", feature = "usb")))]
const LOCAL_SEAT_HDMI_KEYBOARD_WAIT_LINE: &str = "USB console ready";
#[cfg(any(test, all(feature = "kernel", feature = "usb")))]
const LOCAL_SEAT_HDMI_KEYBOARD_WAIT_LINE_BYTES: usize = 96;
#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
const LOCAL_SEAT_HDMI_PROMPT_WAIT_REASON_NONE: &str = "none";
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

#[cfg(any(
    test,
    all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    )
))]
static LINKED_LOCAL_SEAT_DISPLAY_READY_LOG_COUNT: AtomicUsize = AtomicUsize::new(0);

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
static LINKED_LOCAL_SEAT_PCIE_ENGINE_READY_LOGGED: AtomicBool = AtomicBool::new(false);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_PCIE_OWNER_READY_LOGGED: AtomicBool = AtomicBool::new(false);

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
static LINKED_LOCAL_SEAT_USB_LAST_FRAME_OFFSET: AtomicUsize = AtomicUsize::new(0);

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
static LINKED_LOCAL_SEAT_USB_LAST_FRAME_META: AtomicUsize = AtomicUsize::new(0);

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
// Explicit prompt-side probes must remain responsive; long hub/control waits
// continue through the pre-root cold-boot budget and cached progress evidence.
const LINKED_LOCAL_SEAT_USB_PROBE_STABLE_PROGRESS_BURST_ATTEMPTS: usize = 24;
#[cfg(all(feature = "kernel", feature = "usb"))]
// Hub reset and port-change settle can report the same progress token for a
// few nonblocking turns. Keep that prompt-side settle bounded and token-gated.
const LINKED_LOCAL_SEAT_USB_PROBE_STALLED_PROGRESS_BURST_ATTEMPTS: usize = 6;

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
const LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES: usize = 512;

/// Redraw retries after a missed HDMI completion before yielding the mirror.
const LINKED_LOCAL_SEAT_HDMI_REDRAW_NO_REPLY_RETRY_LIMIT: u8 = 2;

/// Quiet turns after a missed HDMI completion before retrying the display path.
///
/// A missing HDMI completion means the display runtime is not currently
/// accepting frame work. Keep the mirrored snapshot queued, but back off long
/// enough that REST/TCP response flushing and USB input do not repeatedly lose
/// root-task service turns to the same no-reply display retry.
const LINKED_LOCAL_SEAT_HDMI_NO_REPLY_COOLDOWN_TURNS: u8 = 64;

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

/// Bytes in `ESC[H`, used to move to the top-left before a snapshot redraw.
const LINKED_LOCAL_SEAT_HDMI_CURSOR_HOME_BYTES: usize = 3;

/// Bytes in `ESC[K`, used to clear stale text to the physical right edge.
const LINKED_LOCAL_SEAT_HDMI_CLEAR_EOL_BYTES: usize = 3;

/// Bytes in `ESC7`/`ESC8`, used to preserve the post-snapshot cursor position.
const LINKED_LOCAL_SEAT_HDMI_SAVE_CURSOR_BYTES: usize = 2;
const LINKED_LOCAL_SEAT_HDMI_RESTORE_CURSOR_BYTES: usize = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LinkedHdmiSnapshotGeometry {
    cols: usize,
    rows: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LinkedHdmiPayloadReceipt {
    None,
    QueuedOutput {
        payload_bytes: usize,
        completes_scroll_fence: bool,
        scroll_fence_line_generation: u64,
        scroll_fence_tail_sequence: u64,
    },
    Redraw {
        final_chunk: bool,
    },
    ScrollStep {
        target_offset: usize,
        mirrored_line_generation: u64,
        mirrored_tail_sequence: u64,
    },
    InputRow {
        generation: u64,
    },
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
#[derive(Debug)]
struct LinkedHdmiRetainedFrame {
    command: crate::hal::driver_task::DriverTaskCommandRecord,
    payload: Vec<u8>,
    reason: &'static str,
    redraw: bool,
    receipt: LinkedHdmiPayloadReceipt,
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

/// Short cooldown after a post-first-byte steady poll misses while the runtime
/// still reports a fully armed, idle interrupt-IN queue.
const LINKED_LOCAL_SEAT_USB_POLL_READY_IDLE_COOLDOWN: u8 = 2;

/// Exactly one interrupt-IN transfer remains active for the keyboard lifetime.
/// Completion retires that transfer before the runtime rearms one successor.
const LINKED_LOCAL_SEAT_USB_ACTIVE_INTERRUPT_QUEUE_DEPTH: u32 = 1;

/// The linked USB runtime's pre-first-report interrupt-IN target.
const LINKED_LOCAL_SEAT_USB_FIRST_REPORT_MIN_QUEUED_REPORTS: u32 =
    LINKED_LOCAL_SEAT_USB_ACTIVE_INTERRUPT_QUEUE_DEPTH;

/// The linked USB runtime's steady interrupt-IN target after first HID proof.
const LINKED_LOCAL_SEAT_USB_READY_MAX_QUEUED_REPORTS: u32 =
    LINKED_LOCAL_SEAT_USB_ACTIVE_INTERRUPT_QUEUE_DEPTH;

/// The single active entry remains the upper bound during recovery.
const LINKED_LOCAL_SEAT_USB_RECOVERY_PROBE_MAX_QUEUED_REPORTS: u32 =
    LINKED_LOCAL_SEAT_USB_ACTIVE_INTERRUPT_QUEUE_DEPTH;

/// Consecutive post-first-byte no-reply polls before asking the runtime to
/// recover the already accepted interrupt-IN endpoint.
const LINKED_LOCAL_SEAT_USB_POST_FIRST_BYTE_RECOVERY_NO_REPLY_THRESHOLD: u64 = 48;

/// A full, idle steady interrupt-IN queue that stops replying after first byte
/// proof is already accepted is a live input stall, so recover sooner.
const LINKED_LOCAL_SEAT_USB_POST_FIRST_BYTE_IDLE_RECOVERY_NO_REPLY_THRESHOLD: u64 = 8;
/// Suppress transient missed keyboard polls from HDMI; they are common during
/// prompt settling and do not prove that local keyboard input is unavailable.
const LINKED_LOCAL_SEAT_USB_VISIBLE_BUSY_NO_REPLY_THRESHOLD: u64 =
    LINKED_LOCAL_SEAT_USB_POST_FIRST_BYTE_IDLE_RECOVERY_NO_REPLY_THRESHOLD;
#[cfg(any(
    test,
    all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    )
))]
const LINKED_LOCAL_SEAT_USB_PROMPT_READY_CLEAN_POLLS: u8 = 2;

/// Repeated raw HDMI frame no-reply logs are throttled after this many misses.
const LINKED_LOCAL_SEAT_HDMI_NO_REPLY_VERBOSE_LIMIT: usize = 4;

/// Initial post-prompt HDMI success row kept before sampling takes over.
const LINKED_LOCAL_SEAT_HDMI_READY_VERBOSE_LIMIT: usize = 1;

/// Sampling interval for routine successful HDMI frames after the prompt.
const LINKED_LOCAL_SEAT_HDMI_READY_SAMPLE_STRIDE: usize = 256;

/// Maximum queued HDMI bytes allowed before suppressing network-origin mirror
/// lines. This keeps interactive cohsh output visible while REST burst replies
/// stop competing with TCP response flushes once the display path is behind.
const LINKED_LOCAL_SEAT_NET_MIRROR_PENDING_BYTE_LIMIT: usize = 1_024;

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

/// Return whether a high-impact status line may consume a linked HDMI turn.
///
/// Before the root console reaches steady state, serial is the authoritative
/// progress surface and HDMI work is limited to its bounded bootstrap banner.
#[must_use]
pub(crate) const fn local_seat_high_impact_hdmi_mirror_allowed(root_console_ready: bool) -> bool {
    root_console_ready
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
/// keyboard polling. Missing replies are recoverable; the bounded no-reply
/// backoff keeps the serial shell responsive while owner-state proof converges.
#[must_use]
pub(crate) const fn local_seat_keyboard_poll_suspends_on_missing_reply(
    physical_pi_owner_state: bool,
    root_console_ready: bool,
    keyboard_ready: bool,
) -> bool {
    let _ = (physical_pi_owner_state, root_console_ready, keyboard_ready);
    false
}

/// Return whether HDMI may show root-owned local-seat feedback.
#[must_use]
pub(crate) const fn local_seat_hdmi_prompt_ready_for_root_state(
    physical_pi_owner_state: bool,
    root_console_ready: bool,
) -> bool {
    !physical_pi_owner_state || root_console_ready
}

/// Return whether HDMI may show the interactive local-seat prompt.
///
/// The prompt is a usability contract: when it is visible, USB command input
/// must already be admitted and the canonical HDMI ready banner must already
/// have completed on the display runtime. Serial remains independently
/// interactive while USB and HDMI
/// converge.
#[must_use]
pub(crate) const fn local_seat_hdmi_prompt_ready_for_display_state(
    physical_pi_owner_state: bool,
    root_console_ready: bool,
    usb_command_ready: bool,
    console_ready_line_completed: bool,
    _display_idle: bool,
    display_retry_idle: bool,
) -> bool {
    local_seat_hdmi_prompt_ready_for_root_state(physical_pi_owner_state, root_console_ready)
        && (!physical_pi_owner_state
            || (usb_command_ready && console_ready_line_completed && display_retry_idle))
}

/// Return whether a deferred HDMI prompt may be released to the operator.
#[must_use]
pub(crate) const fn local_seat_hdmi_prompt_release_ready(
    display_ready: bool,
    usb_command_ready: bool,
) -> bool {
    display_ready && usb_command_ready
}

/// Return whether the HDMI root prompt should show the pre-HID startup notice.
#[must_use]
pub(crate) const fn local_seat_hdmi_keyboard_starting_line_due(
    usb_keyboard_ready: bool,
    usb_first_byte_ready: bool,
    starting_line_emitted: bool,
) -> bool {
    !usb_keyboard_ready && !usb_first_byte_ready && !starting_line_emitted
}

/// Return whether a deferred HDMI prompt should print a separate USB-ready line.
/// The prompt itself is the readiness signal; Cohesix never asks the operator
/// to press a sacrificial key before command input.
#[must_use]
pub(crate) const fn local_seat_hdmi_keyboard_wait_line_due(
    usb_keyboard_ready: bool,
    usb_first_byte_ready: bool,
    wait_line_emitted: bool,
) -> bool {
    let _ = (usb_keyboard_ready, usb_first_byte_ready, wait_line_emitted);
    false
}

/// Return whether a USB-ready line should supersede an earlier recovery note.
#[must_use]
pub(crate) const fn local_seat_hdmi_keyboard_ready_line_supersedes_busy(
    command_ready: bool,
    busy_line_emitted: bool,
    wait_line_emitted: bool,
) -> bool {
    let _ = (command_ready, busy_line_emitted, wait_line_emitted);
    false
}

pub(crate) const CYW43_GATE8_READY_HDMI_LINE: &str =
    "[drivers] WiFi Gate 8 stable; DHCP and TCP continuing";
pub(crate) const CYW43_READY_TO_USE_HDMI_PREFIX: &str = "[drivers] WiFi ready to use:";

fn local_seat_cyw43_stale_ready_line(line: &str) -> bool {
    line.starts_with("[drivers] WiFi Gate 8 stable;")
        || line.starts_with(CYW43_READY_TO_USE_HDMI_PREFIX)
}

/// Return whether HDMI may publish the final interactive-console banner.
///
/// This is a local-console contract, not a network-readiness claim. The banner
/// becomes due after root and USB command admission even while deferred Wi-Fi
/// bootstrap continues. The interactive prompt separately waits for the exact
/// banner frame completion and current display health.
#[must_use]
pub(crate) const fn local_seat_hdmi_console_ready_line_due(
    root_console_ready: bool,
    usb_command_ready: bool,
    ready_line_emitted: bool,
) -> bool {
    root_console_ready && usb_command_ready && !ready_line_emitted
}

/// Return whether an attached HDMI runtime still needs its first snapshot.
#[must_use]
pub(crate) const fn local_seat_hdmi_initial_snapshot_due(
    display_ready: bool,
    snapshot_scheduled: bool,
) -> bool {
    display_ready && !snapshot_scheduled
}

/// Return whether USB input may be echoed onto the interactive HDMI row.
///
/// Once the root console accepts keyboard input, the prompt and typed
/// characters remain visible during Wi-Fi startup, quarantine, and terminal
/// failure. Network state does not gate this local-console surface.
#[must_use]
pub(crate) const fn local_seat_hdmi_input_echo_allowed(root_console_ready: bool) -> bool {
    root_console_ready
}

/// Return whether linked-runtime USB keyboard bytes may enter the shared parser.
///
/// On physical Pi, typed linked-runtime keyboard frames enter the parser as soon
/// as the root console is ready and the USB keyboard endpoint has been admitted.
#[must_use]
pub(crate) const fn local_seat_keyboard_bytes_enter_parser_state(
    physical_pi_owner_state: bool,
    root_console_ready: bool,
    usb_keyboard_ready: bool,
) -> bool {
    !physical_pi_owner_state || (root_console_ready && usb_keyboard_ready)
}

/// Return how many terminal bytes belong to the one operator arming key.
#[must_use]
#[cfg_attr(not(test), allow(dead_code))]
const fn keyboard_arming_sequence_len(bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }
    if bytes[0] != 0x1b || bytes.len() == 1 {
        return 1;
    }
    if bytes[1] == b'O' {
        return if bytes.len() < 3 { bytes.len() } else { 3 };
    }
    if bytes[1] != b'[' {
        return 1;
    }

    let mut index = 2usize;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte >= 0x40 && byte <= 0x7e {
            return index + 1;
        }
        if index >= 7 {
            return 8;
        }
        index += 1;
    }
    bytes.len()
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
    if steady_queue_idle {
        LINKED_LOCAL_SEAT_USB_POLL_READY_IDLE_COOLDOWN
    } else if keyboard_ready {
        1
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

/// Return the user-visible cooldown after a successful idle keyboard poll.
#[must_use]
pub(crate) const fn local_seat_keyboard_poll_idle_completion_cooldown(
    _steady_queue_idle: bool,
) -> u8 {
    0
}

/// Return whether a keyboard-poll cooldown should be visible as USB busy.
#[must_use]
pub(crate) const fn local_seat_keyboard_cooldown_should_show_busy(
    command_ready: bool,
    no_reply_streak: u64,
    recovery_pending: bool,
) -> bool {
    command_ready
        && (recovery_pending
            || no_reply_streak >= LINKED_LOCAL_SEAT_USB_VISIBLE_BUSY_NO_REPLY_THRESHOLD)
}

/// Return whether HDMI should show the USB-console busy line.
#[must_use]
pub(crate) const fn local_seat_hdmi_keyboard_busy_line_warranted_state(
    command_ready: bool,
    prompt_hard_blocked: bool,
    keyboard_poll_reason: bool,
    no_reply_streak: u64,
    recovery_pending: bool,
) -> bool {
    if !command_ready || !prompt_hard_blocked {
        return false;
    }
    if keyboard_poll_reason {
        return local_seat_keyboard_cooldown_should_show_busy(
            true,
            no_reply_streak,
            recovery_pending,
        );
    }
    true
}

/// Return whether keyboard polling should be treated as prompt-visible recovery.
#[must_use]
pub(crate) const fn local_seat_keyboard_prompt_recovery_pending(
    command_ready: bool,
    aux_recovery_pending: bool,
    no_reply_streak: u64,
    hard_block_threshold: u64,
) -> bool {
    aux_recovery_pending || (!command_ready && no_reply_streak >= hard_block_threshold)
}

/// Return whether the `count`th repeat of a no-reply diagnostic should still be
/// logged to the raw UART.
#[must_use]
const fn repeated_no_reply_log_visible(count: usize) -> bool {
    count < LINKED_LOCAL_SEAT_HDMI_NO_REPLY_VERBOSE_LIMIT
        || (count != 0 && (count & count.saturating_sub(1)) == 0)
}

/// Return whether a visible no-reply HDMI diagnostic should include the
/// detailed ring/queue/progress rows instead of just the submit summary.
#[must_use]
const fn repeated_no_reply_detail_log_visible(count: usize) -> bool {
    count == 0 || (count >= 64 && (count & count.saturating_sub(1)) == 0)
}

#[must_use]
fn keyboard_recovery_request_log_visible(count: usize, action: &str) -> bool {
    if action == "no-reply" {
        return count <= 1;
    }
    count <= 2 || (count >= 256 && (count & count.saturating_sub(1)) == 0)
}

#[cfg(any(
    test,
    all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    )
))]
/// Return the visible HDMI chunk count for a submitted burst snapshot.
#[must_use]
const fn linked_hdmi_reported_chunk_count(
    bytes_len: usize,
    remaining_bytes: usize,
    chunk_limit: usize,
    minimum_chunk_count: usize,
) -> usize {
    let safe_limit = if chunk_limit == 0 { 1 } else { chunk_limit };
    let burst_bytes = bytes_len.saturating_add(remaining_bytes);
    let burst_chunk_count = burst_bytes.saturating_add(safe_limit - 1) / safe_limit;
    if burst_chunk_count < minimum_chunk_count {
        minimum_chunk_count
    } else if burst_chunk_count == 0 {
        1
    } else {
        burst_chunk_count
    }
}

#[cfg(any(
    test,
    all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    )
))]
fn routine_hdmi_ready_log_visible(
    reason: &'static str,
    chunk_redraw: bool,
    display_trace: Option<LocalSeatDisplayTrace>,
    root_console_ready: bool,
) -> bool {
    if matches!(reason, "driver-resource-progress") {
        return false;
    }
    if !root_console_ready || !matches!(reason, "queued-output" | "keyboard-scrollback") {
        return true;
    }
    if chunk_redraw {
        if let Some(trace) = display_trace {
            if trace.redraw_no_reply_streak != 0
                || trace.stale_after_retry_exhaustion
                || trace.backpressure_bytes != 0
            {
                return true;
            }
        }
    }
    let count = LINKED_LOCAL_SEAT_DISPLAY_READY_LOG_COUNT
        .fetch_add(1, Ordering::AcqRel)
        .wrapping_add(1);
    count <= LINKED_LOCAL_SEAT_HDMI_READY_VERBOSE_LIMIT
        || count.is_multiple_of(LINKED_LOCAL_SEAT_HDMI_READY_SAMPLE_STRIDE)
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
fn local_seat_hdmi_keyboard_wait_line_payload(
    direct_submit_needs_newline: bool,
) -> heapless::String<LOCAL_SEAT_HDMI_KEYBOARD_WAIT_LINE_BYTES> {
    let mut line = heapless::String::<LOCAL_SEAT_HDMI_KEYBOARD_WAIT_LINE_BYTES>::new();
    let _ = line.push_str(LOCAL_SEAT_HDMI_KEYBOARD_WAIT_LINE);
    if direct_submit_needs_newline {
        let _ = line.push('\n');
    }
    line
}

#[cfg(all(feature = "kernel", feature = "usb"))]
const fn local_seat_keyboard_poll_aux(
    keyboard_ready: bool,
    enumeration_pending: bool,
    request_recovery: bool,
) -> u32 {
    if request_recovery {
        DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_AUX
    } else if !keyboard_ready && enumeration_pending {
        DRIVER_RUNTIME_USB_ENUMERATE_AUX
    } else {
        0
    }
}

#[cfg(all(feature = "kernel", feature = "usb"))]
fn linked_usb_keyboard_input_frame_valid(
    completion: crate::hal::driver_task::DriverTaskCompletionRecord,
) -> bool {
    completion.code == crate::hal::driver_task::DriverTaskCompletionCode::FrameReady.as_u16()
        && completion.detail == crate::hal::driver_task::DriverTaskFaultCode::None.as_u16()
        && completion.frame.len != 0
        && completion.result == u32::from(completion.frame.len)
        && completion.frame.flags == DRIVER_RUNTIME_USB_KEYBOARD_FRAME_FLAG_INPUT
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn emit_linked_usb_keyboard_frame_rejected(
    contract: DriverTaskContract,
    completion: crate::hal::driver_task::DriverTaskCompletionRecord,
) {
    let mut line = heapless::String::<192>::new();
    let _ = core::fmt::Write::write_fmt(
        &mut line,
        format_args!(
            "DRIVER_TASK_FRAME_REJECTED contract={} hot_path=usb-keyboard reason=invalid-input-frame code={} detail={} result={} frame_len={} frame_flags=0x{:04x}",
            contract.name,
            completion.code,
            completion.detail,
            completion.result,
            completion.frame.len,
            completion.frame.flags,
        ),
    );
    boot_log::force_log_buffer_line_or_uart_without_prompt_refresh(line.as_str());
}

const fn local_seat_keyboard_recovery_aux_allowed(
    queue_empty: bool,
    no_reply_streak: u64,
    recovery_pending: bool,
) -> bool {
    local_seat_keyboard_recovery_aux_allowed_for_status(
        queue_empty,
        no_reply_streak,
        recovery_pending,
        false,
        false,
        false,
        false,
    )
}

const fn local_seat_keyboard_recovery_aux_allowed_for_status(
    queue_empty: bool,
    no_reply_streak: u64,
    recovery_pending: bool,
    cached_unmatched_transfer: bool,
    cached_recovery_required: bool,
    cached_pre_first_report_full_no_event: bool,
    cached_full_idle_runtime_queue: bool,
) -> bool {
    if recovery_pending {
        return false;
    }
    if cached_unmatched_transfer || cached_recovery_required {
        return true;
    }
    if !queue_empty {
        return false;
    }
    if cached_pre_first_report_full_no_event {
        return true;
    }
    let threshold = if cached_full_idle_runtime_queue {
        LINKED_LOCAL_SEAT_USB_POST_FIRST_BYTE_IDLE_RECOVERY_NO_REPLY_THRESHOLD
    } else {
        LINKED_LOCAL_SEAT_USB_POST_FIRST_BYTE_RECOVERY_NO_REPLY_THRESHOLD
    };
    no_reply_streak >= threshold && no_reply_streak.is_multiple_of(threshold)
}

const fn local_seat_keyboard_recovery_aux_keyboard_gate(
    linked_keyboard_ready: bool,
    first_report_ready: bool,
    pre_first_report_no_event_recovery_due: bool,
) -> bool {
    if pre_first_report_no_event_recovery_due {
        return true;
    }
    linked_keyboard_ready && first_report_ready
}

#[cfg(all(feature = "kernel", feature = "usb"))]
const fn local_seat_keyboard_active_queue_idle(queued_reports: u32, report_status: u32) -> bool {
    queued_reports == LINKED_LOCAL_SEAT_USB_ACTIVE_INTERRUPT_QUEUE_DEPTH
        && (report_status == DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_NONE as u32
            || report_status == DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_IDLE as u32
            || report_status == DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_DECODED_EMPTY as u32)
}

#[cfg(all(feature = "kernel", feature = "usb"))]
const fn local_seat_keyboard_pre_first_report_full_queue_stalled(result: u32) -> bool {
    let queued_reports = result & 0xff;
    queued_reports > LINKED_LOCAL_SEAT_USB_ACTIVE_INTERRUPT_QUEUE_DEPTH
        && local_seat_keyboard_pre_first_report_no_event_queue_stalled(result)
}

#[cfg(all(feature = "kernel", feature = "usb"))]
const fn local_seat_keyboard_pre_first_report_steady_full_queue_stalled(result: u32) -> bool {
    let queued_reports = result & 0xff;
    queued_reports > LINKED_LOCAL_SEAT_USB_ACTIVE_INTERRUPT_QUEUE_DEPTH
        && local_seat_keyboard_pre_first_report_no_event_queue_stalled(result)
}

#[cfg(all(feature = "kernel", feature = "usb"))]
const fn local_seat_keyboard_pre_first_report_no_event_queue_stalled(result: u32) -> bool {
    let queued_reports = result & 0xff;
    let doorbell_pending = (result & (1 << 8)) != 0;
    let report_status = (result >> DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_SHIFT)
        & DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_MASK;
    let transfer_events = (result >> 24) & 0xff;
    queued_reports > LINKED_LOCAL_SEAT_USB_ACTIVE_INTERRUPT_QUEUE_DEPTH
        && transfer_events == 0
        && !doorbell_pending
        && (report_status == DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_NONE as u32
            || report_status == DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_IDLE as u32
            || report_status == DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_DECODED_EMPTY as u32
            || report_status == DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_RECOVERY_SUCCESS as u32)
}

#[cfg(all(feature = "kernel", feature = "usb"))]
const fn local_seat_keyboard_pre_first_report_endpoint_armed(result: u32) -> bool {
    let queued_reports = result & 0xff;
    let doorbell_pending = (result & (1 << 8)) != 0;
    let report_status = (result >> DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_SHIFT)
        & DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_MASK;
    let transfer_events = (result >> 24) & 0xff;
    queued_reports >= LINKED_LOCAL_SEAT_USB_FIRST_REPORT_MIN_QUEUED_REPORTS
        && queued_reports <= LINKED_LOCAL_SEAT_USB_READY_MAX_QUEUED_REPORTS
        && transfer_events == 0
        && !doorbell_pending
        && (report_status == DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_NONE as u32
            || report_status == DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_IDLE as u32
            || report_status == DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_DECODED_EMPTY as u32
            || report_status == DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_RECOVERY_SUCCESS as u32
            || report_status == DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_NO_IDLE_REPORT as u32)
}

#[cfg(all(feature = "kernel", feature = "usb"))]
const fn local_seat_keyboard_pre_first_report_recovery_due(
    first_byte_ready: bool,
    result: u32,
) -> bool {
    !first_byte_ready && local_seat_keyboard_pre_first_report_no_event_queue_stalled(result)
}

#[cfg(all(feature = "kernel", feature = "usb"))]
const fn local_seat_keyboard_hard_recovery_report_status(report_status: u32) -> bool {
    report_status == DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_UNMATCHED_TRANSFER as u32
        || report_status == DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_QUEUE_COLLAPSE as u32
        || report_status == DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_RECOVERY_FAILED as u32
}

#[cfg(all(feature = "kernel", feature = "usb"))]
const fn local_seat_keyboard_no_idle_report_status(report_status: u32) -> bool {
    report_status == DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_NO_IDLE_REPORT as u32
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
const fn local_seat_keyboard_report_status_name(report_status: u32) -> &'static str {
    if report_status == DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_NONE as u32 {
        "none"
    } else if report_status == DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_IDLE as u32 {
        "idle"
    } else if report_status == DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_DECODED_EMPTY as u32 {
        "decoded-empty"
    } else if report_status == DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_QUEUE_COLLAPSE as u32 {
        "queue-collapse"
    } else if report_status == DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_UNMATCHED_TRANSFER as u32 {
        "unmatched-transfer"
    } else if report_status == DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_RECOVERY_SUCCESS as u32 {
        "recovery-success"
    } else if report_status == DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_RECOVERY_FAILED as u32 {
        "recovery-failed"
    } else if report_status == DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_NO_IDLE_REPORT as u32 {
        "no-idle-report"
    } else {
        "other"
    }
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
const fn local_seat_keyboard_recovery_probe_stalled(
    queued_reports: u32,
    report_status: u32,
) -> bool {
    queued_reports != 0
        && queued_reports <= LINKED_LOCAL_SEAT_USB_RECOVERY_PROBE_MAX_QUEUED_REPORTS
        && local_seat_keyboard_hard_recovery_report_status(report_status)
}

#[cfg(all(feature = "kernel", feature = "usb"))]
const fn local_seat_keyboard_result_blocks_prompt_ready(
    result: u32,
    first_byte_ready: bool,
) -> bool {
    let queued_reports = result & 0xff;
    let report_status = (result >> DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_SHIFT)
        & DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_MASK;
    queued_reports > LINKED_LOCAL_SEAT_USB_READY_MAX_QUEUED_REPORTS
        || (queued_reports != 0 && local_seat_keyboard_hard_recovery_report_status(report_status))
        || (!first_byte_ready
            && local_seat_keyboard_pre_first_report_no_event_queue_stalled(result))
}

#[cfg(all(feature = "kernel", feature = "usb"))]
const fn local_seat_keyboard_first_report_fresh(
    result: u32,
    first_report_ready: bool,
    first_byte_ready: bool,
) -> bool {
    if first_byte_ready {
        return true;
    }
    if !first_report_ready || local_seat_keyboard_pre_first_report_no_event_queue_stalled(result) {
        return false;
    }
    if local_seat_keyboard_pre_first_report_endpoint_armed(result) {
        return true;
    }
    let transfer_events = (result >> 24) & 0xff;
    let report_status = (result >> DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_SHIFT)
        & DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_MASK;
    local_seat_keyboard_no_idle_report_status(report_status)
        || (transfer_events != 0 && !local_seat_keyboard_hard_recovery_report_status(report_status))
}

#[cfg(all(feature = "kernel", feature = "usb"))]
const fn local_seat_usb_first_byte_ready_allowed(first_byte_ready: bool, accepted: usize) -> bool {
    first_byte_ready && accepted != 0
}

#[cfg(all(feature = "kernel", feature = "usb"))]
const fn local_seat_usb_controller_owner_state_ready_allowed(controller_ready: bool) -> bool {
    controller_ready
}

#[cfg(any(
    test,
    all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    )
))]
#[must_use]
#[derive(Clone, Copy)]
pub(crate) struct LocalSeatUsbPromptSafeReadyState {
    physical_pi_owner_state: bool,
    root_console_ready: bool,
    keyboard_ready: bool,
    recovery_pending: bool,
    no_reply_streak: u64,
    runtime_queue_blocked: bool,
}

#[cfg(any(
    test,
    all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    )
))]
#[must_use]
pub(crate) const fn local_seat_usb_prompt_safe_ready_state(
    state: LocalSeatUsbPromptSafeReadyState,
) -> bool {
    let _ = state.no_reply_streak;
    !state.physical_pi_owner_state
        || (state.root_console_ready
            && state.keyboard_ready
            && !state.recovery_pending
            && !state.runtime_queue_blocked)
}

#[cfg(any(
    test,
    all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    )
))]
#[must_use]
#[derive(Clone, Copy)]
pub(crate) struct LocalSeatUsbCommandReadyState {
    prompt_safe_ready: bool,
    first_report_ready: bool,
    first_report_fresh: bool,
    first_byte_ready: bool,
    clean_polls: u8,
    post_first_byte_pressure: bool,
}

#[cfg(any(
    test,
    all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    )
))]
#[must_use]
pub(crate) const fn local_seat_usb_command_ready_state(
    state: LocalSeatUsbCommandReadyState,
) -> bool {
    if !state.prompt_safe_ready {
        return false;
    }
    if !state.first_report_ready {
        return false;
    }
    if !state.first_report_fresh && !state.first_byte_ready {
        return false;
    }
    state.clean_polls >= LINKED_LOCAL_SEAT_USB_PROMPT_READY_CLEAN_POLLS
        && !state.post_first_byte_pressure
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
const fn linked_local_seat_usb_resume_enumeration_required(
    controller_attached: bool,
    keyboard_ready: bool,
    enumeration_pending: bool,
) -> bool {
    controller_attached && !keyboard_ready && !enumeration_pending
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
    /// A backend keyboard completed a live service turn during the probe.
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

#[cfg(all(feature = "kernel", feature = "usb"))]
const fn local_seat_keyboard_probe_result(
    outcome: LocalSeatServiceTurn,
    keyboard_attached: bool,
    controller_attached: bool,
) -> LocalSeatKeyboardProbeResult {
    if matches!(outcome, LocalSeatServiceTurn::Complete) && keyboard_attached {
        LocalSeatKeyboardProbeResult::Attached
    } else if controller_attached {
        LocalSeatKeyboardProbeResult::KeyboardUnavailable
    } else {
        LocalSeatKeyboardProbeResult::BackendUnavailable
    }
}

/// Result of one retained local-seat service turn.
///
/// A `Pending` turn may have issued one immutable child-runtime request or
/// polled its exact retained completion once. Callers must return to the outer
/// event pump before asking for another turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalSeatServiceTurn {
    /// More retained work remains for a later outer event turn.
    Pending,
    /// The requested local-seat action completed in this turn.
    Complete,
    /// The retained action failed closed.
    Failed(&'static str),
}

#[cfg(all(feature = "kernel", feature = "usb"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalSeatUsbAttachPhase {
    PcieDescriptor,
    PciePrepared,
    PcieOwner,
    UsbDescriptor,
    UsbInit,
    UsbEnumerate,
    Complete,
    Failed(&'static str),
}

#[cfg(all(feature = "kernel", feature = "usb"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LocalSeatKeyboardProbeCursor {
    active: bool,
    was_enabled: bool,
    turns: usize,
    stalled_progress_turns: usize,
    previous_progress: Option<(u32, u32, u32)>,
}

#[cfg(all(feature = "kernel", feature = "usb"))]
impl LocalSeatKeyboardProbeCursor {
    const fn new() -> Self {
        Self {
            active: false,
            was_enabled: false,
            turns: 0,
            stalled_progress_turns: 0,
            previous_progress: None,
        }
    }

    fn begin(&mut self, was_enabled: bool, progress: Option<(u32, u32, u32)>) {
        if self.active {
            return;
        }
        self.active = true;
        self.was_enabled = was_enabled;
        self.turns = 0;
        self.stalled_progress_turns = 0;
        self.previous_progress = progress;
    }

    fn retain_after_turn(
        &mut self,
        progress: Option<(u32, u32, u32)>,
        active_request: bool,
        root_console_ready: bool,
        retained_action: bool,
    ) -> bool {
        self.turns = self.turns.saturating_add(1);
        let advanced = usb_enumeration_progress_token_allows_probe_burst(
            self.previous_progress,
            progress,
            active_request,
        );
        let stalled = root_console_ready
            && usb_enumeration_stalled_progress_token_allows_probe_burst(
                self.previous_progress,
                progress,
                self.stalled_progress_turns,
            );
        if stalled && !advanced {
            self.stalled_progress_turns = self.stalled_progress_turns.saturating_add(1);
        } else if advanced {
            self.stalled_progress_turns = 0;
        }
        self.previous_progress = progress;
        self.turns < LINKED_LOCAL_SEAT_USB_PROBE_STABLE_PROGRESS_BURST_ATTEMPTS
            && (retained_action || advanced || stalled)
    }

    fn finish(&mut self) -> bool {
        let was_enabled = self.was_enabled;
        *self = Self::new();
        was_enabled
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
    /// First proof bytes consumed as local-console arming input, not commands.
    pub arming_bytes: u64,
    /// Bytes drained from the queue into the console parser path.
    pub drained_bytes: u64,
    /// Bytes applied to root-owned echo state after entering the parser path.
    ///
    /// This is parser/renderer-intent evidence, not proof that an HDMI frame
    /// reached the display; `LocalSeatDisplayTrace` carries that transport
    /// evidence separately.
    pub echoed_bytes: u64,
    /// Bytes dropped because the queue was full.
    pub dropped_bytes: u64,
    /// Service turns stopped by the HAL driver-task budget.
    pub driver_task_budget_overruns: u64,
    /// Steady USB keyboard polls that did not receive a driver-task reply.
    pub driver_task_no_replies: u64,
    /// Consecutive no-reply polls since the last driver-task completion.
    pub driver_task_no_reply_streak: u64,
    /// Prompt-side enumeration aux polls submitted by root before keyboard ready.
    pub enumeration_aux_requests: u64,
    /// Post-first-byte recovery aux polls submitted by root.
    pub recovery_aux_requests: u64,
    /// Whether a recovery aux poll is still waiting for a reply or retry window.
    pub recovery_aux_pending: bool,
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
    /// Queued redraw bytes waiting for chunked snapshot submission.
    pub redraw_bytes: usize,
    /// Whether a coalesced scrollback redraw is waiting.
    pub pending_redraw: bool,
    /// Current root-owned scrollback offset from the live tail.
    pub scrollback_offset: usize,
    /// Whether HDMI is holding an open prompt/input line.
    pub open_line: bool,
    /// Root-owned snapshot generation for correlating redraw supersession.
    pub snapshot_generation: u64,
    /// HDMI frames submitted by the deferred display pump.
    pub submitted_frames: u64,
    /// Display frames intentionally deferred because HDMI was not idle.
    pub deferred_frames: u64,
    /// Pump attempts blocked by an active HDMI driver-task command.
    pub busy_frames: u64,
    /// Pump attempts that did not receive a driver-task reply.
    pub no_reply_frames: u64,
    /// Consecutive redraw chunks that missed a driver-task reply.
    pub redraw_no_reply_streak: u8,
    /// Remaining quiet turns before retrying HDMI after a missing reply.
    pub no_reply_cooldown_turns: u8,
    /// Whether incremental output must wait for a fresh canonical snapshot.
    pub stale_after_retry_exhaustion: bool,
    /// Scrollback redraw requests collapsed into an already pending redraw.
    pub coalesced_redraws: u64,
    /// Display bytes dropped because the bounded mirror queue was full.
    pub backpressure_bytes: u64,
    /// Stale queued bytes discarded because a redraw superseded them.
    pub superseded_bytes: u64,
}

#[cfg(any(
    test,
    all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    )
))]
fn format_usb_keyboard_command_ready_receipt(
    clean_polls: u8,
    no_reply_streak: u64,
    recovery_pending: bool,
) -> heapless::String<192> {
    let mut line = heapless::String::<192>::new();
    let _ = core::fmt::Write::write_fmt(
        &mut line,
        format_args!(
            "[local-seat] usb keyboard command-ready action=enable-command-input source=linked-runtime-hid clean_polls={} no_reply={} recovery_pending={}",
            clean_polls,
            no_reply_streak,
            if recovery_pending { "yes" } else { "no" },
        ),
    );
    line
}

#[cfg(any(
    test,
    all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    )
))]
fn format_usb_keyboard_command_ready_input_detail(
    keyboard: LocalSeatKeyboardTrace,
) -> heapless::String<160> {
    let mut line = heapless::String::<160>::new();
    let _ = core::fmt::Write::write_fmt(
        &mut line,
        format_args!(
            "usb: command_ready_input source=linked-runtime-hid arming_bytes={} queued={} accepted={}",
            keyboard.arming_bytes, keyboard.queued_bytes, keyboard.accepted_bytes,
        ),
    );
    line
}

#[cfg(any(
    test,
    all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    )
))]
fn format_usb_keyboard_command_ready_delivery_detail(
    keyboard: LocalSeatKeyboardTrace,
) -> heapless::String<128> {
    let mut line = heapless::String::<128>::new();
    let _ = core::fmt::Write::write_fmt(
        &mut line,
        format_args!(
            "usb: command_ready_delivery source=linked-runtime-hid drained={} echoed={}",
            keyboard.drained_bytes, keyboard.echoed_bytes,
        ),
    );
    line
}

#[cfg(any(
    test,
    all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    )
))]
fn format_usb_keyboard_command_ready_display_detail(
    display: LocalSeatDisplayTrace,
) -> heapless::String<128> {
    let mut line = heapless::String::<128>::new();
    let _ = core::fmt::Write::write_fmt(
        &mut line,
        format_args!(
            "usb: command_ready_display source=linked-runtime-hid hdmi_pending={} hdmi_submitted={}",
            display.pending_bytes, display.submitted_frames,
        ),
    );
    line
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
    mirrored_line_generation: u64,
    mirrored_line_generation_wrapped: bool,
    hdmi_pending_bytes: VecDeque<u8>,
    hdmi_redraw_bytes: VecDeque<u8>,
    hdmi_pending_redraw: bool,
    hdmi_scrollback_offset: usize,
    hdmi_rendered_scrollback_offset: usize,
    hdmi_rendered_viewport_valid: bool,
    hdmi_active_redraw_target_offset: usize,
    hdmi_active_redraw_input_generation: u64,
    hdmi_active_redraw_line_generation: u64,
    hdmi_last_payload_receipt: LinkedHdmiPayloadReceipt,
    hdmi_scroll_output_fence_active: bool,
    hdmi_scroll_output_fence_bytes: usize,
    hdmi_scroll_output_fence_line_generation: u64,
    hdmi_scroll_output_fence_tail_sequence: u64,
    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    hdmi_retained_frame: Option<LinkedHdmiRetainedFrame>,
    hdmi_input_escape_state: u8,
    hdmi_open_line: bool,
    hdmi_open_line_floor_bytes: usize,
    hdmi_open_line_mirrors_input: bool,
    hdmi_input_row_generation: u64,
    hdmi_input_row_dirty: bool,
    hdmi_input_row_payload: Vec<u8>,
    hdmi_pending_high_impact: bool,
    hdmi_high_impact_bytes_remaining: usize,
    hdmi_initial_snapshot_scheduled: bool,
    hdmi_bootstrap_terminal_ready: bool,
    hdmi_console_ready_line_emitted: bool,
    hdmi_console_ready_line_completed: bool,
    hdmi_console_ready_line_generation: u64,
    hdmi_console_ready_pending_bytes: usize,
    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    hdmi_prompt_pending_until_display_ready: bool,
    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    hdmi_pending_prompt: heapless::String<LOCAL_SEAT_HDMI_PENDING_PROMPT_BYTES>,
    #[cfg(any(
        test,
        all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        )
    ))]
    hdmi_keyboard_starting_line_emitted: bool,
    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    hdmi_keyboard_wait_line_emitted: bool,
    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    hdmi_prompt_pending_reason: &'static str,
    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    hdmi_keyboard_command_ready_latched: bool,
    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    hdmi_keyboard_busy_line_emitted: bool,
    hdmi_snapshot_generation: u64,
    dropped_keyboard_bytes: u64,
    dropped_mirrored_lines: u64,
    hdmi_submitted_frames: u64,
    hdmi_deferred_frames: u64,
    hdmi_busy_frames: u64,
    hdmi_no_reply_frames: u64,
    hdmi_redraw_no_reply_streak: u8,
    hdmi_no_reply_cooldown: u8,
    hdmi_stale_after_retry_exhaustion: bool,
    hdmi_coalesced_redraws: u64,
    hdmi_backpressure_bytes: u64,
    hdmi_superseded_bytes: u64,
    backend_keyboard_poll_calls: u64,
    backend_keyboard_read_bytes: u64,
    accepted_keyboard_bytes: u64,
    arming_keyboard_bytes: u64,
    keyboard_arming_escape_state: u8,
    drained_keyboard_bytes: u64,
    echoed_keyboard_bytes: u64,
    driver_task_budget_overruns: u64,
    driver_task_no_replies: u64,
    keyboard_poll_no_reply_streak: u64,
    keyboard_poll_no_reply_cooldown: u8,
    keyboard_poll_no_reply_backoff: u8,
    keyboard_enumeration_aux_requests: u64,
    keyboard_recovery_aux_pending: bool,
    keyboard_recovery_aux_requests: u64,
    keyboard_post_first_byte_clean_polls: u8,
    keyboard_post_first_byte_no_reply_baseline: u64,
    keyboard_post_first_byte_cooldown_skip_baseline: u64,
    keyboard_post_first_byte_hdmi_deferred_baseline: u64,
    keyboard_post_first_byte_hdmi_busy_baseline: u64,
    keyboard_post_first_byte_hdmi_no_reply_baseline: u64,
    keyboard_poll_cooldown_skips: u64,
    backend_keyboard_polling_enabled: bool,
    backend_keyboard_poll_deferred_logged: bool,
    root_console_ready: bool,
    #[cfg(all(feature = "kernel", feature = "usb"))]
    keyboard_probe_cursor: LocalSeatKeyboardProbeCursor,
    #[cfg(all(feature = "kernel", feature = "usb"))]
    linked_usb_attach_phase: LocalSeatUsbAttachPhase,
    #[cfg(all(feature = "kernel", feature = "usb"))]
    linked_usb_attach_command: Option<crate::hal::driver_task::DriverTaskCommandRecord>,
    #[cfg(all(feature = "kernel", feature = "usb"))]
    linked_usb_poll_command: Option<crate::hal::driver_task::DriverTaskCommandRecord>,
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
            mirrored_line_generation: 0,
            mirrored_line_generation_wrapped: false,
            hdmi_pending_bytes: VecDeque::new(),
            hdmi_redraw_bytes: VecDeque::new(),
            hdmi_pending_redraw: false,
            hdmi_scrollback_offset: 0,
            hdmi_rendered_scrollback_offset: 0,
            hdmi_rendered_viewport_valid: false,
            hdmi_active_redraw_target_offset: 0,
            hdmi_active_redraw_input_generation: 0,
            hdmi_active_redraw_line_generation: 0,
            hdmi_last_payload_receipt: LinkedHdmiPayloadReceipt::None,
            hdmi_scroll_output_fence_active: false,
            hdmi_scroll_output_fence_bytes: 0,
            hdmi_scroll_output_fence_line_generation: 0,
            hdmi_scroll_output_fence_tail_sequence: 0,
            #[cfg(all(
                feature = "kernel",
                feature = "usb",
                target_arch = "aarch64",
                target_os = "none"
            ))]
            hdmi_retained_frame: None,
            hdmi_input_escape_state: 0,
            hdmi_open_line: false,
            hdmi_open_line_floor_bytes: 0,
            hdmi_open_line_mirrors_input: false,
            hdmi_input_row_generation: 0,
            hdmi_input_row_dirty: false,
            hdmi_input_row_payload: Vec::new(),
            hdmi_pending_high_impact: false,
            hdmi_high_impact_bytes_remaining: 0,
            hdmi_initial_snapshot_scheduled: false,
            hdmi_bootstrap_terminal_ready: true,
            hdmi_console_ready_line_emitted: false,
            hdmi_console_ready_line_completed: false,
            hdmi_console_ready_line_generation: 0,
            hdmi_console_ready_pending_bytes: 0,
            #[cfg(all(
                feature = "kernel",
                feature = "usb",
                target_arch = "aarch64",
                target_os = "none"
            ))]
            hdmi_prompt_pending_until_display_ready: false,
            #[cfg(all(
                feature = "kernel",
                feature = "usb",
                target_arch = "aarch64",
                target_os = "none"
            ))]
            hdmi_pending_prompt: heapless::String::new(),
            #[cfg(any(
                test,
                all(
                    feature = "kernel",
                    feature = "usb",
                    target_arch = "aarch64",
                    target_os = "none"
                )
            ))]
            hdmi_keyboard_starting_line_emitted: false,
            #[cfg(all(
                feature = "kernel",
                feature = "usb",
                target_arch = "aarch64",
                target_os = "none"
            ))]
            hdmi_keyboard_wait_line_emitted: false,
            #[cfg(all(
                feature = "kernel",
                feature = "usb",
                target_arch = "aarch64",
                target_os = "none"
            ))]
            hdmi_prompt_pending_reason: LOCAL_SEAT_HDMI_PROMPT_WAIT_REASON_NONE,
            #[cfg(all(
                feature = "kernel",
                feature = "usb",
                target_arch = "aarch64",
                target_os = "none"
            ))]
            hdmi_keyboard_command_ready_latched: false,
            #[cfg(all(
                feature = "kernel",
                feature = "usb",
                target_arch = "aarch64",
                target_os = "none"
            ))]
            hdmi_keyboard_busy_line_emitted: false,
            hdmi_snapshot_generation: 0,
            dropped_keyboard_bytes: 0,
            dropped_mirrored_lines: 0,
            hdmi_submitted_frames: 0,
            hdmi_deferred_frames: 0,
            hdmi_busy_frames: 0,
            hdmi_no_reply_frames: 0,
            hdmi_redraw_no_reply_streak: 0,
            hdmi_no_reply_cooldown: 0,
            hdmi_stale_after_retry_exhaustion: false,
            hdmi_coalesced_redraws: 0,
            hdmi_backpressure_bytes: 0,
            hdmi_superseded_bytes: 0,
            backend_keyboard_poll_calls: 0,
            backend_keyboard_read_bytes: 0,
            accepted_keyboard_bytes: 0,
            arming_keyboard_bytes: 0,
            keyboard_arming_escape_state: 0,
            drained_keyboard_bytes: 0,
            echoed_keyboard_bytes: 0,
            driver_task_budget_overruns: 0,
            driver_task_no_replies: 0,
            keyboard_poll_no_reply_streak: 0,
            keyboard_poll_no_reply_cooldown: 0,
            keyboard_poll_no_reply_backoff: 0,
            keyboard_enumeration_aux_requests: 0,
            keyboard_recovery_aux_pending: false,
            keyboard_recovery_aux_requests: 0,
            keyboard_post_first_byte_clean_polls: 0,
            keyboard_post_first_byte_no_reply_baseline: 0,
            keyboard_post_first_byte_cooldown_skip_baseline: 0,
            keyboard_post_first_byte_hdmi_deferred_baseline: 0,
            keyboard_post_first_byte_hdmi_busy_baseline: 0,
            keyboard_post_first_byte_hdmi_no_reply_baseline: 0,
            keyboard_poll_cooldown_skips: 0,
            // Keep boot fail-open: the root shell must stay reachable even if
            // a platform keyboard backend can still wedge during first probe.
            backend_keyboard_polling_enabled: false,
            backend_keyboard_poll_deferred_logged: false,
            root_console_ready: false,
            #[cfg(all(feature = "kernel", feature = "usb"))]
            keyboard_probe_cursor: LocalSeatKeyboardProbeCursor::new(),
            #[cfg(all(feature = "kernel", feature = "usb"))]
            linked_usb_attach_phase: LocalSeatUsbAttachPhase::PcieDescriptor,
            #[cfg(all(feature = "kernel", feature = "usb"))]
            linked_usb_attach_command: None,
            #[cfg(all(feature = "kernel", feature = "usb"))]
            linked_usb_poll_command: None,
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

    /// Consume bytes used only by legacy tests for the retired arming path.
    ///
    /// Production USB keyboard input no longer uses a sacrificial arming
    /// keystroke; typed runtime frames enter the parser when command input is
    /// ready and are otherwise ignored rather than partially interpreted.
    pub fn accept_keyboard_arming_bytes(&mut self, bytes: &[u8]) -> usize {
        self.accept_keyboard_arming_bytes_with_trace(bytes).0
    }

    fn accept_keyboard_arming_bytes_with_trace(&mut self, bytes: &[u8]) -> (usize, usize) {
        if bytes.is_empty() {
            return (0, 0);
        }
        let arming_len = self.consume_keyboard_arming_sequence_prefix(bytes);
        self.arming_keyboard_bytes = self.arming_keyboard_bytes.saturating_add(arming_len as u64);
        let queued = self.enqueue_keyboard_bytes(&bytes[arming_len..]);
        (arming_len.saturating_add(queued), arming_len)
    }

    fn consume_keyboard_arming_sequence_prefix(&mut self, bytes: &[u8]) -> usize {
        let mut index = 0usize;
        while index < bytes.len() {
            let byte = bytes[index];
            match self.keyboard_arming_escape_state {
                0 if index == 0 && byte == 0x1b => {
                    self.keyboard_arming_escape_state = 1;
                }
                0 => return index.saturating_add(1),
                1 if byte == b'[' => {
                    self.keyboard_arming_escape_state = 2;
                }
                1 if byte == b'O' => {
                    self.keyboard_arming_escape_state = 3;
                }
                1 => {
                    self.keyboard_arming_escape_state = 0;
                    return index.saturating_add(1);
                }
                2 if (0x40..=0x7e).contains(&byte) => {
                    self.keyboard_arming_escape_state = 0;
                    return index.saturating_add(1);
                }
                2 => {}
                3 => {
                    self.keyboard_arming_escape_state = 0;
                    return index.saturating_add(1);
                }
                _ => {
                    self.keyboard_arming_escape_state = 0;
                    return index.saturating_add(1);
                }
            }
            index = index.saturating_add(1);
        }
        bytes.len()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn keyboard_arming_sequence_pending(&self) -> bool {
        self.keyboard_arming_escape_state != 0
    }

    fn accept_keyboard_bytes_for_parser_state(
        &mut self,
        bytes: &[u8],
        parser_ingress: bool,
        _first_byte_ready_before: bool,
        _physical_pi_owner_state: bool,
    ) -> (usize, usize) {
        if parser_ingress {
            return (self.enqueue_keyboard_bytes(bytes), 0);
        }
        (0, 0)
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

    /// Drain at most one line of queued keyboard input for one dispatcher turn.
    ///
    /// Bytes following the first line feed remain retained in the queue. This
    /// lets the ordinary event pump complete and drain one command response
    /// before admitting the next pasted or packet-coalesced command.
    pub fn drain_keyboard_dispatch_bytes(&mut self, out: &mut [u8]) -> usize {
        let mut written = 0usize;
        for slot in out.iter_mut() {
            match self.keyboard_queue.pop_front() {
                Some(byte) => {
                    *slot = byte;
                    written = written.saturating_add(1);
                    if byte == b'\n' {
                        break;
                    }
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
            if self.keyboard_queue.front() != Some(&0x1b)
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
                    self.mirror_linked_hdmi_line_owner_state(line);
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

    #[cfg(any(
        test,
        all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        )
    ))]
    fn mirror_linked_hdmi_line_owner_state(&mut self, line: &str) {
        let previous_rendered_offset = self.linked_hdmi_unsubmitted_live_tail_anchor();
        let queue_tail = !self.root_console_ready || self.linked_hdmi_settled_live_tail();
        self.close_linked_hdmi_open_line(queue_tail);
        self.mirror_line_current_tcb(line);
        self.rebase_linked_hdmi_unsubmitted_live_tail(previous_rendered_offset);
        if queue_tail && !self.queue_linked_hdmi_line(line) {
            self.request_linked_hdmi_snapshot_redraw();
        } else if !queue_tail {
            self.refresh_linked_hdmi_redraw_after_content_mutation();
        }
    }

    /// Mirror an explicit high-impact progress line.
    ///
    /// On physical Pi 4 this queues HDMI text for the linked `hdmi-text`
    /// runtime. The event pump submits it later on an idle turn.
    pub fn mirror_high_impact_line(&mut self, line: &str) -> bool {
        #[cfg(test)]
        if self.hdmi_open_line && self.linked_hdmi_settled_live_tail() {
            return self.mirror_linked_hdmi_high_impact_above_open_line(line);
        }
        #[cfg(all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        ))]
        {
            if crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active() {
                if self.hdmi_open_line && self.linked_hdmi_settled_live_tail() {
                    return self.mirror_linked_hdmi_high_impact_above_open_line(line);
                }
                let queue_tail = !self.root_console_ready || self.linked_hdmi_settled_live_tail();
                if self.hdmi_stale_after_retry_exhaustion
                    && self.linked_hdmi_tail_line_matches(line)
                {
                    return true;
                }
                self.close_linked_hdmi_open_line(queue_tail);
                let previous_rendered_offset = self.linked_hdmi_unsubmitted_live_tail_anchor();
                self.mirror_line_current_tcb(line);
                self.rebase_linked_hdmi_unsubmitted_live_tail(previous_rendered_offset);
                if queue_tail {
                    if self.queue_linked_hdmi_line(line) {
                        self.mark_linked_hdmi_high_impact_queued();
                        return true;
                    }
                    self.request_linked_hdmi_snapshot_redraw();
                } else {
                    self.refresh_linked_hdmi_redraw_after_content_mutation();
                }
                return true;
            }
        }
        self.mirror_line(line);
        true
    }

    /// Insert an asynchronous status immediately above the live command row.
    ///
    /// The linked HDMI terminal supports carriage return and CSI erase-line.
    /// Repainting the status and then the exact canonical input row keeps the
    /// prompt, typed bytes, and cursor stable without a full-screen redraw.
    fn mirror_linked_hdmi_high_impact_above_open_line(&mut self, line: &str) -> bool {
        let Some(open_line) = self.mirrored_lines.pop_back() else {
            self.hdmi_open_line = false;
            self.hdmi_open_line_floor_bytes = 0;
            self.hdmi_open_line_mirrors_input = false;
            self.mirror_line_current_tcb(line);
            if self.queue_linked_hdmi_line(line) {
                self.mark_linked_hdmi_high_impact_queued();
                return true;
            }
            self.request_linked_hdmi_snapshot_redraw();
            return true;
        };
        let open_floor = self.hdmi_open_line_floor_bytes.min(open_line.len());
        let open_mirrors_input = self.hdmi_open_line_mirrors_input;

        self.hdmi_open_line = false;
        self.hdmi_open_line_floor_bytes = 0;
        self.hdmi_open_line_mirrors_input = false;
        self.mirror_line_current_tcb(line);
        self.mirror_line_current_tcb(open_line.as_str());
        self.hdmi_open_line = true;
        self.hdmi_open_line_floor_bytes = open_floor;
        self.hdmi_open_line_mirrors_input = open_mirrors_input;

        if self.linked_hdmi_snapshot_recovery_required() {
            self.refresh_linked_hdmi_redraw_after_content_mutation();
            return true;
        }

        let status = truncate_for_display(line, self.status.line_bytes);
        let mut payload = Vec::new();
        payload.extend_from_slice(b"\r\x1b[2K");
        payload.extend_from_slice(status.as_bytes());
        payload.push(b'\n');
        payload.extend_from_slice(open_line.as_bytes());
        payload.extend_from_slice(b"\x1b[K");
        if self.queue_linked_hdmi_payload(payload.as_slice()) {
            self.mark_linked_hdmi_high_impact_queued();
        } else {
            self.request_linked_hdmi_snapshot_redraw();
        }
        true
    }

    fn mark_linked_hdmi_high_impact_queued(&mut self) {
        // The priority boundary includes any older output already ahead of the
        // status. Drain that complete FIFO prefix before repainting the
        // canonical command row; later ordinary response bytes remain behind
        // the row.
        self.hdmi_high_impact_bytes_remaining = self.hdmi_pending_bytes.len();
        self.hdmi_pending_high_impact = self.hdmi_high_impact_bytes_remaining != 0;
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
        self.mirrored_line_generation = match self.mirrored_line_generation.checked_add(1) {
            Some(generation) => generation,
            None => {
                self.mirrored_line_generation_wrapped = true;
                0
            }
        };
        if was_scrolled_back {
            self.hdmi_scrollback_offset = self
                .hdmi_scrollback_offset
                .saturating_add(1)
                .min(self.max_linked_hdmi_scrollback_offset());
            if self.hdmi_rendered_viewport_valid {
                self.hdmi_rendered_scrollback_offset = self
                    .hdmi_rendered_scrollback_offset
                    .saturating_add(1)
                    .min(self.max_linked_hdmi_scrollback_offset());
            }
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
        self.linked_hdmi_tail_line_matches(line)
    }

    fn linked_hdmi_tail_line_matches(&self, line: &str) -> bool {
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
                self.update_linked_hdmi_keyboard_startup_line();
                self.hdmi_pending_prompt.clear();
                let _ = self.hdmi_pending_prompt.push_str(prompt);
                if !self.linked_hdmi_prompt_ready_for_display() {
                    self.defer_linked_hdmi_prompt_until_display_ready(prompt);
                    return;
                }
                self.mirror_linked_hdmi_prompt_now(prompt);
                return;
            }
        }
        self.mirror_line(prompt);
    }

    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    fn linked_hdmi_prompt_ready_for_display(&self) -> bool {
        let physical_pi_owner_state =
            crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active();
        local_seat_hdmi_prompt_ready_for_display_state(
            physical_pi_owner_state,
            self.root_console_ready,
            self.usb_keyboard_command_ready_latched(),
            self.hdmi_console_ready_line_completed,
            !self.linked_hdmi_pending_work(),
            self.hdmi_no_reply_cooldown == 0
                && self.hdmi_redraw_no_reply_streak == 0
                && !self.hdmi_stale_after_retry_exhaustion,
        )
    }

    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    fn linked_usb_prompt_safe_ready(&self) -> bool {
        let runtime_queue_blocked = local_seat_keyboard_result_blocks_prompt_ready(
            LINKED_LOCAL_SEAT_USB_LAST_RESULT.load(Ordering::Acquire) as u32,
            LINKED_LOCAL_SEAT_USB_FIRST_BYTE_READY_LOGGED.load(Ordering::Acquire),
        );
        local_seat_usb_prompt_safe_ready_state(LocalSeatUsbPromptSafeReadyState {
            physical_pi_owner_state:
                crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active(),
            root_console_ready: self.root_console_ready,
            keyboard_ready: linked_local_seat_usb_keyboard_ready(),
            recovery_pending: local_seat_keyboard_prompt_recovery_pending(
                self.hdmi_keyboard_command_ready_latched,
                self.keyboard_recovery_aux_pending,
                self.keyboard_poll_no_reply_streak,
                self.linked_usb_no_reply_hard_block_threshold(),
            ),
            no_reply_streak: self.keyboard_poll_no_reply_streak,
            runtime_queue_blocked,
        })
    }

    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    fn linked_usb_command_input_ready(&self) -> bool {
        let result = LINKED_LOCAL_SEAT_USB_LAST_RESULT.load(Ordering::Acquire) as u32;
        let first_report_ready =
            LINKED_LOCAL_SEAT_USB_FIRST_REPORT_READY_LOGGED.load(Ordering::Acquire);
        let first_byte_ready =
            LINKED_LOCAL_SEAT_USB_FIRST_BYTE_READY_LOGGED.load(Ordering::Acquire);
        local_seat_usb_command_ready_state(LocalSeatUsbCommandReadyState {
            prompt_safe_ready: self.linked_usb_prompt_safe_ready(),
            first_report_ready,
            first_report_fresh: local_seat_keyboard_first_report_fresh(
                result,
                first_report_ready,
                first_byte_ready,
            ),
            first_byte_ready,
            clean_polls: self.keyboard_post_first_byte_clean_polls,
            post_first_byte_pressure: self.linked_usb_post_first_byte_pressure_active(),
        })
    }

    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    fn linked_usb_prompt_hard_blocked(&self) -> bool {
        local_seat_keyboard_prompt_recovery_pending(
            self.hdmi_keyboard_command_ready_latched,
            self.keyboard_recovery_aux_pending,
            self.keyboard_poll_no_reply_streak,
            self.linked_usb_no_reply_hard_block_threshold(),
        ) || self.keyboard_poll_stale_runtime_queue()
    }

    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    fn linked_usb_post_first_byte_pressure_active(&self) -> bool {
        self.linked_usb_prompt_hard_blocked()
    }

    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    fn linked_usb_no_reply_hard_block_threshold(&self) -> u64 {
        if self.keyboard_poll_active_queue_idle() {
            LINKED_LOCAL_SEAT_USB_POST_FIRST_BYTE_IDLE_RECOVERY_NO_REPLY_THRESHOLD
        } else {
            LINKED_LOCAL_SEAT_USB_POST_FIRST_BYTE_RECOVERY_NO_REPLY_THRESHOLD
        }
    }

    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    fn reset_keyboard_post_first_byte_pressure_baseline(&mut self) {
        self.keyboard_post_first_byte_no_reply_baseline = self.driver_task_no_replies;
        self.keyboard_post_first_byte_cooldown_skip_baseline = self.keyboard_poll_cooldown_skips;
        self.keyboard_post_first_byte_hdmi_deferred_baseline = self.hdmi_deferred_frames;
        self.keyboard_post_first_byte_hdmi_busy_baseline = self.hdmi_busy_frames;
        self.keyboard_post_first_byte_hdmi_no_reply_baseline = self.hdmi_no_reply_frames;
    }

    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    fn linked_hdmi_prompt_wait_reason(&self) -> &'static str {
        if !self.root_console_ready {
            "root-console-pending"
        } else if !self.usb_keyboard_command_ready_latched() {
            "usb-command-pending"
        } else if !self.hdmi_console_ready_line_emitted {
            "console-ready-queue-pending"
        } else if !self.hdmi_console_ready_line_completed {
            "console-ready-completion-pending"
        } else if self.hdmi_no_reply_cooldown != 0
            || self.hdmi_redraw_no_reply_streak != 0
            || self.hdmi_stale_after_retry_exhaustion
        {
            "display-retry-pending"
        } else {
            "display-output-queued"
        }
    }

    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    fn defer_linked_hdmi_prompt_until_display_ready(&mut self, prompt: &str) {
        let reason = self.linked_hdmi_prompt_wait_reason();
        if !self.hdmi_prompt_pending_until_display_ready
            || self.hdmi_prompt_pending_reason != reason
        {
            let mut line = heapless::String::<128>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] hdmi prompt pending reason={} action=wait-for-prompt-ready",
                    reason
                ),
            );
            boot_log::force_uart_line_raw_without_prompt_refresh(line.as_str());
        }
        self.hdmi_prompt_pending_until_display_ready = true;
        self.hdmi_prompt_pending_reason = reason;
        self.hdmi_pending_prompt.clear();
        let _ = self.hdmi_pending_prompt.push_str(prompt);
        self.update_linked_hdmi_keyboard_startup_line();
    }

    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    fn update_linked_hdmi_keyboard_startup_line(&mut self) {
        let usb_keyboard_ready = linked_local_seat_usb_keyboard_ready();
        let usb_first_byte_ready = linked_local_seat_usb_first_byte_ready();
        self.update_linked_hdmi_keyboard_starting_line_for_state(
            usb_keyboard_ready,
            usb_first_byte_ready,
        );
        if local_seat_hdmi_keyboard_wait_line_due(
            usb_keyboard_ready,
            usb_first_byte_ready,
            self.hdmi_keyboard_wait_line_emitted,
        ) {
            self.hdmi_keyboard_wait_line_emitted = true;
            let direct_submit_needs_newline =
                !crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active();
            let line = local_seat_hdmi_keyboard_wait_line_payload(direct_submit_needs_newline);
            self.mirror_line(line.as_str());
        }
    }

    #[cfg(any(
        test,
        all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        )
    ))]
    fn update_linked_hdmi_keyboard_starting_line_for_state(
        &mut self,
        usb_keyboard_ready: bool,
        usb_first_byte_ready: bool,
    ) {
        if local_seat_hdmi_keyboard_starting_line_due(
            usb_keyboard_ready,
            usb_first_byte_ready,
            self.hdmi_keyboard_starting_line_emitted,
        ) {
            self.hdmi_keyboard_starting_line_emitted = true;
            self.mirror_line(LOCAL_SEAT_HDMI_KEYBOARD_STARTING_LINE);
        }
    }

    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    fn release_pending_linked_hdmi_prompt_if_display_ready(&mut self) {
        self.update_linked_hdmi_keyboard_startup_line();
        let usb_command_ready = self.usb_keyboard_command_ready_latched();
        if !self.hdmi_prompt_pending_until_display_ready
            || !local_seat_hdmi_prompt_release_ready(
                self.linked_hdmi_prompt_ready_for_display(),
                usb_command_ready,
            )
        {
            return;
        }

        let mut prompt = heapless::String::<LOCAL_SEAT_HDMI_PENDING_PROMPT_BYTES>::new();
        let _ = prompt.push_str(self.hdmi_pending_prompt.as_str());
        self.hdmi_prompt_pending_until_display_ready = false;
        self.hdmi_prompt_pending_reason = LOCAL_SEAT_HDMI_PROMPT_WAIT_REASON_NONE;
        if prompt.is_empty() {
            return;
        }
        boot_log::force_uart_line_raw_without_prompt_refresh(
            "[local-seat] hdmi prompt enabled reason=usb-console-command-ready action=show-prompt",
        );
        self.mirror_linked_hdmi_prompt_now(prompt.as_str());
    }

    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    fn emit_linked_hdmi_keyboard_busy_line_once(&mut self, reason: &'static str) {
        if self.hdmi_keyboard_busy_line_emitted {
            return;
        }
        if !self.linked_hdmi_keyboard_busy_line_warranted(reason) {
            return;
        }
        self.hdmi_keyboard_busy_line_emitted = true;
        let mut line = heapless::String::<192>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] usb keyboard command-deferred reason={} action=log-recovery-deferred",
                reason
            ),
        );
        boot_log::force_uart_line_raw_and_log(line.as_str());
    }

    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    fn linked_hdmi_keyboard_busy_line_warranted(&self, reason: &'static str) -> bool {
        local_seat_hdmi_keyboard_busy_line_warranted_state(
            self.hdmi_keyboard_command_ready_latched,
            self.linked_usb_prompt_hard_blocked(),
            matches!(reason, "keyboard-poll-no-reply" | "keyboard-poll-cooldown"),
            self.keyboard_poll_no_reply_streak,
            self.keyboard_recovery_aux_pending,
        )
    }

    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    fn mark_linked_hdmi_keyboard_command_ready_once(&mut self) {
        if !self.hdmi_keyboard_command_ready_latched {
            if !self.linked_usb_command_input_ready() {
                return;
            }
            self.hdmi_keyboard_command_ready_latched = true;
            self.hdmi_keyboard_busy_line_emitted = false;
            let keyboard = self.keyboard_trace();
            let display = self.display_trace();
            let line = format_usb_keyboard_command_ready_receipt(
                self.keyboard_post_first_byte_clean_polls,
                keyboard.driver_task_no_reply_streak,
                keyboard.recovery_aux_pending,
            );
            // EventPump is the sole serial projector for this receipt. Retain
            // the source record here without a pre-cutover raw-UART fallback,
            // or one early transition could appear twice on serial.
            crate::log_buffer::append_log_line(line.as_str());
            let input_detail = format_usb_keyboard_command_ready_input_detail(keyboard);
            crate::log_buffer::append_log_line(input_detail.as_str());
            let delivery_detail = format_usb_keyboard_command_ready_delivery_detail(keyboard);
            crate::log_buffer::append_log_line(delivery_detail.as_str());
            let display_detail = format_usb_keyboard_command_ready_display_detail(display);
            crate::log_buffer::append_log_line(display_detail.as_str());
        }
        self.emit_linked_hdmi_console_ready_if_due(self.linked_usb_command_input_ready());
    }

    fn emit_linked_hdmi_console_ready_if_due(&mut self, usb_command_ready: bool) {
        if !local_seat_hdmi_console_ready_line_due(
            self.root_console_ready,
            usb_command_ready,
            self.hdmi_console_ready_line_emitted,
        ) {
            return;
        }
        self.hdmi_console_ready_line_emitted = true;
        self.hdmi_console_ready_line_completed = false;
        self.hdmi_console_ready_pending_bytes = 0;
        #[cfg(any(
            test,
            all(
                feature = "kernel",
                feature = "usb",
                target_arch = "aarch64",
                target_os = "none"
            )
        ))]
        {
            if self.hdmi_open_line {
                if !self.linked_hdmi_settled_live_tail() {
                    self.request_linked_hdmi_snapshot_redraw();
                }
                self.mirror_linked_hdmi_high_impact_above_open_line("Cohesix console ready");
            } else {
                self.mirror_linked_hdmi_line_owner_state("Cohesix console ready");
            }
            self.hdmi_console_ready_line_generation = self.mirrored_line_generation;
            if !self.linked_hdmi_redraw_pending() {
                self.hdmi_console_ready_pending_bytes = self.hdmi_pending_bytes.len();
                if self.hdmi_console_ready_pending_bytes == 0 {
                    self.request_linked_hdmi_snapshot_redraw();
                }
            }
        }
        #[cfg(not(any(
            test,
            all(
                feature = "kernel",
                feature = "usb",
                target_arch = "aarch64",
                target_os = "none"
            )
        )))]
        {
            self.mirror_line("Cohesix console ready");
            self.hdmi_console_ready_line_completed = true;
        }
    }

    #[cfg(any(
        test,
        all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        )
    ))]
    fn mirror_linked_hdmi_prompt_now(&mut self, prompt: &str) {
        if self.promote_linked_hdmi_open_input_line_to_prompt(prompt) {
            return;
        }
        let queue_tail = self.root_console_ready && self.linked_hdmi_settled_live_tail();
        if self.linked_hdmi_open_line_matches(prompt) {
            return;
        }
        self.close_linked_hdmi_open_line(queue_tail);
        let previous_rendered_offset = self.linked_hdmi_unsubmitted_live_tail_anchor();
        self.mirror_line_current_tcb(prompt);
        self.rebase_linked_hdmi_unsubmitted_live_tail(previous_rendered_offset);
        self.open_linked_hdmi_prompt_line(prompt);
        if queue_tail && !self.queue_linked_hdmi_prompt(prompt) {
            self.request_linked_hdmi_snapshot_redraw();
        } else if !queue_tail {
            self.refresh_linked_hdmi_redraw_after_content_mutation();
        }
    }

    fn promote_linked_hdmi_open_input_line_to_prompt(&mut self, prompt: &str) -> bool {
        if !self.hdmi_open_line
            || !self.hdmi_open_line_mirrors_input
            || self.hdmi_open_line_floor_bytes != 0
        {
            return false;
        }
        let prompt = truncate_for_display(prompt, self.status.line_bytes);
        let Some(input) = self.mirrored_lines.back_mut() else {
            return false;
        };
        let mut promoted = String::new();
        promoted.push_str(prompt);
        let remaining = usize::from(self.status.line_bytes).saturating_sub(promoted.len());
        promoted.push_str(truncate_for_display(input.as_str(), remaining as u16));
        *input = promoted;
        self.hdmi_open_line_floor_bytes = prompt.len();

        if self.hdmi_scrollback_offset != 0 || self.linked_hdmi_snapshot_recovery_required() {
            self.request_linked_hdmi_snapshot_redraw();
            return true;
        }
        self.retain_linked_hdmi_fifo_before_input_row();
        self.mark_linked_hdmi_input_row_dirty();
        true
    }

    fn refresh_linked_hdmi_redraw_after_content_mutation(&mut self) {
        if self.linked_hdmi_snapshot_recovery_required() {
            if !self.hdmi_pending_redraw && !self.hdmi_redraw_bytes.is_empty() {
                self.hdmi_snapshot_generation = self.hdmi_snapshot_generation.wrapping_add(1);
            }
            self.request_linked_hdmi_snapshot_redraw();
        }
    }

    fn request_linked_hdmi_snapshot_redraw(&mut self) {
        if self.linked_hdmi_redraw_pending() {
            self.hdmi_coalesced_redraws = self.hdmi_coalesced_redraws.saturating_add(1);
            if self.hdmi_pending_redraw {
                return;
            }
            // A submitted snapshot chunk has a definite place in the physical
            // viewport. Let the active snapshot finish, then chase the newest
            // canonical state. Restarting here makes held arrows perpetually
            // supersede the redraw before it can become visible.
            self.hdmi_pending_redraw = true;
            self.hdmi_stale_after_retry_exhaustion = false;
            return;
        }
        self.hdmi_snapshot_generation = self.hdmi_snapshot_generation.wrapping_add(1);
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
        self.hdmi_pending_high_impact = false;
        self.hdmi_high_impact_bytes_remaining = 0;
        self.clear_linked_hdmi_scroll_output_fence();
        if self.hdmi_console_ready_line_emitted && !self.hdmi_console_ready_line_completed {
            self.hdmi_console_ready_pending_bytes = 0;
        }
        self.hdmi_pending_redraw = true;
        self.hdmi_stale_after_retry_exhaustion = false;
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
        let clear_eol = b"\x1b[K";
        debug_assert_eq!(clear_eol.len(), LINKED_LOCAL_SEAT_HDMI_CLEAR_EOL_BYTES);
        payload.extend_from_slice(clear_eol);
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
        let cursor_home = b"\x1b[H";
        debug_assert_eq!(cursor_home.len(), LINKED_LOCAL_SEAT_HDMI_CURSOR_HOME_BYTES);
        payload.extend_from_slice(cursor_home);
        let mut rendered_rows = 0usize;
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
            if !self.append_linked_hdmi_snapshot_line(
                &mut payload,
                "",
                input_preview,
                geometry,
                false,
            ) {
                return payload;
            }
            rendered_rows = 1;
        } else {
            let mut cursor_at_blank_tail_row = false;
            for index in start..end {
                let tail_line = index.saturating_add(1) == end;
                let append_newline = !tail_line
                    || (!self.hdmi_open_line && geometry.rows > rendered_rows.saturating_add(1));
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
                rendered_rows = rendered_rows.saturating_add(1);
                cursor_at_blank_tail_row = append_newline;
            }
            self.append_linked_hdmi_snapshot_blank_tail(
                &mut payload,
                geometry.rows.saturating_sub(rendered_rows),
                cursor_at_blank_tail_row,
            );
            return payload;
        }
        self.append_linked_hdmi_snapshot_blank_tail(
            &mut payload,
            geometry.rows.saturating_sub(rendered_rows),
            false,
        );
        payload
    }

    fn append_linked_hdmi_snapshot_blank_tail(
        &self,
        payload: &mut Vec<u8>,
        rows: usize,
        cursor_at_blank_tail_row: bool,
    ) {
        if rows == 0 {
            return;
        }
        let save_cursor = b"\x1b7";
        let restore_cursor = b"\x1b8";
        debug_assert_eq!(save_cursor.len(), LINKED_LOCAL_SEAT_HDMI_SAVE_CURSOR_BYTES);
        debug_assert_eq!(
            restore_cursor.len(),
            LINKED_LOCAL_SEAT_HDMI_RESTORE_CURSOR_BYTES
        );
        payload.extend_from_slice(save_cursor);
        let mut remaining_rows = rows;
        if cursor_at_blank_tail_row {
            payload.extend_from_slice(b"\x1b[K");
            remaining_rows = remaining_rows.saturating_sub(1);
        }
        for _ in 0..remaining_rows {
            payload.push(b'\n');
            payload.extend_from_slice(b"\x1b[K");
        }
        payload.extend_from_slice(restore_cursor);
    }

    fn pop_linked_hdmi_chunk(queue: &mut VecDeque<u8>) -> Vec<u8> {
        Self::pop_linked_hdmi_chunk_with_limit(queue, LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES)
    }

    fn pop_linked_hdmi_chunk_with_limit(queue: &mut VecDeque<u8>, limit: usize) -> Vec<u8> {
        let mut payload = Vec::new();
        while payload.len() < limit.min(LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES) {
            let Some(byte) = queue.pop_front() else {
                break;
            };
            payload.push(byte);
        }
        payload
    }

    fn next_linked_hdmi_payload(&mut self) -> Option<(Vec<u8>, &'static str, bool)> {
        self.hdmi_last_payload_receipt = LinkedHdmiPayloadReceipt::None;
        if self.mirrored_line_generation_wrapped
            && self.hdmi_rendered_viewport_valid
            && self.hdmi_rendered_scrollback_offset != self.hdmi_scrollback_offset
            && !self.linked_hdmi_redraw_pending()
        {
            self.request_linked_hdmi_snapshot_redraw();
        }
        if self.hdmi_pending_redraw && self.hdmi_redraw_bytes.is_empty() {
            self.hdmi_pending_redraw = false;
            self.hdmi_active_redraw_target_offset = self.hdmi_scrollback_offset;
            self.hdmi_active_redraw_input_generation = self.hdmi_input_row_generation;
            self.hdmi_active_redraw_line_generation = self.mirrored_line_generation;
            for byte in self.build_linked_hdmi_scrollback_payload() {
                self.hdmi_redraw_bytes.push_back(byte);
            }
        }
        if !self.hdmi_redraw_bytes.is_empty() {
            let payload = Self::pop_linked_hdmi_chunk(&mut self.hdmi_redraw_bytes);
            self.hdmi_last_payload_receipt = LinkedHdmiPayloadReceipt::Redraw {
                final_chunk: self.hdmi_redraw_bytes.is_empty(),
            };
            return Some((payload, "keyboard-scrollback", true));
        }
        if self.hdmi_scroll_output_fence_active {
            if self.hdmi_scroll_output_fence_bytes == 0 {
                self.hdmi_rendered_viewport_valid = false;
                self.request_linked_hdmi_snapshot_redraw();
                return None;
            }
            let payload = Self::pop_linked_hdmi_chunk_with_limit(
                &mut self.hdmi_pending_bytes,
                self.hdmi_scroll_output_fence_bytes,
            );
            if payload.is_empty() {
                self.hdmi_rendered_viewport_valid = false;
                self.request_linked_hdmi_snapshot_redraw();
                return None;
            }
            self.hdmi_scroll_output_fence_bytes = self
                .hdmi_scroll_output_fence_bytes
                .saturating_sub(payload.len());
            if self.hdmi_pending_high_impact {
                self.hdmi_high_impact_bytes_remaining = self
                    .hdmi_high_impact_bytes_remaining
                    .saturating_sub(payload.len());
                if self.hdmi_high_impact_bytes_remaining == 0 {
                    self.hdmi_pending_high_impact = false;
                }
            }
            self.hdmi_last_payload_receipt = LinkedHdmiPayloadReceipt::QueuedOutput {
                payload_bytes: payload.len(),
                completes_scroll_fence: self.hdmi_scroll_output_fence_bytes == 0,
                scroll_fence_line_generation: self.hdmi_scroll_output_fence_line_generation,
                scroll_fence_tail_sequence: self.hdmi_scroll_output_fence_tail_sequence,
            };
            return Some((payload, "queued-output", false));
        }
        if let Some((payload, target_offset)) = self.next_linked_hdmi_scroll_step_payload() {
            let Some(mirrored_tail_sequence) = self.linked_hdmi_tail_sequence() else {
                self.hdmi_rendered_viewport_valid = false;
                self.request_linked_hdmi_snapshot_redraw();
                return None;
            };
            self.hdmi_last_payload_receipt = LinkedHdmiPayloadReceipt::ScrollStep {
                target_offset,
                mirrored_line_generation: self.mirrored_line_generation,
                mirrored_tail_sequence,
            };
            return Some((payload, "keyboard-scroll-step", false));
        }
        if self.hdmi_scrollback_offset == 0
            && self.hdmi_pending_high_impact
            && !self.hdmi_pending_bytes.is_empty()
        {
            let payload = Self::pop_linked_hdmi_chunk_with_limit(
                &mut self.hdmi_pending_bytes,
                self.hdmi_high_impact_bytes_remaining,
            );
            self.hdmi_high_impact_bytes_remaining = self
                .hdmi_high_impact_bytes_remaining
                .saturating_sub(payload.len());
            if self.hdmi_high_impact_bytes_remaining == 0 {
                self.hdmi_pending_high_impact = false;
            }
            self.hdmi_last_payload_receipt = LinkedHdmiPayloadReceipt::QueuedOutput {
                payload_bytes: payload.len(),
                completes_scroll_fence: false,
                scroll_fence_line_generation: 0,
                scroll_fence_tail_sequence: 0,
            };
            return Some((payload, "queued-output", false));
        }
        if self.hdmi_input_row_dirty
            && self.hdmi_scrollback_offset == 0
            && !self.hdmi_input_row_payload.is_empty()
        {
            self.hdmi_last_payload_receipt = LinkedHdmiPayloadReceipt::InputRow {
                generation: self.hdmi_input_row_generation,
            };
            return Some((
                self.hdmi_input_row_payload.clone(),
                "keyboard-input-row",
                false,
            ));
        }
        if self.hdmi_pending_bytes.is_empty() || self.hdmi_scrollback_offset != 0 {
            return None;
        }
        let payload = Self::pop_linked_hdmi_chunk(&mut self.hdmi_pending_bytes);
        if self.hdmi_pending_bytes.is_empty() {
            self.hdmi_pending_high_impact = false;
            self.hdmi_high_impact_bytes_remaining = 0;
        }
        self.hdmi_last_payload_receipt = LinkedHdmiPayloadReceipt::QueuedOutput {
            payload_bytes: payload.len(),
            completes_scroll_fence: false,
            scroll_fence_line_generation: 0,
            scroll_fence_tail_sequence: 0,
        };
        Some((payload, "queued-output", false))
    }

    fn append_linked_hdmi_decimal(payload: &mut Vec<u8>, mut value: usize) {
        let mut digits = [0u8; 20];
        let mut len = 0usize;
        loop {
            digits[len] = b'0'.saturating_add((value % 10) as u8);
            len = len.saturating_add(1);
            value /= 10;
            if value == 0 {
                break;
            }
        }
        while len != 0 {
            len = len.saturating_sub(1);
            payload.push(digits[len]);
        }
    }

    fn linked_hdmi_decimal_len(mut value: usize) -> usize {
        let mut len = 1usize;
        while value >= 10 {
            value /= 10;
            len = len.saturating_add(1);
        }
        len
    }

    fn next_linked_hdmi_scroll_step_payload(&self) -> Option<(Vec<u8>, usize)> {
        if !self.hdmi_rendered_viewport_valid
            || self.hdmi_rendered_scrollback_offset == self.hdmi_scrollback_offset
        {
            return None;
        }
        let geometry = linked_hdmi_snapshot_geometry();
        let visible = linked_hdmi_scrollback_visible_lines_for_geometry(
            self.status.buffer_lines,
            self.status.line_bytes,
            geometry,
        );
        let total = self.mirrored_lines.len();
        if visible == 0 || total < visible {
            return None;
        }
        let toward_older = self.hdmi_scrollback_offset > self.hdmi_rendered_scrollback_offset;
        let distance = self
            .hdmi_scrollback_offset
            .abs_diff(self.hdmi_rendered_scrollback_offset);
        let step_limit = distance.min(if visible > 1 { visible - 1 } else { 1 });
        let rendered_end = total.saturating_sub(self.hdmi_rendered_scrollback_offset);
        let rendered_start = rendered_end.saturating_sub(visible);
        let line_width = linked_hdmi_snapshot_width_u16(linked_hdmi_snapshot_line_width(
            self.status.line_bytes,
            geometry,
        ));
        let mut line_bytes = 0usize;
        let mut step_count = 0usize;
        for candidate in 1..=step_limit {
            let line_index = if toward_older {
                rendered_start.checked_sub(candidate)?
            } else {
                rendered_end.checked_add(candidate.saturating_sub(1))?
            };
            let line = self.mirrored_lines.get(line_index)?;
            line_bytes = line_bytes.saturating_add(truncate_for_display(line, line_width).len());
            let target_row = visible.saturating_sub(candidate).saturating_add(1);
            let position_bytes = if toward_older {
                b"\x1b[H".len()
            } else {
                b"\x1b["
                    .len()
                    .saturating_add(Self::linked_hdmi_decimal_len(target_row))
                    .saturating_add(b";1H".len())
            };
            let candidate_bytes = b"\x1b7\x1b["
                .len()
                .saturating_add(Self::linked_hdmi_decimal_len(candidate))
                .saturating_add(1)
                .saturating_add(position_bytes)
                .saturating_add(line_bytes)
                .saturating_add(candidate.saturating_mul(b"\x1b[K".len()))
                .saturating_add(candidate.saturating_sub(1))
                .saturating_add(b"\x1b8".len());
            if candidate_bytes > LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES {
                break;
            }
            step_count = candidate;
        }
        if step_count == 0 {
            return None;
        }
        let target_offset = if toward_older {
            self.hdmi_rendered_scrollback_offset
                .saturating_add(step_count)
        } else {
            self.hdmi_rendered_scrollback_offset
                .saturating_sub(step_count)
        };
        let (line_start, line_end) = if toward_older {
            (rendered_start.saturating_sub(step_count), rendered_start)
        } else {
            (rendered_end, rendered_end.saturating_add(step_count))
        };
        let mut payload = Vec::new();
        payload.extend_from_slice(b"\x1b7\x1b[");
        Self::append_linked_hdmi_decimal(&mut payload, step_count);
        payload.push(if toward_older { b'T' } else { b'S' });
        if toward_older {
            payload.extend_from_slice(b"\x1b[H");
        } else {
            payload.extend_from_slice(b"\x1b[");
            Self::append_linked_hdmi_decimal(
                &mut payload,
                visible.saturating_sub(step_count).saturating_add(1),
            );
            payload.extend_from_slice(b";1H");
        }
        for index in line_start..line_end {
            let line = self.mirrored_lines.get(index)?;
            payload.extend_from_slice(truncate_for_display(line, line_width).as_bytes());
            payload.extend_from_slice(b"\x1b[K");
            if index.saturating_add(1) != line_end {
                payload.push(b'\n');
            }
        }
        payload.extend_from_slice(b"\x1b8");
        debug_assert!(payload.len() <= LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES);
        Some((payload, target_offset))
    }

    fn linked_hdmi_receipt_offset(
        &self,
        target_offset: usize,
        mirrored_line_generation: u64,
        mirrored_tail_sequence: u64,
    ) -> Option<usize> {
        if self.mirrored_line_generation_wrapped
            || self
                .mirrored_line_generation
                .checked_sub(mirrored_line_generation)
                .is_none()
        {
            return None;
        }
        let current_tail_sequence = self.linked_hdmi_tail_sequence()?;
        let tail_delta = current_tail_sequence.checked_sub(mirrored_tail_sequence)?;
        let tail_delta = usize::try_from(tail_delta).ok()?;
        target_offset
            .checked_add(tail_delta)
            .filter(|offset| *offset <= self.max_linked_hdmi_scrollback_offset())
    }

    fn linked_hdmi_console_ready_line_canonical(&self) -> bool {
        self.mirrored_lines
            .iter()
            .any(|line| line.as_str() == "Cohesix console ready")
    }

    fn record_linked_hdmi_console_ready_output_success(&mut self, payload_bytes: usize) {
        if !self.hdmi_console_ready_line_emitted
            || self.hdmi_console_ready_line_completed
            || self.hdmi_console_ready_pending_bytes == 0
        {
            return;
        }
        self.hdmi_console_ready_pending_bytes = self
            .hdmi_console_ready_pending_bytes
            .saturating_sub(payload_bytes);
        if self.hdmi_console_ready_pending_bytes == 0
            && !self.linked_hdmi_redraw_pending()
            && !self.mirrored_line_generation_wrapped
            && self
                .mirrored_line_generation
                .checked_sub(self.hdmi_console_ready_line_generation)
                .is_some()
            && self.linked_hdmi_console_ready_line_canonical()
        {
            self.hdmi_console_ready_line_completed = true;
        }
    }

    fn record_linked_hdmi_submit_success(&mut self, receipt: LinkedHdmiPayloadReceipt) {
        match receipt {
            LinkedHdmiPayloadReceipt::Redraw { final_chunk: true } => {
                self.hdmi_rendered_scrollback_offset = self.hdmi_active_redraw_target_offset;
                self.hdmi_rendered_viewport_valid = true;
                if self.mirrored_line_generation == self.hdmi_active_redraw_line_generation {
                    self.mirrored_line_generation_wrapped = false;
                }
                if self.hdmi_input_row_generation == self.hdmi_active_redraw_input_generation {
                    self.hdmi_input_row_dirty = false;
                    self.hdmi_input_row_payload.clear();
                }
                if self.hdmi_console_ready_line_emitted
                    && !self.hdmi_console_ready_line_completed
                    && !self.hdmi_pending_redraw
                    && !self.mirrored_line_generation_wrapped
                    && self
                        .hdmi_active_redraw_line_generation
                        .checked_sub(self.hdmi_console_ready_line_generation)
                        .is_some()
                    && self.linked_hdmi_console_ready_line_canonical()
                {
                    self.hdmi_console_ready_line_completed = true;
                    self.hdmi_console_ready_pending_bytes = 0;
                }
            }
            LinkedHdmiPayloadReceipt::ScrollStep {
                target_offset,
                mirrored_line_generation,
                mirrored_tail_sequence,
            } => {
                if let Some(adjusted_target) = self.linked_hdmi_receipt_offset(
                    target_offset,
                    mirrored_line_generation,
                    mirrored_tail_sequence,
                ) {
                    self.hdmi_rendered_scrollback_offset = adjusted_target;
                    self.hdmi_rendered_viewport_valid = true;
                } else {
                    self.hdmi_rendered_viewport_valid = false;
                    self.request_linked_hdmi_snapshot_redraw();
                }
            }
            LinkedHdmiPayloadReceipt::InputRow { generation } => {
                if self.hdmi_input_row_generation == generation {
                    self.hdmi_input_row_dirty = false;
                    self.hdmi_input_row_payload.clear();
                }
            }
            LinkedHdmiPayloadReceipt::QueuedOutput {
                payload_bytes,
                completes_scroll_fence,
                scroll_fence_line_generation,
                scroll_fence_tail_sequence,
            } => {
                self.record_linked_hdmi_console_ready_output_success(payload_bytes);
                if completes_scroll_fence {
                    let exact_fence = self.hdmi_scroll_output_fence_active
                        && self.hdmi_scroll_output_fence_bytes == 0
                        && self.hdmi_scroll_output_fence_line_generation
                            == scroll_fence_line_generation
                        && self.hdmi_scroll_output_fence_tail_sequence
                            == scroll_fence_tail_sequence
                        && !self.linked_hdmi_redraw_pending();
                    let adjusted_target = exact_fence
                        .then(|| {
                            self.linked_hdmi_receipt_offset(
                                0,
                                scroll_fence_line_generation,
                                scroll_fence_tail_sequence,
                            )
                        })
                        .flatten();
                    self.clear_linked_hdmi_scroll_output_fence();
                    if let Some(adjusted_target) = adjusted_target {
                        self.hdmi_rendered_scrollback_offset = adjusted_target;
                        self.hdmi_rendered_viewport_valid = true;
                    } else {
                        self.hdmi_rendered_viewport_valid = false;
                        self.request_linked_hdmi_snapshot_redraw();
                    }
                }
            }
            LinkedHdmiPayloadReceipt::None
            | LinkedHdmiPayloadReceipt::Redraw { final_chunk: false } => {}
        }
        self.hdmi_last_payload_receipt = LinkedHdmiPayloadReceipt::None;
    }

    fn record_linked_hdmi_submit_miss(&mut self, payload: &[u8], redraw: bool) {
        self.hdmi_rendered_viewport_valid = false;
        self.hdmi_last_payload_receipt = LinkedHdmiPayloadReceipt::None;
        self.hdmi_no_reply_frames = self.hdmi_no_reply_frames.saturating_add(1);
        self.hdmi_deferred_frames = self.hdmi_deferred_frames.saturating_add(1);
        self.hdmi_no_reply_cooldown = LINKED_LOCAL_SEAT_HDMI_NO_REPLY_COOLDOWN_TURNS;
        if redraw {
            self.hdmi_redraw_no_reply_streak = self.hdmi_redraw_no_reply_streak.saturating_add(1);
            let superseded = payload.len().saturating_add(self.hdmi_redraw_bytes.len());
            if self.hdmi_redraw_no_reply_streak
                <= LINKED_LOCAL_SEAT_HDMI_REDRAW_NO_REPLY_RETRY_LIMIT
            {
                self.hdmi_superseded_bytes =
                    self.hdmi_superseded_bytes.saturating_add(superseded as u64);
                self.hdmi_redraw_bytes.clear();
                self.hdmi_pending_redraw = true;
            } else {
                self.hdmi_superseded_bytes =
                    self.hdmi_superseded_bytes.saturating_add(superseded as u64);
                self.hdmi_redraw_bytes.clear();
                self.hdmi_pending_redraw = false;
                self.hdmi_stale_after_retry_exhaustion = true;
            }
        } else {
            self.hdmi_redraw_no_reply_streak = 0;
            self.hdmi_superseded_bytes = self
                .hdmi_superseded_bytes
                .saturating_add(payload.len() as u64);
            self.request_linked_hdmi_snapshot_redraw();
        }
    }

    /// Return whether local-seat HDMI has queued work waiting for an idle turn.
    #[must_use]
    pub fn linked_hdmi_pending_work(&self) -> bool {
        self.linked_hdmi_retained_frame_pending()
            || self.linked_hdmi_redraw_pending()
            || self.linked_hdmi_output_ready_to_issue()
            || (self.hdmi_input_row_dirty && self.hdmi_scrollback_offset == 0)
            || (self.hdmi_rendered_viewport_valid
                && self.hdmi_rendered_scrollback_offset != self.hdmi_scrollback_offset)
    }

    /// Return whether a network-origin console line should be mirrored to HDMI.
    ///
    /// Serial and local-seat traffic always use the normal mirror path. This
    /// guard applies only to remote console output so REST/TCP bursts can flush
    /// replies without letting a pressured HDMI runtime become the pacing item.
    #[must_use]
    pub fn can_accept_network_origin_mirror(&self) -> bool {
        let queued_bytes = self
            .hdmi_pending_bytes
            .len()
            .saturating_add(self.hdmi_redraw_bytes.len());
        queued_bytes <= LINKED_LOCAL_SEAT_NET_MIRROR_PENDING_BYTE_LIMIT
            && self.hdmi_no_reply_cooldown == 0
            && self.hdmi_redraw_no_reply_streak == 0
            && !self.hdmi_stale_after_retry_exhaustion
            && self.hdmi_backpressure_bytes == 0
    }

    #[cfg(test)]
    pub(crate) fn inject_linked_hdmi_pending_bytes_for_test(&mut self, count: usize) {
        self.hdmi_pending_bytes.clear();
        for _ in 0..count.min(DISPLAY_QUEUE_MAX_BYTES) {
            self.hdmi_pending_bytes.push_back(b'x');
        }
    }

    fn linked_hdmi_redraw_pending(&self) -> bool {
        self.hdmi_pending_redraw || !self.hdmi_redraw_bytes.is_empty()
    }

    fn linked_hdmi_tail_sequence(&self) -> Option<u64> {
        let retained = u64::try_from(self.mirrored_lines.len()).ok()?;
        self.dropped_mirrored_lines.checked_add(retained)
    }

    fn clear_linked_hdmi_scroll_output_fence(&mut self) {
        self.hdmi_scroll_output_fence_active = false;
        self.hdmi_scroll_output_fence_bytes = 0;
        self.hdmi_scroll_output_fence_line_generation = 0;
        self.hdmi_scroll_output_fence_tail_sequence = 0;
    }

    fn begin_linked_hdmi_scroll_output_fence(&mut self) -> bool {
        if self.hdmi_scroll_output_fence_active {
            return true;
        }
        let Some(tail_sequence) = self.linked_hdmi_tail_sequence() else {
            return false;
        };
        if self.mirrored_line_generation_wrapped || self.hdmi_pending_bytes.is_empty() {
            return false;
        }
        self.hdmi_scroll_output_fence_active = true;
        self.hdmi_scroll_output_fence_bytes = self.hdmi_pending_bytes.len();
        self.hdmi_scroll_output_fence_line_generation = self.mirrored_line_generation;
        self.hdmi_scroll_output_fence_tail_sequence = tail_sequence;
        true
    }

    fn linked_hdmi_output_ready_to_issue(&self) -> bool {
        (self.hdmi_scroll_output_fence_active && self.hdmi_scroll_output_fence_bytes != 0)
            || (self.hdmi_scrollback_offset == 0 && !self.hdmi_pending_bytes.is_empty())
    }

    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    fn linked_hdmi_retained_frame_pending(&self) -> bool {
        self.hdmi_retained_frame.is_some()
    }

    #[cfg(not(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    )))]
    const fn linked_hdmi_retained_frame_pending(&self) -> bool {
        false
    }

    #[cfg(any(
        test,
        all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        )
    ))]
    fn linked_hdmi_scroll_step_in_flight(&self) -> bool {
        matches!(
            self.hdmi_last_payload_receipt,
            LinkedHdmiPayloadReceipt::ScrollStep { .. }
        )
    }

    #[cfg(any(
        test,
        all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        )
    ))]
    fn linked_hdmi_settled_live_tail(&self) -> bool {
        self.hdmi_rendered_viewport_valid
            && self.hdmi_rendered_scrollback_offset == 0
            && self.hdmi_scrollback_offset == 0
            && !self.linked_hdmi_scroll_step_in_flight()
            && !self.linked_hdmi_snapshot_recovery_required()
    }

    #[cfg(any(
        test,
        all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        )
    ))]
    fn linked_hdmi_unsubmitted_live_tail_anchor(&self) -> Option<usize> {
        (self.root_console_ready
            && self.hdmi_scrollback_offset == 0
            && self.hdmi_rendered_viewport_valid
            && self.hdmi_rendered_scrollback_offset != 0
            && !self.linked_hdmi_scroll_step_in_flight()
            && !self.linked_hdmi_snapshot_recovery_required())
        .then_some(self.hdmi_rendered_scrollback_offset)
    }

    #[cfg(any(
        test,
        all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        )
    ))]
    fn rebase_linked_hdmi_unsubmitted_live_tail(&mut self, previous_offset: Option<usize>) {
        let Some(previous_offset) = previous_offset else {
            return;
        };
        match previous_offset
            .checked_add(1)
            .filter(|offset| *offset <= self.max_linked_hdmi_scrollback_offset())
        {
            Some(offset) => self.hdmi_rendered_scrollback_offset = offset,
            None => {
                self.hdmi_rendered_viewport_valid = false;
                self.request_linked_hdmi_snapshot_redraw();
            }
        }
    }

    fn linked_hdmi_snapshot_recovery_required(&self) -> bool {
        self.linked_hdmi_redraw_pending() || self.hdmi_stale_after_retry_exhaustion
    }

    fn linked_hdmi_retry_cooldown_active(&mut self) -> bool {
        if self.hdmi_no_reply_cooldown == 0 {
            return false;
        }
        self.hdmi_no_reply_cooldown = self.hdmi_no_reply_cooldown.saturating_sub(1);
        self.hdmi_deferred_frames = self.hdmi_deferred_frames.saturating_add(1);
        true
    }

    /// Submit at most one queued HDMI frame on a quiet event-loop turn.
    #[must_use]
    pub fn pump_linked_hdmi_once(&mut self) -> bool {
        #[cfg(all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        ))]
        self.release_pending_linked_hdmi_prompt_if_display_ready();
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
                let ready = try_attach_linked_display_runtime(self.root_console_ready);
                let _ = self.schedule_linked_hdmi_initial_snapshot_if_due(ready);
                self.hdmi_deferred_frames = self.hdmi_deferred_frames.saturating_add(1);
                return false;
            }
            let display_attached = LINKED_LOCAL_SEAT_DISPLAY_ATTACHED.load(Ordering::Acquire);
            let _ = self.schedule_linked_hdmi_initial_snapshot_if_due(display_attached);
            if !local_seat_linked_display_service_allowed(
                true,
                display_attached,
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
            if self.hdmi_retained_frame.is_none()
                && crate::hal::driver_task::driver_task_ring_command_active(contract)
            {
                self.hdmi_busy_frames = self.hdmi_busy_frames.saturating_add(1);
                self.hdmi_deferred_frames = self.hdmi_deferred_frames.saturating_add(1);
                return false;
            }
            if self.hdmi_retained_frame.is_none() {
                if self.linked_hdmi_retry_cooldown_active() {
                    return false;
                }
                let Some((payload, reason, redraw)) = self.next_linked_hdmi_payload() else {
                    return false;
                };
                let receipt = self.hdmi_last_payload_receipt;
                let retained = match build_linked_hdmi_retained_frame(
                    contract, payload, reason, redraw, receipt,
                ) {
                    Ok(retained) => retained,
                    Err(payload) => {
                        self.record_linked_hdmi_submit_miss(payload.as_slice(), redraw);
                        return false;
                    }
                };
                self.hdmi_retained_frame = Some(retained);
            }
            let Some(retained) = self.hdmi_retained_frame.as_ref() else {
                return false;
            };
            let command = retained.command;
            let staging_segments = [
                crate::hal::driver_task::DriverTaskStagingSegment::ring_frame(
                    retained.payload.as_slice(),
                    0,
                ),
            ];
            let turn =
                crate::hal::driver_task::run_driver_task_ring_service_retained_service_turn_staged(
                    contract,
                    command,
                    &staging_segments,
                );
            match turn {
                crate::hal::driver_task::DriverTaskRetainedServiceTurn::Pending => {
                    // The child may have consumed its current MCS budget in the
                    // middle of this exact frame. Keep both command identity
                    // and staged bytes stable until its next replenishment.
                    self.hdmi_deferred_frames = self.hdmi_deferred_frames.saturating_add(1);
                    false
                }
                crate::hal::driver_task::DriverTaskRetainedServiceTurn::Complete(completion) => {
                    let Some(retained) = self.hdmi_retained_frame.take() else {
                        return false;
                    };
                    let ready = completion.code
                        == crate::hal::driver_task::DriverTaskCompletionCode::Progress.as_u16()
                        && completion.result != 0;
                    emit_hdmi_frame_submit_state(
                        retained.reason,
                        retained.payload.len(),
                        hdmi_payload_signature(retained.payload.as_slice()),
                        0,
                        1,
                        retained.redraw,
                        Some(self.display_trace()),
                        self.root_console_ready,
                        Some(completion),
                        ready,
                        false,
                    );
                    if !ready {
                        self.record_linked_hdmi_submit_miss(
                            retained.payload.as_slice(),
                            retained.redraw,
                        );
                        return false;
                    }
                    let _ = credit_linked_display_runtime_frame_owner_state(
                        retained.reason,
                        self.root_console_ready,
                        completion,
                    );
                    self.hdmi_submitted_frames = self.hdmi_submitted_frames.saturating_add(1);
                    self.hdmi_redraw_no_reply_streak = 0;
                    self.hdmi_no_reply_cooldown = 0;
                    if retained.redraw {
                        self.hdmi_stale_after_retry_exhaustion = false;
                    }
                    self.record_linked_hdmi_submit_success(retained.receipt);
                    self.release_pending_linked_hdmi_prompt_if_display_ready();
                    true
                }
                crate::hal::driver_task::DriverTaskRetainedServiceTurn::Failed => {
                    let Some(retained) = self.hdmi_retained_frame.take() else {
                        return false;
                    };
                    emit_hdmi_frame_submit_state(
                        retained.reason,
                        retained.payload.len(),
                        hdmi_payload_signature(retained.payload.as_slice()),
                        0,
                        1,
                        retained.redraw,
                        Some(self.display_trace()),
                        self.root_console_ready,
                        None,
                        false,
                        false,
                    );
                    self.record_linked_hdmi_submit_miss(
                        retained.payload.as_slice(),
                        retained.redraw,
                    );
                    false
                }
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

    /// Return whether USB keyboard command input has been admitted.
    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    #[must_use]
    pub fn usb_keyboard_command_ready_latched(&self) -> bool {
        self.hdmi_keyboard_command_ready_latched && self.linked_usb_command_input_ready()
    }

    /// Format the bounded canonical receipt for EventPump serial projection.
    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    #[must_use]
    pub(crate) fn usb_keyboard_command_ready_receipt_line(&self) -> Option<heapless::String<192>> {
        if !self.usb_keyboard_command_ready_latched() {
            return None;
        }
        Some(format_usb_keyboard_command_ready_receipt(
            self.keyboard_post_first_byte_clean_polls,
            self.keyboard_poll_no_reply_streak,
            self.keyboard_recovery_aux_pending,
        ))
    }

    /// Return whether USB keyboard command input has been admitted.
    #[cfg(not(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    )))]
    #[must_use]
    pub const fn usb_keyboard_command_ready_latched(&self) -> bool {
        false
    }

    /// Host and non-Pi builds have no linked USB command-ready receipt.
    #[cfg(not(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    )))]
    #[must_use]
    pub(crate) const fn usb_keyboard_command_ready_receipt_line(
        &self,
    ) -> Option<heapless::String<192>> {
        None
    }

    /// Return whether queued keyboard bytes may enter the console parser.
    #[must_use]
    pub fn keyboard_parser_ingress_ready(&self) -> bool {
        #[cfg(all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        ))]
        {
            !crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active()
                || (self.hdmi_keyboard_command_ready_latched
                    && linked_local_seat_usb_keyboard_ready()
                    && !self.linked_usb_prompt_hard_blocked())
                || self.linked_usb_command_input_ready()
        }
        #[cfg(not(all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        )))]
        {
            true
        }
    }

    /// Legacy diagnostic accessor; the HDMI ready banner is no longer mirrored.
    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    #[must_use]
    pub const fn hdmi_keyboard_ready_line_emitted(&self) -> bool {
        self.hdmi_keyboard_command_ready_latched
    }

    /// Legacy diagnostic accessor; host tests have no linked Pi USB latch.
    #[cfg(not(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    )))]
    #[must_use]
    pub const fn hdmi_keyboard_ready_line_emitted(&self) -> bool {
        false
    }

    /// Return keyboard ingress counters for `usb status` diagnostics.
    #[must_use]
    pub fn keyboard_trace(&self) -> LocalSeatKeyboardTrace {
        LocalSeatKeyboardTrace {
            queued_bytes: self.keyboard_queue.len(),
            backend_poll_calls: self.backend_keyboard_poll_calls,
            backend_read_bytes: self.backend_keyboard_read_bytes,
            accepted_bytes: self.accepted_keyboard_bytes,
            arming_bytes: self.arming_keyboard_bytes,
            drained_bytes: self.drained_keyboard_bytes,
            echoed_bytes: self.echoed_keyboard_bytes,
            dropped_bytes: self.dropped_keyboard_bytes,
            driver_task_budget_overruns: self.driver_task_budget_overruns,
            driver_task_no_replies: self.driver_task_no_replies,
            driver_task_no_reply_streak: self.keyboard_poll_no_reply_streak,
            enumeration_aux_requests: self.keyboard_enumeration_aux_requests,
            recovery_aux_requests: self.keyboard_recovery_aux_requests,
            recovery_aux_pending: self.keyboard_recovery_aux_pending,
            poll_cooldown_turns: self.keyboard_poll_no_reply_cooldown,
            poll_cooldown_skips: self.keyboard_poll_cooldown_skips,
        }
    }

    /// Return HDMI mirror pressure counters for diagnostics.
    #[must_use]
    pub fn display_trace(&self) -> LocalSeatDisplayTrace {
        let pending_input_row_bytes =
            if self.hdmi_input_row_dirty && self.hdmi_scrollback_offset == 0 {
                self.hdmi_input_row_payload.len()
            } else {
                0
            };
        LocalSeatDisplayTrace {
            pending_bytes: self
                .hdmi_pending_bytes
                .len()
                .saturating_add(self.hdmi_redraw_bytes.len())
                .saturating_add(pending_input_row_bytes),
            redraw_bytes: self.hdmi_redraw_bytes.len(),
            pending_redraw: self.linked_hdmi_redraw_pending()
                || (self.hdmi_rendered_viewport_valid
                    && self.hdmi_rendered_scrollback_offset != self.hdmi_scrollback_offset),
            scrollback_offset: self.hdmi_scrollback_offset,
            open_line: self.hdmi_open_line,
            snapshot_generation: self.hdmi_snapshot_generation,
            submitted_frames: self.hdmi_submitted_frames,
            deferred_frames: self.hdmi_deferred_frames,
            busy_frames: self.hdmi_busy_frames,
            no_reply_frames: self.hdmi_no_reply_frames,
            redraw_no_reply_streak: self.hdmi_redraw_no_reply_streak,
            no_reply_cooldown_turns: self.hdmi_no_reply_cooldown,
            stale_after_retry_exhaustion: self.hdmi_stale_after_retry_exhaustion,
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
        self.keyboard_recovery_aux_pending = false;
        self.clear_keyboard_poll_no_reply_backoff();
    }

    /// Mark that the serial root console may settle local-seat work.
    pub fn mark_root_console_ready(&mut self) {
        self.root_console_ready = true;
        self.backend_keyboard_poll_deferred_logged = false;
        #[cfg(all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        ))]
        {
            if crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active() {
                self.enable_backend_keyboard_polling();
                self.mark_linked_hdmi_keyboard_command_ready_once();
            }
        }
    }

    /// Mark the finite deferred Wi-Fi episode active for HDMI status.
    ///
    /// Wi-Fi milestones may be retracted and replaced while the network
    /// bootstrap converges. The independent local-console banner and prompt
    /// remain governed only by root, USB-command, and exact display completion.
    pub fn defer_hdmi_console_ready_until_cyw43_terminal(&mut self) {
        let stale_ready_visible = self
            .mirrored_lines
            .iter()
            .any(|line| local_seat_cyw43_stale_ready_line(line.as_str()));
        self.hdmi_bootstrap_terminal_ready = false;
        if !stale_ready_visible {
            return;
        }

        let retained_open_line = if self.hdmi_open_line {
            let floor = self.hdmi_open_line_floor_bytes;
            let mirrors_input = self.hdmi_open_line_mirrors_input;
            self.mirrored_lines.pop_back().and_then(|line| {
                (!local_seat_cyw43_stale_ready_line(line.as_str())).then_some((
                    line,
                    floor,
                    mirrors_input,
                ))
            })
        } else {
            None
        };
        self.hdmi_open_line = false;
        self.hdmi_open_line_floor_bytes = 0;
        self.hdmi_open_line_mirrors_input = false;
        self.mirrored_lines
            .retain(|line| !local_seat_cyw43_stale_ready_line(line.as_str()));
        self.mirror_line_current_tcb("[drivers] WiFi stabilizing; readiness retracted");
        if let Some((open_line, floor, mirrors_input)) = retained_open_line {
            self.mirrored_lines.push_back(open_line);
            self.hdmi_open_line = true;
            self.hdmi_open_line_floor_bytes = floor;
            self.hdmi_open_line_mirrors_input = mirrors_input;
        }
        let superseded = self
            .hdmi_pending_bytes
            .len()
            .saturating_add(self.hdmi_redraw_bytes.len());
        self.hdmi_pending_bytes.clear();
        self.hdmi_redraw_bytes.clear();
        self.hdmi_pending_redraw = false;
        self.hdmi_pending_high_impact = false;
        self.hdmi_high_impact_bytes_remaining = 0;
        self.hdmi_superseded_bytes = self.hdmi_superseded_bytes.saturating_add(superseded as u64);
        // A full canonical redraw supersedes Wi-Fi Ready bytes that were
        // queued but not yet issued while retaining the independent root
        // prompt/open input row. If a stale frame is already in flight, this
        // is the first legal following display transaction and replaces it
        // with the retracted state.
        self.request_linked_hdmi_snapshot_redraw();
    }

    /// Record terminal Wi-Fi display status without gating local readiness.
    pub fn mark_cyw43_bootstrap_terminal_for_hdmi(&mut self) {
        self.hdmi_bootstrap_terminal_ready = true;
        #[cfg(all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        ))]
        {
            self.mark_linked_hdmi_keyboard_command_ready_once();
            self.release_pending_linked_hdmi_prompt_if_display_ready();
        }
    }

    /// Return whether the deferred Wi-Fi status episode reached a terminal.
    #[must_use]
    pub const fn hdmi_bootstrap_terminal_ready(&self) -> bool {
        self.hdmi_bootstrap_terminal_ready
    }

    /// Return whether the final interactive HDMI banner was queued.
    #[must_use]
    pub const fn hdmi_console_ready_line_emitted(&self) -> bool {
        self.hdmi_console_ready_line_emitted
    }

    /// Return whether the exact local-console banner frame completed.
    #[must_use]
    pub const fn hdmi_console_ready_line_completed(&self) -> bool {
        self.hdmi_console_ready_line_completed
    }

    /// Exercise the production HDMI ready-banner and prompt ordering after a
    /// test fixture supplies USB command/display readiness.
    #[cfg(test)]
    pub(crate) fn release_linked_hdmi_console_for_test(&mut self, prompt: &str) {
        self.emit_linked_hdmi_console_ready_if_due(true);
        if self.hdmi_console_ready_line_completed {
            self.mirror_linked_hdmi_prompt_now(prompt);
        }
    }

    /// Complete every currently queued HDMI frame through the production
    /// receipt state machine for cross-module ordering tests.
    #[cfg(test)]
    pub(crate) fn complete_linked_hdmi_output_for_test(&mut self) {
        let mut frames = 0usize;
        while let Some((_, _, _)) = self.next_linked_hdmi_payload() {
            let receipt = self.hdmi_last_payload_receipt;
            self.record_linked_hdmi_submit_success(receipt);
            frames = frames.saturating_add(1);
            assert!(frames <= 64, "bounded HDMI test output must converge");
        }
    }

    /// Apply the physical Pi USB-not-ready prompt fence in a host event test.
    #[cfg(test)]
    pub(crate) fn withhold_linked_hdmi_prompt_for_test(&mut self, prompt: &str) {
        self.update_linked_hdmi_keyboard_starting_line_for_state(false, false);
        let prompt = truncate_for_display(prompt, self.status.line_bytes);
        self.mirrored_lines.retain(|line| line.as_str() != prompt);
        self.hdmi_open_line = false;
        self.hdmi_open_line_floor_bytes = 0;
        self.hdmi_open_line_mirrors_input = false;
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
            let _ = self.schedule_linked_hdmi_initial_snapshot_if_due(ready);
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

    /// Whether the isolated HDMI runtime has completed exact owner transfer.
    #[must_use]
    pub fn linked_display_runtime_attached(&self) -> bool {
        #[cfg(all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        ))]
        {
            return LINKED_LOCAL_SEAT_DISPLAY_ATTACHED.load(Ordering::Acquire);
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

    /// Schedule the canonical first viewport immediately after HDMI attach.
    ///
    /// A full snapshot supersedes startup bytes already queued before attach.
    /// Scheduling at the attach transition prevents that snapshot from being
    /// deferred until the operator types the first USB command.
    #[cfg(any(
        test,
        all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        )
    ))]
    fn schedule_linked_hdmi_initial_snapshot_if_due(&mut self, display_ready: bool) -> bool {
        if !local_seat_hdmi_initial_snapshot_due(
            display_ready,
            self.hdmi_initial_snapshot_scheduled,
        ) {
            return false;
        }
        self.hdmi_initial_snapshot_scheduled = true;
        self.render_linked_hdmi_scrollback();
        true
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
                let was_enabled = if self.keyboard_probe_cursor.active {
                    self.keyboard_probe_cursor.finish()
                } else {
                    false
                };
                self.backend_keyboard_polling_enabled = false;
                if was_enabled {
                    self.backend_keyboard_polling_enabled = true;
                }
                self.backend_keyboard_poll_deferred_logged = false;
                return LocalSeatKeyboardProbeResult::DeferredUntilRootConsole;
            }
            self.keyboard_probe_cursor.begin(
                self.backend_keyboard_polling_enabled,
                latest_usb_enumeration_progress_token(),
            );
            self.backend_keyboard_polling_enabled = true;
            self.clear_keyboard_poll_no_reply_backoff();
            let turn = self.service_backend_keyboard_turn();
            return local_seat_keyboard_probe_result(
                turn,
                local_seat_driver_runtime_keyboard_attached(),
                local_seat_usb_controller_runtime_attached(),
            );
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
        let changed = self.hdmi_scrollback_offset != previous;
        if changed
            && previous == 0
            && self.hdmi_rendered_scrollback_offset == 0
            && !self.hdmi_pending_bytes.is_empty()
        {
            // A CSI scroll is defined against the physical viewport. Seal the
            // finite FIFO prefix that existed when the operator pressed the
            // arrow, drain that prefix with immutable receipt identity, then
            // scroll from its exact completed tail anchor. Later mirror output
            // remains behind the scroll and cannot extend this fence.
            if !self.hdmi_rendered_viewport_valid
                || self.linked_hdmi_snapshot_recovery_required()
                || !self.begin_linked_hdmi_scroll_output_fence()
            {
                self.hdmi_rendered_viewport_valid = false;
                self.request_linked_hdmi_snapshot_redraw();
            }
        }
        changed
    }

    fn ensure_linked_hdmi_open_input_line(&mut self) {
        if self.hdmi_open_line {
            return;
        }
        #[cfg(any(
            test,
            all(
                feature = "kernel",
                feature = "usb",
                target_arch = "aarch64",
                target_os = "none"
            )
        ))]
        let previous_rendered_offset = self.linked_hdmi_unsubmitted_live_tail_anchor();
        self.mirror_line_current_tcb("");
        #[cfg(any(
            test,
            all(
                feature = "kernel",
                feature = "usb",
                target_arch = "aarch64",
                target_os = "none"
            )
        ))]
        self.rebase_linked_hdmi_unsubmitted_live_tail(previous_rendered_offset);
        self.hdmi_open_line = true;
        self.hdmi_open_line_floor_bytes = 0;
        self.hdmi_open_line_mirrors_input = true;
    }

    fn record_linked_hdmi_input_echo_byte(&mut self, byte: u8) -> bool {
        match byte {
            b'\r' | b'\n' => {
                let was_open = self.hdmi_open_line;
                self.close_linked_hdmi_open_line(false);
                was_open
            }
            0x08 | 0x7f => {
                if !self.hdmi_open_line {
                    return false;
                }
                if let Some(line) = self.mirrored_lines.back_mut() {
                    if line.len() > self.hdmi_open_line_floor_bytes {
                        line.pop();
                        self.hdmi_open_line_mirrors_input = true;
                        return true;
                    }
                }
                false
            }
            b'\t' => {
                let mut changed = false;
                for _ in 0..4 {
                    changed |= self.record_linked_hdmi_input_echo_byte(b' ');
                }
                changed
            }
            byte if byte.is_ascii_control() => false,
            byte => {
                self.ensure_linked_hdmi_open_input_line();
                if let Some(line) = self.mirrored_lines.back_mut() {
                    if line.len() < usize::from(self.status.line_bytes) {
                        line.push(byte as char);
                        self.hdmi_open_line_mirrors_input = true;
                        return true;
                    }
                }
                false
            }
        }
    }

    fn mark_linked_hdmi_input_row_dirty(&mut self) {
        let Some(payload) = self.build_linked_hdmi_current_input_line_payload() else {
            return;
        };
        self.set_linked_hdmi_input_row_payload(payload);
    }

    fn set_linked_hdmi_input_row_payload(&mut self, payload: Vec<u8>) {
        self.hdmi_input_row_generation = self.hdmi_input_row_generation.wrapping_add(1);
        if self.hdmi_input_row_generation == 0 {
            self.hdmi_input_row_generation = 1;
        }
        self.hdmi_input_row_payload = payload;
        self.hdmi_input_row_dirty = true;
    }

    fn retain_linked_hdmi_fifo_before_input_row(&mut self) {
        if self.hdmi_pending_bytes.is_empty() {
            return;
        }
        self.hdmi_high_impact_bytes_remaining = self.hdmi_pending_bytes.len();
        self.hdmi_pending_high_impact = true;
    }

    #[cfg(any(
        test,
        all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        )
    ))]
    fn retract_linked_hdmi_prompt_until_usb_ready(&mut self, prompt: &str) -> bool {
        if !self.hdmi_open_line {
            return false;
        }
        let prompt = truncate_for_display(prompt, self.status.line_bytes);
        let Some(line) = self.mirrored_lines.back_mut() else {
            return false;
        };
        let suffix_start = if line.starts_with(prompt) {
            prompt.len()
        } else {
            self.hdmi_open_line_floor_bytes.min(line.len())
        };
        let Some(suffix) = line.get(suffix_start..) else {
            return false;
        };
        let retained_input = String::from(suffix);
        *line = retained_input;
        self.hdmi_open_line_floor_bytes = 0;
        self.hdmi_open_line_mirrors_input = true;

        self.retain_linked_hdmi_fifo_before_input_row();
        let mut clear_row = Vec::new();
        clear_row.extend_from_slice(b"\r\x1b[2K");
        self.set_linked_hdmi_input_row_payload(clear_row);
        true
    }

    #[cfg(any(
        test,
        all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        )
    ))]
    fn retract_linked_hdmi_console_ready_until_usb_ready(&mut self) -> bool {
        let ready_line = "Cohesix console ready";
        let ready_visible = self.hdmi_console_ready_line_emitted
            || self
                .mirrored_lines
                .iter()
                .any(|line| line.as_str() == ready_line);
        self.hdmi_console_ready_line_emitted = false;
        self.hdmi_console_ready_line_completed = false;
        self.hdmi_console_ready_line_generation = 0;
        self.hdmi_console_ready_pending_bytes = 0;
        if !ready_visible {
            return false;
        }

        let retained_open_line = if self.hdmi_open_line {
            let floor = self.hdmi_open_line_floor_bytes;
            let mirrors_input = self.hdmi_open_line_mirrors_input;
            self.mirrored_lines.pop_back().and_then(|line| {
                (line.as_str() != ready_line).then_some((line, floor, mirrors_input))
            })
        } else {
            None
        };
        self.hdmi_open_line = false;
        self.hdmi_open_line_floor_bytes = 0;
        self.hdmi_open_line_mirrors_input = false;
        self.mirrored_lines
            .retain(|line| line.as_str() != ready_line);
        if let Some((open_line, floor, mirrors_input)) = retained_open_line {
            self.mirrored_lines.push_back(open_line);
            self.hdmi_open_line = true;
            self.hdmi_open_line_floor_bytes = floor;
            self.hdmi_open_line_mirrors_input = mirrors_input;
        }

        let superseded = self
            .hdmi_pending_bytes
            .len()
            .saturating_add(self.hdmi_redraw_bytes.len());
        self.hdmi_pending_bytes.clear();
        self.hdmi_redraw_bytes.clear();
        self.hdmi_pending_redraw = false;
        self.hdmi_pending_high_impact = false;
        self.hdmi_high_impact_bytes_remaining = 0;
        self.hdmi_superseded_bytes = self.hdmi_superseded_bytes.saturating_add(superseded as u64);
        self.request_linked_hdmi_snapshot_redraw();
        true
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn queue_linked_hdmi_input_row_or_redraw(&mut self) {
        self.mark_linked_hdmi_input_row_dirty();
        if self.hdmi_scrollback_offset != 0 {
            return;
        }
        if self.hdmi_stale_after_retry_exhaustion {
            self.request_linked_hdmi_snapshot_redraw();
            return;
        }
        if self.linked_hdmi_snapshot_recovery_required() {
            return;
        }
        self.retain_linked_hdmi_fifo_before_input_row();
    }

    fn build_linked_hdmi_current_input_line_payload(&self) -> Option<Vec<u8>> {
        let line = self.mirrored_lines.back()?;
        let row_closed = !self.hdmi_open_line;
        let mut payload = Vec::new();
        payload.extend_from_slice(b"\r\x1b[2K");
        for &byte in line.as_bytes() {
            payload.push(byte);
        }
        payload.extend_from_slice(b"\x1b[K");
        if row_closed {
            payload.push(b'\n');
        }
        Some(payload)
    }

    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    fn mirror_input_bytes_to_display(&mut self, bytes: &[u8], preview_changed: bool) {
        if bytes.is_empty() || !local_seat_hdmi_input_echo_allowed(self.root_console_ready) {
            return;
        }
        if crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active() {
            self.release_pending_linked_hdmi_prompt_if_display_ready();
            let mut scroll_delta = 0i32;
            let mut viewport_changed = false;
            let mut input_row_changed = false;
            for &byte in bytes {
                match self.hdmi_input_escape_state {
                    0 if byte == 0x1b => {
                        self.hdmi_input_escape_state = 1;
                    }
                    0 => {
                        if local_seat_hdmi_input_echo_byte(byte) {
                            if self.record_linked_hdmi_input_echo_byte(byte) {
                                input_row_changed = true;
                            }
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
            if input_row_changed {
                if self.hdmi_scrollback_offset != 0 {
                    self.hdmi_scrollback_offset = 0;
                    viewport_changed = true;
                }
                self.queue_linked_hdmi_input_row_or_redraw();
            } else if preview_changed && self.hdmi_scrollback_offset != 0 {
                self.hdmi_scrollback_offset = 0;
                viewport_changed = true;
            }
            if self.apply_linked_hdmi_scroll_delta(scroll_delta) {
                viewport_changed = true;
            }
            if viewport_changed
                && !self.hdmi_rendered_viewport_valid
                && !self.linked_hdmi_redraw_pending()
            {
                self.request_linked_hdmi_snapshot_redraw();
            }
        }
    }

    #[cfg(any(
        test,
        all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        )
    ))]
    fn render_linked_hdmi_scrollback(&mut self) {
        let previous_offset = self.hdmi_scrollback_offset;
        let max_offset = self.max_linked_hdmi_scrollback_offset();
        self.hdmi_scrollback_offset = self.hdmi_scrollback_offset.min(max_offset);
        if self.hdmi_scrollback_offset == previous_offset
            && (self.linked_hdmi_redraw_pending()
                || (self.hdmi_rendered_viewport_valid
                    && self.hdmi_rendered_scrollback_offset != self.hdmi_scrollback_offset))
        {
            self.hdmi_coalesced_redraws = self.hdmi_coalesced_redraws.saturating_add(1);
            return;
        }
        if !self.hdmi_rendered_viewport_valid && !self.linked_hdmi_redraw_pending() {
            self.request_linked_hdmi_snapshot_redraw();
        }
    }

    fn keyboard_poll_no_reply_cooldown_active(&mut self) -> bool {
        if self.keyboard_poll_no_reply_cooldown == 0 {
            return false;
        }
        self.keyboard_poll_no_reply_cooldown =
            self.keyboard_poll_no_reply_cooldown.saturating_sub(1);
        self.keyboard_poll_cooldown_skips = self.keyboard_poll_cooldown_skips.saturating_add(1);
        #[cfg(all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        ))]
        {
            if local_seat_keyboard_cooldown_should_show_busy(
                self.hdmi_keyboard_command_ready_latched || self.linked_usb_command_input_ready(),
                self.keyboard_poll_no_reply_streak,
                self.keyboard_recovery_aux_pending,
            ) {
                self.emit_linked_hdmi_keyboard_busy_line_once("keyboard-poll-cooldown");
            }
        }
        true
    }

    fn clear_keyboard_poll_no_reply_backoff(&mut self) {
        self.keyboard_poll_no_reply_cooldown = 0;
        self.keyboard_poll_no_reply_backoff = 0;
    }

    fn record_keyboard_poll_completion(&mut self) {
        self.keyboard_poll_no_reply_streak = 0;
        self.keyboard_recovery_aux_pending = false;
        self.clear_keyboard_poll_no_reply_backoff();
    }

    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    fn reset_keyboard_post_first_byte_clean_proof(&mut self) {
        self.keyboard_post_first_byte_clean_polls = 0;
        self.reset_keyboard_post_first_byte_pressure_baseline();
    }

    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    fn invalidate_linked_usb_command_ready(&mut self, reason: &'static str) {
        if !self.hdmi_keyboard_command_ready_latched
            && self.keyboard_post_first_byte_clean_polls == 0
            && !self.hdmi_keyboard_wait_line_emitted
        {
            return;
        }
        self.hdmi_keyboard_command_ready_latched = false;
        self.hdmi_keyboard_wait_line_emitted = false;
        self.hdmi_keyboard_busy_line_emitted = false;
        let _ = self.retract_linked_hdmi_console_ready_until_usb_ready();
        if !self.hdmi_pending_prompt.is_empty() {
            let mut prompt = heapless::String::<LOCAL_SEAT_HDMI_PENDING_PROMPT_BYTES>::new();
            let _ = prompt.push_str(self.hdmi_pending_prompt.as_str());
            self.hdmi_prompt_pending_until_display_ready = true;
            self.hdmi_prompt_pending_reason = "usb-command-pending";
            let _ = self.retract_linked_hdmi_prompt_until_usb_ready(prompt.as_str());
        }
        self.reset_keyboard_post_first_byte_clean_proof();
        let mut line = heapless::String::<192>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] usb keyboard command-ready invalidated reason={} action=fail-closed",
                reason
            ),
        );
        boot_log::force_log_buffer_line_or_uart_without_prompt_refresh(line.as_str());
    }

    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    fn mark_keyboard_post_first_byte_recovery(&mut self, reason: &'static str) {
        self.reset_keyboard_post_first_byte_clean_proof();
        if self.linked_usb_prompt_hard_blocked() {
            self.emit_linked_hdmi_keyboard_busy_line_once(reason);
        }
    }

    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    fn record_keyboard_post_first_byte_clean_poll(&mut self, result: u32) {
        if !crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active()
            || !LINKED_LOCAL_SEAT_USB_FIRST_REPORT_READY_LOGGED.load(Ordering::Acquire)
        {
            return;
        }
        if self.keyboard_recovery_aux_pending || self.keyboard_poll_no_reply_streak != 0 {
            let reason = if self.keyboard_poll_no_reply_streak != 0 {
                "keyboard-poll-no-reply"
            } else {
                "keyboard-recovery-pending"
            };
            self.mark_keyboard_post_first_byte_recovery(reason);
            return;
        }
        if local_seat_keyboard_result_blocks_prompt_ready(
            result,
            LINKED_LOCAL_SEAT_USB_FIRST_BYTE_READY_LOGGED.load(Ordering::Acquire),
        ) {
            self.reset_keyboard_post_first_byte_clean_proof();
            return;
        }
        if self.linked_usb_post_first_byte_pressure_active() {
            self.emit_linked_hdmi_keyboard_busy_line_once("post-first-byte-pressure");
            self.reset_keyboard_post_first_byte_clean_proof();
            return;
        }
        self.keyboard_post_first_byte_clean_polls = self
            .keyboard_post_first_byte_clean_polls
            .saturating_add(1)
            .min(LINKED_LOCAL_SEAT_USB_PROMPT_READY_CLEAN_POLLS);
        self.release_pending_linked_hdmi_prompt_if_display_ready();
    }

    fn record_keyboard_poll_idle_completion(&mut self) {
        self.keyboard_poll_no_reply_streak = 0;
        self.keyboard_recovery_aux_pending = false;
        let idle_cooldown = local_seat_keyboard_poll_idle_completion_cooldown(
            self.keyboard_poll_active_queue_idle(),
        );
        if idle_cooldown == 0 {
            self.clear_keyboard_poll_no_reply_backoff();
        } else {
            self.keyboard_poll_no_reply_backoff = idle_cooldown;
            self.keyboard_poll_no_reply_cooldown = idle_cooldown;
        }
    }

    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    fn emit_keyboard_recovery_request(&self, action: &str) {
        if !keyboard_recovery_request_log_visible(
            self.keyboard_recovery_aux_requests as usize,
            action,
        ) {
            return;
        }
        emit_usb_keyboard_recovery_request(action, self.keyboard_trace());
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    fn record_keyboard_poll_aux_submission(&mut self, aux0: u32) -> bool {
        if aux0 == DRIVER_RUNTIME_USB_ENUMERATE_AUX {
            self.keyboard_enumeration_aux_requests =
                self.keyboard_enumeration_aux_requests.saturating_add(1);
            false
        } else if aux0 == DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_AUX {
            #[cfg(all(target_arch = "aarch64", target_os = "none"))]
            self.reset_keyboard_post_first_byte_clean_proof();
            self.keyboard_recovery_aux_pending = true;
            self.keyboard_recovery_aux_requests =
                self.keyboard_recovery_aux_requests.saturating_add(1);
            #[cfg(all(target_arch = "aarch64", target_os = "none"))]
            self.emit_keyboard_recovery_request("submit");
            true
        } else {
            false
        }
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

    fn keyboard_poll_active_queue_idle(&self) -> bool {
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
            local_seat_keyboard_active_queue_idle(queued_reports, report_status)
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

    fn keyboard_poll_stale_runtime_queue(&self) -> bool {
        #[cfg(all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        ))]
        {
            if !crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active() {
                return false;
            }
            let first_report_ready =
                LINKED_LOCAL_SEAT_USB_FIRST_REPORT_READY_LOGGED.load(Ordering::Acquire);
            let first_byte_ready =
                LINKED_LOCAL_SEAT_USB_FIRST_BYTE_READY_LOGGED.load(Ordering::Acquire);
            let result = LINKED_LOCAL_SEAT_USB_LAST_RESULT.load(Ordering::Acquire) as u32;
            let queued_reports = result & 0xff;
            let report_status = (result >> DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_SHIFT)
                & DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_MASK;
            if !first_report_ready && !first_byte_ready {
                let pre_first_no_event_queue =
                    local_seat_keyboard_pre_first_report_no_event_queue_stalled(result);
                let pre_first_steady_full_queue =
                    local_seat_keyboard_pre_first_report_steady_full_queue_stalled(result);
                let pre_first_no_event_recovery_due = pre_first_no_event_queue
                    && self.keyboard_poll_no_reply_streak
                        >= LINKED_LOCAL_SEAT_USB_POST_FIRST_BYTE_RECOVERY_NO_REPLY_THRESHOLD;
                return local_seat_keyboard_recovery_probe_stalled(queued_reports, report_status)
                    || pre_first_steady_full_queue
                    || pre_first_no_event_recovery_due;
            }
            if !first_byte_ready && local_seat_keyboard_pre_first_report_full_queue_stalled(result)
            {
                return true;
            }
            queued_reports > LINKED_LOCAL_SEAT_USB_READY_MAX_QUEUED_REPORTS
                || local_seat_keyboard_recovery_probe_stalled(queued_reports, report_status)
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
        let result = LINKED_LOCAL_SEAT_USB_LAST_RESULT.load(Ordering::Acquire) as u32;
        let queued_reports = result & 0xff;
        let report_status = (result >> DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_SHIFT)
            & DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_MASK;
        let cached_unmatched_transfer = report_status
            == u32::from(DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_UNMATCHED_TRANSFER);
        let cached_stale_runtime_queue = self.keyboard_poll_stale_runtime_queue();
        let cached_active_idle_runtime_queue =
            local_seat_keyboard_active_queue_idle(queued_reports, report_status);
        let first_report_ready =
            LINKED_LOCAL_SEAT_USB_FIRST_REPORT_READY_LOGGED.load(Ordering::Acquire);
        let first_byte_ready =
            LINKED_LOCAL_SEAT_USB_FIRST_BYTE_READY_LOGGED.load(Ordering::Acquire);
        let cached_pre_first_report_steady_full_queue = !first_byte_ready
            && local_seat_keyboard_pre_first_report_steady_full_queue_stalled(result);
        let cached_pre_first_report_no_event_queue =
            local_seat_keyboard_pre_first_report_recovery_due(first_byte_ready, result);
        let cached_pre_first_report_no_event_recovery_due =
            cached_pre_first_report_steady_full_queue || cached_pre_first_report_no_event_queue;
        local_seat_keyboard_recovery_aux_allowed_for_status(
            self.keyboard_queue.is_empty(),
            self.keyboard_poll_no_reply_streak,
            self.keyboard_recovery_aux_pending,
            cached_unmatched_transfer,
            cached_stale_runtime_queue,
            cached_pre_first_report_no_event_recovery_due,
            cached_active_idle_runtime_queue,
        ) && crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active()
            && local_seat_keyboard_recovery_aux_keyboard_gate(
                LINKED_LOCAL_SEAT_USB_KEYBOARD_READY.load(Ordering::Acquire),
                first_report_ready,
                cached_pre_first_report_no_event_recovery_due,
            )
    }

    fn record_keyboard_poll_no_reply(&mut self) {
        self.driver_task_no_replies = self.driver_task_no_replies.saturating_add(1);
        self.keyboard_poll_no_reply_streak = self.keyboard_poll_no_reply_streak.saturating_add(1);
        #[cfg(all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        ))]
        if LINKED_LOCAL_SEAT_USB_FIRST_BYTE_READY_LOGGED.load(Ordering::Acquire) {
            self.mark_keyboard_post_first_byte_recovery("keyboard-poll-no-reply");
        }
        let active_queue_idle = self.keyboard_poll_active_queue_idle();
        let recovery_pending_clear_threshold = if active_queue_idle {
            LINKED_LOCAL_SEAT_USB_POST_FIRST_BYTE_IDLE_RECOVERY_NO_REPLY_THRESHOLD
        } else {
            LINKED_LOCAL_SEAT_USB_POST_FIRST_BYTE_RECOVERY_NO_REPLY_THRESHOLD
        };
        if self.keyboard_recovery_aux_pending
            && !self.keyboard_poll_stale_runtime_queue()
            && self
                .keyboard_poll_no_reply_streak
                .is_multiple_of(recovery_pending_clear_threshold)
        {
            self.keyboard_recovery_aux_pending = false;
        }
        let next = local_seat_keyboard_poll_next_no_reply_backoff_for_queue(
            self.keyboard_poll_fast_recovery_active(),
            active_queue_idle,
            self.keyboard_poll_no_reply_backoff,
        );
        self.keyboard_poll_no_reply_backoff = next;
        self.keyboard_poll_no_reply_cooldown = next;
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    fn resolve_linked_usb_poll_retained_turn(
        &mut self,
        turn: crate::hal::driver_task::DriverTaskRetainedServiceTurn,
    ) -> Result<Option<crate::hal::driver_task::DriverTaskCompletionRecord>, &'static str> {
        match turn {
            crate::hal::driver_task::DriverTaskRetainedServiceTurn::Pending => Ok(None),
            crate::hal::driver_task::DriverTaskRetainedServiceTurn::Complete(completion) => {
                self.linked_usb_poll_command = None;
                Ok(Some(completion))
            }
            crate::hal::driver_task::DriverTaskRetainedServiceTurn::Failed => {
                self.linked_usb_poll_command = None;
                #[cfg(all(target_arch = "aarch64", target_os = "none"))]
                self.invalidate_linked_usb_command_ready("retained-service-failed");
                self.record_keyboard_poll_no_reply();
                Err("usb-retained-service-failed")
            }
        }
    }

    /// Execute one retained platform-keyboard service turn.
    ///
    /// The returned outcome describes retained child-runtime work without
    /// inviting callers to spin. `Pending` requires a fresh outer event turn.
    pub fn service_backend_keyboard_turn(&mut self) -> LocalSeatServiceTurn {
        let outcome = self.poll_backend_keyboard_turn();
        #[cfg(all(
            feature = "kernel",
            feature = "usb",
            target_arch = "aarch64",
            target_os = "none"
        ))]
        self.reconcile_keyboard_probe_cursor(outcome);
        outcome
    }

    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    fn reconcile_keyboard_probe_cursor(&mut self, outcome: LocalSeatServiceTurn) {
        if !self.keyboard_probe_cursor.active {
            return;
        }
        if matches!(outcome, LocalSeatServiceTurn::Complete)
            && local_seat_driver_runtime_keyboard_attached()
        {
            let _ = self.keyboard_probe_cursor.finish();
            self.backend_keyboard_polling_enabled = true;
            return;
        }
        let progress = latest_usb_enumeration_progress_token();
        let active_request =
            crate::hal::driver_task::driver_task_ring_command_active(driver_task_contract());
        let retained_action = matches!(outcome, LocalSeatServiceTurn::Pending)
            || self.linked_usb_attach_command.is_some()
            || self.linked_usb_poll_command.is_some();
        let retain = self.keyboard_probe_cursor.retain_after_turn(
            progress,
            active_request,
            self.root_console_ready,
            retained_action,
        );
        if !retain || matches!(outcome, LocalSeatServiceTurn::Failed(_)) {
            let keep_polling = self.keyboard_probe_cursor.finish()
                || local_seat_driver_runtime_keyboard_attached();
            self.backend_keyboard_polling_enabled = keep_polling;
            if !keep_polling {
                self.backend_keyboard_poll_deferred_logged = false;
            }
        }
    }

    /// Whether an explicit keyboard probe retains unfinished work.
    #[must_use]
    pub fn backend_keyboard_probe_pending(&self) -> bool {
        #[cfg(all(feature = "kernel", feature = "usb"))]
        {
            return self.keyboard_probe_cursor.active;
        }
        #[cfg(not(all(feature = "kernel", feature = "usb")))]
        {
            false
        }
    }

    /// Poll the platform local-seat input backend and enqueue discovered bytes.
    ///
    /// This compatibility wrapper performs no more than one retained child
    /// operation. New orchestration should use
    /// [`Self::service_backend_keyboard_turn`] and honor `Pending`.
    pub fn poll_backend_keyboard(&mut self) {
        let _ = self.poll_backend_keyboard_turn();
    }

    fn poll_backend_keyboard_turn(&mut self) -> LocalSeatServiceTurn {
        if !self.backend_keyboard_polling_enabled {
            #[cfg(all(
                feature = "kernel",
                feature = "usb",
                target_arch = "aarch64",
                target_os = "none"
            ))]
            {
                if crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active()
                    && (LINKED_LOCAL_SEAT_USB_KEYBOARD_READY.load(Ordering::Acquire)
                        || LINKED_LOCAL_SEAT_USB_ENUMERATION_PENDING.load(Ordering::Acquire)
                        || self.root_console_ready)
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
            return LocalSeatServiceTurn::Complete;
        }
        // A retained request already owns the runtime ring. Poll that exact
        // ticket on every outer turn; applying a newly armed no-reply
        // cooldown here would strand the issued request until the cooldown
        // expired and make completion latency depend on unrelated backoff.
        #[cfg(all(feature = "kernel", feature = "usb"))]
        let retained_poll_active = self.linked_usb_poll_command.is_some()
            || self.linked_usb_attach_command.is_some()
            || self.keyboard_probe_cursor.active
            || crate::hal::driver_task::driver_task_ring_command_active(
                crate::hal::driver_task::PCIE_ROOT_DRIVER_TASK_CONTRACT,
            )
            || crate::hal::driver_task::driver_task_ring_command_active(driver_task_contract());
        #[cfg(not(all(feature = "kernel", feature = "usb")))]
        let retained_poll_active = false;
        if !retained_poll_active && self.keyboard_poll_no_reply_cooldown_active() {
            return LocalSeatServiceTurn::Pending;
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
            return LocalSeatServiceTurn::Pending;
        }

        let contract = driver_task_contract();
        #[cfg(feature = "kernel")]
        {
            #[cfg(all(feature = "usb", target_arch = "aarch64", target_os = "none"))]
            if crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active() {
                let controller_attached =
                    LINKED_LOCAL_SEAT_RUNTIME_ATTACHED.load(Ordering::Acquire);
                let keyboard_ready = LINKED_LOCAL_SEAT_USB_KEYBOARD_READY.load(Ordering::Acquire);
                let mut enumeration_pending =
                    LINKED_LOCAL_SEAT_USB_ENUMERATION_PENDING.load(Ordering::Acquire);
                if linked_local_seat_usb_resume_enumeration_required(
                    controller_attached,
                    keyboard_ready,
                    enumeration_pending,
                ) {
                    LINKED_LOCAL_SEAT_USB_ENUMERATION_PENDING.store(true, Ordering::Release);
                    enumeration_pending = true;
                }
                if linked_local_seat_usb_pre_prompt_retry_deferred(
                    controller_attached,
                    keyboard_ready,
                    enumeration_pending,
                    self.root_console_ready,
                ) {
                    return LocalSeatServiceTurn::Pending;
                }
                if linked_local_seat_usb_attach_probe_required(
                    controller_attached,
                    keyboard_ready,
                    enumeration_pending,
                ) && self.linked_usb_poll_command.is_none()
                {
                    let outcome = service_linked_local_seat_runtime_attach_turn(self);
                    if matches!(outcome, LocalSeatServiceTurn::Failed(_)) {
                        self.record_keyboard_poll_no_reply();
                        let _ = local_seat_keyboard_poll_suspends_on_missing_reply(
                            true,
                            self.root_console_ready,
                            keyboard_ready || enumeration_pending,
                        );
                    }
                    return outcome;
                }
                if LINKED_LOCAL_SEAT_USB_KEYBOARD_READY.load(Ordering::Acquire) {
                    self.mark_linked_hdmi_keyboard_command_ready_once();
                    self.release_pending_linked_hdmi_prompt_if_display_ready();
                }
                crate::hal::driver_task::register_driver_task_pointer_free_ring_service(
                    contract,
                    crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard.as_u32() as usize,
                    usb_keyboard_runtime_ring_service_driver_task,
                );
                self.backend_keyboard_poll_calls =
                    self.backend_keyboard_poll_calls.saturating_add(1);
                let command = if let Some(command) = self.linked_usb_poll_command {
                    command
                } else {
                    let mut command =
                        crate::hal::driver_task::DriverTaskCommandRecord::pi4_hot_path(
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
                        LINKED_LOCAL_SEAT_USB_ENUMERATION_PENDING.load(Ordering::Acquire),
                        self.keyboard_poll_recovery_aux_requested(),
                    );
                    let _ = self.record_keyboard_poll_aux_submission(command.aux0);
                    self.linked_usb_poll_command = Some(command);
                    command
                };
                let submitted_recovery_aux =
                    command.aux0 == DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_AUX;
                let turn =
                    crate::hal::driver_task::run_driver_task_ring_service_retained_service_turn(
                        contract, command,
                    );
                let completion = match self.resolve_linked_usb_poll_retained_turn(turn) {
                    Ok(None) => return LocalSeatServiceTurn::Pending,
                    Ok(Some(completion)) => completion,
                    Err(reason) => {
                        if submitted_recovery_aux {
                            self.emit_keyboard_recovery_request("failed");
                        }
                        let _ = local_seat_keyboard_poll_suspends_on_missing_reply(
                            true,
                            self.root_console_ready,
                            LINKED_LOCAL_SEAT_USB_KEYBOARD_READY.load(Ordering::Acquire)
                                || LINKED_LOCAL_SEAT_USB_ENUMERATION_PENDING
                                    .load(Ordering::Acquire),
                        );
                        return LocalSeatServiceTurn::Failed(reason);
                    }
                };
                if linked_usb_keyboard_input_frame_valid(completion) {
                    self.record_keyboard_poll_completion();
                    self.record_keyboard_post_first_byte_clean_poll(completion.result);
                    if !LINKED_LOCAL_SEAT_USB_FIRST_REPORT_READY_LOGGED.swap(true, Ordering::AcqRel)
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
                        let first_byte_ready_before =
                            LINKED_LOCAL_SEAT_USB_FIRST_BYTE_READY_LOGGED.load(Ordering::Acquire);
                        self.backend_keyboard_read_bytes = self
                            .backend_keyboard_read_bytes
                            .saturating_add(bytes.len() as u64);
                        LINKED_LOCAL_SEAT_USB_KEYBOARD_READY.store(true, Ordering::Release);
                        LINKED_LOCAL_SEAT_USB_ENUMERATION_PENDING.store(false, Ordering::Release);
                        self.mark_linked_hdmi_keyboard_command_ready_once();
                        let physical_pi_owner_state =
                                crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active();
                        let parser_ingress = local_seat_keyboard_bytes_enter_parser_state(
                            physical_pi_owner_state,
                            self.root_console_ready,
                            true,
                        );
                        let (accepted, arming_bytes) = self.accept_keyboard_bytes_for_parser_state(
                            bytes,
                            parser_ingress,
                            first_byte_ready_before,
                            physical_pi_owner_state,
                        );
                        if accepted != 0 && !first_byte_ready_before {
                            self.reset_keyboard_post_first_byte_clean_proof();
                        }
                        publish_linked_local_seat_usb_first_report_event(
                            contract,
                            completion,
                            bytes,
                            accepted,
                            arming_bytes,
                            parser_ingress,
                        );
                    }
                    return LocalSeatServiceTurn::Complete;
                }
                if completion.code
                    == crate::hal::driver_task::DriverTaskCompletionCode::FrameReady.as_u16()
                {
                    #[cfg(all(
                        feature = "kernel",
                        feature = "usb",
                        target_arch = "aarch64",
                        target_os = "none"
                    ))]
                    emit_linked_usb_keyboard_frame_rejected(contract, completion);
                    return LocalSeatServiceTurn::Complete;
                }
                if completion.code
                    == crate::hal::driver_task::DriverTaskCompletionCode::Progress.as_u16()
                    && completion.result != 0
                {
                    self.record_keyboard_poll_idle_completion();
                    if local_seat_usb_first_report_requires_reenumeration(completion) {
                        self.invalidate_linked_usb_command_ready("first-report-reenumeration");
                        self.reset_keyboard_post_first_byte_clean_proof();
                        mark_linked_local_seat_usb_keyboard_reenumeration_pending(
                            contract, completion,
                        );
                        return LocalSeatServiceTurn::Complete;
                    }
                    if completion.detail == DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_READY {
                        record_linked_local_seat_usb_detail(Some(completion));
                        LINKED_LOCAL_SEAT_USB_KEYBOARD_READY.store(true, Ordering::Release);
                        LINKED_LOCAL_SEAT_USB_ENUMERATION_PENDING.store(false, Ordering::Release);
                        self.mark_linked_hdmi_keyboard_command_ready_once();
                        self.release_pending_linked_hdmi_prompt_if_display_ready();
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
                        self.record_keyboard_post_first_byte_clean_poll(completion.result);
                        return LocalSeatServiceTurn::Complete;
                    }
                    if local_seat_usb_first_report_pending_endpoint_armed(completion) {
                        // Endpoint-armed and command-ready are deliberately
                        // separate proofs. The runtime returns PENDING while
                        // its attach/recovery idle guard suppresses input, so a
                        // prior READY/first-byte latch must not reopen parser
                        // ingress or Gates 9/10 before a fresh safe release.
                        let stale_report = LINKED_LOCAL_SEAT_USB_FIRST_REPORT_READY_LOGGED
                            .swap(false, Ordering::AcqRel);
                        let stale_byte = LINKED_LOCAL_SEAT_USB_FIRST_BYTE_READY_LOGGED
                            .swap(false, Ordering::AcqRel);
                        LINKED_LOCAL_SEAT_USB_KEYBOARD_READY.store(true, Ordering::Release);
                        LINKED_LOCAL_SEAT_USB_ENUMERATION_PENDING.store(false, Ordering::Release);
                        if stale_report || stale_byte || self.hdmi_keyboard_command_ready_latched {
                            self.invalidate_linked_usb_command_ready("first-report-rebaseline");
                        }
                        self.release_pending_linked_hdmi_prompt_if_display_ready();
                    }
                    if local_seat_usb_keyboard_enumeration_progress(completion) {
                        publish_local_seat_usb_enumeration_progress(contract, completion);
                        self.record_keyboard_post_first_byte_clean_poll(completion.result);
                        return LocalSeatServiceTurn::Complete;
                    }
                    record_linked_local_seat_usb_detail(Some(completion));
                    self.record_keyboard_post_first_byte_clean_poll(completion.result);
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
                    return LocalSeatServiceTurn::Complete;
                }
                if completion.code
                    == crate::hal::driver_task::DriverTaskCompletionCode::Idle.as_u16()
                {
                    self.record_keyboard_poll_idle_completion();
                    self.record_keyboard_post_first_byte_clean_poll(completion.result);
                    if !LINKED_LOCAL_SEAT_USB_FIRST_REPORT_PENDING_LOGGED
                        .swap(true, Ordering::AcqRel)
                    {
                        let status = if LINKED_LOCAL_SEAT_USB_KEYBOARD_READY.load(Ordering::Acquire)
                        {
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
                    return LocalSeatServiceTurn::Complete;
                }
                self.record_keyboard_poll_idle_completion();
                return LocalSeatServiceTurn::Complete;
            }
            if !crate::hal::driver_task::steady_state_root_compatibility_service_allowed(contract) {
                self.driver_task_budget_overruns =
                    self.driver_task_budget_overruns.saturating_add(1);
                return LocalSeatServiceTurn::Failed("root-compatibility-service-rejected");
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
                return LocalSeatServiceTurn::Complete;
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
                return LocalSeatServiceTurn::Complete;
            }
            self.record_keyboard_poll_no_reply();
            if !crate::hal::driver_task::admit_root_task_compatibility_service(contract) {
                self.driver_task_budget_overruns =
                    self.driver_task_budget_overruns.saturating_add(1);
                return LocalSeatServiceTurn::Failed("root-compatibility-service-unavailable");
            }
        }
        self.poll_backend_keyboard_current_tcb(contract);
        LocalSeatServiceTurn::Complete
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
        emit_hdmi_diagnostic_line(line.as_str());
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
            crate::hal::driver_task::emit_owner_state_transition_boot_contract_proof(
                crate::hal::driver_task::DriverTaskHotPath::HdmiText,
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

#[cfg(all(feature = "kernel", feature = "usb"))]
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

#[cfg(all(feature = "kernel", feature = "usb"))]
fn local_seat_usb_keyboard_report_status_from_result(result: u32) -> u32 {
    (result >> DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_SHIFT)
        & DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_MASK
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn local_seat_usb_first_report_requires_reenumeration(
    completion: crate::hal::driver_task::DriverTaskCompletionRecord,
) -> bool {
    local_seat_usb_first_report_requires_reenumeration_with_first_byte(
        completion,
        LINKED_LOCAL_SEAT_USB_FIRST_BYTE_READY_LOGGED.load(Ordering::Acquire),
    )
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    not(all(target_arch = "aarch64", target_os = "none"))
))]
fn local_seat_usb_first_report_requires_reenumeration(
    completion: crate::hal::driver_task::DriverTaskCompletionRecord,
) -> bool {
    local_seat_usb_first_report_requires_reenumeration_with_first_byte(completion, false)
}

#[cfg(all(feature = "kernel", feature = "usb"))]
fn local_seat_usb_first_report_requires_reenumeration_with_first_byte(
    completion: crate::hal::driver_task::DriverTaskCompletionRecord,
    first_byte_ready: bool,
) -> bool {
    if first_byte_ready
        || completion.code != crate::hal::driver_task::DriverTaskCompletionCode::Progress.as_u16()
    {
        return false;
    }
    if matches!(
        completion.detail,
        DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_PENDING
            | DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_READY
    ) && (local_seat_usb_keyboard_report_status_from_result(completion.result)
        == u32::from(DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_RECOVERY_FAILED)
        || local_seat_keyboard_pre_first_report_no_event_queue_stalled(completion.result))
    {
        return true;
    }
    local_seat_usb_completion_progress(completion)
        && linked_local_seat_usb_detail_warrants_recovery(completion.detail)
}

#[cfg(all(feature = "kernel", feature = "usb"))]
fn local_seat_usb_first_report_pending_endpoint_armed(
    completion: crate::hal::driver_task::DriverTaskCompletionRecord,
) -> bool {
    completion.code == crate::hal::driver_task::DriverTaskCompletionCode::Progress.as_u16()
        && completion.detail == DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_PENDING
        && local_seat_keyboard_pre_first_report_endpoint_armed(completion.result)
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn local_seat_usb_ready_invalidated_by_progress(
    completion: crate::hal::driver_task::DriverTaskCompletionRecord,
) -> bool {
    if !local_seat_usb_keyboard_enumeration_progress(completion) {
        return false;
    }
    let first_report_logged =
        LINKED_LOCAL_SEAT_USB_FIRST_REPORT_READY_LOGGED.load(Ordering::Acquire);
    let first_byte_logged = LINKED_LOCAL_SEAT_USB_FIRST_BYTE_READY_LOGGED.load(Ordering::Acquire);
    let keyboard_ready = LINKED_LOCAL_SEAT_USB_KEYBOARD_READY.load(Ordering::Acquire);
    if !(keyboard_ready || first_report_logged || first_byte_logged) {
        return false;
    }
    if first_byte_logged {
        return linked_local_seat_usb_detail_warrants_recovery(completion.detail);
    }
    linked_local_seat_usb_detail_warrants_recovery(completion.detail)
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

#[cfg(all(feature = "kernel", feature = "usb"))]
const fn linked_local_seat_usb_detail_warrants_recovery(detail: u16) -> bool {
    matches!(
        detail,
        pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_ENABLE_SLOT_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_ADDRESS_DEVICE_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_DESCRIPTOR_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_CONFIG_DESCRIPTOR_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HID_ATTACH_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_ATTACH_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_SET_CONFIG_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_DESCRIPTOR_FAILED
            | pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_CONTEXT_FAILED
    )
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn store_linked_local_seat_usb_completion(
    completion: crate::hal::driver_task::DriverTaskCompletionRecord,
) {
    LINKED_LOCAL_SEAT_USB_LAST_DETAIL.store(completion.detail as usize, Ordering::Release);
    LINKED_LOCAL_SEAT_USB_LAST_RESULT.store(completion.result as usize, Ordering::Release);
    LINKED_LOCAL_SEAT_USB_LAST_FRAME_OFFSET
        .store(completion.frame.offset as usize, Ordering::Release);
    LINKED_LOCAL_SEAT_USB_LAST_FRAME_META.store(
        usize::from(completion.frame.len) | (usize::from(completion.frame.flags) << 16),
        Ordering::Release,
    );
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
                store_linked_local_seat_usb_completion(completion);
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
fn mark_linked_local_seat_usb_keyboard_reenumeration_pending(
    contract: crate::hal::driver_task::DriverTaskContract,
    completion: crate::hal::driver_task::DriverTaskCompletionRecord,
) {
    store_linked_local_seat_usb_completion(completion);
    LINKED_LOCAL_SEAT_USB_KEYBOARD_READY.store(false, Ordering::Release);
    LINKED_LOCAL_SEAT_USB_ENUMERATION_PENDING.store(true, Ordering::Release);
    LINKED_LOCAL_SEAT_USB_FIRST_REPORT_READY_LOGGED.store(false, Ordering::Release);
    LINKED_LOCAL_SEAT_USB_FIRST_BYTE_READY_LOGGED.store(false, Ordering::Release);
    LINKED_LOCAL_SEAT_USB_OWNER_READY_LOGGED.store(false, Ordering::Release);
    crate::hal::driver_task::emit_driver_task_resource_init_status(
        contract,
        crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
        "usb-keyboard-recovery",
        "reenumerate",
        Some(completion),
    );
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
    let _ = publish_local_seat_usb_controller_owner_ready(contract, Some(completion));
    LINKED_LOCAL_SEAT_USB_KEYBOARD_READY.store(true, Ordering::Release);
    store_linked_local_seat_usb_completion(completion);
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
fn publish_local_seat_usb_controller_owner_ready(
    contract: crate::hal::driver_task::DriverTaskContract,
    completion: Option<crate::hal::driver_task::DriverTaskCompletionRecord>,
) -> bool {
    let usb_hot_path = crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard;
    let usb_owner =
        crate::hal::driver_task::driver_task_runtime_owner_state_registered(usb_hot_path)
            || crate::hal::driver_task::register_driver_task_runtime_owner_state(usb_hot_path);
    if usb_owner {
        if !LINKED_LOCAL_SEAT_USB_OWNER_READY_LOGGED.swap(true, Ordering::AcqRel) {
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                contract,
                usb_hot_path,
                "usb-owner-state",
                "ready",
                completion,
            );
            crate::hal::driver_task::emit_owner_state_transition_boot_contract_proof(usb_hot_path);
        }
        true
    } else {
        crate::hal::driver_task::emit_driver_task_resource_init_status(
            contract,
            usb_hot_path,
            "usb-owner-state",
            "descriptor-rejected",
            completion,
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
fn publish_local_seat_usb_keyboard_owner_ready(
    contract: crate::hal::driver_task::DriverTaskContract,
    completion: crate::hal::driver_task::DriverTaskCompletionRecord,
) {
    publish_local_seat_usb_keyboard_ready(contract, completion);
    LINKED_LOCAL_SEAT_USB_ENUMERATION_PENDING.store(false, Ordering::Release);
    let _ = publish_local_seat_usb_controller_owner_ready(contract, Some(completion));
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
    arming_bytes: usize,
    parser_ingress: bool,
) {
    use core::fmt::Write;

    if bytes.is_empty() {
        return;
    }
    if !LINKED_LOCAL_SEAT_USB_FIRST_REPORT_EVENT_LOGGED.swap(true, Ordering::AcqRel) {
        let mut line = heapless::String::<320>::new();
        let _ = write!(
            line,
            "[local-seat] usb hid input frame contract={} source=linked-runtime-hid tag=usb-hid-report-event len={} accepted={} arming={} parser_ingress={} detail=0x{:04x} result=0x{:08x} transfer_event=yes",
            contract.name,
            bytes.len(),
            accepted,
            arming_bytes,
            local_seat_yes_no(parser_ingress),
            completion.detail,
            completion.result,
        );
        boot_log::force_uart_line_raw_without_prompt_refresh(line.as_str());
    }
    if !local_seat_usb_first_byte_ready_allowed(true, accepted) {
        return;
    }
    if !LINKED_LOCAL_SEAT_USB_FIRST_REPORT_READY_LOGGED.swap(true, Ordering::AcqRel) {
        crate::hal::driver_task::emit_driver_task_resource_init_status(
            contract,
            crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
            "usb-keyboard-first-report",
            "ready",
            Some(completion),
        );
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
        boot_log::force_uart_line_raw_without_prompt_refresh(line.as_str());
        emit_linked_local_seat_usb_keyboard_ready_alert(contract, completion, arming_bytes);
    }
    publish_local_seat_usb_keyboard_owner_ready(contract, completion);
}

#[cfg(all(feature = "kernel", feature = "usb"))]
fn linked_local_seat_usb_keyboard_ready_alert_line(
    completion: crate::hal::driver_task::DriverTaskCompletionRecord,
    arming_bytes: usize,
) -> heapless::String<192> {
    use core::fmt::Write;

    let mut line = heapless::String::<192>::new();
    let _ = write!(
        line,
        "usb keyboard input ready: first_report=yes first_byte=yes input=local-seat source=linked-runtime-hid arming={} parser_ingress=yes detail=0x{:04x} result=0x{:08x} action=command-input",
        arming_bytes, completion.detail, completion.result,
    );
    line
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn emit_linked_local_seat_usb_keyboard_ready_alert(
    _contract: crate::hal::driver_task::DriverTaskContract,
    completion: crate::hal::driver_task::DriverTaskCompletionRecord,
    arming_bytes: usize,
) {
    let line = linked_local_seat_usb_keyboard_ready_alert_line(completion, arming_bytes);
    let console_seq = boot_log::next_console_event_seq();
    boot_log::force_uart_line_raw_and_log_without_prompt_refresh(line.as_str(), console_seq);
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
    emit_usb_runtime_progress_diagnostic_line(line.as_str());
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
    emit_usb_runtime_progress_diagnostic_line(line.as_str());
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

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn emit_usb_keyboard_recovery_request(action: &str, trace: LocalSeatKeyboardTrace) {
    use core::fmt::Write;

    let detail = LINKED_LOCAL_SEAT_USB_LAST_DETAIL.load(Ordering::Acquire);
    let result = LINKED_LOCAL_SEAT_USB_LAST_RESULT.load(Ordering::Acquire) as u32;
    let queued_reports = result & 0xff;
    let report_status = (result >> DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_SHIFT)
        & DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_MASK;
    let stale_runtime_queue = queued_reports > LINKED_LOCAL_SEAT_USB_READY_MAX_QUEUED_REPORTS
        || local_seat_keyboard_recovery_probe_stalled(queued_reports, report_status);
    let active_idle_queue = local_seat_keyboard_active_queue_idle(queued_reports, report_status);
    let pre_first_report_no_completion =
        local_seat_keyboard_pre_first_report_no_event_queue_stalled(result);
    let debt = stale_runtime_queue || pre_first_report_no_completion;
    let mut line = heapless::String::<576>::new();
    let _ = write!(
        line,
        "usb: recovery_request action={action} aux0=0x{:08x} no_reply={} streak={} cooldown={} recovery_aux_requests={} recovery_aux_pending={} queue_empty={} accepted={} drained={} echoed={} detail=0x{:04x} result=0x{:08x} queued_reports={} report_status={} report_status_code={} stale_runtime_queue={} active_idle_queue={} pre_first_report_no_completion={} debt={}",
        DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_AUX,
        trace.driver_task_no_replies,
        trace.driver_task_no_reply_streak,
        trace.poll_cooldown_turns,
        trace.recovery_aux_requests,
        local_seat_yes_no(trace.recovery_aux_pending),
        local_seat_yes_no(trace.queued_bytes == 0),
        trace.accepted_bytes,
        trace.drained_bytes,
        trace.echoed_bytes,
        detail,
        result,
        queued_reports,
        local_seat_keyboard_report_status_name(report_status),
        report_status,
        local_seat_yes_no(stale_runtime_queue),
        local_seat_yes_no(active_idle_queue),
        local_seat_yes_no(pre_first_report_no_completion),
        local_seat_yes_no(debt),
    );
    boot_log::force_log_buffer_line_or_uart_without_prompt_refresh(line.as_str());
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
fn usb_enumeration_stalled_progress_token_allows_probe_burst(
    previous: Option<(u32, u32, u32)>,
    current: Option<(u32, u32, u32)>,
    stalled_bursts: usize,
) -> bool {
    previous.is_some()
        && previous == current
        && stalled_bursts < LINKED_LOCAL_SEAT_USB_PROBE_STALLED_PROGRESS_BURST_ATTEMPTS
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
    if local_seat_usb_ready_invalidated_by_progress(completion) {
        mark_linked_local_seat_usb_keyboard_reenumeration_pending(contract, completion);
        return;
    }
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
    let console_seq = boot_log::next_console_event_seq();
    emit_hdmi_diagnostic_line_with_console_seq(line.as_str(), console_seq);
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
    payload_sig: u32,
    chunk_index: usize,
    chunk_count: usize,
    chunk_redraw: bool,
    display_trace: Option<LocalSeatDisplayTrace>,
    root_console_ready: bool,
    completion: Option<crate::hal::driver_task::DriverTaskCompletionRecord>,
    ready: bool,
    fatal: bool,
) {
    use core::fmt::Write;

    let (reported_chunk_index, reported_chunk_count) =
        display_trace.map_or((chunk_index, chunk_count), |trace| {
            let remaining_bytes = if chunk_redraw {
                trace.redraw_bytes
            } else {
                trace.pending_bytes
            };
            let chunk_limit = LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES
                .min(crate::hal::driver_task::MAX_DRIVER_TASK_FRAME_BYTES)
                .max(1);
            (
                chunk_index,
                linked_hdmi_reported_chunk_count(
                    bytes_len,
                    remaining_bytes,
                    chunk_limit,
                    chunk_count,
                ),
            )
        });
    let contract = crate::hal::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT;
    let active_request = crate::hal::driver_task::active_driver_task_ring_request(contract);
    let progress = crate::hal::driver_task::latest_driver_task_ring_progress(contract);
    let progress_request_match = match (active_request, progress) {
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
    let status = local_seat_completion_status(completion, ready);
    let no_reply = completion.is_none() && !ready;
    let mut emit_detail_rows = fatal || !root_console_ready;
    if no_reply {
        let repeat = LINKED_LOCAL_SEAT_DISPLAY_NO_REPLY_LOG_COUNT.fetch_add(1, Ordering::AcqRel);
        if !fatal && !repeated_no_reply_log_visible(repeat) {
            return;
        }
        emit_detail_rows |= repeated_no_reply_detail_log_visible(repeat);
    } else {
        if !fatal
            && !routine_hdmi_ready_log_visible(
                reason,
                chunk_redraw,
                display_trace,
                root_console_ready,
            )
        {
            return;
        }
        emit_detail_rows |= !matches!(reason, "queued-output" | "keyboard-scrollback");
    }
    let console_seq = boot_log::next_console_event_seq();
    let mut line = heapless::String::<384>::new();
    if let Some(completion) = completion {
        let _ = write!(
            line,
            "HDMI_FRAME_SUBMIT reason={reason} status={status} root_console_ready={} attached={} failed={} fatal={} redraw={} bytes={} chunk_index={} chunk_count={} payload_sig=0x{:08x} completion_sequence={} code={} detail={} result={} frame_len={}",
            local_seat_yes_no(root_console_ready),
            local_seat_yes_no(LINKED_LOCAL_SEAT_DISPLAY_ATTACHED.load(Ordering::Acquire)),
            local_seat_yes_no(LINKED_LOCAL_SEAT_DISPLAY_FAILED.load(Ordering::Acquire)),
            local_seat_yes_no(fatal),
            local_seat_yes_no(chunk_redraw),
            bytes_len,
            reported_chunk_index,
            reported_chunk_count,
            payload_sig,
            completion.sequence,
            completion.code,
            completion.detail,
            completion.result,
            completion.frame.len,
        );
    } else {
        let _ = write!(
            line,
            "HDMI_FRAME_SUBMIT reason={reason} status={status} root_console_ready={} attached={} failed={} fatal={} redraw={} bytes={} chunk_index={} chunk_count={} payload_sig=0x{:08x} completion_sequence=none code=none detail=none result=none frame_len=0",
            local_seat_yes_no(root_console_ready),
            local_seat_yes_no(LINKED_LOCAL_SEAT_DISPLAY_ATTACHED.load(Ordering::Acquire)),
            local_seat_yes_no(LINKED_LOCAL_SEAT_DISPLAY_FAILED.load(Ordering::Acquire)),
            local_seat_yes_no(fatal),
            local_seat_yes_no(chunk_redraw),
            bytes_len,
            reported_chunk_index,
            reported_chunk_count,
            payload_sig,
        );
    }
    emit_hdmi_diagnostic_line_with_console_seq(line.as_str(), console_seq);
    if !emit_detail_rows {
        return;
    }

    let mut ring_line = heapless::String::<160>::new();
    let _ = write!(
        ring_line,
        "HDMI_FRAME_RING reason={reason} active_request={} progress_present={} progress_match={}",
        active_request.unwrap_or(0),
        local_seat_yes_no(progress.is_some()),
        progress_request_match,
    );
    emit_hdmi_diagnostic_line_with_console_seq(ring_line.as_str(), console_seq);

    if let Some(trace) = display_trace {
        let mut queue_line = heapless::String::<384>::new();
        let _ = write!(
            queue_line,
            "HDMI_FRAME_QUEUE reason={reason} chunk_bytes={} chunk_redraw={} generation={} pending_bytes={} redraw_bytes={} pending_redraw={} scrollback={} open_line={} submitted={} deferred={} busy={} no_reply={} cooldown={}",
            bytes_len,
            local_seat_yes_no(chunk_redraw),
            trace.snapshot_generation,
            trace.pending_bytes,
            trace.redraw_bytes,
            local_seat_yes_no(trace.pending_redraw),
            trace.scrollback_offset,
            local_seat_yes_no(trace.open_line),
            trace.submitted_frames,
            trace.deferred_frames,
            trace.busy_frames,
            trace.no_reply_frames,
            trace.no_reply_cooldown_turns,
        );
        emit_hdmi_diagnostic_line_with_console_seq(queue_line.as_str(), console_seq);

        let mut counters_line = heapless::String::<224>::new();
        let _ = write!(
            counters_line,
            "HDMI_FRAME_COUNTERS reason={reason} coalesced={} backpressure_bytes={} superseded_bytes={} redraw_no_reply_streak={} stale_after_retry={}",
            trace.coalesced_redraws,
            trace.backpressure_bytes,
            trace.superseded_bytes,
            trace.redraw_no_reply_streak,
            local_seat_yes_no(trace.stale_after_retry_exhaustion),
        );
        emit_hdmi_diagnostic_line_with_console_seq(counters_line.as_str(), console_seq);
    }

    if let Some(progress) = progress {
        let mut progress_line = heapless::String::<224>::new();
        let _ = write!(
            progress_line,
            "HDMI_FRAME_PROGRESS reason={reason} marker_valid={} sequence={} phase={} phase_name={} aux0=0x{:08x} active_request={} progress_match={}",
            local_seat_yes_no(progress.marker_valid),
            progress.sequence,
            progress.phase,
            progress.phase_name,
            progress.aux0,
            active_request.unwrap_or(0),
            progress_request_match,
        );
        emit_hdmi_diagnostic_line_with_console_seq(progress_line.as_str(), console_seq);
    }
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn emit_hdmi_diagnostic_line(line: &str) {
    boot_log::force_uart_line_raw_and_log(line);
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn emit_hdmi_diagnostic_line_with_console_seq(line: &str, console_seq: u32) {
    boot_log::force_uart_line_raw_and_log_without_prompt_refresh(line, console_seq);
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn emit_usb_runtime_progress_diagnostic_line(line: &str) {
    boot_log::force_log_buffer_line_or_uart_without_prompt_refresh(line);
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn hdmi_payload_signature(bytes: &[u8]) -> u32 {
    let mut hash = 0x811c_9dc5u32;
    for &byte in bytes {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
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
    let retained_init_active = crate::hal::driver_task::driver_task_ring_command_active(
        crate::hal::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT,
    );
    if !retained_init_active
        && LINKED_LOCAL_SEAT_DISPLAY_INIT_ATTEMPTED.swap(true, Ordering::AcqRel)
    {
        let attempts = LINKED_LOCAL_SEAT_DISPLAY_INIT_ATTEMPTS.load(Ordering::Acquire);
        if !local_seat_display_attach_retry_allowed(root_console_ready, false, attempts) {
            return false;
        }
    }
    let attempt = if retained_init_active {
        LINKED_LOCAL_SEAT_DISPLAY_INIT_ATTEMPTS
            .load(Ordering::Acquire)
            .max(1)
    } else {
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
        attempt
    };
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
    let hdmi_completion = crate::hal::driver_task::run_driver_task_ring_service_retained_turn(
        crate::hal::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT,
        hdmi_command,
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
        emit_hdmi_diagnostic_line(
            "[local-seat] linked HDMI runtime active action=serial-safe-mirror",
        );
        crate::hal::driver_task::emit_owner_state_transition_boot_contract_proof(
            crate::hal::driver_task::DriverTaskHotPath::HdmiText,
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
        emit_hdmi_diagnostic_line(
            "[local-seat] linked HDMI runtime pending reason=driver-task-init-no-reply action=serial-shell",
        );
    }
    if !crate::hal::driver_task::driver_task_ring_command_active(
        crate::hal::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT,
    ) {
        LINKED_LOCAL_SEAT_DISPLAY_INIT_ATTEMPTED.store(false, Ordering::Release);
    }
    false
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
fn service_linked_local_seat_runtime_attach_turn(
    runtime: &mut LocalSeatRuntime,
) -> LocalSeatServiceTurn {
    let root_console_ready = runtime.root_console_ready;
    if !local_seat_prompt_steady_service_allowed(
        crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active(),
        root_console_ready,
        crate::hal::driver_task::driver_task_runtime_proof().pointer_free_ipc_proof,
    ) {
        return LocalSeatServiceTurn::Pending;
    }
    let usb_contract = driver_task_contract();
    crate::hal::driver_task::register_driver_task_pointer_free_ring_service(
        usb_contract,
        crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard.as_u32() as usize,
        usb_keyboard_runtime_ring_service_driver_task,
    );

    let controller_attached = LINKED_LOCAL_SEAT_RUNTIME_ATTACHED.load(Ordering::Acquire);
    let keyboard_ready = LINKED_LOCAL_SEAT_USB_KEYBOARD_READY.load(Ordering::Acquire);
    let enumeration_pending = LINKED_LOCAL_SEAT_USB_ENUMERATION_PENDING.load(Ordering::Acquire);
    if keyboard_ready {
        runtime.linked_usb_attach_phase = LocalSeatUsbAttachPhase::Complete;
        runtime.linked_usb_attach_command = None;
        return LocalSeatServiceTurn::Complete;
    }
    if controller_attached
        && (enumeration_pending
            || matches!(
                runtime.linked_usb_attach_phase,
                LocalSeatUsbAttachPhase::Complete
            ))
    {
        LINKED_LOCAL_SEAT_USB_ENUMERATION_PENDING.store(true, Ordering::Release);
        runtime.linked_usb_attach_phase = LocalSeatUsbAttachPhase::UsbEnumerate;
    }

    match runtime.linked_usb_attach_phase {
        LocalSeatUsbAttachPhase::PcieDescriptor => {
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
            match crate::hal::driver_task::step_deferred_runtime_init_descriptor(
                crate::hal::driver_task::PCIE_ROOT_DRIVER_TASK_CONTRACT,
                crate::hal::driver_task::DriverTaskHotPath::PcieRoot,
            ) {
                crate::hal::driver_task::DriverTaskDescriptorReplayTurn::Pending => {
                    LocalSeatServiceTurn::Pending
                }
                crate::hal::driver_task::DriverTaskDescriptorReplayTurn::Complete => {
                    runtime.linked_usb_attach_phase = LocalSeatUsbAttachPhase::PciePrepared;
                    if !LINKED_LOCAL_SEAT_PCIE_REPLAY_READY_LOGGED.swap(true, Ordering::AcqRel) {
                        crate::hal::driver_task::emit_driver_task_resource_init_status(
                            crate::hal::driver_task::PCIE_ROOT_DRIVER_TASK_CONTRACT,
                            crate::hal::driver_task::DriverTaskHotPath::PcieRoot,
                            "usb-prereq-pcie-replay",
                            "ready",
                            None,
                        );
                    }
                    LocalSeatServiceTurn::Pending
                }
                crate::hal::driver_task::DriverTaskDescriptorReplayTurn::Failed(reason) => {
                    runtime.linked_usb_attach_phase = LocalSeatUsbAttachPhase::Failed(reason);
                    LocalSeatServiceTurn::Failed(reason)
                }
            }
        }
        LocalSeatUsbAttachPhase::PciePrepared => {
            if !linked_local_seat_pcie_hal_prep_ready() {
                if !LINKED_LOCAL_SEAT_PCIE_HAL_PREP_BLOCKED_LOGGED.swap(true, Ordering::AcqRel) {
                    crate::hal::driver_task::emit_driver_task_resource_init_status(
                        crate::hal::driver_task::PCIE_ROOT_DRIVER_TASK_CONTRACT,
                        crate::hal::driver_task::DriverTaskHotPath::PcieRoot,
                        "usb-prereq-pcie-hal-prep",
                        "blocked-pre-event-proof-missing",
                        None,
                    );
                }
                return LocalSeatServiceTurn::Pending;
            }
            runtime.linked_usb_attach_phase = LocalSeatUsbAttachPhase::PcieOwner;
            LocalSeatServiceTurn::Pending
        }
        LocalSeatUsbAttachPhase::PcieOwner => {
            let pcie_contract = crate::hal::driver_task::PCIE_ROOT_DRIVER_TASK_CONTRACT;
            if !crate::hal::driver_task::register_driver_task_runtime_owner_state(
                crate::hal::driver_task::DriverTaskHotPath::PcieRoot,
            ) {
                runtime.linked_usb_attach_phase =
                    LocalSeatUsbAttachPhase::Failed("pcie-owner-state-rejected");
                return LocalSeatServiceTurn::Failed("pcie-owner-state-rejected");
            }
            if !LINKED_LOCAL_SEAT_PCIE_OWNER_READY_LOGGED.swap(true, Ordering::AcqRel) {
                crate::hal::driver_task::emit_driver_task_resource_init_status(
                    pcie_contract,
                    crate::hal::driver_task::DriverTaskHotPath::PcieRoot,
                    "pcie-owner-state",
                    "ready",
                    None,
                );
                crate::hal::driver_task::emit_owner_state_transition_boot_contract_proof(
                    crate::hal::driver_task::DriverTaskHotPath::PcieRoot,
                );
            }
            runtime.linked_usb_attach_phase = LocalSeatUsbAttachPhase::UsbDescriptor;
            LocalSeatServiceTurn::Pending
        }
        LocalSeatUsbAttachPhase::UsbDescriptor => {
            if !LINKED_LOCAL_SEAT_USB_REPLAY_BEGIN_LOGGED.swap(true, Ordering::AcqRel) {
                crate::hal::driver_task::emit_driver_task_resource_init_status(
                    usb_contract,
                    crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
                    "usb-runtime-descriptor-replay",
                    "begin",
                    None,
                );
            }
            match crate::hal::driver_task::step_deferred_runtime_init_descriptor(
                usb_contract,
                crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
            ) {
                crate::hal::driver_task::DriverTaskDescriptorReplayTurn::Pending => {
                    LocalSeatServiceTurn::Pending
                }
                crate::hal::driver_task::DriverTaskDescriptorReplayTurn::Complete => {
                    runtime.linked_usb_attach_phase = LocalSeatUsbAttachPhase::UsbInit;
                    if !LINKED_LOCAL_SEAT_USB_REPLAY_READY_LOGGED.swap(true, Ordering::AcqRel) {
                        crate::hal::driver_task::emit_driver_task_resource_init_status(
                            usb_contract,
                            crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
                            "usb-runtime-descriptor-replay",
                            "ready",
                            None,
                        );
                    }
                    LocalSeatServiceTurn::Pending
                }
                crate::hal::driver_task::DriverTaskDescriptorReplayTurn::Failed(reason) => {
                    runtime.linked_usb_attach_phase = LocalSeatUsbAttachPhase::Failed(reason);
                    LocalSeatServiceTurn::Failed(reason)
                }
            }
        }
        LocalSeatUsbAttachPhase::UsbInit => {
            let command = *runtime.linked_usb_attach_command.get_or_insert_with(|| {
                let mut command = crate::hal::driver_task::DriverTaskCommandRecord::pi4_hot_path(
                    0,
                    crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
                    crate::hal::driver_task::DriverTaskBudgetGrant::from_contract(usb_contract),
                    crate::hal::driver_task::DriverFrameDescriptor {
                        offset: 0,
                        len: 0,
                        flags: 0,
                    },
                );
                command.aux0 = DRIVER_RUNTIME_LOCAL_SEAT_INIT_AUX;
                command
            });
            if !LINKED_LOCAL_SEAT_USB_ENGINE_BEGIN_LOGGED.swap(true, Ordering::AcqRel) {
                crate::hal::driver_task::emit_driver_task_resource_init_status(
                    usb_contract,
                    crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
                    "usb-engine-init",
                    "begin",
                    None,
                );
            }
            let completion =
                match crate::hal::driver_task::run_driver_task_ring_service_retained_service_turn(
                    usb_contract,
                    command,
                ) {
                    crate::hal::driver_task::DriverTaskRetainedServiceTurn::Pending => {
                        return LocalSeatServiceTurn::Pending;
                    }
                    crate::hal::driver_task::DriverTaskRetainedServiceTurn::Complete(
                        completion,
                    ) => completion,
                    crate::hal::driver_task::DriverTaskRetainedServiceTurn::Failed => {
                        runtime.linked_usb_attach_command = None;
                        runtime.linked_usb_attach_phase =
                            LocalSeatUsbAttachPhase::Failed("usb-engine-init-retained-failed");
                        return LocalSeatServiceTurn::Failed("usb-engine-init-retained-failed");
                    }
                };
            runtime.linked_usb_attach_command = None;
            record_linked_local_seat_usb_detail(Some(completion));
            let controller_ready = local_seat_usb_engine_init_ready(Some(completion));
            let keyboard_ready = local_seat_usb_keyboard_init_ready(Some(completion));
            crate::hal::driver_task::emit_driver_task_resource_init_status(
                usb_contract,
                crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
                "usb-engine-init",
                local_seat_usb_engine_init_status(Some(completion)),
                Some(completion),
            );
            if !controller_ready {
                if completion.code
                    == crate::hal::driver_task::DriverTaskCompletionCode::Fault.as_u16()
                {
                    runtime.linked_usb_attach_phase =
                        LocalSeatUsbAttachPhase::Failed("usb-engine-init-fault");
                    return LocalSeatServiceTurn::Failed("usb-engine-init-fault");
                }
                return LocalSeatServiceTurn::Pending;
            }
            LINKED_LOCAL_SEAT_RUNTIME_ATTACHED.store(true, Ordering::Release);
            let _ = publish_local_seat_usb_controller_owner_ready(usb_contract, Some(completion));
            if keyboard_ready {
                publish_local_seat_usb_keyboard_ready(usb_contract, completion);
                runtime.linked_usb_attach_phase = LocalSeatUsbAttachPhase::Complete;
                LocalSeatServiceTurn::Complete
            } else {
                LINKED_LOCAL_SEAT_USB_ENUMERATION_PENDING.store(true, Ordering::Release);
                runtime.linked_usb_attach_phase = LocalSeatUsbAttachPhase::UsbEnumerate;
                LocalSeatServiceTurn::Pending
            }
        }
        LocalSeatUsbAttachPhase::UsbEnumerate => {
            let new_command = runtime.linked_usb_attach_command.is_none();
            let command = *runtime.linked_usb_attach_command.get_or_insert_with(|| {
                let mut command = crate::hal::driver_task::DriverTaskCommandRecord::pi4_hot_path(
                    0,
                    crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
                    crate::hal::driver_task::DriverTaskBudgetGrant::from_contract(usb_contract),
                    crate::hal::driver_task::DriverFrameDescriptor {
                        offset: 0,
                        len: 0,
                        flags: 0,
                    },
                );
                command.aux0 = DRIVER_RUNTIME_USB_ENUMERATE_AUX;
                command
            });
            if new_command {
                runtime.keyboard_enumeration_aux_requests =
                    runtime.keyboard_enumeration_aux_requests.saturating_add(1);
            }
            let completion =
                match crate::hal::driver_task::run_driver_task_ring_service_retained_service_turn(
                    usb_contract,
                    command,
                ) {
                    crate::hal::driver_task::DriverTaskRetainedServiceTurn::Pending => {
                        return LocalSeatServiceTurn::Pending;
                    }
                    crate::hal::driver_task::DriverTaskRetainedServiceTurn::Complete(
                        completion,
                    ) => completion,
                    crate::hal::driver_task::DriverTaskRetainedServiceTurn::Failed => {
                        runtime.linked_usb_attach_command = None;
                        runtime.linked_usb_attach_phase =
                            LocalSeatUsbAttachPhase::Failed("usb-enumeration-retained-failed");
                        return LocalSeatServiceTurn::Failed("usb-enumeration-retained-failed");
                    }
                };
            runtime.linked_usb_attach_command = None;
            record_linked_local_seat_usb_detail(Some(completion));
            if local_seat_usb_keyboard_init_ready(Some(completion)) {
                LINKED_LOCAL_SEAT_USB_ENUMERATION_PENDING.store(false, Ordering::Release);
                publish_local_seat_usb_keyboard_ready(usb_contract, completion);
                runtime.linked_usb_attach_phase = LocalSeatUsbAttachPhase::Complete;
                LocalSeatServiceTurn::Complete
            } else if local_seat_usb_engine_init_ready(Some(completion)) {
                LINKED_LOCAL_SEAT_USB_ENUMERATION_PENDING.store(true, Ordering::Release);
                publish_local_seat_usb_enumeration_progress(usb_contract, completion);
                LocalSeatServiceTurn::Pending
            } else if completion.code
                == crate::hal::driver_task::DriverTaskCompletionCode::Fault.as_u16()
            {
                runtime.linked_usb_attach_phase =
                    LocalSeatUsbAttachPhase::Failed("usb-enumeration-fault");
                LocalSeatServiceTurn::Failed("usb-enumeration-fault")
            } else {
                LocalSeatServiceTurn::Pending
            }
        }
        LocalSeatUsbAttachPhase::Complete => LocalSeatServiceTurn::Complete,
        LocalSeatUsbAttachPhase::Failed(reason) => LocalSeatServiceTurn::Failed(reason),
    }
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
pub(crate) fn linked_local_seat_usb_enumeration_pending() -> bool {
    LINKED_LOCAL_SEAT_USB_ENUMERATION_PENDING.load(Ordering::Acquire)
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

#[cfg(not(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
)))]
pub(crate) const fn linked_local_seat_usb_runtime_detail() -> u16 {
    0
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

#[cfg(not(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
)))]
pub(crate) const fn linked_local_seat_usb_runtime_result() -> u32 {
    0
}

#[cfg(all(
    feature = "kernel",
    feature = "usb",
    target_arch = "aarch64",
    target_os = "none"
))]
pub(crate) fn linked_local_seat_usb_runtime_frame() -> crate::hal::driver_task::DriverFrameDescriptor
{
    let meta = LINKED_LOCAL_SEAT_USB_LAST_FRAME_META.load(Ordering::Acquire);
    crate::hal::driver_task::DriverFrameDescriptor {
        offset: LINKED_LOCAL_SEAT_USB_LAST_FRAME_OFFSET.load(Ordering::Acquire) as u32,
        len: (meta & 0xffff) as u16,
        flags: ((meta >> 16) & 0xffff) as u16,
    }
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
    if !local_seat_high_impact_hdmi_mirror_allowed(root_console_ready) {
        return false;
    }
    if !crate::hal::driver_task::physical_pi_driver_task_only_owner_state_active() {
        return false;
    }
    let contract = crate::hal::driver_task::HDMI_TEXT_DRIVER_TASK_CONTRACT;
    let _ = adopt_linked_display_runtime_owner_state(reason);
    if !LINKED_LOCAL_SEAT_DISPLAY_ATTACHED.load(Ordering::Acquire)
        && !LINKED_LOCAL_SEAT_DISPLAY_FAILED.load(Ordering::Acquire)
    {
        let _ = try_attach_linked_display_runtime(root_console_ready);
        return false;
    }
    if !local_seat_linked_display_service_allowed(
        true,
        LINKED_LOCAL_SEAT_DISPLAY_ATTACHED.load(Ordering::Acquire),
        LINKED_LOCAL_SEAT_DISPLAY_FAILED.load(Ordering::Acquire),
    ) {
        if !LINKED_LOCAL_SEAT_DISPLAY_DEFERRED_LOGGED.swap(true, Ordering::AcqRel) {
            emit_hdmi_diagnostic_line(
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
    emit_hdmi_frame_submit_state(
        reason,
        payload.len(),
        hdmi_payload_signature(payload.as_slice()),
        0,
        1,
        false,
        None,
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
    let status = local_seat_completion_status(completion, ready);
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
fn build_linked_hdmi_retained_frame(
    contract: crate::hal::driver_task::DriverTaskContract,
    payload: Vec<u8>,
    reason: &'static str,
    redraw: bool,
    receipt: LinkedHdmiPayloadReceipt,
) -> Result<LinkedHdmiRetainedFrame, Vec<u8>> {
    let chunk_limit = LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES
        .clamp(1, crate::hal::driver_task::MAX_DRIVER_TASK_FRAME_BYTES);
    if payload.is_empty() || payload.len() > chunk_limit {
        return Err(payload);
    }
    let Some(frame) =
        crate::hal::driver_task::describe_driver_task_ring_frame(payload.as_slice(), 0)
    else {
        return Err(payload);
    };
    let command = crate::hal::driver_task::DriverTaskCommandRecord::pi4_hot_path(
        0,
        crate::hal::driver_task::DriverTaskHotPath::HdmiText,
        crate::hal::driver_task::DriverTaskBudgetGrant::from_contract(contract),
        frame,
    );
    Ok(LinkedHdmiRetainedFrame {
        command,
        payload,
        reason,
        redraw,
        receipt,
    })
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
/// Prepare the Pi 4 PCIe prerequisite before the ordinary event pump starts.
///
/// The current HAL proof is internally synchronous, so EventPump descendants
/// must never call this function. Once HAL exposes a retained proof cursor this
/// admission hook can be replaced by ordinary one-turn orchestration.
pub fn prepare_linked_local_seat_pcie_hal_before_event_pump() -> bool {
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
            DRIVER_RUNTIME_LOCAL_SEAT_INIT_AUX
                | DRIVER_RUNTIME_USB_ENUMERATE_AUX
                | DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_AUX
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
        emit_hdmi_diagnostic_line(
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
    use std::sync::Mutex as TestMutex;

    static HDMI_READY_LOG_TEST_LOCK: TestMutex<()> = TestMutex::new(());
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
        assert_eq!(trace.arming_bytes, 0);
        assert_eq!(trace.drained_bytes, 2);
        assert_eq!(trace.echoed_bytes, 2);
        assert_eq!(trace.dropped_bytes, 0);
    }

    #[test]
    fn runtime_arming_consumes_one_key_and_keeps_burst_tail() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 4,
        });

        assert_eq!(runtime.accept_keyboard_arming_bytes(b"ping\n"), 5);
        let trace = runtime.keyboard_trace();
        assert_eq!(trace.arming_bytes, 1);
        assert_eq!(trace.accepted_bytes, 4);
        assert_eq!(trace.queued_bytes, 4);

        let mut drained = [0u8; 8];
        assert_eq!(runtime.drain_keyboard_bytes(&mut drained), 4);
        assert_eq!(&drained[..4], b"ing\n");
        assert_eq!(runtime.enqueue_keyboard_bytes(b"ping\n"), 5);
        assert_eq!(runtime.drain_keyboard_bytes(&mut drained), 5);
        assert_eq!(&drained[..5], b"ping\n");
    }

    #[test]
    fn runtime_arming_consumes_one_escape_sequence() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 4,
        });

        assert_eq!(keyboard_arming_sequence_len(b"\x1b[Ahelp\n"), 3);
        assert_eq!(runtime.accept_keyboard_arming_bytes(b"\x1b[Ahelp\n"), 8);
        let trace = runtime.keyboard_trace();
        assert_eq!(trace.arming_bytes, 3);
        assert_eq!(trace.accepted_bytes, 5);
        assert_eq!(trace.queued_bytes, 5);

        let mut drained = [0u8; 8];
        assert_eq!(runtime.drain_keyboard_bytes(&mut drained), 5);
        assert_eq!(&drained[..5], b"help\n");
    }

    #[test]
    fn runtime_pre_ready_keyboard_bytes_do_not_enter_parser() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 4,
        });

        assert_eq!(
            runtime.accept_keyboard_bytes_for_parser_state(b"p", false, false, true),
            (0, 0)
        );
        assert_eq!(
            runtime.accept_keyboard_bytes_for_parser_state(b"i", false, true, true),
            (0, 0)
        );
        assert_eq!(
            runtime.accept_keyboard_bytes_for_parser_state(b"n", false, true, true),
            (0, 0)
        );
        assert_eq!(
            runtime.accept_keyboard_bytes_for_parser_state(b"g\n", false, true, true),
            (0, 0)
        );

        let trace = runtime.keyboard_trace();
        assert_eq!(trace.arming_bytes, 0);
        assert_eq!(trace.accepted_bytes, 0);
        assert_eq!(trace.queued_bytes, 0);

        let mut drained = [0u8; 8];
        assert_eq!(runtime.drain_keyboard_bytes(&mut drained), 0);
    }

    #[test]
    fn runtime_parser_ready_accepts_first_key_without_arming() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 4,
        });

        assert_eq!(
            runtime.accept_keyboard_bytes_for_parser_state(b"p", true, false, true),
            (1, 0)
        );
        assert_eq!(
            runtime.accept_keyboard_bytes_for_parser_state(b"ing\n", true, true, true),
            (4, 0)
        );

        let trace = runtime.keyboard_trace();
        assert_eq!(trace.arming_bytes, 0);
        assert_eq!(trace.accepted_bytes, 5);
        assert_eq!(trace.queued_bytes, 5);

        let mut drained = [0u8; 8];
        assert_eq!(runtime.drain_keyboard_bytes(&mut drained), 5);
        assert_eq!(&drained[..5], b"ping\n");
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
    fn linked_hdmi_reported_chunk_count_covers_pending_burst_tail() {
        assert_eq!(
            linked_hdmi_reported_chunk_count(
                LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES,
                2389,
                LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES,
                1,
            ),
            6
        );
        assert_eq!(
            linked_hdmi_reported_chunk_count(220, 0, LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES, 1,),
            1
        );
        assert_eq!(
            linked_hdmi_reported_chunk_count(
                LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES,
                200,
                LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES,
                1,
            ),
            2
        );
    }

    #[test]
    fn routine_hdmi_ready_log_suppresses_driver_resource_success_spam() {
        let _guard = HDMI_READY_LOG_TEST_LOCK
            .lock()
            .expect("HDMI log test lock must not be poisoned");
        LINKED_LOCAL_SEAT_DISPLAY_READY_LOG_COUNT.store(0, Ordering::Release);

        assert!(!routine_hdmi_ready_log_visible(
            "driver-resource-progress",
            false,
            None,
            false,
        ));
        assert!(!routine_hdmi_ready_log_visible(
            "driver-resource-progress",
            false,
            None,
            true,
        ));
    }

    #[test]
    fn routine_hdmi_ready_log_keeps_first_proof_then_samples() {
        let _guard = HDMI_READY_LOG_TEST_LOCK
            .lock()
            .expect("HDMI log test lock must not be poisoned");
        LINKED_LOCAL_SEAT_DISPLAY_READY_LOG_COUNT.store(0, Ordering::Release);

        assert!(routine_hdmi_ready_log_visible(
            "keyboard-scrollback",
            false,
            None,
            true,
        ));
        assert!(!routine_hdmi_ready_log_visible(
            "queued-output",
            false,
            None,
            true,
        ));
        LINKED_LOCAL_SEAT_DISPLAY_READY_LOG_COUNT.store(
            LINKED_LOCAL_SEAT_HDMI_READY_SAMPLE_STRIDE - 1,
            Ordering::Release,
        );
        assert!(routine_hdmi_ready_log_visible(
            "queued-output",
            false,
            None,
            true,
        ));
    }

    #[test]
    fn routine_hdmi_ready_log_samples_completed_redraws() {
        let _guard = HDMI_READY_LOG_TEST_LOCK
            .lock()
            .expect("HDMI log test lock must not be poisoned");
        let runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 4,
        });
        LINKED_LOCAL_SEAT_DISPLAY_READY_LOG_COUNT.store(
            LINKED_LOCAL_SEAT_HDMI_READY_VERBOSE_LIMIT,
            Ordering::Release,
        );

        assert!(!routine_hdmi_ready_log_visible(
            "keyboard-scrollback",
            true,
            Some(runtime.display_trace()),
            true,
        ));

        let mut retry_trace = runtime.display_trace();
        retry_trace.redraw_no_reply_streak = 1;
        assert!(routine_hdmi_ready_log_visible(
            "keyboard-scrollback",
            true,
            Some(retry_trace),
            true,
        ));
    }

    #[test]
    fn routine_hdmi_ready_log_samples_clean_active_redraw_chunks() {
        let _guard = HDMI_READY_LOG_TEST_LOCK
            .lock()
            .expect("HDMI log test lock must not be poisoned");
        let runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 160,
            buffer_lines: 128,
        });
        let mut trace = runtime.display_trace();
        trace.redraw_bytes = LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES;
        trace.pending_redraw = true;
        LINKED_LOCAL_SEAT_DISPLAY_READY_LOG_COUNT.store(
            LINKED_LOCAL_SEAT_HDMI_READY_VERBOSE_LIMIT,
            Ordering::Release,
        );

        assert!(!routine_hdmi_ready_log_visible(
            "keyboard-scrollback",
            true,
            Some(trace),
            true,
        ));
    }

    #[test]
    fn local_seat_pre_root_runtime_init_requires_pointer_free_proof_on_physical_pi() {
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
        assert!(!local_seat_driver_task_prompt_slice_required(
            DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_AUX,
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
        assert!(local_seat_driver_task_prompt_slice_required(
            DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_AUX,
            true
        ));
        assert!(!local_seat_driver_task_prompt_slice_required(0, true));
    }

    #[test]
    fn prompt_side_keyboard_poll_uses_backoff_after_missing_driver_reply() {
        assert!(!local_seat_keyboard_poll_suspends_on_missing_reply(
            true, false, false
        ));
        assert!(!local_seat_keyboard_poll_suspends_on_missing_reply(
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
    fn high_impact_hdmi_mirroring_waits_for_root_console_steady_state() {
        assert!(!local_seat_high_impact_hdmi_mirror_allowed(false));
        assert!(local_seat_high_impact_hdmi_mirror_allowed(true));
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
        let full_viewport_bytes = linked_hdmi_scrollback_visible_lines(128, 160)
            .saturating_mul(
                linked_hdmi_snapshot_line_width(160, fallback)
                    .saturating_add(LINKED_LOCAL_SEAT_HDMI_CLEAR_EOL_BYTES)
                    .saturating_add(1),
            )
            .saturating_add(LINKED_LOCAL_SEAT_HDMI_CURSOR_HOME_BYTES);
        assert!(full_viewport_bytes > LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES);
    }

    fn assert_no_blink_snapshot_payload(payload: &[u8]) {
        assert!(payload.starts_with(b"\x1b[H"));
        assert!(!payload.windows(3).any(|window| window == b"\x1b[J"));
        assert!(!payload.contains(&0x0c));
        assert!(payload.windows(3).any(|window| window == b"\x1b[K"));
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

        assert_no_blink_snapshot_payload(payload.as_slice());
        assert_eq!(payload.iter().filter(|&&byte| byte == b'\n').count(), 58);
        assert!(payload.len() > LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES);

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
    fn runtime_hdmi_snapshot_redraw_clears_before_repaint() {
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
        assert_no_blink_snapshot_payload(payload.as_slice());
        assert!(payload
            .windows(b"cohesix> ".len())
            .any(|window| window == b"cohesix> "));
        assert!(payload.windows(2).any(|window| window == b"\x1b7"));
        assert!(payload.ends_with(b"\x1b8"));
    }

    #[test]
    fn runtime_hdmi_snapshot_redraw_coalesces_pending_without_payload_churn() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 2,
        });

        runtime.mirror_line_current_tcb("cohesix> ");
        runtime.request_linked_hdmi_snapshot_redraw();
        let first = runtime.display_trace();
        runtime.request_linked_hdmi_snapshot_redraw();
        let second = runtime.display_trace();

        assert!(second.pending_redraw);
        assert_eq!(second.redraw_bytes, 0);
        assert_eq!(second.pending_bytes, 0);
        assert_eq!(second.snapshot_generation, first.snapshot_generation);
        assert_eq!(second.superseded_bytes, first.superseded_bytes);
        assert_eq!(second.coalesced_redraws, first.coalesced_redraws + 1);
    }

    #[test]
    fn runtime_hdmi_snapshot_redraw_coalesces_active_redraw_without_generation_churn() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 64,
            buffer_lines: 12,
        });

        for _ in 0..12 {
            runtime.mirror_line_current_tcb(
                "cohesix> redraw payload line large enough to span multiple hdmi chunks",
            );
        }
        runtime.request_linked_hdmi_snapshot_redraw();
        let Some((chunk, _, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected redraw chunk");
        };
        assert!(redraw);
        assert!(!chunk.is_empty());
        let active = runtime.display_trace();
        assert!(active.redraw_bytes != 0);

        runtime.request_linked_hdmi_snapshot_redraw();
        let coalesced = runtime.display_trace();

        assert!(coalesced.pending_redraw);
        assert_eq!(coalesced.redraw_bytes, active.redraw_bytes);
        assert_eq!(coalesced.snapshot_generation, active.snapshot_generation);
        assert!(coalesced.coalesced_redraws > active.coalesced_redraws);
        assert_eq!(coalesced.superseded_bytes, active.superseded_bytes);
    }

    #[test]
    fn runtime_hdmi_render_scrollback_does_not_restart_active_redraw() {
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

        runtime.render_linked_hdmi_scrollback();
        let Some((first_chunk, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected active redraw chunk");
        };
        assert_eq!(reason, "keyboard-scrollback");
        assert!(redraw);
        assert_eq!(first_chunk.len(), LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES);
        let active = runtime.display_trace();
        assert!(active.redraw_bytes > 0);
        assert!(active.pending_redraw);

        runtime.render_linked_hdmi_scrollback();
        let coalesced = runtime.display_trace();

        assert_eq!(coalesced.redraw_bytes, active.redraw_bytes);
        assert!(coalesced.pending_redraw);
        assert_eq!(coalesced.snapshot_generation, active.snapshot_generation);
        assert_eq!(coalesced.superseded_bytes, active.superseded_bytes);
        assert_eq!(coalesced.coalesced_redraws, active.coalesced_redraws + 1);
    }

    #[test]
    fn runtime_hdmi_snapshot_terminates_closed_wait_line_before_prompt() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 160,
            buffer_lines: 4,
        });

        runtime.mirror_line_current_tcb(LOCAL_SEAT_HDMI_KEYBOARD_WAIT_LINE);
        runtime.request_linked_hdmi_snapshot_redraw();

        let Some((payload, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected snapshot payload");
        };
        assert_eq!(reason, "keyboard-scrollback");
        assert!(redraw);

        let mut expected = Vec::new();
        expected.extend_from_slice(LOCAL_SEAT_HDMI_KEYBOARD_WAIT_LINE.as_bytes());
        expected.extend_from_slice(b"\x1b[K\n");
        assert!(
            payload
                .windows(expected.len())
                .any(|window| window == expected.as_slice()),
            "{:?}",
            String::from_utf8_lossy(payload.as_slice())
        );
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
        assert_no_blink_snapshot_payload(payload.as_slice());
        assert!(payload
            .windows(b"cohesix> hel".len())
            .any(|window| window == b"cohesix> hel"));
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
        assert_no_blink_snapshot_payload(payload.as_slice());
        assert!(payload
            .windows(b"cohesix> help".len())
            .any(|window| window == b"cohesix> help"));
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
        assert_no_blink_snapshot_payload(payload.as_slice());
        assert_eq!(payload.iter().filter(|&&byte| byte == b'\n').count(), 27);
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

        runtime.record_linked_hdmi_submit_miss(submitted.as_slice(), redraw);

        let display = runtime.display_trace();
        assert_eq!(display.pending_bytes, 0);
        assert_eq!(display.redraw_bytes, 0);
        assert!(display.pending_redraw);
        assert_eq!(display.scrollback_offset, 0);
        assert!(!display.open_line);
        assert_eq!(display.no_reply_frames, 1);
        assert_eq!(display.redraw_no_reply_streak, 0);
        assert_eq!(
            display.no_reply_cooldown_turns,
            LINKED_LOCAL_SEAT_HDMI_NO_REPLY_COOLDOWN_TURNS
        );
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
        assert_no_blink_snapshot_payload(snapshot.as_slice());
    }

    #[test]
    fn runtime_hdmi_redraw_no_reply_restarts_from_fresh_snapshot() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 4,
        });

        runtime.mirror_line_current_tcb("cohesix> help");
        runtime.request_linked_hdmi_snapshot_redraw();
        let Some((submitted, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected redraw payload");
        };
        assert_eq!(reason, "keyboard-scrollback");
        assert!(redraw);

        runtime.record_linked_hdmi_submit_miss(submitted.as_slice(), redraw);

        let display = runtime.display_trace();
        assert_eq!(display.no_reply_frames, 1);
        assert_eq!(display.deferred_frames, 1);
        assert!(display.pending_redraw);
        assert_eq!(display.redraw_bytes, 0);
        assert_eq!(display.coalesced_redraws, 0);
        assert_eq!(display.superseded_bytes, submitted.len() as u64);
        assert_eq!(display.redraw_no_reply_streak, 1);
        assert_eq!(
            display.no_reply_cooldown_turns,
            LINKED_LOCAL_SEAT_HDMI_NO_REPLY_COOLDOWN_TURNS
        );

        let Some((restart, restart_reason, restart_redraw)) = runtime.next_linked_hdmi_payload()
        else {
            panic!("expected restarted redraw payload");
        };
        assert_eq!(restart_reason, "keyboard-scrollback");
        assert!(restart_redraw);
        assert_no_blink_snapshot_payload(restart.as_slice());
    }

    #[test]
    fn runtime_hdmi_redraw_no_reply_stops_replay_loop() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 4,
        });

        runtime.mirror_line_current_tcb("cohesix> help");
        runtime.request_linked_hdmi_snapshot_redraw();

        for _ in 0..=LINKED_LOCAL_SEAT_HDMI_REDRAW_NO_REPLY_RETRY_LIMIT {
            let Some((submitted, _, redraw)) = runtime.next_linked_hdmi_payload() else {
                panic!("expected redraw payload before retry limit");
            };
            assert!(redraw);
            runtime.record_linked_hdmi_submit_miss(submitted.as_slice(), redraw);
        }

        let display = runtime.display_trace();
        assert_eq!(
            display.no_reply_frames,
            u64::from(LINKED_LOCAL_SEAT_HDMI_REDRAW_NO_REPLY_RETRY_LIMIT) + 1
        );
        assert_eq!(
            display.deferred_frames,
            u64::from(LINKED_LOCAL_SEAT_HDMI_REDRAW_NO_REPLY_RETRY_LIMIT) + 1
        );
        assert!(!display.pending_redraw);
        assert_eq!(display.redraw_bytes, 0);
        assert_eq!(display.pending_bytes, 0);
        assert!(display.stale_after_retry_exhaustion);
        assert!(display.superseded_bytes != 0);
        assert_eq!(
            display.redraw_no_reply_streak,
            LINKED_LOCAL_SEAT_HDMI_REDRAW_NO_REPLY_RETRY_LIMIT + 1
        );
        assert!(!runtime.linked_hdmi_pending_work());

        runtime.mirror_line_current_tcb("fresh-tail");
        runtime.refresh_linked_hdmi_redraw_after_content_mutation();
        let display = runtime.display_trace();
        assert!(display.pending_redraw);
        assert!(!display.stale_after_retry_exhaustion);
        assert_eq!(display.pending_bytes, 0);
    }

    #[test]
    fn runtime_hdmi_tail_match_uses_display_truncation() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 4,
        });

        runtime.mirror_line_current_tcb("[drivers] USB usb-engine-init fault detail=0x0003");

        assert!(runtime
            .linked_hdmi_tail_line_matches("[drivers] USB usb-engine-init fault detail=0x0003"));
        assert!(runtime.linked_hdmi_tail_line_matches(
            "[drivers] USB usb-engine-init fault detail=0x0003 result=0"
        ));
        assert!(!runtime.linked_hdmi_tail_line_matches("[drivers] WiFi cyw43 fault"));
    }

    #[test]
    fn runtime_hdmi_no_reply_cooldown_defers_retry_without_dropping_snapshot() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 4,
        });

        runtime.mirror_line_current_tcb("cohesix> help");
        runtime.request_linked_hdmi_snapshot_redraw();
        let Some((submitted, _, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected redraw payload");
        };
        assert!(redraw);

        runtime.record_linked_hdmi_submit_miss(submitted.as_slice(), redraw);
        assert!(runtime.linked_hdmi_pending_work());
        assert_eq!(
            runtime.display_trace().no_reply_cooldown_turns,
            LINKED_LOCAL_SEAT_HDMI_NO_REPLY_COOLDOWN_TURNS
        );

        for _ in 0..LINKED_LOCAL_SEAT_HDMI_NO_REPLY_COOLDOWN_TURNS {
            assert!(runtime.linked_hdmi_retry_cooldown_active());
        }
        assert!(!runtime.linked_hdmi_retry_cooldown_active());
        assert!(runtime.linked_hdmi_pending_work());
        assert_eq!(
            runtime.display_trace().deferred_frames,
            u64::from(LINKED_LOCAL_SEAT_HDMI_NO_REPLY_COOLDOWN_TURNS) + 1
        );
    }

    #[test]
    fn runtime_hdmi_help_snapshot_supersedes_stale_netstats_bytes() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 32,
            buffer_lines: 4,
        });
        let stale = b"netstats rx=old tx=old\nnetstats drops=old\n";

        runtime.mirror_line_current_tcb("help");
        assert!(runtime.queue_linked_hdmi_payload(stale));
        runtime.request_linked_hdmi_snapshot_redraw();

        let display = runtime.display_trace();
        assert_eq!(display.pending_bytes, 0);
        assert_eq!(display.superseded_bytes, stale.len() as u64);
        assert!(display.pending_redraw);

        let Some((snapshot, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected help snapshot");
        };
        assert_eq!(reason, "keyboard-scrollback");
        assert!(redraw);
        assert_no_blink_snapshot_payload(snapshot.as_slice());
        assert!(snapshot
            .windows(b"help".len())
            .any(|window| window == b"help"));
        assert!(!snapshot
            .windows(b"netstats".len())
            .any(|window| window == b"netstats"));
    }

    #[test]
    fn runtime_hdmi_new_tail_finishes_materialized_redraw_before_refresh() {
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

        runtime.request_linked_hdmi_snapshot_redraw();
        let Some((first_chunk, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected first redraw chunk");
        };
        assert_eq!(reason, "keyboard-scrollback");
        assert!(redraw);
        assert_eq!(first_chunk.len(), LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES);
        assert!(runtime.display_trace().redraw_bytes > 0);
        let generation_before = runtime.display_trace().snapshot_generation;

        runtime.mirror_line_current_tcb("new-tail");
        runtime.refresh_linked_hdmi_redraw_after_content_mutation();

        let display = runtime.display_trace();
        assert!(display.pending_redraw);
        assert!(display.redraw_bytes > 0);
        assert_eq!(display.superseded_bytes, 0);
        assert!(display.snapshot_generation > generation_before);

        let mut rebuilt = first_chunk;
        while let Some((chunk, reason, redraw)) = runtime.next_linked_hdmi_payload() {
            assert_eq!(reason, "keyboard-scrollback");
            assert!(redraw);
            rebuilt.extend_from_slice(chunk.as_slice());
        }
        assert_no_blink_snapshot_payload(rebuilt.as_slice());
        assert!(rebuilt
            .windows(b"new-tail".len())
            .any(|window| window == b"new-tail"));
    }

    #[test]
    fn runtime_hdmi_input_echo_preserves_queued_output_until_receipt() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 32,
            buffer_lines: 4,
        });
        let queued = b"netstats rx=old tx=old\n";

        runtime.mirror_line_current_tcb("cohesix> ");
        runtime.open_linked_hdmi_prompt_line("cohesix> ");
        assert!(runtime.queue_linked_hdmi_payload(queued));
        runtime.record_linked_hdmi_input_echo_byte(b'h');
        runtime.queue_linked_hdmi_input_row_or_redraw();

        let display = runtime.display_trace();
        assert_eq!(runtime.hdmi_pending_bytes.len(), queued.len());
        assert_eq!(
            display.pending_bytes,
            queued.len() + runtime.hdmi_input_row_payload.len(),
        );
        assert_eq!(display.superseded_bytes, 0);
        assert!(!display.pending_redraw);

        let Some((output, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected retained queued output");
        };
        assert_eq!(reason, "queued-output");
        assert!(!redraw);
        assert_eq!(output.as_slice(), queued);
        let receipt = runtime.hdmi_last_payload_receipt;
        runtime.record_linked_hdmi_submit_success(receipt);

        let Some((input_line, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected input line after retained output");
        };
        assert_eq!(reason, "keyboard-input-row");
        assert!(!redraw);
        assert!(input_line.starts_with(b"\r\x1b[2K"));
        assert!(!input_line.starts_with(b"\n"));
        assert!(input_line
            .windows(b"cohesix> h".len())
            .any(|window| window == b"cohesix> h"));
        assert!(!input_line
            .windows(b"netstats".len())
            .any(|window| window == b"netstats"));
    }

    #[test]
    fn runtime_hdmi_high_impact_status_restores_partial_command_row() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 96,
            buffer_lines: 8,
        });
        runtime.mirror_line_current_tcb("cohesix> ");
        runtime.open_linked_hdmi_prompt_line("cohesix> ");
        for &byte in b"hel" {
            runtime.record_linked_hdmi_input_echo_byte(byte);
        }

        assert!(runtime.mirror_linked_hdmi_high_impact_above_open_line(
            "[drivers] WiFi repairing CYW43/SDIO within the active boot episode",
        ));

        let lines = runtime.mirrored_lines_snapshot();
        assert_eq!(
            lines[lines.len() - 2],
            "[drivers] WiFi repairing CYW43/SDIO within the active boot episode"
        );
        assert_eq!(lines[lines.len() - 1], "cohesix> hel");
        assert!(runtime.hdmi_open_line);
        assert_eq!(runtime.hdmi_open_line_floor_bytes, "cohesix> ".len());

        let Some((payload, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected atomic status and command-row payload");
        };
        assert_eq!(reason, "queued-output");
        assert!(!redraw);
        assert!(payload.starts_with(b"\r\x1b[2K[drivers] WiFi"));
        assert!(payload.ends_with(b"\ncohesix> hel\x1b[K"));
        assert_eq!(payload.iter().filter(|&&byte| byte == b'\n').count(), 1);
    }

    #[test]
    fn runtime_hdmi_repeated_input_repaints_one_row_without_losing_status() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 96,
            buffer_lines: 8,
        });
        runtime.mirror_line_current_tcb("cohesix> ");
        runtime.open_linked_hdmi_prompt_line("cohesix> ");
        assert!(
            runtime.mirror_linked_hdmi_high_impact_above_open_line("[drivers] WiFi attempt active")
        );

        runtime.record_linked_hdmi_input_echo_byte(b'h');
        runtime.queue_linked_hdmi_input_row_or_redraw();
        runtime.record_linked_hdmi_input_echo_byte(b'i');
        runtime.queue_linked_hdmi_input_row_or_redraw();
        runtime.record_linked_hdmi_input_echo_byte(0x08);
        runtime.queue_linked_hdmi_input_row_or_redraw();

        let mut payload = Vec::new();
        while let Some((chunk, reason, redraw)) = runtime.next_linked_hdmi_payload() {
            assert!(matches!(reason, "queued-output" | "keyboard-input-row"));
            assert!(!redraw);
            let receipt = runtime.hdmi_last_payload_receipt;
            payload.extend_from_slice(chunk.as_slice());
            runtime.record_linked_hdmi_submit_success(receipt);
        }
        assert!(payload
            .windows(b"[drivers] WiFi attempt active".len())
            .any(|window| window == b"[drivers] WiFi attempt active"));
        assert!(payload.ends_with(b"\r\x1b[2Kcohesix> h\x1b[K"));
        assert_eq!(
            runtime.mirrored_lines_snapshot().last().map(String::as_str),
            Some("cohesix> h")
        );
        assert!(!runtime.display_trace().pending_redraw);
    }

    #[test]
    fn runtime_hdmi_input_row_waits_for_matching_completion_receipt() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 32,
            buffer_lines: 8,
        });
        runtime.mirror_line_current_tcb("cohesix> ");
        runtime.open_linked_hdmi_prompt_line("cohesix> ");

        assert!(runtime.record_linked_hdmi_input_echo_byte(b'h'));
        runtime.queue_linked_hdmi_input_row_or_redraw();
        let Some((first, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected first canonical input row");
        };
        assert_eq!(reason, "keyboard-input-row");
        assert!(!redraw);
        assert!(first.windows(10).any(|window| window == b"cohesix> h"));
        let first_receipt = runtime.hdmi_last_payload_receipt;
        assert!(runtime.hdmi_input_row_dirty);

        assert!(runtime.record_linked_hdmi_input_echo_byte(b'i'));
        runtime.queue_linked_hdmi_input_row_or_redraw();
        runtime.record_linked_hdmi_submit_success(first_receipt);
        assert!(
            runtime.hdmi_input_row_dirty,
            "an older completion cannot acknowledge a newer canonical row",
        );

        let Some((second, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected refreshed canonical input row");
        };
        assert_eq!(reason, "keyboard-input-row");
        assert!(!redraw);
        assert!(second.windows(11).any(|window| window == b"cohesix> hi"));
        let second_receipt = runtime.hdmi_last_payload_receipt;
        runtime.record_linked_hdmi_submit_success(second_receipt);
        assert!(!runtime.hdmi_input_row_dirty);
        assert!(!runtime.linked_hdmi_pending_work());
    }

    #[test]
    fn runtime_hdmi_input_row_preserves_fifo_output_before_and_after_it() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 32,
            buffer_lines: 8,
        });
        runtime.mirror_line_current_tcb("cohesix> ");
        runtime.open_linked_hdmi_prompt_line("cohesix> ");
        assert!(runtime.queue_linked_hdmi_line("prior output"));

        assert!(runtime.record_linked_hdmi_input_echo_byte(b'h'));
        runtime.queue_linked_hdmi_input_row_or_redraw();
        assert!(runtime.queue_linked_hdmi_line("later output"));

        let Some((prior, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected retained FIFO prefix");
        };
        assert_eq!(reason, "queued-output");
        assert!(!redraw);
        assert_eq!(prior.as_slice(), b"prior output\n");
        let receipt = runtime.hdmi_last_payload_receipt;
        runtime.record_linked_hdmi_submit_success(receipt);

        let Some((input, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected canonical input row after FIFO prefix");
        };
        assert_eq!(reason, "keyboard-input-row");
        assert!(!redraw);
        assert_eq!(input.as_slice(), b"\r\x1b[2Kcohesix> h\x1b[K");
        let receipt = runtime.hdmi_last_payload_receipt;
        runtime.record_linked_hdmi_submit_success(receipt);

        let Some((later, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected later FIFO output after canonical row");
        };
        assert_eq!(reason, "queued-output");
        assert!(!redraw);
        assert_eq!(later.as_slice(), b"later output\n");
    }

    #[test]
    fn runtime_hdmi_input_row_extends_existing_priority_fifo_boundary() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 32,
            buffer_lines: 8,
        });
        runtime.mirror_line_current_tcb("cohesix> ");
        runtime.open_linked_hdmi_prompt_line("cohesix> ");
        assert!(runtime.queue_linked_hdmi_line("priority status"));
        runtime.mark_linked_hdmi_high_impact_queued();
        assert!(runtime.queue_linked_hdmi_line("older ordinary output"));

        assert!(runtime.record_linked_hdmi_input_echo_byte(b'h'));
        runtime.queue_linked_hdmi_input_row_or_redraw();

        let Some((older, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected the complete older FIFO boundary");
        };
        assert_eq!(reason, "queued-output");
        assert!(!redraw);
        assert_eq!(
            older.as_slice(),
            b"priority status\nolder ordinary output\n",
        );
        let receipt = runtime.hdmi_last_payload_receipt;
        runtime.record_linked_hdmi_submit_success(receipt);

        let Some((input, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected canonical input row after all older output");
        };
        assert_eq!(reason, "keyboard-input-row");
        assert!(!redraw);
        assert_eq!(input.as_slice(), b"\r\x1b[2Kcohesix> h\x1b[K");
    }

    #[test]
    fn runtime_hdmi_input_after_retry_exhaustion_rearms_canonical_redraw() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 32,
            buffer_lines: 8,
        });
        runtime.mirror_line_current_tcb("cohesix> ");
        runtime.open_linked_hdmi_prompt_line("cohesix> ");
        runtime.hdmi_stale_after_retry_exhaustion = true;

        assert!(runtime.record_linked_hdmi_input_echo_byte(b'h'));
        runtime.queue_linked_hdmi_input_row_or_redraw();

        assert!(runtime.hdmi_pending_redraw);
        assert!(!runtime.hdmi_stale_after_retry_exhaustion);
        assert!(runtime.hdmi_input_row_dirty);
        let mut snapshot = Vec::new();
        while let Some((chunk, reason, redraw)) = runtime.next_linked_hdmi_payload() {
            assert_eq!(reason, "keyboard-scrollback");
            assert!(redraw);
            let receipt = runtime.hdmi_last_payload_receipt;
            snapshot.extend_from_slice(chunk.as_slice());
            runtime.record_linked_hdmi_submit_success(receipt);
        }
        assert!(snapshot.windows(10).any(|window| window == b"cohesix> h"));
        assert!(!runtime.hdmi_input_row_dirty);
        assert!(!runtime.hdmi_stale_after_retry_exhaustion);
    }

    #[test]
    fn runtime_hdmi_usb_readiness_retracts_and_rereleases_ready_banner() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 64,
            buffer_lines: 8,
        });
        runtime.root_console_ready = true;
        runtime.hdmi_bootstrap_terminal_ready = true;
        runtime.hdmi_console_ready_line_emitted = true;
        runtime.mirror_line_current_tcb("Cohesix console ready");
        assert!(runtime.queue_linked_hdmi_line("Cohesix console ready"));
        runtime.mirror_line_current_tcb("cohesix> ");
        runtime.open_linked_hdmi_prompt_line("cohesix> ");

        assert!(runtime.retract_linked_hdmi_console_ready_until_usb_ready());
        assert!(!runtime.hdmi_console_ready_line_emitted);
        assert!(!runtime
            .mirrored_lines_snapshot()
            .iter()
            .any(|line| line == "Cohesix console ready"));

        let mut retracted = Vec::new();
        while let Some((chunk, reason, redraw)) = runtime.next_linked_hdmi_payload() {
            assert_eq!(reason, "keyboard-scrollback");
            assert!(redraw);
            let receipt = runtime.hdmi_last_payload_receipt;
            retracted.extend_from_slice(chunk.as_slice());
            runtime.record_linked_hdmi_submit_success(receipt);
        }
        assert!(!retracted
            .windows(b"Cohesix console ready".len())
            .any(|window| window == b"Cohesix console ready"));
        assert!(retracted
            .windows(b"cohesix> ".len())
            .any(|window| window == b"cohesix> "));

        runtime.emit_linked_hdmi_console_ready_if_due(true);
        assert!(runtime.hdmi_console_ready_line_emitted);
        assert_eq!(
            runtime
                .mirrored_lines_snapshot()
                .iter()
                .filter(|line| line.as_str() == "Cohesix console ready")
                .count(),
            1,
        );
    }

    #[test]
    fn runtime_hdmi_usb_readiness_invalidation_retracts_and_rereleases_prompt() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 32,
            buffer_lines: 8,
        });
        runtime.mirror_line_current_tcb("cohesix> ");
        runtime.open_linked_hdmi_prompt_line("cohesix> ");
        assert!(runtime.record_linked_hdmi_input_echo_byte(b'h'));
        assert!(runtime.record_linked_hdmi_input_echo_byte(b'i'));
        runtime.queue_linked_hdmi_input_row_or_redraw();
        let Some((_, _, _)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected initial prompt row");
        };
        let receipt = runtime.hdmi_last_payload_receipt;
        runtime.record_linked_hdmi_submit_success(receipt);

        assert!(runtime.retract_linked_hdmi_prompt_until_usb_ready("cohesix> "));
        assert_eq!(
            runtime.mirrored_lines_snapshot().last().map(String::as_str),
            Some("hi"),
        );
        assert_eq!(runtime.hdmi_open_line_floor_bytes, 0);
        let Some((clear, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected prompt retraction row");
        };
        assert_eq!(reason, "keyboard-input-row");
        assert!(!redraw);
        assert_eq!(clear.as_slice(), b"\r\x1b[2K");
        let stale_clear_receipt = runtime.hdmi_last_payload_receipt;

        runtime.mirror_linked_hdmi_prompt_now("cohesix> ");
        runtime.record_linked_hdmi_submit_success(stale_clear_receipt);
        assert!(runtime.hdmi_input_row_dirty);
        assert_eq!(
            runtime.mirrored_lines_snapshot().last().map(String::as_str),
            Some("cohesix> hi"),
        );

        let Some((restored, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected freshly admitted prompt row");
        };
        assert_eq!(reason, "keyboard-input-row");
        assert!(!redraw);
        assert_eq!(restored.as_slice(), b"\r\x1b[2Kcohesix> hi\x1b[K");
        let receipt = runtime.hdmi_last_payload_receipt;
        runtime.record_linked_hdmi_submit_success(receipt);
        assert!(!runtime.hdmi_input_row_dirty);
    }

    #[test]
    fn runtime_hdmi_backspace_stops_at_prompt_floor() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 32,
            buffer_lines: 8,
        });
        runtime.mirror_line_current_tcb("cohesix> ");
        runtime.open_linked_hdmi_prompt_line("cohesix> ");

        assert!(!runtime.record_linked_hdmi_input_echo_byte(0x08));
        assert_eq!(
            runtime.mirrored_lines_snapshot().last().map(String::as_str),
            Some("cohesix> ")
        );
        assert!(!runtime.hdmi_input_row_dirty);

        assert!(runtime.record_linked_hdmi_input_echo_byte(b'x'));
        assert!(runtime.record_linked_hdmi_input_echo_byte(0x08));
        assert!(!runtime.record_linked_hdmi_input_echo_byte(0x7f));
        runtime.queue_linked_hdmi_input_row_or_redraw();
        let Some((payload, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected protected prompt repaint");
        };
        assert_eq!(reason, "keyboard-input-row");
        assert!(!redraw);
        assert!(!payload.contains(&0x08));
        assert!(!payload.contains(&0x7f));
        assert!(payload.windows(9).any(|window| window == b"cohesix> "));
    }

    #[test]
    fn runtime_hdmi_rapid_arrows_coalesce_to_symmetric_bounded_scroll_spans() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 32,
            buffer_lines: 40,
        });
        for index in 0..40 {
            let mut line = String::from("line-");
            let _ = core::fmt::Write::write_fmt(&mut line, format_args!("{index:02}"));
            runtime.mirror_line_current_tcb(line.as_str());
        }
        runtime.request_linked_hdmi_snapshot_redraw();
        while let Some((_, _, redraw)) = runtime.next_linked_hdmi_payload() {
            assert!(redraw);
            let receipt = runtime.hdmi_last_payload_receipt;
            runtime.record_linked_hdmi_submit_success(receipt);
        }
        assert!(runtime.hdmi_rendered_viewport_valid);
        assert_eq!(runtime.hdmi_rendered_scrollback_offset, 0);

        assert!(runtime.apply_linked_hdmi_scroll_delta(6));
        let Some((older, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected coalesced older scroll span");
        };
        assert_eq!(reason, "keyboard-scroll-step");
        assert!(!redraw);
        assert_eq!(
            older.as_slice(),
            b"\x1b7\x1b[6T\x1b[Hline-06\x1b[K\nline-07\x1b[K\nline-08\x1b[K\nline-09\x1b[K\nline-10\x1b[K\nline-11\x1b[K\x1b8",
        );
        assert!(older.len() <= LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES);
        let receipt = runtime.hdmi_last_payload_receipt;
        runtime.record_linked_hdmi_submit_success(receipt);
        assert_eq!(runtime.hdmi_rendered_scrollback_offset, 6);
        assert!(!runtime.linked_hdmi_pending_work());

        assert!(runtime.apply_linked_hdmi_scroll_delta(-99));
        let Some((newer, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected coalesced live-tail scroll span");
        };
        assert_eq!(reason, "keyboard-scroll-step");
        assert!(!redraw);
        assert_eq!(
            newer.as_slice(),
            b"\x1b7\x1b[6S\x1b[23;1Hline-34\x1b[K\nline-35\x1b[K\nline-36\x1b[K\nline-37\x1b[K\nline-38\x1b[K\nline-39\x1b[K\x1b8",
        );
        assert!(newer.len() <= LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES);
        let receipt = runtime.hdmi_last_payload_receipt;
        runtime.record_linked_hdmi_submit_success(receipt);
        assert_eq!(runtime.hdmi_rendered_scrollback_offset, 0);
        assert_eq!(runtime.hdmi_scrollback_offset, 0);
        assert!(!runtime.linked_hdmi_pending_work());
        assert!(!runtime.apply_linked_hdmi_scroll_delta(-1));
        assert!(runtime.next_linked_hdmi_payload().is_none());
    }

    #[test]
    fn runtime_hdmi_full_buffer_down_receipt_rebases_before_bounded_chase() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 32,
            buffer_lines: 40,
        });
        for index in 0..40 {
            let mut line = String::from("line-");
            let _ = core::fmt::Write::write_fmt(&mut line, format_args!("{index:02}"));
            runtime.mirror_line_current_tcb(line.as_str());
        }
        runtime.request_linked_hdmi_snapshot_redraw();
        while let Some((_, _, redraw)) = runtime.next_linked_hdmi_payload() {
            assert!(redraw);
            let receipt = runtime.hdmi_last_payload_receipt;
            runtime.record_linked_hdmi_submit_success(receipt);
        }

        assert!(runtime.apply_linked_hdmi_scroll_delta(6));
        let Some((_, _, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected initial older scroll");
        };
        assert!(!redraw);
        let receipt = runtime.hdmi_last_payload_receipt;
        runtime.record_linked_hdmi_submit_success(receipt);
        assert_eq!(runtime.hdmi_rendered_scrollback_offset, 6);

        assert!(runtime.apply_linked_hdmi_scroll_delta(-3));
        let Some((submitted_down, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected retained down-scroll frame");
        };
        assert_eq!(reason, "keyboard-scroll-step");
        assert!(!redraw);
        assert!(submitted_down
            .windows(b"\x1b[3S".len())
            .any(|window| window == b"\x1b[3S"));
        let retained_receipt = runtime.hdmi_last_payload_receipt;

        runtime.mirror_line_current_tcb("line-40");
        assert_eq!(runtime.dropped_mirrored_lines, 1);
        assert_eq!(runtime.hdmi_scrollback_offset, 4);
        assert_eq!(runtime.hdmi_rendered_scrollback_offset, 7);
        assert!(runtime.apply_linked_hdmi_scroll_delta(-2));
        assert_eq!(runtime.hdmi_scrollback_offset, 2);

        runtime.record_linked_hdmi_submit_success(retained_receipt);
        assert_eq!(
            runtime.hdmi_rendered_scrollback_offset, 4,
            "the completed frame is rebased by the one exact anchored append",
        );
        assert!(runtime.hdmi_rendered_viewport_valid);
        assert!(!runtime.hdmi_pending_redraw);

        let Some((follow_up, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected bounded follow-up chase to the newer target");
        };
        assert_eq!(reason, "keyboard-scroll-step");
        assert!(!redraw);
        assert_eq!(
            follow_up.as_slice(),
            b"\x1b7\x1b[2S\x1b[27;1Hline-37\x1b[K\nline-38\x1b[K\x1b8",
        );
        let receipt = runtime.hdmi_last_payload_receipt;
        runtime.record_linked_hdmi_submit_success(receipt);
        assert_eq!(runtime.hdmi_rendered_scrollback_offset, 2);
        assert!(!runtime.linked_hdmi_pending_work());
    }

    #[test]
    fn runtime_hdmi_live_tail_append_during_scroll_has_one_success_or_recovery_renderer() {
        let settled_scrolled_runtime = || {
            let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
                keyboard_device: "usb-kbd0",
                display_device: "hdmi0",
                line_bytes: 32,
                buffer_lines: 64,
            });
            runtime.root_console_ready = true;
            for index in 0..40 {
                let mut line = String::from("line-");
                let _ = core::fmt::Write::write_fmt(&mut line, format_args!("{index:02}"));
                runtime.mirror_line_current_tcb(line.as_str());
            }
            runtime.request_linked_hdmi_snapshot_redraw();
            while let Some((_, _, redraw)) = runtime.next_linked_hdmi_payload() {
                assert!(redraw);
                let receipt = runtime.hdmi_last_payload_receipt;
                runtime.record_linked_hdmi_submit_success(receipt);
            }
            assert!(runtime.apply_linked_hdmi_scroll_delta(6));
            let Some((_, _, redraw)) = runtime.next_linked_hdmi_payload() else {
                panic!("expected initial older scroll");
            };
            assert!(!redraw);
            let receipt = runtime.hdmi_last_payload_receipt;
            runtime.record_linked_hdmi_submit_success(receipt);
            runtime
        };

        let mut success = settled_scrolled_runtime();
        assert!(success.apply_linked_hdmi_scroll_delta(-6));
        let Some((submitted_down, _, redraw)) = success.next_linked_hdmi_payload() else {
            panic!("expected retained live-tail scroll");
        };
        assert!(!redraw);
        assert!(submitted_down
            .windows(b"\x1b[6S".len())
            .any(|window| window == b"\x1b[6S"));
        let retained_receipt = success.hdmi_last_payload_receipt;
        success.mirror_linked_hdmi_line_owner_state("line-40");
        assert!(
            success.hdmi_pending_bytes.is_empty(),
            "the generation-bound chase is the sole renderer while scroll is retained",
        );
        success.record_linked_hdmi_submit_success(retained_receipt);
        assert_eq!(success.hdmi_rendered_scrollback_offset, 1);
        let Some((chase, reason, redraw)) = success.next_linked_hdmi_payload() else {
            panic!("expected one generation-bound live-tail chase");
        };
        assert_eq!(reason, "keyboard-scroll-step");
        assert!(!redraw);
        assert_eq!(
            chase.as_slice(),
            b"\x1b7\x1b[1S\x1b[28;1Hline-40\x1b[K\x1b8",
        );
        let receipt = success.hdmi_last_payload_receipt;
        success.record_linked_hdmi_submit_success(receipt);
        assert!(!success.linked_hdmi_pending_work());

        success.mirror_linked_hdmi_line_owner_state("line-41");
        let Some((ordinary_tail, reason, redraw)) = success.next_linked_hdmi_payload() else {
            panic!("ordinary live-tail output must retain its FIFO path");
        };
        assert_eq!(reason, "queued-output");
        assert!(!redraw);
        assert_eq!(ordinary_tail.as_slice(), b"line-41\n");
        let receipt = success.hdmi_last_payload_receipt;
        success.record_linked_hdmi_submit_success(receipt);

        let mut failed = settled_scrolled_runtime();
        assert!(failed.apply_linked_hdmi_scroll_delta(-6));
        let Some((failed_down, _, redraw)) = failed.next_linked_hdmi_payload() else {
            panic!("expected retained live-tail scroll");
        };
        assert!(!redraw);
        failed.mirror_linked_hdmi_line_owner_state("line-40");
        assert!(failed.hdmi_pending_bytes.is_empty());
        failed.record_linked_hdmi_submit_miss(failed_down.as_slice(), false);
        assert!(failed.hdmi_pending_redraw);
        let mut recovery = Vec::new();
        let mut recovery_frames = 0usize;
        while let Some((payload, reason, redraw)) = failed.next_linked_hdmi_payload() {
            assert_eq!(reason, "keyboard-scrollback");
            assert!(redraw);
            recovery.extend_from_slice(payload.as_slice());
            let receipt = failed.hdmi_last_payload_receipt;
            failed.record_linked_hdmi_submit_success(receipt);
            recovery_frames = recovery_frames.saturating_add(1);
            assert!(recovery_frames <= 16);
        }
        assert_eq!(
            recovery
                .windows(b"line-40".len())
                .filter(|window| *window == b"line-40")
                .count(),
            1,
        );
        assert!(!failed.linked_hdmi_pending_work());
    }

    #[test]
    fn runtime_hdmi_live_tail_append_before_scroll_submit_rebases_physical_anchor() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 32,
            buffer_lines: 64,
        });
        runtime.root_console_ready = true;
        for index in 0..40 {
            let mut line = String::from("line-");
            let _ = core::fmt::Write::write_fmt(&mut line, format_args!("{index:02}"));
            runtime.mirror_line_current_tcb(line.as_str());
        }
        runtime.request_linked_hdmi_snapshot_redraw();
        while let Some((_, _, redraw)) = runtime.next_linked_hdmi_payload() {
            assert!(redraw);
            let receipt = runtime.hdmi_last_payload_receipt;
            runtime.record_linked_hdmi_submit_success(receipt);
        }

        assert!(runtime.apply_linked_hdmi_scroll_delta(6));
        let Some((_, _, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected initial older scroll");
        };
        assert!(!redraw);
        let receipt = runtime.hdmi_last_payload_receipt;
        runtime.record_linked_hdmi_submit_success(receipt);
        assert_eq!(runtime.hdmi_rendered_scrollback_offset, 6);

        assert!(runtime.apply_linked_hdmi_scroll_delta(-6));
        assert_eq!(runtime.hdmi_scrollback_offset, 0);
        assert_eq!(runtime.hdmi_rendered_scrollback_offset, 6);
        assert!(!runtime.linked_hdmi_scroll_step_in_flight());

        runtime.mirror_linked_hdmi_line_owner_state("line-40");
        assert!(
            runtime.hdmi_pending_bytes.is_empty(),
            "an unsettled physical viewport must not also receive live-tail FIFO bytes",
        );
        assert_eq!(runtime.hdmi_scrollback_offset, 0);
        assert_eq!(runtime.hdmi_rendered_scrollback_offset, 7);

        let Some((chase, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected one canonical chase from the rebased physical anchor");
        };
        assert_eq!(reason, "keyboard-scroll-step");
        assert!(!redraw);
        assert_eq!(
            chase.as_slice(),
            b"\x1b7\x1b[7S\x1b[22;1Hline-34\x1b[K\nline-35\x1b[K\nline-36\x1b[K\nline-37\x1b[K\nline-38\x1b[K\nline-39\x1b[K\nline-40\x1b[K\x1b8",
        );
        assert_eq!(
            chase
                .windows(b"line-40".len())
                .filter(|window| *window == b"line-40")
                .count(),
            1,
        );
        let receipt = runtime.hdmi_last_payload_receipt;
        runtime.record_linked_hdmi_submit_success(receipt);
        assert_eq!(runtime.hdmi_rendered_scrollback_offset, 0);
        assert!(!runtime.linked_hdmi_pending_work());
    }

    #[test]
    fn runtime_hdmi_printable_before_live_tail_scroll_submit_rebases_open_row() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 32,
            buffer_lines: 64,
        });
        runtime.root_console_ready = true;
        for index in 0..40 {
            let mut line = String::from("line-");
            let _ = core::fmt::Write::write_fmt(&mut line, format_args!("{index:02}"));
            runtime.mirror_line_current_tcb(line.as_str());
        }
        runtime.request_linked_hdmi_snapshot_redraw();
        while let Some((_, _, redraw)) = runtime.next_linked_hdmi_payload() {
            assert!(redraw);
            let receipt = runtime.hdmi_last_payload_receipt;
            runtime.record_linked_hdmi_submit_success(receipt);
        }
        assert!(runtime.apply_linked_hdmi_scroll_delta(6));
        let Some((_, _, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected initial older scroll");
        };
        assert!(!redraw);
        let receipt = runtime.hdmi_last_payload_receipt;
        runtime.record_linked_hdmi_submit_success(receipt);
        assert!(runtime.apply_linked_hdmi_scroll_delta(-6));
        assert!(!runtime.hdmi_open_line);
        assert!(!runtime.linked_hdmi_scroll_step_in_flight());

        assert!(runtime.record_linked_hdmi_input_echo_byte(b'x'));
        runtime.queue_linked_hdmi_input_row_or_redraw();
        assert_eq!(runtime.hdmi_scrollback_offset, 0);
        assert_eq!(runtime.hdmi_rendered_scrollback_offset, 7);
        assert_eq!(
            runtime.mirrored_lines_snapshot().last().map(String::as_str),
            Some("x"),
        );

        let Some((chase, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected canonical chase including the new input row");
        };
        assert_eq!(reason, "keyboard-scroll-step");
        assert!(!redraw);
        assert_eq!(
            chase.as_slice(),
            b"\x1b7\x1b[7S\x1b[22;1Hline-34\x1b[K\nline-35\x1b[K\nline-36\x1b[K\nline-37\x1b[K\nline-38\x1b[K\nline-39\x1b[K\nx\x1b[K\x1b8",
        );
        let receipt = runtime.hdmi_last_payload_receipt;
        runtime.record_linked_hdmi_submit_success(receipt);
        assert_eq!(runtime.hdmi_rendered_scrollback_offset, 0);
        let Some((input_row, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected generation-tracked input-row completion");
        };
        assert_eq!(reason, "keyboard-input-row");
        assert!(!redraw);
        assert_eq!(input_row.as_slice(), b"\r\x1b[2Kx\x1b[K");
        let receipt = runtime.hdmi_last_payload_receipt;
        runtime.record_linked_hdmi_submit_success(receipt);
        assert!(!runtime.hdmi_input_row_dirty);
        assert!(!runtime.linked_hdmi_pending_work());
    }

    #[test]
    fn runtime_hdmi_ready_banner_during_unsubmitted_down_uses_canonical_chase() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 32,
            buffer_lines: 64,
        });
        runtime.root_console_ready = true;
        runtime.hdmi_bootstrap_terminal_ready = true;
        for index in 0..40 {
            let mut line = String::from("line-");
            let _ = core::fmt::Write::write_fmt(&mut line, format_args!("{index:02}"));
            runtime.mirror_line_current_tcb(line.as_str());
        }
        runtime.request_linked_hdmi_snapshot_redraw();
        while let Some((_, _, redraw)) = runtime.next_linked_hdmi_payload() {
            assert!(redraw);
            let receipt = runtime.hdmi_last_payload_receipt;
            runtime.record_linked_hdmi_submit_success(receipt);
        }
        assert!(runtime.apply_linked_hdmi_scroll_delta(6));
        let Some((_, _, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected initial older scroll");
        };
        assert!(!redraw);
        let receipt = runtime.hdmi_last_payload_receipt;
        runtime.record_linked_hdmi_submit_success(receipt);
        assert!(runtime.apply_linked_hdmi_scroll_delta(-6));
        runtime.open_linked_hdmi_prompt_line("line-39");

        runtime.emit_linked_hdmi_console_ready_if_due(true);
        assert!(runtime.hdmi_console_ready_line_emitted);
        assert!(runtime.hdmi_pending_bytes.is_empty());
        assert_eq!(runtime.hdmi_scrollback_offset, 0);
        assert!(runtime.hdmi_pending_redraw);
        assert_eq!(
            runtime.mirrored_lines_snapshot().last().map(String::as_str),
            Some("line-39"),
        );

        let mut recovery = Vec::new();
        let mut frames = 0usize;
        while let Some((payload, reason, redraw)) = runtime.next_linked_hdmi_payload() {
            assert_eq!(reason, "keyboard-scrollback");
            assert!(redraw);
            recovery.extend_from_slice(payload.as_slice());
            let receipt = runtime.hdmi_last_payload_receipt;
            runtime.record_linked_hdmi_submit_success(receipt);
            frames = frames.saturating_add(1);
            assert!(frames <= 16);
        }
        assert_eq!(
            recovery
                .windows(b"Cohesix console ready".len())
                .filter(|window| *window == b"Cohesix console ready")
                .count(),
            1,
        );
        assert!(!runtime.linked_hdmi_pending_work());
    }

    #[test]
    fn runtime_hdmi_arrow_drains_sealed_live_tail_prefix_before_fast_csi_scroll() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 32,
            buffer_lines: 64,
        });
        runtime.root_console_ready = true;
        for index in 0..40 {
            let mut line = String::from("line-");
            let _ = core::fmt::Write::write_fmt(&mut line, format_args!("{index:02}"));
            runtime.mirror_line_current_tcb(line.as_str());
        }
        runtime.request_linked_hdmi_snapshot_redraw();
        while let Some((_, _, redraw)) = runtime.next_linked_hdmi_payload() {
            assert!(redraw);
            let receipt = runtime.hdmi_last_payload_receipt;
            runtime.record_linked_hdmi_submit_success(receipt);
        }

        runtime.mirror_linked_hdmi_line_owner_state("line-40");
        assert!(!runtime.hdmi_pending_bytes.is_empty());
        assert!(runtime.apply_linked_hdmi_scroll_delta(1));
        assert!(runtime.hdmi_scroll_output_fence_active);
        assert!(!runtime.hdmi_pending_redraw);

        runtime.mirror_linked_hdmi_line_owner_state("line-41");
        assert_eq!(
            runtime
                .hdmi_pending_bytes
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            b"line-40\n",
            "later canonical output must not extend the already-sealed FIFO prefix",
        );
        assert_eq!(runtime.hdmi_scrollback_offset, 2);
        assert!(runtime.hdmi_scroll_output_fence_active);

        let Some((prefix, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("the sealed live-tail prefix must issue before the scroll");
        };
        assert_eq!(reason, "queued-output");
        assert!(!redraw);
        assert_eq!(prefix.as_slice(), b"line-40\n");
        let receipt = runtime.hdmi_last_payload_receipt;
        runtime.record_linked_hdmi_submit_success(receipt);
        assert!(!runtime.hdmi_scroll_output_fence_active);
        assert_eq!(
            runtime.hdmi_rendered_scrollback_offset, 1,
            "the completed prefix must rebase behind output mirrored after the fence",
        );

        let Some((older, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("the exact completed tail must permit one fast CSI scroll");
        };
        assert_eq!(reason, "keyboard-scroll-step");
        assert!(!redraw);
        assert!(older.windows(4).any(|window| window == b"\x1b[1T"));
        assert!(older
            .windows(b"line-12".len())
            .any(|window| window == b"line-12"));
        let receipt = runtime.hdmi_last_payload_receipt;
        runtime.record_linked_hdmi_submit_success(receipt);
        assert_eq!(runtime.hdmi_rendered_scrollback_offset, 2);
        assert!(!runtime.hdmi_pending_redraw);
        assert!(runtime.hdmi_redraw_bytes.is_empty());

        assert!(runtime.apply_linked_hdmi_scroll_delta(-1));
        let Some((newer, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("the first down-scroll must reveal the fenced tail");
        };
        assert_eq!(reason, "keyboard-scroll-step");
        assert!(!redraw);
        assert!(newer.windows(4).any(|window| window == b"\x1b[1S"));
        assert!(newer
            .windows(b"line-40".len())
            .any(|window| window == b"line-40"));
        let receipt = runtime.hdmi_last_payload_receipt;
        runtime.record_linked_hdmi_submit_success(receipt);
        assert_eq!(runtime.hdmi_rendered_scrollback_offset, 1);

        assert!(runtime.apply_linked_hdmi_scroll_delta(-1));
        let Some((live, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("the second down-scroll must reveal later canonical output at live tail");
        };
        assert_eq!(reason, "keyboard-scroll-step");
        assert!(!redraw);
        assert!(live.windows(4).any(|window| window == b"\x1b[1S"));
        assert!(live
            .windows(b"line-41".len())
            .any(|window| window == b"line-41"));
        let receipt = runtime.hdmi_last_payload_receipt;
        runtime.record_linked_hdmi_submit_success(receipt);
        assert_eq!(runtime.hdmi_rendered_scrollback_offset, 0);
        assert!(!runtime.linked_hdmi_pending_work());
    }

    #[test]
    fn runtime_hdmi_scroll_output_fence_identity_mismatch_fails_to_one_snapshot() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 32,
            buffer_lines: 64,
        });
        runtime.root_console_ready = true;
        for index in 0..40 {
            let mut line = String::from("line-");
            let _ = core::fmt::Write::write_fmt(&mut line, format_args!("{index:02}"));
            runtime.mirror_line_current_tcb(line.as_str());
        }
        runtime.request_linked_hdmi_snapshot_redraw();
        while let Some((_, _, redraw)) = runtime.next_linked_hdmi_payload() {
            assert!(redraw);
            let receipt = runtime.hdmi_last_payload_receipt;
            runtime.record_linked_hdmi_submit_success(receipt);
        }

        runtime.mirror_linked_hdmi_line_owner_state("line-40");
        assert!(runtime.apply_linked_hdmi_scroll_delta(1));
        let Some((_, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected the sealed output prefix");
        };
        assert_eq!(reason, "queued-output");
        assert!(!redraw);
        let stale_receipt = runtime.hdmi_last_payload_receipt;
        runtime.hdmi_scroll_output_fence_tail_sequence = runtime
            .hdmi_scroll_output_fence_tail_sequence
            .saturating_add(1);
        let snapshot_generation = runtime.hdmi_snapshot_generation;

        runtime.record_linked_hdmi_submit_success(stale_receipt);

        assert!(!runtime.hdmi_rendered_viewport_valid);
        assert!(!runtime.hdmi_scroll_output_fence_active);
        assert!(runtime.hdmi_pending_redraw);
        assert_eq!(
            runtime.hdmi_snapshot_generation,
            snapshot_generation.wrapping_add(1),
            "one receipt mismatch requests exactly one canonical snapshot",
        );
        let mut frames = 0usize;
        while let Some((_, reason, redraw)) = runtime.next_linked_hdmi_payload() {
            assert_eq!(reason, "keyboard-scrollback");
            assert!(redraw);
            let receipt = runtime.hdmi_last_payload_receipt;
            runtime.record_linked_hdmi_submit_success(receipt);
            frames = frames.saturating_add(1);
            assert!(frames <= 16);
        }
        assert!(runtime.hdmi_rendered_viewport_valid);
        assert_eq!(runtime.hdmi_rendered_scrollback_offset, 1);
    }

    #[test]
    fn runtime_hdmi_scroll_receipt_redraws_only_for_evicted_anchor_or_generation_wrap() {
        let mut evicted = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 32,
            buffer_lines: 40,
        });
        for index in 0..40 {
            let mut line = String::from("line-");
            let _ = core::fmt::Write::write_fmt(&mut line, format_args!("{index:02}"));
            evicted.mirror_line_current_tcb(line.as_str());
        }
        evicted.request_linked_hdmi_snapshot_redraw();
        while let Some((_, _, redraw)) = evicted.next_linked_hdmi_payload() {
            assert!(redraw);
            let receipt = evicted.hdmi_last_payload_receipt;
            evicted.record_linked_hdmi_submit_success(receipt);
        }
        assert!(evicted.apply_linked_hdmi_scroll_delta(10));
        let Some((_, _, redraw)) = evicted.next_linked_hdmi_payload() else {
            panic!("expected scroll near the oldest retained anchor");
        };
        assert!(!redraw);
        let receipt = evicted.hdmi_last_payload_receipt;
        evicted.record_linked_hdmi_submit_success(receipt);
        assert_eq!(evicted.hdmi_rendered_scrollback_offset, 10);
        assert!(evicted.apply_linked_hdmi_scroll_delta(2));
        let Some((_, _, redraw)) = evicted.next_linked_hdmi_payload() else {
            panic!("expected retained scroll to the oldest anchor");
        };
        assert!(!redraw);
        let retained_receipt = evicted.hdmi_last_payload_receipt;
        evicted.mirror_line_current_tcb("line-40");
        assert_eq!(evicted.max_linked_hdmi_scrollback_offset(), 12);
        evicted.record_linked_hdmi_submit_success(retained_receipt);
        assert!(!evicted.hdmi_rendered_viewport_valid);
        assert!(evicted.hdmi_pending_redraw);

        let mut wrapped = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 32,
            buffer_lines: 64,
        });
        for index in 0..40 {
            let mut line = String::from("line-");
            let _ = core::fmt::Write::write_fmt(&mut line, format_args!("{index:02}"));
            wrapped.mirror_line_current_tcb(line.as_str());
        }
        wrapped.request_linked_hdmi_snapshot_redraw();
        while let Some((_, _, redraw)) = wrapped.next_linked_hdmi_payload() {
            assert!(redraw);
            let receipt = wrapped.hdmi_last_payload_receipt;
            wrapped.record_linked_hdmi_submit_success(receipt);
        }
        assert!(wrapped.apply_linked_hdmi_scroll_delta(6));
        let Some((_, _, redraw)) = wrapped.next_linked_hdmi_payload() else {
            panic!("expected initial older scroll");
        };
        assert!(!redraw);
        let receipt = wrapped.hdmi_last_payload_receipt;
        wrapped.record_linked_hdmi_submit_success(receipt);
        wrapped.mirrored_line_generation = u64::MAX;
        assert!(wrapped.apply_linked_hdmi_scroll_delta(-3));
        let Some((_, _, redraw)) = wrapped.next_linked_hdmi_payload() else {
            panic!("expected retained down-scroll frame");
        };
        assert!(!redraw);
        let retained_receipt = wrapped.hdmi_last_payload_receipt;
        wrapped.mirror_line_current_tcb("line-40");
        wrapped.record_linked_hdmi_submit_success(retained_receipt);
        assert!(!wrapped.hdmi_rendered_viewport_valid);
        assert!(wrapped.hdmi_pending_redraw);

        for runtime in [&mut evicted, &mut wrapped] {
            let mut redraw_frames = 0usize;
            while let Some((payload, reason, redraw)) = runtime.next_linked_hdmi_payload() {
                assert_eq!(reason, "keyboard-scrollback");
                assert!(redraw);
                assert!(payload.len() <= LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES);
                let receipt = runtime.hdmi_last_payload_receipt;
                runtime.record_linked_hdmi_submit_success(receipt);
                redraw_frames = redraw_frames.saturating_add(1);
                assert!(redraw_frames <= 16, "canonical recovery stays bounded");
            }
            assert!(runtime.hdmi_rendered_viewport_valid);
            assert_eq!(
                runtime.hdmi_rendered_scrollback_offset,
                runtime.hdmi_scrollback_offset,
            );
            assert!(!runtime.linked_hdmi_pending_work());
            assert!(!runtime.mirrored_line_generation_wrapped);
        }
    }

    #[test]
    fn runtime_hdmi_single_down_to_live_tail_preserves_input_and_output_without_redraw() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 64,
            buffer_lines: 40,
        });
        for index in 0..39 {
            let mut line = String::from("line-");
            let _ = core::fmt::Write::write_fmt(&mut line, format_args!("{index:02}"));
            runtime.mirror_line_current_tcb(line.as_str());
        }
        runtime.mirror_line_current_tcb("cohesix> ");
        runtime.open_linked_hdmi_prompt_line("cohesix> ");
        runtime.request_linked_hdmi_snapshot_redraw();
        while let Some((payload, reason, redraw)) = runtime.next_linked_hdmi_payload() {
            assert_eq!(reason, "keyboard-scrollback");
            assert!(redraw);
            assert!(payload.len() <= LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES);
            let receipt = runtime.hdmi_last_payload_receipt;
            runtime.record_linked_hdmi_submit_success(receipt);
        }

        assert!(runtime.apply_linked_hdmi_scroll_delta(1));
        let Some((older, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected one older scroll step");
        };
        assert_eq!(reason, "keyboard-scroll-step");
        assert!(!redraw);
        assert!(older.windows(4).any(|window| window == b"\x1b[1T"));
        let receipt = runtime.hdmi_last_payload_receipt;
        runtime.record_linked_hdmi_submit_success(receipt);
        assert_eq!(runtime.hdmi_rendered_scrollback_offset, 1);

        assert!(runtime.queue_linked_hdmi_line("queued while scrolled"));
        assert!(runtime.record_linked_hdmi_input_echo_byte(b'x'));
        runtime.queue_linked_hdmi_input_row_or_redraw();
        assert!(runtime.hdmi_input_row_dirty);
        assert!(!runtime.hdmi_pending_redraw);
        assert!(runtime.hdmi_redraw_bytes.is_empty());
        assert!(!runtime.hdmi_stale_after_retry_exhaustion);

        assert!(runtime.apply_linked_hdmi_scroll_delta(-1));
        let Some((newer, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected the single live-tail scroll step before queued work");
        };
        assert_eq!(reason, "keyboard-scroll-step");
        assert!(!redraw);
        assert!(newer.windows(4).any(|window| window == b"\x1b[1S"));
        assert!(newer.len() <= LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES);
        let receipt = runtime.hdmi_last_payload_receipt;
        runtime.record_linked_hdmi_submit_success(receipt);
        assert_eq!(runtime.hdmi_scrollback_offset, 0);
        assert_eq!(runtime.hdmi_rendered_scrollback_offset, 0);
        assert!(!runtime.hdmi_pending_redraw);
        assert!(runtime.hdmi_redraw_bytes.is_empty());

        let Some((input, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected the retained input row at live tail");
        };
        assert_eq!(reason, "keyboard-input-row");
        assert!(!redraw);
        assert_eq!(input.as_slice(), b"\r\x1b[2Kcohesix> x\x1b[K");
        let receipt = runtime.hdmi_last_payload_receipt;
        runtime.record_linked_hdmi_submit_success(receipt);

        let Some((output, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("expected output queued during scrollback");
        };
        assert_eq!(reason, "queued-output");
        assert!(!redraw);
        assert_eq!(output.as_slice(), b"queued while scrolled\n");
        let receipt = runtime.hdmi_last_payload_receipt;
        runtime.record_linked_hdmi_submit_success(receipt);

        assert!(!runtime.linked_hdmi_pending_work());
        assert!(!runtime.hdmi_pending_redraw);
        assert!(runtime.hdmi_redraw_bytes.is_empty());
        assert!(!runtime.hdmi_stale_after_retry_exhaustion);
    }

    #[test]
    fn runtime_hdmi_long_line_scroll_spans_reach_both_bounds_symmetrically() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 256,
            buffer_lines: 128,
        });
        let mut long_line = String::new();
        for _ in 0..256 {
            long_line.push('x');
        }
        for _ in 0..128 {
            runtime.mirror_line_current_tcb(long_line.as_str());
        }
        runtime.request_linked_hdmi_snapshot_redraw();
        while let Some((payload, reason, redraw)) = runtime.next_linked_hdmi_payload() {
            assert_eq!(reason, "keyboard-scrollback");
            assert!(redraw);
            assert!(payload.len() <= LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES);
            let receipt = runtime.hdmi_last_payload_receipt;
            runtime.record_linked_hdmi_submit_success(receipt);
        }

        let max_offset = runtime.max_linked_hdmi_scrollback_offset();
        assert!(max_offset > 1);
        assert!(runtime.apply_linked_hdmi_scroll_delta(i32::MAX));
        let mut older_frames = 0usize;
        while runtime.hdmi_rendered_scrollback_offset != runtime.hdmi_scrollback_offset {
            let previous = runtime.hdmi_rendered_scrollback_offset;
            let Some((payload, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
                panic!("bounded older scroll must keep making progress");
            };
            assert_eq!(reason, "keyboard-scroll-step");
            assert!(!redraw);
            assert!(payload.len() <= LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES);
            assert!(payload.iter().take(12).any(|byte| *byte == b'T'));
            let receipt = runtime.hdmi_last_payload_receipt;
            runtime.record_linked_hdmi_submit_success(receipt);
            assert!(runtime.hdmi_rendered_scrollback_offset > previous);
            assert!(runtime.hdmi_rendered_scrollback_offset <= max_offset);
            older_frames = older_frames.saturating_add(1);
            assert!(older_frames <= max_offset);
        }
        assert_eq!(runtime.hdmi_rendered_scrollback_offset, max_offset);
        assert!(older_frames < max_offset, "repeats must coalesce");

        assert!(runtime.apply_linked_hdmi_scroll_delta(i32::MIN));
        let mut newer_frames = 0usize;
        while runtime.hdmi_rendered_scrollback_offset != 0 {
            let previous = runtime.hdmi_rendered_scrollback_offset;
            let Some((payload, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
                panic!("bounded live-tail scroll must keep making progress");
            };
            assert_eq!(reason, "keyboard-scroll-step");
            assert!(!redraw);
            assert!(payload.len() <= LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES);
            assert!(payload.iter().take(12).any(|byte| *byte == b'S'));
            let receipt = runtime.hdmi_last_payload_receipt;
            runtime.record_linked_hdmi_submit_success(receipt);
            assert!(runtime.hdmi_rendered_scrollback_offset < previous);
            newer_frames = newer_frames.saturating_add(1);
            assert!(newer_frames <= max_offset);
        }
        assert_eq!(runtime.hdmi_scrollback_offset, 0);
        assert_eq!(older_frames, newer_frames);
        assert!(!runtime.linked_hdmi_pending_work());
    }

    #[test]
    fn runtime_hdmi_pending_status_preserves_enter_before_response() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 96,
            buffer_lines: 8,
        });
        runtime.mirror_line_current_tcb("cohesix> ");
        runtime.open_linked_hdmi_prompt_line("cohesix> ");
        assert!(runtime.queue_linked_hdmi_line("prior output"));
        assert!(
            runtime.mirror_linked_hdmi_high_impact_above_open_line("[drivers] WiFi attempt active")
        );
        for &byte in b"netstats" {
            runtime.record_linked_hdmi_input_echo_byte(byte);
        }
        runtime.record_linked_hdmi_input_echo_byte(b'\n');
        runtime.queue_linked_hdmi_input_row_or_redraw();
        runtime.mirror_line_current_tcb("ACK netstats");
        assert!(runtime.queue_linked_hdmi_line("ACK netstats"));

        let mut payload = Vec::new();
        while let Some((chunk, reason, redraw)) = runtime.next_linked_hdmi_payload() {
            assert!(matches!(reason, "queued-output" | "keyboard-input-row"));
            assert!(!redraw);
            let receipt = runtime.hdmi_last_payload_receipt;
            payload.extend_from_slice(chunk.as_slice());
            runtime.record_linked_hdmi_submit_success(receipt);
        }
        let command = b"\r\x1b[2Kcohesix> netstats\x1b[K\n";
        let command_index = payload
            .windows(command.len())
            .position(|window| window == command)
            .expect("closed command row retains its newline");
        let prior_index = payload
            .windows(b"prior output\n".len())
            .position(|window| window == b"prior output\n")
            .expect("older FIFO output remains ahead of the priority status");
        let status_index = payload
            .windows(b"[drivers] WiFi attempt active".len())
            .position(|window| window == b"[drivers] WiFi attempt active")
            .expect("priority status remains contiguous ahead of the command row");
        let response_index = payload
            .windows(b"ACK netstats\n".len())
            .position(|window| window == b"ACK netstats\n")
            .expect("response remains on the following row");
        assert!(prior_index < status_index);
        assert!(status_index < command_index);
        assert!(command_index < response_index);
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
        assert_eq!(display.redraw_bytes, 0);
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
            LINKED_LOCAL_SEAT_USB_POLL_READY_IDLE_COOLDOWN
        );
        #[cfg(all(feature = "kernel", feature = "usb"))]
        {
            assert!(local_seat_keyboard_active_queue_idle(
                LINKED_LOCAL_SEAT_USB_READY_MAX_QUEUED_REPORTS,
                u32::from(DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_NONE)
            ));
            assert!(local_seat_keyboard_active_queue_idle(
                LINKED_LOCAL_SEAT_USB_READY_MAX_QUEUED_REPORTS,
                u32::from(DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_IDLE)
            ));
            assert!(local_seat_keyboard_active_queue_idle(
                LINKED_LOCAL_SEAT_USB_READY_MAX_QUEUED_REPORTS,
                u32::from(DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_DECODED_EMPTY)
            ));
            assert!(!local_seat_keyboard_active_queue_idle(
                LINKED_LOCAL_SEAT_USB_READY_MAX_QUEUED_REPORTS,
                u32::from(pi4_driver_abi::DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_PRODUCED_BYTE)
            ));
            let pre_first_overqueue_no_event = LINKED_LOCAL_SEAT_USB_READY_MAX_QUEUED_REPORTS + 1
                | (u32::from(DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_NONE)
                    << DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_SHIFT);
            assert!(local_seat_keyboard_pre_first_report_full_queue_stalled(
                pre_first_overqueue_no_event
            ));
            assert!(local_seat_keyboard_pre_first_report_no_event_queue_stalled(
                pre_first_overqueue_no_event
            ));
            assert!(!local_seat_keyboard_pre_first_report_full_queue_stalled(
                pre_first_overqueue_no_event | (1 << 8)
            ));
            assert!(!local_seat_keyboard_pre_first_report_full_queue_stalled(
                pre_first_overqueue_no_event | (1 << 24)
            ));
            let pre_first_recovered_no_event = (LINKED_LOCAL_SEAT_USB_READY_MAX_QUEUED_REPORTS + 1)
                | (u32::from(DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_RECOVERY_SUCCESS)
                    << DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_SHIFT);
            assert!(local_seat_keyboard_pre_first_report_full_queue_stalled(
                pre_first_recovered_no_event
            ));
            let pre_first_single_no_event = 1
                | (u32::from(DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_NONE)
                    << DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_SHIFT);
            assert!(!local_seat_keyboard_pre_first_report_full_queue_stalled(
                pre_first_single_no_event
            ));
            assert!(
                !local_seat_keyboard_pre_first_report_no_event_queue_stalled(
                    pre_first_single_no_event
                )
            );
            assert!(
                !local_seat_keyboard_pre_first_report_steady_full_queue_stalled(
                    pre_first_single_no_event
                )
            );
        }
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    #[test]
    fn retained_usb_pending_preserves_ticket_and_does_not_create_no_reply_debt() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 4,
        });
        let command = crate::hal::driver_task::DriverTaskCommandRecord::pi4_hot_path(
            0,
            crate::hal::driver_task::DriverTaskHotPath::UsbKeyboard,
            crate::hal::driver_task::DriverTaskBudgetGrant::from_contract(driver_task_contract()),
            crate::hal::driver_task::DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        );
        runtime.linked_usb_poll_command = Some(command);

        assert_eq!(
            runtime.resolve_linked_usb_poll_retained_turn(
                crate::hal::driver_task::DriverTaskRetainedServiceTurn::Pending,
            ),
            Ok(None),
        );
        assert_eq!(runtime.linked_usb_poll_command, Some(command));
        assert_eq!(runtime.keyboard_trace().driver_task_no_replies, 0);
        assert_eq!(runtime.keyboard_trace().driver_task_no_reply_streak, 0);
        assert_eq!(runtime.keyboard_trace().poll_cooldown_turns, 0);

        let completion = crate::hal::driver_task::DriverTaskCompletionRecord::progress(1, 0);
        assert_eq!(
            runtime.resolve_linked_usb_poll_retained_turn(
                crate::hal::driver_task::DriverTaskRetainedServiceTurn::Complete(completion),
            ),
            Ok(Some(completion)),
        );
        assert!(runtime.linked_usb_poll_command.is_none());
        assert_eq!(runtime.keyboard_trace().driver_task_no_replies, 0);

        runtime.linked_usb_poll_command = Some(command);
        assert_eq!(
            runtime.resolve_linked_usb_poll_retained_turn(
                crate::hal::driver_task::DriverTaskRetainedServiceTurn::Failed,
            ),
            Err("usb-retained-service-failed"),
        );
        assert!(runtime.linked_usb_poll_command.is_none());
        assert_eq!(runtime.keyboard_trace().driver_task_no_replies, 1);
        assert_eq!(runtime.keyboard_trace().driver_task_no_reply_streak, 1);
    }

    #[test]
    fn runtime_keyboard_idle_completion_does_not_start_busy_cooldown() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 4,
        });

        runtime.record_keyboard_poll_no_reply();
        assert!(runtime.keyboard_trace().poll_cooldown_turns > 0);
        runtime.record_keyboard_poll_idle_completion();

        let trace = runtime.keyboard_trace();
        assert_eq!(trace.driver_task_no_reply_streak, 0);
        assert_eq!(trace.poll_cooldown_turns, 0);
        assert_eq!(trace.poll_cooldown_skips, 0);
        assert_eq!(local_seat_keyboard_poll_idle_completion_cooldown(false), 0);
        assert_eq!(local_seat_keyboard_poll_idle_completion_cooldown(true), 0);
    }

    #[test]
    fn keyboard_cooldown_busy_message_is_reserved_for_post_ready_pressure() {
        assert!(!local_seat_keyboard_cooldown_should_show_busy(
            true, 0, false
        ));
        assert!(!local_seat_keyboard_cooldown_should_show_busy(
            false, 0, false
        ));
        assert!(!local_seat_keyboard_cooldown_should_show_busy(
            false, 1, false
        ));
        assert!(!local_seat_keyboard_cooldown_should_show_busy(
            false,
            LINKED_LOCAL_SEAT_USB_VISIBLE_BUSY_NO_REPLY_THRESHOLD,
            false
        ));
        assert!(!local_seat_keyboard_cooldown_should_show_busy(
            true, 1, false
        ));
        assert!(!local_seat_keyboard_cooldown_should_show_busy(
            true, 2, false
        ));
        assert!(local_seat_keyboard_cooldown_should_show_busy(
            true,
            LINKED_LOCAL_SEAT_USB_POST_FIRST_BYTE_IDLE_RECOVERY_NO_REPLY_THRESHOLD,
            false
        ));
        assert!(local_seat_keyboard_cooldown_should_show_busy(true, 0, true));
    }

    #[test]
    fn keyboard_busy_line_waits_for_material_no_reply_streak() {
        assert_eq!(
            LINKED_LOCAL_SEAT_USB_VISIBLE_BUSY_NO_REPLY_THRESHOLD,
            LINKED_LOCAL_SEAT_USB_POST_FIRST_BYTE_IDLE_RECOVERY_NO_REPLY_THRESHOLD
        );
        assert!(!local_seat_keyboard_cooldown_should_show_busy(
            false,
            LINKED_LOCAL_SEAT_USB_VISIBLE_BUSY_NO_REPLY_THRESHOLD - 1,
            false
        ));
        assert!(!local_seat_keyboard_cooldown_should_show_busy(
            false,
            LINKED_LOCAL_SEAT_USB_VISIBLE_BUSY_NO_REPLY_THRESHOLD,
            false
        ));
        assert!(local_seat_keyboard_cooldown_should_show_busy(
            true,
            LINKED_LOCAL_SEAT_USB_VISIBLE_BUSY_NO_REPLY_THRESHOLD,
            false
        ));
    }

    #[test]
    fn keyboard_busy_line_policy_is_never_visible_before_command_ready() {
        assert!(!local_seat_hdmi_keyboard_busy_line_warranted_state(
            false, true, false, 0, false
        ));
        assert!(!local_seat_hdmi_keyboard_busy_line_warranted_state(
            false,
            true,
            true,
            LINKED_LOCAL_SEAT_USB_VISIBLE_BUSY_NO_REPLY_THRESHOLD,
            false
        ));
        assert!(!local_seat_hdmi_keyboard_busy_line_warranted_state(
            true, false, false, 0, false
        ));
        assert!(local_seat_hdmi_keyboard_busy_line_warranted_state(
            true, true, false, 0, false
        ));
    }

    #[test]
    fn keyboard_prompt_recovery_does_not_revoke_command_ready_on_no_reply() {
        let threshold = LINKED_LOCAL_SEAT_USB_POST_FIRST_BYTE_IDLE_RECOVERY_NO_REPLY_THRESHOLD;

        assert!(local_seat_keyboard_prompt_recovery_pending(
            false, false, threshold, threshold
        ));
        assert!(!local_seat_keyboard_prompt_recovery_pending(
            true, false, threshold, threshold
        ));
        assert!(local_seat_keyboard_prompt_recovery_pending(
            true, true, 0, threshold
        ));
    }

    #[test]
    fn runtime_keyboard_recovery_aux_waits_for_root_queue_drain_unless_runtime_is_stale() {
        const {
            assert!(LINKED_LOCAL_SEAT_USB_POST_FIRST_BYTE_RECOVERY_NO_REPLY_THRESHOLD >= 32);
            assert!(LINKED_LOCAL_SEAT_USB_POST_FIRST_BYTE_RECOVERY_NO_REPLY_THRESHOLD <= 64);
            assert!(
                LINKED_LOCAL_SEAT_USB_POST_FIRST_BYTE_RECOVERY_NO_REPLY_THRESHOLD
                    >= LINKED_LOCAL_SEAT_USB_POST_FIRST_BYTE_IDLE_RECOVERY_NO_REPLY_THRESHOLD * 4
            );
        }
        assert!(!local_seat_keyboard_recovery_aux_allowed(
            false,
            LINKED_LOCAL_SEAT_USB_POST_FIRST_BYTE_RECOVERY_NO_REPLY_THRESHOLD,
            false
        ));
        assert!(!local_seat_keyboard_recovery_aux_allowed(
            true,
            LINKED_LOCAL_SEAT_USB_POST_FIRST_BYTE_RECOVERY_NO_REPLY_THRESHOLD - 1,
            false
        ));
        assert!(local_seat_keyboard_recovery_aux_allowed(
            true,
            LINKED_LOCAL_SEAT_USB_POST_FIRST_BYTE_RECOVERY_NO_REPLY_THRESHOLD,
            false
        ));
        assert!(!local_seat_keyboard_recovery_aux_allowed(
            true,
            LINKED_LOCAL_SEAT_USB_POST_FIRST_BYTE_RECOVERY_NO_REPLY_THRESHOLD + 1,
            false
        ));
        assert!(local_seat_keyboard_recovery_aux_allowed(
            true,
            LINKED_LOCAL_SEAT_USB_POST_FIRST_BYTE_RECOVERY_NO_REPLY_THRESHOLD * 2,
            false
        ));
        assert!(!local_seat_keyboard_recovery_aux_allowed(
            true,
            LINKED_LOCAL_SEAT_USB_POST_FIRST_BYTE_RECOVERY_NO_REPLY_THRESHOLD,
            true
        ));
        assert!(local_seat_keyboard_recovery_aux_allowed_for_status(
            true, 0, false, true, false, false, false
        ));
        assert!(local_seat_keyboard_recovery_aux_allowed_for_status(
            false, 0, false, true, false, false, false
        ));
        assert!(!local_seat_keyboard_recovery_aux_allowed_for_status(
            true, 0, true, true, false, false, false
        ));
        assert!(local_seat_keyboard_recovery_aux_allowed_for_status(
            true, 0, false, false, true, false, false
        ));
        assert!(local_seat_keyboard_recovery_aux_allowed_for_status(
            false, 0, false, false, true, false, false
        ));
        assert!(local_seat_keyboard_recovery_aux_allowed_for_status(
            true, 0, false, false, false, true, false
        ));
        assert!(!local_seat_keyboard_recovery_aux_allowed_for_status(
            false, 0, false, false, false, true, false
        ));
        assert!(!local_seat_keyboard_recovery_aux_allowed_for_status(
            true, 0, true, false, false, true, false
        ));
        assert!(!local_seat_keyboard_recovery_aux_allowed_for_status(
            true, 0, false, false, false, false, true
        ));
        assert!(!local_seat_keyboard_recovery_aux_allowed_for_status(
            false, 0, false, false, false, false, true
        ));
        assert!(!local_seat_keyboard_recovery_aux_allowed_for_status(
            true,
            LINKED_LOCAL_SEAT_USB_POST_FIRST_BYTE_IDLE_RECOVERY_NO_REPLY_THRESHOLD - 1,
            false,
            false,
            false,
            false,
            true
        ));
        assert!(local_seat_keyboard_recovery_aux_allowed_for_status(
            true,
            LINKED_LOCAL_SEAT_USB_POST_FIRST_BYTE_IDLE_RECOVERY_NO_REPLY_THRESHOLD,
            false,
            false,
            false,
            false,
            true
        ));
        assert!(local_seat_keyboard_recovery_aux_allowed_for_status(
            true,
            LINKED_LOCAL_SEAT_USB_POST_FIRST_BYTE_IDLE_RECOVERY_NO_REPLY_THRESHOLD * 2,
            false,
            false,
            false,
            false,
            true
        ));
        assert!(local_seat_keyboard_recovery_aux_keyboard_gate(
            false, false, true
        ));
        assert!(!local_seat_keyboard_recovery_aux_keyboard_gate(
            false, true, false
        ));
        assert!(!local_seat_keyboard_recovery_aux_keyboard_gate(
            true, false, false
        ));
        assert!(local_seat_keyboard_recovery_aux_keyboard_gate(
            true, true, false
        ));
    }

    #[test]
    fn runtime_keyboard_recovery_aux_latch_clears_after_no_reply_window() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 4,
        });
        runtime.keyboard_recovery_aux_pending = true;
        runtime.keyboard_poll_no_reply_streak =
            LINKED_LOCAL_SEAT_USB_POST_FIRST_BYTE_RECOVERY_NO_REPLY_THRESHOLD;

        runtime.record_keyboard_poll_no_reply();
        assert!(runtime.keyboard_recovery_aux_pending);

        runtime.keyboard_poll_no_reply_streak =
            LINKED_LOCAL_SEAT_USB_POST_FIRST_BYTE_RECOVERY_NO_REPLY_THRESHOLD * 2 - 1;
        runtime.record_keyboard_poll_no_reply();
        assert!(!runtime.keyboard_recovery_aux_pending);
        assert!(local_seat_keyboard_recovery_aux_allowed(
            true,
            runtime.keyboard_poll_no_reply_streak,
            runtime.keyboard_recovery_aux_pending
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
    fn repeated_no_reply_detail_logging_keeps_detail_rows_sparse() {
        assert!(repeated_no_reply_detail_log_visible(0));
        assert!(!repeated_no_reply_detail_log_visible(1));
        assert!(!repeated_no_reply_detail_log_visible(4));
        assert!(!repeated_no_reply_detail_log_visible(32));
        assert!(repeated_no_reply_detail_log_visible(64));
        assert!(repeated_no_reply_detail_log_visible(128));
        assert!(!repeated_no_reply_detail_log_visible(129));
    }

    #[test]
    fn usb_keyboard_recovery_request_logging_samples_no_reply_spam() {
        assert!(keyboard_recovery_request_log_visible(0, "no-reply"));
        assert!(keyboard_recovery_request_log_visible(1, "no-reply"));
        assert!(!keyboard_recovery_request_log_visible(2, "no-reply"));
        assert!(!keyboard_recovery_request_log_visible(8, "no-reply"));
        assert!(keyboard_recovery_request_log_visible(1, "submit"));
        assert!(keyboard_recovery_request_log_visible(2, "submit"));
        assert!(!keyboard_recovery_request_log_visible(8, "submit"));
        assert!(keyboard_recovery_request_log_visible(256, "submit"));
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
    fn physical_pi_keyboard_poll_keeps_owner_state_retry_after_missing_reply() {
        assert!(!local_seat_keyboard_poll_suspends_on_missing_reply(
            true, false, false
        ));
        assert!(!local_seat_keyboard_poll_suspends_on_missing_reply(
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
    fn physical_pi_hdmi_prompt_visibility_requires_usb_command_readiness() {
        assert!(!local_seat_hdmi_prompt_ready_for_root_state(true, false));
        assert!(local_seat_hdmi_prompt_ready_for_root_state(true, true));
        assert!(local_seat_hdmi_prompt_ready_for_root_state(false, false));
        assert!(!local_seat_hdmi_prompt_release_ready(false, true));
        assert!(!local_seat_hdmi_prompt_release_ready(true, false));
        assert!(local_seat_hdmi_prompt_release_ready(true, true));
        assert!(local_seat_hdmi_keyboard_starting_line_due(
            false, false, false,
        ));
        assert!(
            !local_seat_keyboard_bytes_enter_parser_state(true, true, false),
            "visible HDMI feedback cannot manufacture USB parser authority",
        );
        assert!(local_seat_keyboard_bytes_enter_parser_state(
            true, true, true,
        ));
        assert!(
            !local_seat_hdmi_console_ready_line_due(true, false, false),
            "the independent prompt cannot manufacture USB command readiness",
        );
    }

    #[test]
    fn physical_pi_hdmi_prompt_waits_for_display_retry_health_not_empty_queue() {
        assert!(local_seat_hdmi_prompt_ready_for_display_state(
            true, true, true, true, false, true
        ));
        assert!(!local_seat_hdmi_prompt_ready_for_display_state(
            true, true, true, true, true, false
        ));
        assert!(local_seat_hdmi_prompt_ready_for_display_state(
            true, true, true, true, true, true
        ));
        assert!(!local_seat_hdmi_prompt_ready_for_display_state(
            true, false, true, true, true, true
        ));
        assert!(!local_seat_hdmi_prompt_ready_for_display_state(
            true, true, false, true, true, true
        ));
        assert!(local_seat_hdmi_prompt_ready_for_display_state(
            false, false, false, false, false, false
        ));
    }

    #[test]
    fn physical_pi_hdmi_prompt_waits_for_canonical_ready_banner_after_usb_admission() {
        assert!(local_seat_hdmi_console_ready_line_due(true, true, false,));
        assert!(!local_seat_hdmi_prompt_ready_for_display_state(
            true, true, true, false, true, true,
        ));
        assert!(local_seat_hdmi_prompt_ready_for_display_state(
            true, true, true, true, true, true,
        ));
        assert!(local_seat_hdmi_prompt_ready_for_display_state(
            false, false, false, false, false, false,
        ));
    }

    #[test]
    fn physical_pi_hdmi_starting_line_does_not_request_arming_key() {
        assert!(local_seat_hdmi_keyboard_starting_line_due(
            false, false, false
        ));
        assert!(!local_seat_hdmi_keyboard_starting_line_due(
            false, false, true
        ));
        assert!(!local_seat_hdmi_keyboard_starting_line_due(
            true, false, false
        ));
        assert!(!local_seat_hdmi_keyboard_starting_line_due(
            false, true, false
        ));
        assert!(!local_seat_hdmi_keyboard_wait_line_due(false, false, false));
        assert!(!local_seat_hdmi_keyboard_wait_line_due(true, false, false));
        assert!(!local_seat_hdmi_keyboard_wait_line_due(true, false, true));
        assert!(!local_seat_hdmi_keyboard_wait_line_due(true, true, false));
        assert!(!local_seat_hdmi_keyboard_ready_line_supersedes_busy(
            true, true, false
        ));
        assert!(!local_seat_hdmi_keyboard_ready_line_supersedes_busy(
            true, true, true
        ));
        assert!(!local_seat_hdmi_keyboard_ready_line_supersedes_busy(
            true, false, false
        ));
        assert!(!local_seat_hdmi_keyboard_ready_line_supersedes_busy(
            false, true, false
        ));
    }

    #[test]
    fn physical_pi_hdmi_ready_is_independent_of_wifi_and_waits_for_exact_completion() {
        assert!(!local_seat_hdmi_console_ready_line_due(false, true, false));
        assert!(!local_seat_hdmi_console_ready_line_due(true, false, false));
        assert!(local_seat_hdmi_console_ready_line_due(true, true, false));
        assert!(!local_seat_hdmi_console_ready_line_due(true, true, true));

        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 64,
            buffer_lines: 8,
        });
        runtime.mark_root_console_ready();
        runtime.defer_hdmi_console_ready_until_cyw43_terminal();
        runtime.emit_linked_hdmi_console_ready_if_due(true);
        assert!(!runtime.hdmi_bootstrap_terminal_ready());
        assert!(runtime.hdmi_console_ready_line_emitted());
        assert!(!runtime.hdmi_console_ready_line_completed());
        assert!(runtime
            .mirrored_lines_snapshot()
            .iter()
            .any(|line| line == "Cohesix console ready"));

        let mut frames = 0usize;
        while let Some((_, _, _)) = runtime.next_linked_hdmi_payload() {
            let receipt = runtime.hdmi_last_payload_receipt;
            runtime.record_linked_hdmi_submit_success(receipt);
            frames = frames.saturating_add(1);
            assert!(frames <= 16);
        }
        assert!(runtime.hdmi_console_ready_line_completed());
        runtime.release_linked_hdmi_console_for_test("cohesix> ");
        assert_eq!(
            runtime
                .mirrored_lines_snapshot()
                .iter()
                .filter(|line| line.as_str() == "Cohesix console ready")
                .count(),
            1
        );
        assert_eq!(
            runtime.mirrored_lines_snapshot().last().map(String::as_str),
            Some("cohesix> "),
        );
        assert!(!runtime.hdmi_bootstrap_terminal_ready());

        runtime.mark_cyw43_bootstrap_terminal_for_hdmi();
        runtime.emit_linked_hdmi_console_ready_if_due(true);
        assert!(runtime.hdmi_bootstrap_terminal_ready());
        assert_eq!(
            runtime
                .mirrored_lines_snapshot()
                .iter()
                .filter(|line| line.as_str() == "Cohesix console ready")
                .count(),
            1,
        );
    }

    #[test]
    fn runtime_hdmi_ready_completion_tracks_fifo_prefix_across_interleaved_output() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 96,
            buffer_lines: 64,
        });
        runtime.root_console_ready = true;
        runtime.hdmi_rendered_viewport_valid = true;

        let prior = "p".repeat(80);
        for _ in 0..7 {
            runtime.mirror_linked_hdmi_line_owner_state(prior.as_str());
        }
        runtime.emit_linked_hdmi_console_ready_if_due(true);
        let ready_prefix_bytes = runtime.hdmi_console_ready_pending_bytes;
        assert!(ready_prefix_bytes > LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES);

        let later = "l".repeat(80);
        for _ in 0..8 {
            runtime.mirror_linked_hdmi_line_owner_state(later.as_str());
        }
        let total_bytes = runtime.hdmi_pending_bytes.len();
        assert!(total_bytes > LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES.saturating_mul(2));

        let Some((first, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("the first ready-banner prefix frame must be retained");
        };
        assert_eq!(reason, "queued-output");
        assert!(!redraw);
        assert_eq!(first.len(), LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES);
        let receipt = runtime.hdmi_last_payload_receipt;
        runtime.record_linked_hdmi_submit_success(receipt);
        assert!(!runtime.hdmi_console_ready_line_completed());

        let Some((second, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("the completing ready-banner prefix frame must be retained");
        };
        assert_eq!(reason, "queued-output");
        assert!(!redraw);
        assert_eq!(second.len(), LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES);
        let receipt = runtime.hdmi_last_payload_receipt;
        runtime.record_linked_hdmi_submit_success(receipt);

        assert!(runtime.hdmi_console_ready_line_completed());
        assert_eq!(
            runtime.hdmi_pending_bytes.len(),
            total_bytes.saturating_sub(LINKED_LOCAL_SEAT_HDMI_FRAME_CHUNK_BYTES * 2),
        );
        assert!(
            !runtime.hdmi_pending_bytes.is_empty(),
            "later output remains ordered behind the completed ready-banner prefix",
        );
    }

    #[test]
    fn terminal_ready_promotes_promptless_partial_input_onto_prompt_row() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 64,
            buffer_lines: 8,
        });
        runtime.mark_root_console_ready();
        runtime.defer_hdmi_console_ready_until_cyw43_terminal();
        for &byte in b"hi" {
            runtime.record_linked_hdmi_input_echo_byte(byte);
        }
        runtime.queue_linked_hdmi_input_row_or_redraw();
        assert_eq!(
            runtime.mirrored_lines_snapshot().last().map(String::as_str),
            Some("hi")
        );

        runtime.mark_cyw43_bootstrap_terminal_for_hdmi();
        runtime.emit_linked_hdmi_console_ready_if_due(true);
        runtime.mirror_linked_hdmi_prompt_now("cohesix> ");

        let lines = runtime.mirrored_lines_snapshot();
        assert!(lines.iter().any(|line| line == "Cohesix console ready"));
        assert_eq!(lines.last().map(String::as_str), Some("cohesix> hi"));
        assert_eq!(runtime.hdmi_open_line_floor_bytes, "cohesix> ".len());
        assert!(runtime.hdmi_open_line_mirrors_input);
        assert!(runtime.linked_hdmi_pending_work());
    }

    #[test]
    fn cyw43_retraction_supersedes_ready_while_preserving_prompt_in_stabilizing_snapshot() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 96,
            buffer_lines: 16,
        });
        let wifi_ready = CYW43_GATE8_READY_HDMI_LINE;
        let wifi_ready_to_use =
            "[drivers] WiFi ready to use: DHCP bound at 192.168.86.154; TCP console listening on port 31337";
        runtime.hdmi_bootstrap_terminal_ready = true;
        runtime.hdmi_console_ready_line_emitted = true;
        runtime.hdmi_console_ready_line_completed = true;
        for line in [
            wifi_ready,
            wifi_ready_to_use,
            "Cohesix console ready",
            "cohesix> ",
        ] {
            runtime.mirror_line_current_tcb(line);
            assert!(runtime.queue_linked_hdmi_line(line));
        }
        runtime.open_linked_hdmi_prompt_line("cohesix> ");
        runtime.hdmi_pending_redraw = true;
        runtime.hdmi_redraw_bytes.extend(b"stale redraw bytes");
        let queued_stale_bytes = runtime.display_trace().pending_bytes;
        assert!(queued_stale_bytes != 0);
        assert!(runtime.display_trace().redraw_bytes != 0);

        runtime.defer_hdmi_console_ready_until_cyw43_terminal();

        let trace = runtime.display_trace();
        assert_eq!(trace.pending_bytes, 0);
        assert!(trace.pending_redraw);
        assert!(trace.superseded_bytes >= queued_stale_bytes as u64);
        assert!(!runtime
            .mirrored_lines_snapshot()
            .iter()
            .any(|line| line.as_str() == wifi_ready_to_use));
        assert!(!runtime.hdmi_bootstrap_terminal_ready());
        assert!(runtime.hdmi_console_ready_line_emitted());
        assert!(runtime.hdmi_console_ready_line_completed());
        assert!(!runtime
            .mirrored_lines_snapshot()
            .iter()
            .any(|line| local_seat_cyw43_stale_ready_line(line.as_str())));
        assert_eq!(
            runtime
                .mirrored_lines_snapshot()
                .iter()
                .filter(|line| line.as_str() == "cohesix> ")
                .count(),
            1,
            "WiFi readiness retraction must preserve one independent shell prompt",
        );
        assert!(runtime
            .mirrored_lines_snapshot()
            .iter()
            .any(|line| line == "[drivers] WiFi stabilizing; readiness retracted"));

        let mut snapshot = Vec::new();
        while let Some((chunk, _, redraw)) = runtime.next_linked_hdmi_payload() {
            assert!(redraw);
            snapshot.extend_from_slice(chunk.as_slice());
        }
        assert!(snapshot
            .windows(b"WiFi stabilizing; readiness retracted".len())
            .any(|window| window == b"WiFi stabilizing; readiness retracted"));
        for stale in [b"WiFi Gate 8 stable".as_slice()] {
            assert!(
                !snapshot.windows(stale.len()).any(|window| window == stale),
                "stale readiness bytes survived redraw: {:?}",
                core::str::from_utf8(stale).unwrap_or("non-utf8"),
            );
        }
        assert!(snapshot
            .windows(b"Cohesix console ready".len())
            .any(|window| window == b"Cohesix console ready"));
        assert!(snapshot
            .windows(b"cohesix> ".len())
            .any(|window| window == b"cohesix> "));
    }

    #[test]
    fn physical_pi_hdmi_initial_snapshot_is_scheduled_once_per_runtime() {
        assert!(!local_seat_hdmi_initial_snapshot_due(false, false));
        assert!(local_seat_hdmi_initial_snapshot_due(true, false));
        assert!(!local_seat_hdmi_initial_snapshot_due(true, true));

        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 96,
            buffer_lines: 16,
        });
        runtime.mirror_line_current_tcb("Cohesix startup in progress; serial console available");
        assert!(
            runtime.queue_linked_hdmi_line("Cohesix startup in progress; serial console available")
        );
        runtime.mirror_line_current_tcb(
            "[drivers] WiFi bootstrap starting (single production attempt)",
        );
        assert!(runtime.queue_linked_hdmi_line(
            "[drivers] WiFi bootstrap starting (single production attempt)"
        ));
        let queued_before_attach = runtime.display_trace().pending_bytes;
        assert!(queued_before_attach > 0);

        assert!(runtime.schedule_linked_hdmi_initial_snapshot_if_due(true));
        let scheduled = runtime.display_trace();
        assert!(scheduled.pending_redraw);
        assert_eq!(scheduled.pending_bytes, 0);
        assert_eq!(scheduled.superseded_bytes, queued_before_attach as u64);
        assert!(!runtime.schedule_linked_hdmi_initial_snapshot_if_due(true));

        let generation = scheduled.snapshot_generation;
        let mut snapshot = Vec::new();
        while let Some((chunk, reason, redraw)) = runtime.next_linked_hdmi_payload() {
            assert_eq!(reason, "keyboard-scrollback");
            assert!(redraw);
            let receipt = runtime.hdmi_last_payload_receipt;
            snapshot.extend_from_slice(chunk.as_slice());
            runtime.record_linked_hdmi_submit_success(receipt);
        }
        assert_no_blink_snapshot_payload(snapshot.as_slice());
        assert!(snapshot
            .windows(b"WiFi bootstrap starting (single production attempt)".len())
            .any(|window| { window == b"WiFi bootstrap starting (single production attempt)" }));

        runtime.mirror_line_current_tcb("cohesix> ");
        runtime.open_linked_hdmi_prompt_line("cohesix> ");
        runtime.record_linked_hdmi_input_echo_byte(b'h');
        runtime.queue_linked_hdmi_input_row_or_redraw();
        let Some((input, reason, redraw)) = runtime.next_linked_hdmi_payload() else {
            panic!("first command byte must use incremental output");
        };
        assert_eq!(reason, "keyboard-input-row");
        assert!(!redraw);
        assert_eq!(input.as_slice(), b"\r\x1b[2Kcohesix> h\x1b[K");
        assert_eq!(runtime.display_trace().snapshot_generation, generation);
        assert!(!runtime.display_trace().pending_redraw);
    }

    #[test]
    fn physical_pi_hdmi_input_echo_follows_root_console_not_wifi_terminal() {
        assert!(!local_seat_hdmi_input_echo_allowed(false));
        assert!(local_seat_hdmi_input_echo_allowed(true));
    }

    #[test]
    fn physical_pi_keyboard_bytes_enter_parser_after_keyboard_admission() {
        assert!(!local_seat_keyboard_bytes_enter_parser_state(
            true, false, true
        ));
        assert!(!local_seat_keyboard_bytes_enter_parser_state(
            true, true, false
        ));
        assert!(local_seat_keyboard_bytes_enter_parser_state(
            true, true, true
        ));
        assert!(local_seat_keyboard_bytes_enter_parser_state(
            false, false, false
        ));
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    #[test]
    fn linked_usb_keyboard_input_frame_requires_typed_payload() {
        let valid_frame = crate::hal::driver_task::DriverFrameDescriptor {
            offset: crate::hal::driver_task::DRIVER_TASK_RING_FRAME_OFFSET as u32,
            len: 5,
            flags: DRIVER_RUNTIME_USB_KEYBOARD_FRAME_FLAG_INPUT,
        };
        let valid = crate::hal::driver_task::DriverTaskCompletionRecord {
            sequence: 42,
            code: crate::hal::driver_task::DriverTaskCompletionCode::FrameReady.as_u16(),
            detail: crate::hal::driver_task::DriverTaskFaultCode::None.as_u16(),
            result: 5,
            frame: valid_frame,
        };
        assert!(linked_usb_keyboard_input_frame_valid(valid));

        let untyped_stale_help = crate::hal::driver_task::DriverTaskCompletionRecord {
            frame: crate::hal::driver_task::DriverFrameDescriptor {
                flags: 0,
                ..valid_frame
            },
            ..valid
        };
        assert!(!linked_usb_keyboard_input_frame_valid(untyped_stale_help));

        let zero_len = crate::hal::driver_task::DriverTaskCompletionRecord {
            result: 0,
            frame: crate::hal::driver_task::DriverFrameDescriptor {
                len: 0,
                ..valid_frame
            },
            ..valid
        };
        assert!(!linked_usb_keyboard_input_frame_valid(zero_len));

        let mismatched_len =
            crate::hal::driver_task::DriverTaskCompletionRecord { result: 4, ..valid };
        assert!(!linked_usb_keyboard_input_frame_valid(mismatched_len));
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    #[test]
    fn physical_pi_usb_first_byte_ready_waits_for_accepted_report() {
        assert!(!local_seat_usb_first_byte_ready_allowed(false, 0));
        assert!(!local_seat_usb_first_byte_ready_allowed(true, 0));
        assert!(!local_seat_usb_first_byte_ready_allowed(false, 1));
        assert!(local_seat_usb_first_byte_ready_allowed(true, 1));
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    #[test]
    fn physical_pi_usb_owner_state_tracks_controller_ready_not_first_byte() {
        assert!(local_seat_usb_controller_owner_state_ready_allowed(true));
        assert!(!local_seat_usb_controller_owner_state_ready_allowed(false));
        assert!(!local_seat_usb_first_byte_ready_allowed(false, 1));
    }

    #[test]
    fn physical_pi_prompt_ready_does_not_block_on_active_idle_queue_after_first_byte() {
        #[cfg(all(feature = "kernel", feature = "usb"))]
        {
            let active_idle_result = LINKED_LOCAL_SEAT_USB_READY_MAX_QUEUED_REPORTS
                | (u32::from(DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_IDLE)
                    << DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_SHIFT);

            assert!(local_seat_keyboard_active_queue_idle(
                LINKED_LOCAL_SEAT_USB_READY_MAX_QUEUED_REPORTS,
                u32::from(DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_IDLE)
            ));
            assert!(!local_seat_keyboard_result_blocks_prompt_ready(
                active_idle_result,
                true,
            ));
            assert!(local_seat_usb_prompt_safe_ready_state(
                LocalSeatUsbPromptSafeReadyState {
                    physical_pi_owner_state: true,
                    root_console_ready: true,
                    keyboard_ready: true,
                    recovery_pending: false,
                    no_reply_streak: 0,
                    runtime_queue_blocked: local_seat_keyboard_result_blocks_prompt_ready(
                        active_idle_result,
                        true,
                    ),
                },
            ));
        }
    }

    #[test]
    fn physical_pi_usb_prompt_safe_ready_waits_for_keyboard_admission_and_hard_blockers() {
        let ready = LocalSeatUsbPromptSafeReadyState {
            physical_pi_owner_state: true,
            root_console_ready: true,
            keyboard_ready: true,
            recovery_pending: false,
            no_reply_streak: 0,
            runtime_queue_blocked: false,
        };
        assert!(!local_seat_usb_prompt_safe_ready_state(
            LocalSeatUsbPromptSafeReadyState {
                keyboard_ready: false,
                ..ready
            },
        ));
        assert!(!local_seat_usb_prompt_safe_ready_state(
            LocalSeatUsbPromptSafeReadyState {
                root_console_ready: false,
                ..ready
            },
        ));
        assert!(!local_seat_usb_prompt_safe_ready_state(
            LocalSeatUsbPromptSafeReadyState {
                recovery_pending: true,
                ..ready
            },
        ));
        assert!(local_seat_usb_prompt_safe_ready_state(
            LocalSeatUsbPromptSafeReadyState {
                no_reply_streak: 1,
                ..ready
            },
        ));
        assert!(!local_seat_usb_prompt_safe_ready_state(
            LocalSeatUsbPromptSafeReadyState {
                runtime_queue_blocked: true,
                ..ready
            },
        ));
        assert!(local_seat_usb_prompt_safe_ready_state(ready));
        assert!(local_seat_usb_prompt_safe_ready_state(
            LocalSeatUsbPromptSafeReadyState {
                physical_pi_owner_state: false,
                root_console_ready: false,
                keyboard_ready: false,
                recovery_pending: true,
                no_reply_streak: 9,
                runtime_queue_blocked: true,
            },
        ));
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    #[test]
    fn physical_pi_usb_recovery_details_include_hub_and_device_failures() {
        for detail in [
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_ENABLE_SLOT_FAILED,
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_ADDRESS_DEVICE_FAILED,
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_DEVICE_DESCRIPTOR_FAILED,
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_CONFIG_DESCRIPTOR_FAILED,
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HID_ATTACH_FAILED,
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_ATTACH_FAILED,
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_SET_CONFIG_FAILED,
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_DESCRIPTOR_FAILED,
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HUB_CONTEXT_FAILED,
        ] {
            assert!(linked_local_seat_usb_detail_warrants_recovery(detail));
        }

        for detail in [
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_XHCI_READY,
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_COMMAND_RING_READY,
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_ROOT_PORT_CONNECTED,
            pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_KEYBOARD_READY,
        ] {
            assert!(!linked_local_seat_usb_detail_warrants_recovery(detail));
        }
    }

    #[test]
    fn physical_pi_usb_command_ready_requires_report_and_clean_polls() {
        let endpoint_ready = LocalSeatUsbCommandReadyState {
            prompt_safe_ready: true,
            first_report_ready: false,
            first_report_fresh: false,
            first_byte_ready: false,
            clean_polls: 0,
            post_first_byte_pressure: false,
        };
        assert!(!local_seat_usb_command_ready_state(endpoint_ready));
        assert!(!local_seat_usb_command_ready_state(
            LocalSeatUsbCommandReadyState {
                prompt_safe_ready: false,
                ..endpoint_ready
            },
        ));
        assert!(!local_seat_usb_command_ready_state(
            LocalSeatUsbCommandReadyState {
                first_report_ready: true,
                ..endpoint_ready
            },
        ));
        assert!(!local_seat_usb_command_ready_state(
            LocalSeatUsbCommandReadyState {
                first_report_ready: true,
                first_report_fresh: false,
                clean_polls: LINKED_LOCAL_SEAT_USB_PROMPT_READY_CLEAN_POLLS,
                ..endpoint_ready
            },
        ));
        assert!(local_seat_usb_command_ready_state(
            LocalSeatUsbCommandReadyState {
                first_report_ready: true,
                first_report_fresh: true,
                clean_polls: LINKED_LOCAL_SEAT_USB_PROMPT_READY_CLEAN_POLLS,
                ..endpoint_ready
            },
        ));
        assert!(!local_seat_usb_command_ready_state(
            LocalSeatUsbCommandReadyState {
                first_report_ready: true,
                first_report_fresh: true,
                first_byte_ready: true,
                clean_polls: LINKED_LOCAL_SEAT_USB_PROMPT_READY_CLEAN_POLLS - 1,
                ..endpoint_ready
            },
        ));
        assert!(!local_seat_usb_command_ready_state(
            LocalSeatUsbCommandReadyState {
                first_report_ready: true,
                first_report_fresh: true,
                first_byte_ready: true,
                clean_polls: LINKED_LOCAL_SEAT_USB_PROMPT_READY_CLEAN_POLLS,
                post_first_byte_pressure: true,
                ..endpoint_ready
            },
        ));
        assert!(local_seat_usb_command_ready_state(
            LocalSeatUsbCommandReadyState {
                first_report_ready: true,
                first_report_fresh: true,
                first_byte_ready: true,
                clean_polls: LINKED_LOCAL_SEAT_USB_PROMPT_READY_CLEAN_POLLS,
                ..endpoint_ready
            },
        ));
    }

    #[test]
    fn usb_command_ready_receipt_and_details_fit_bounded_log_records() {
        let keyboard = LocalSeatKeyboardTrace {
            queued_bytes: usize::MAX,
            accepted_bytes: u64::MAX,
            arming_bytes: u64::MAX,
            drained_bytes: u64::MAX,
            echoed_bytes: u64::MAX,
            driver_task_no_reply_streak: u64::MAX,
            recovery_aux_pending: true,
            ..LocalSeatKeyboardTrace::default()
        };
        let display = LocalSeatDisplayTrace {
            pending_bytes: usize::MAX,
            submitted_frames: u64::MAX,
            ..LocalSeatDisplayTrace::default()
        };

        let receipt = format_usb_keyboard_command_ready_receipt(
            u8::MAX,
            keyboard.driver_task_no_reply_streak,
            keyboard.recovery_aux_pending,
        );
        let input = format_usb_keyboard_command_ready_input_detail(keyboard);
        let delivery = format_usb_keyboard_command_ready_delivery_detail(keyboard);
        let display = format_usb_keyboard_command_ready_display_detail(display);

        assert!(receipt
            .starts_with("[local-seat] usb keyboard command-ready action=enable-command-input "));
        assert!(receipt.ends_with("recovery_pending=yes"));
        assert!(input.ends_with("accepted=18446744073709551615"));
        assert!(delivery.ends_with("echoed=18446744073709551615"));
        assert!(display.ends_with("hdmi_submitted=18446744073709551615"));
        for line in [
            receipt.as_str(),
            input.as_str(),
            delivery.as_str(),
            display.as_str(),
        ] {
            assert!(
                line.len() <= 160,
                "ordered log payload is too large: {line}"
            );
            assert!(line.len() <= crate::serial::DEFAULT_LINE_CAPACITY);
        }
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    #[test]
    fn physical_pi_usb_command_ready_rejects_overqueue_without_transfer_events() {
        let overqueue_no_event = (LINKED_LOCAL_SEAT_USB_ACTIVE_INTERRUPT_QUEUE_DEPTH + 1)
            | (u32::from(DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_IDLE)
                << DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_SHIFT);

        assert!(local_seat_keyboard_pre_first_report_full_queue_stalled(
            overqueue_no_event
        ));
        assert!(local_seat_keyboard_result_blocks_prompt_ready(
            overqueue_no_event,
            false,
        ));
        assert!(!local_seat_keyboard_first_report_fresh(
            overqueue_no_event,
            true,
            false
        ));
        assert!(!local_seat_usb_command_ready_state(
            LocalSeatUsbCommandReadyState {
                prompt_safe_ready: true,
                first_report_ready: true,
                first_report_fresh: false,
                first_byte_ready: false,
                clean_polls: LINKED_LOCAL_SEAT_USB_PROMPT_READY_CLEAN_POLLS,
                post_first_byte_pressure: false,
            },
        ));
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    #[test]
    fn physical_pi_usb_command_ready_retains_proven_report_across_armed_queue_without_keypress() {
        let one_queue_no_event = 1
            | (u32::from(DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_NONE)
                << DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_SHIFT);

        assert!(!local_seat_keyboard_pre_first_report_full_queue_stalled(
            one_queue_no_event
        ));
        assert!(!local_seat_keyboard_pre_first_report_no_event_queue_stalled(one_queue_no_event));
        assert!(!local_seat_keyboard_pre_first_report_recovery_due(
            false,
            one_queue_no_event
        ));
        assert!(!local_seat_keyboard_pre_first_report_recovery_due(
            true,
            one_queue_no_event
        ));
        assert!(
            !local_seat_keyboard_pre_first_report_steady_full_queue_stalled(one_queue_no_event)
        );
        assert!(!local_seat_keyboard_result_blocks_prompt_ready(
            one_queue_no_event,
            false,
        ));
        assert!(local_seat_keyboard_first_report_fresh(
            one_queue_no_event,
            true,
            false
        ));
        assert!(local_seat_usb_command_ready_state(
            LocalSeatUsbCommandReadyState {
                prompt_safe_ready: true,
                first_report_ready: true,
                first_report_fresh: true,
                first_byte_ready: false,
                clean_polls: LINKED_LOCAL_SEAT_USB_PROMPT_READY_CLEAN_POLLS,
                post_first_byte_pressure: false,
            },
        ));
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    #[test]
    fn physical_pi_usb_command_ready_accepts_fresh_idle_report_without_keypress() {
        let fresh_idle_report = (1u32 << 24)
            | (u32::from(DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_IDLE)
                << DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_SHIFT);

        assert!(!local_seat_keyboard_result_blocks_prompt_ready(
            fresh_idle_report,
            false,
        ));
        assert!(local_seat_keyboard_first_report_fresh(
            fresh_idle_report,
            true,
            false
        ));
        assert!(local_seat_usb_command_ready_state(
            LocalSeatUsbCommandReadyState {
                prompt_safe_ready: true,
                first_report_ready: true,
                first_report_fresh: true,
                first_byte_ready: false,
                clean_polls: LINKED_LOCAL_SEAT_USB_PROMPT_READY_CLEAN_POLLS,
                post_first_byte_pressure: false,
            },
        ));
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    #[test]
    fn physical_pi_usb_command_ready_accepts_no_idle_report_without_keypress() {
        let no_idle_report = u32::from(DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_NO_IDLE_REPORT)
            << DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_SHIFT;

        assert!(!local_seat_keyboard_pre_first_report_full_queue_stalled(
            no_idle_report
        ));
        assert!(!local_seat_keyboard_result_blocks_prompt_ready(
            no_idle_report,
            false,
        ));
        assert!(local_seat_keyboard_first_report_fresh(
            no_idle_report,
            true,
            false
        ));
        assert!(!local_seat_keyboard_first_report_fresh(
            no_idle_report,
            false,
            false
        ));
        assert!(local_seat_usb_command_ready_state(
            LocalSeatUsbCommandReadyState {
                prompt_safe_ready: true,
                first_report_ready: true,
                first_report_fresh: true,
                first_byte_ready: false,
                clean_polls: LINKED_LOCAL_SEAT_USB_PROMPT_READY_CLEAN_POLLS,
                post_first_byte_pressure: false,
            },
        ));
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    #[test]
    fn linked_usb_recovery_failed_first_report_does_not_drop_first_byte_proof() {
        let completion = crate::hal::driver_task::DriverTaskCompletionRecord {
            sequence: 1,
            code: crate::hal::driver_task::DriverTaskCompletionCode::Progress.as_u16(),
            detail: DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_READY,
            result: u32::from(DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_RECOVERY_FAILED)
                << DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_SHIFT,
            frame: crate::hal::driver_task::DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        };

        assert!(
            local_seat_usb_first_report_requires_reenumeration_with_first_byte(completion, false)
        );

        assert!(
            !local_seat_usb_first_report_requires_reenumeration_with_first_byte(completion, true)
        );
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    #[test]
    fn linked_usb_keyboard_ready_alert_reports_first_byte_as_evidence() {
        let completion = crate::hal::driver_task::DriverTaskCompletionRecord {
            sequence: 9,
            code: crate::hal::driver_task::DriverTaskCompletionCode::FrameReady.as_u16(),
            detail: DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_READY,
            result: 0x0f00_0001,
            frame: crate::hal::driver_task::DriverFrameDescriptor {
                offset: 0,
                len: 1,
                flags: DRIVER_RUNTIME_USB_KEYBOARD_FRAME_FLAG_INPUT,
            },
        };

        let line = linked_local_seat_usb_keyboard_ready_alert_line(completion, 1);

        assert!(line.contains("usb keyboard input ready: first_report=yes first_byte=yes"));
        assert!(line.contains("input=local-seat"));
        assert!(line.contains("source=linked-runtime-hid"));
        assert!(line.contains("arming=1"));
        assert!(line.contains("parser_ingress=yes"));
        assert!(line.contains("action=command-input"));
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
    fn keyboard_wait_line_direct_submit_terminates_before_prompt() {
        let linked_line = local_seat_hdmi_keyboard_wait_line_payload(false);
        assert_eq!(linked_line.as_str(), LOCAL_SEAT_HDMI_KEYBOARD_WAIT_LINE);

        let direct_line = local_seat_hdmi_keyboard_wait_line_payload(true);
        assert_eq!(direct_line.as_str(), "USB console ready\n");
        assert_eq!(
            direct_line
                .as_bytes()
                .iter()
                .filter(|&&byte| byte == b'\n')
                .count(),
            1
        );
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    #[test]
    fn linked_usb_keyboard_poll_stays_nonblocking_until_keyboard_ready() {
        assert_eq!(local_seat_keyboard_poll_aux(false, false, false), 0);
        assert_eq!(
            local_seat_keyboard_poll_aux(false, true, false),
            DRIVER_RUNTIME_USB_ENUMERATE_AUX
        );
        assert_eq!(local_seat_keyboard_poll_aux(true, false, false), 0);
        assert_eq!(local_seat_keyboard_poll_aux(true, true, false), 0);
        assert_eq!(
            local_seat_keyboard_poll_aux(false, false, true),
            DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_AUX
        );
        assert_eq!(
            local_seat_keyboard_poll_aux(false, true, true),
            DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_AUX
        );
        assert_eq!(
            local_seat_keyboard_poll_aux(true, false, true),
            DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_AUX
        );
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    #[test]
    fn linked_usb_keyboard_aux_submission_counts_enumeration_separately() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 4,
        });

        assert!(!runtime.record_keyboard_poll_aux_submission(DRIVER_RUNTIME_USB_ENUMERATE_AUX));
        let after_enumerate = runtime.keyboard_trace();
        assert_eq!(after_enumerate.enumeration_aux_requests, 1);
        assert_eq!(after_enumerate.recovery_aux_requests, 0);
        assert!(!after_enumerate.recovery_aux_pending);

        assert!(
            runtime.record_keyboard_poll_aux_submission(DRIVER_RUNTIME_USB_KEYBOARD_RECOVERY_AUX)
        );
        let after_recovery = runtime.keyboard_trace();
        assert_eq!(after_recovery.enumeration_aux_requests, 1);
        assert_eq!(after_recovery.recovery_aux_requests, 1);
        assert!(after_recovery.recovery_aux_pending);
    }

    #[cfg(all(
        feature = "kernel",
        feature = "usb",
        target_arch = "aarch64",
        target_os = "none"
    ))]
    #[test]
    fn keyboard_busy_line_is_never_shown_before_command_ready() {
        let runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 4,
        });

        assert!(!runtime.linked_hdmi_keyboard_busy_line_warranted("post-first-byte-pressure"));
        assert!(!runtime.linked_hdmi_keyboard_busy_line_warranted("keyboard-poll-no-reply"));
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    #[test]
    fn linked_usb_recovery_failed_first_report_reenters_enumeration() {
        let recovery_failed = crate::hal::driver_task::DriverTaskCompletionRecord {
            sequence: 7,
            code: crate::hal::driver_task::DriverTaskCompletionCode::Progress.as_u16(),
            detail: DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_READY,
            result: u32::from(DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_RECOVERY_FAILED)
                << DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_SHIFT,
            frame: crate::hal::driver_task::DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        };
        assert!(local_seat_usb_first_report_requires_reenumeration(
            recovery_failed
        ));

        let pending_failed = crate::hal::driver_task::DriverTaskCompletionRecord {
            sequence: 8,
            code: crate::hal::driver_task::DriverTaskCompletionCode::Progress.as_u16(),
            detail: DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_PENDING,
            result: u32::from(DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_RECOVERY_FAILED)
                << DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_SHIFT,
            frame: crate::hal::driver_task::DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        };
        assert!(local_seat_usb_first_report_requires_reenumeration(
            pending_failed
        ));

        let pending_armed = crate::hal::driver_task::DriverTaskCompletionRecord {
            sequence: 9,
            code: crate::hal::driver_task::DriverTaskCompletionCode::Progress.as_u16(),
            detail: DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_PENDING,
            result: 1
                | (u32::from(DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_NONE)
                    << DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_SHIFT),
            frame: crate::hal::driver_task::DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        };
        assert!(local_seat_usb_first_report_pending_endpoint_armed(
            pending_armed
        ));
        assert!(!local_seat_usb_first_report_requires_reenumeration(
            pending_armed
        ));
        assert!(!local_seat_usb_command_ready_state(
            LocalSeatUsbCommandReadyState {
                prompt_safe_ready: true,
                first_report_ready: false,
                first_report_fresh: false,
                first_byte_ready: false,
                clean_polls: LINKED_LOCAL_SEAT_USB_PROMPT_READY_CLEAN_POLLS,
                post_first_byte_pressure: false,
            },
        ));
        assert!(
            !local_seat_hdmi_prompt_release_ready(true, false),
            "the HDMI prompt must remain withheld while USB parser admission is pending",
        );

        let ready = crate::hal::driver_task::DriverTaskCompletionRecord {
            sequence: 10,
            code: crate::hal::driver_task::DriverTaskCompletionCode::Progress.as_u16(),
            detail: DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_READY,
            result: 1
                | (u32::from(DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_IDLE)
                    << DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_SHIFT)
                | (1 << 24),
            frame: crate::hal::driver_task::DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        };
        assert!(!local_seat_usb_first_report_requires_reenumeration(ready));

        let pending_recovery_success_no_event =
            crate::hal::driver_task::DriverTaskCompletionRecord {
                sequence: 11,
                code: crate::hal::driver_task::DriverTaskCompletionCode::Progress.as_u16(),
                detail: DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_PENDING,
                result: 1
                    | (u32::from(DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_RECOVERY_SUCCESS)
                        << DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_SHIFT),
                frame: crate::hal::driver_task::DriverFrameDescriptor {
                    offset: 0,
                    len: 0,
                    flags: 0,
                },
            };
        assert!(local_seat_usb_first_report_pending_endpoint_armed(
            pending_recovery_success_no_event
        ));
        assert!(!local_seat_usb_first_report_requires_reenumeration(
            pending_recovery_success_no_event
        ));

        let ready_recovery_success_no_event = crate::hal::driver_task::DriverTaskCompletionRecord {
            sequence: 12,
            code: crate::hal::driver_task::DriverTaskCompletionCode::Progress.as_u16(),
            detail: DRIVER_RUNTIME_USB_SERVICE_DETAIL_FIRST_REPORT_READY,
            result: 1
                | (u32::from(DRIVER_RUNTIME_USB_KEYBOARD_REPORT_STATUS_RECOVERY_SUCCESS)
                    << DRIVER_RUNTIME_USB_KEYBOARD_RESULT_REPORT_STATUS_SHIFT),
            frame: crate::hal::driver_task::DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        };
        assert!(!local_seat_usb_first_report_requires_reenumeration(
            ready_recovery_success_no_event
        ));
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    #[test]
    fn linked_usb_pre_first_byte_attach_failures_reenter_enumeration() {
        let address_failed = crate::hal::driver_task::DriverTaskCompletionRecord {
            sequence: 10,
            code: crate::hal::driver_task::DriverTaskCompletionCode::Progress.as_u16(),
            detail: pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_ADDRESS_DEVICE_FAILED,
            result: 0x0f00_0001,
            frame: crate::hal::driver_task::DriverFrameDescriptor {
                offset: 0,
                len: 0,
                flags: 0,
            },
        };
        let hid_failed = crate::hal::driver_task::DriverTaskCompletionRecord {
            sequence: 11,
            detail: pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_HID_ATTACH_FAILED,
            ..address_failed
        };
        let root_port_progress = crate::hal::driver_task::DriverTaskCompletionRecord {
            sequence: 12,
            detail: pi4_driver_abi::DRIVER_RUNTIME_USB_INIT_DETAIL_ROOT_PORT_CONNECTED,
            ..address_failed
        };

        assert!(
            local_seat_usb_first_report_requires_reenumeration_with_first_byte(
                address_failed,
                false,
            )
        );
        assert!(
            local_seat_usb_first_report_requires_reenumeration_with_first_byte(hid_failed, false,)
        );
        assert!(
            !local_seat_usb_first_report_requires_reenumeration_with_first_byte(
                address_failed,
                true,
            )
        );
        assert!(
            !local_seat_usb_first_report_requires_reenumeration_with_first_byte(
                root_port_progress,
                false,
            )
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
        assert!(linked_local_seat_usb_resume_enumeration_required(
            true, false, false
        ));
        assert!(!linked_local_seat_usb_resume_enumeration_required(
            false, false, false
        ));
        assert!(!linked_local_seat_usb_resume_enumeration_required(
            true, false, true
        ));
        assert!(!linked_local_seat_usb_resume_enumeration_required(
            true, true, false
        ));
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    #[test]
    fn linked_usb_probe_progress_burst_is_progress_bounded() {
        assert!((1..=32).contains(&LINKED_LOCAL_SEAT_USB_PROBE_STABLE_PROGRESS_BURST_ATTEMPTS));
        assert!((1..=8).contains(&LINKED_LOCAL_SEAT_USB_PROBE_STALLED_PROGRESS_BURST_ATTEMPTS));
        assert!(LINKED_LOCAL_SEAT_USB_PROBE_STABLE_PROGRESS_BURST_ATTEMPTS >= 16);
        assert!(LINKED_LOCAL_SEAT_USB_PROBE_STALLED_PROGRESS_BURST_ATTEMPTS >= 4);
        assert!(
            LINKED_LOCAL_SEAT_USB_PROBE_STABLE_PROGRESS_BURST_ATTEMPTS
                > LINKED_LOCAL_SEAT_USB_PROBE_STALLED_PROGRESS_BURST_ATTEMPTS
        );
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
        assert!(usb_enumeration_stalled_progress_token_allows_probe_burst(
            Some((8, 190, DRIVER_RUNTIME_USB_ENUMERATE_AUX)),
            Some((8, 190, DRIVER_RUNTIME_USB_ENUMERATE_AUX)),
            0
        ));
        assert!(!usb_enumeration_stalled_progress_token_allows_probe_burst(
            Some((8, 190, DRIVER_RUNTIME_USB_ENUMERATE_AUX)),
            Some((8, 190, DRIVER_RUNTIME_USB_ENUMERATE_AUX)),
            LINKED_LOCAL_SEAT_USB_PROBE_STALLED_PROGRESS_BURST_ATTEMPTS
        ));
        assert!(!usb_enumeration_stalled_progress_token_allows_probe_burst(
            None, None, 0
        ));
        assert!(!usb_enumeration_stalled_progress_token_allows_probe_burst(
            Some((8, 190, DRIVER_RUNTIME_USB_ENUMERATE_AUX)),
            Some((8, 236, DRIVER_RUNTIME_USB_ENUMERATE_AUX)),
            0
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

    #[cfg(all(feature = "kernel", feature = "usb"))]
    #[test]
    fn retained_keyboard_probe_budget_consumes_one_outer_turn_per_attempt() {
        let mut cursor = LocalSeatKeyboardProbeCursor::new();
        cursor.begin(false, Some((8, 190, DRIVER_RUNTIME_USB_ENUMERATE_AUX)));

        for turn in 1..=LINKED_LOCAL_SEAT_USB_PROBE_STABLE_PROGRESS_BURST_ATTEMPTS {
            let retained = cursor.retain_after_turn(
                Some((8, 190, DRIVER_RUNTIME_USB_ENUMERATE_AUX)),
                true,
                true,
                true,
            );
            assert_eq!(cursor.turns, turn);
            assert_eq!(
                retained,
                turn < LINKED_LOCAL_SEAT_USB_PROBE_STABLE_PROGRESS_BURST_ATTEMPTS,
                "the retained probe budget must expire only on its matching outer turn",
            );
        }
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    #[test]
    fn retained_keyboard_probe_restores_prior_polling_policy() {
        let mut cursor = LocalSeatKeyboardProbeCursor::new();
        cursor.begin(true, None);
        assert!(cursor.active);
        assert!(cursor.finish());
        assert!(!cursor.active);

        cursor.begin(false, None);
        assert!(!cursor.finish());
        assert!(!cursor.active);
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    #[test]
    fn keyboard_probe_requires_a_live_completed_service_turn() {
        assert_eq!(
            local_seat_keyboard_probe_result(LocalSeatServiceTurn::Complete, true, true),
            LocalSeatKeyboardProbeResult::Attached,
        );
        assert_eq!(
            local_seat_keyboard_probe_result(LocalSeatServiceTurn::Pending, true, true),
            LocalSeatKeyboardProbeResult::KeyboardUnavailable,
            "cached attachment must not turn a pending retained request into live proof",
        );
        assert_eq!(
            local_seat_keyboard_probe_result(
                LocalSeatServiceTurn::Failed("no-terminal"),
                true,
                true,
            ),
            LocalSeatKeyboardProbeResult::KeyboardUnavailable,
        );
        assert_eq!(
            local_seat_keyboard_probe_result(LocalSeatServiceTurn::Pending, false, false),
            LocalSeatKeyboardProbeResult::BackendUnavailable,
        );
    }

    #[cfg(all(feature = "kernel", feature = "usb"))]
    #[test]
    fn retained_keyboard_commands_keep_an_immutable_fingerprint() {
        let mut runtime = LocalSeatRuntime::new(LocalSeatStatus {
            keyboard_device: "usb-kbd0",
            display_device: "hdmi0",
            line_bytes: 16,
            buffer_lines: 4,
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
        command.aux0 = DRIVER_RUNTIME_USB_ENUMERATE_AUX;
        runtime.linked_usb_attach_command = Some(command);

        runtime.keyboard_probe_cursor.begin(false, None);
        let _ = runtime
            .keyboard_probe_cursor
            .retain_after_turn(None, true, true, true);

        assert_eq!(runtime.linked_usb_attach_command, Some(command));
    }
}
