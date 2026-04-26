// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Raspberry Pi 4 local-seat backend (HDMI text mirror + USB keyboard ingress).
// Author: Lukas Bower

#![allow(unsafe_code)]

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::cmp;
use core::hint::spin_loop;
use core::mem;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, AtomicUsize, Ordering};

use font8x8::legacy::BASIC_LEGACY;
use spin::Mutex;
use usb_oxide::{
    class, completion, desc_type, find_hid_interfaces, hid_protocol, hid_subclass, hub_feature,
    hub_protocol, led, regs, request, scancode, scancode_to_ascii, set_xhci_diag_hook, ConfigDesc,
    DeviceDesc, Dma, DmaShareError, HidDesc, HidDevice, HubDesc, SetupPacket, TtContext, UsbDevice,
    UsbError, XhciControllerParams, XhciCtrl, XhciFirmwareHandoff, XhciRuntimeSeedSnapshot,
};

use crate::bootstrap::log as boot_log;
use crate::hal::{dma, pi4_wifi, DeviceHal, HalError, KernelHal};
use crate::local_seat::{LocalSeatXhciCapabilitySnapshot, LocalSeatXhciStopStateSnapshot};
use crate::sel4::BootInfoExt;

const PAGE_SIZE: usize = 4096;
const PAGE_MASK: usize = PAGE_SIZE - 1;

const MAILBOX_PAGE_PADDR_CANDIDATES: [usize; 2] = [0xFE00_B000, 0x7E00_B000];
const MAILBOX_READ_OFFSET: usize = 0x880;
const MAILBOX_STATUS0_OFFSET: usize = 0x898;
const MAILBOX_WRITE_OFFSET: usize = 0x8A0;
const MAILBOX_STATUS1_OFFSET: usize = 0x8B8;
const MAILBOX_EMPTY: u32 = 0x4000_0000;
const MAILBOX_FULL: u32 = 0x8000_0000;
const MAILBOX_CHANNEL_PROPERTY: u32 = 8;
const MAILBOX_RESPONSE_SUCCESS: u32 = 0x8000_0000;
const MAILBOX_VALUE_RESPONSE: u32 = 1 << 31;
const MAILBOX_WAIT_SPINS: usize = 50_000_000;
const MAILBOX_DRAIN_LIMIT: usize = 64;
const MAILBOX_MAP_EXACT_ATTEMPT_CAP: usize = 1024;

const VC_BUS_ALIAS_BASES: [u32; 2] = [0xC000_0000, 0x4000_0000];
const VC_BUS_MASK: u32 = 0x3FFF_FFFF;

const TAG_SET_PHYSICAL_SIZE: u32 = 0x0004_8003;
const TAG_SET_VIRTUAL_SIZE: u32 = 0x0004_8004;
const TAG_SET_DEPTH: u32 = 0x0004_8005;
const TAG_SET_PIXEL_ORDER: u32 = 0x0004_8006;
const TAG_ALLOCATE_BUFFER: u32 = 0x0004_0001;
const TAG_GET_PITCH: u32 = 0x0004_0008;
// U-Boot only waits 200 us after an acknowledged VL805 reset-notify, but this
// local-seat path uses a spin-only delay helper rather than a calibrated
// microsecond timer. Keep a wider one-shot settle budget here so the first
// post-reset xHCI status read does not race the still-reloading controller.
const VL805_MAILBOX_RESET_SETTLE_MS: u64 = 20;
// A posted reset-notify can complete later than an acknowledged response, so
// give firmware a wider cold-start window before runtime touches VL805 again.
const VL805_MAILBOX_RESET_POSTED_SETTLE_MS: u64 = 100;

const DEFAULT_FB_WIDTH: u32 = 1024;
const DEFAULT_FB_HEIGHT: u32 = 768;
const DEFAULT_FB_DEPTH: u32 = 32;
const DEFAULT_FB_ALIGNMENT: u32 = 16;
const PIXEL_ORDER_RGB: u32 = 1;

const CHAR_WIDTH: usize = 8;
const CHAR_HEIGHT: usize = 16;
const TAB_WIDTH: usize = 4;
const HDMI_SCROLLBACK_MAX_LINES: usize = 512;
const HDMI_SCROLLBACK_MAX_LINE_BYTES: usize = 256;
const HDMI_SCROLL_PAGE_STEP_ROWS: i8 = 8;

const FG_COLOR: u32 = 0xFFFF_FFFF;
const BG_COLOR: u32 = 0xFF00_0000;
const RPI4_XHCI_MMIO_HIGH_CANDIDATE: usize = 0x0000_0006_0000_0000;
const RPI4_XHCI_MMIO_PRIMARY_CANDIDATE: usize = 0x0000_0000_FE98_0000;
const RPI4_XHCI_MMIO_SECONDARY_CANDIDATE: usize = 0x0000_0000_7E98_0000;
const BCM2711_COMMON_PERIPH_BUS_BASE: usize = 0x7E00_0000;
const BCM2711_COMMON_PERIPH_PHYS_BASE: usize = 0xFE00_0000;
const BCM2711_COMMON_PERIPH_SIZE: usize = 0x0180_0000;
const BCM2711_SOC_PERIPH_BUS_BASE: usize = 0x7C00_0000;
const BCM2711_SOC_PERIPH_PHYS_BASE: usize = 0xFC00_0000;
const BCM2711_SOC_PERIPH_SIZE: usize = 0x0200_0000;
const BCM2711_ARM_LOCAL_BUS_BASE: usize = 0x4000_0000;
const BCM2711_ARM_LOCAL_PHYS_BASE: usize = 0xFF80_0000;
const BCM2711_ARM_LOCAL_SIZE: usize = 0x0080_0000;

// Runtime xHCI probing must prefer the high PCIe aperture used by the VL805.
// The low BCM2711 `0xfe980000` / `0x7e980000` aliases are the SoC DWC2 USB
// controller, not the VL805 xHCI block, so they are only retained as explicit
// diagnostics breadcrumbs and never as preferred xHCI runtime sources.
const RPI4_XHCI_MMIO_FALLBACKS: [usize; 2] = [
    RPI4_XHCI_MMIO_PRIMARY_CANDIDATE,
    RPI4_XHCI_MMIO_SECONDARY_CANDIDATE,
];
// Boot-time xHCI pinning must only target controller BAR aliases backed by
// the firmware DTB or a validated runtime hint.
const RPI4_XHCI_MMIO_PRESEED_CANDIDATES: [usize; 2] = [
    RPI4_XHCI_MMIO_PRIMARY_CANDIDATE,
    RPI4_XHCI_MMIO_SECONDARY_CANDIDATE,
];
// Pi4 UEFI + seL4 can raise a fatal asynchronous external abort if we touch
// some legacy ECAM aliases during early boot. Restrict probing to the
// firmware-observed high ECAM aperture used by the working xHCI path.
const VL805_ECAM_BASE_CANDIDATES: [usize; 1] = [0x0000_0006_0000_0000];
const XHCI_MMIO_CANDIDATE_LIMIT: usize = 8;
const VL805_PCI_CFG_ATTEMPT_CAP: usize = 512;
const XHCI_MAX_PROBE_PORTS: usize = 16;
const XHCI_PORT_DETECT_PASSES: usize = 4;
const XHCI_PORT_DETECT_SETTLE_SPINS: usize = 200_000;
const XHCI_PORT_DETECT_FINAL_WAIT_MS: u64 = 100;
const HUB_ENUM_MAX_DEPTH: usize = 2;
const HUB_MAX_DOWNSTREAM_PORTS: usize = 15;
const HUB_DESC_MAX_BYTES: usize = 12;
const HUB_PORT_STATUS_BYTES: usize = 4;
const HUB_PORT_STATUS_RETRY_LOOPS: usize = 64;
const HUB_PORT_STATUS_QUICK_RETRIES: usize = 4;
const HUB_SET_FEATURE_RETRIES: usize = 3;
const HUB_BLIND_PREPARE_RESET_RETRIES: usize = 1;
const HUB_DISCONNECTED_RECOVERY_POWER_RETRIES: usize = 2;
// Some downstream hubs report individual switching but stall on eager
// PORT_POWER requests during early enum. Keep eager power disabled and
// prefer status-driven/on-demand recovery for deterministic bring-up.
const HUB_EAGER_INDIVIDUAL_PORT_POWER: bool = false;
// Fast path: do not blind-address ports that already reported disconnected.
const HUB_ENABLE_DISCONNECTED_BLIND_PROBE: bool = false;
// Blind probing for status-unavailable ports is only allowed after prepare
// succeeds (power/reset path reached the hub).
const HUB_ENABLE_UNAVAILABLE_BLIND_PROBE: bool = true;
// Retry-addressing with alternate speeds is expensive and should stay opt-in.
const HUB_ENABLE_SPEED_FALLBACK_REPROBE: bool = false;
// Some hubs reject hub-class port requests unless wIndex carries a
// non-default interface in the high byte. Probe a bounded interface set to
// keep bring-up deterministic while tolerating firmware descriptor quirks.
const HUB_PORT_IFACE_FALLBACK_MAX: u8 = 3;
const HUB_PORT_INDEX_CANDIDATES_MAX: usize = 2 + HUB_PORT_IFACE_FALLBACK_MAX as usize;
// Alternate wIndex probing is only useful when the device responded with
// STALL; timeout-like failures indicate transport-level issues, not index
// encoding mismatches.
const HUB_INDEX_FALLBACK_ON_STALL_ONLY: bool = true;
const HUB_POST_CONFIG_SETTLE_MS: u64 = 250;
const HUB_POWER_SETTLE_MIN_MS: u64 = 200;
const HUB_RESET_SETTLE_MS: u64 = 100;
const HUB_PORT_STATUS_RETRY_DELAY_MS: u64 = 20;
const HUB_PORT_STATUS_QUICK_RETRY_DELAY_MS: u64 = 10;
const HUB_SET_FEATURE_RETRY_DELAY_MS: u64 = 10;
// Hub-class requests (SET/CLEAR_FEATURE, GET_STATUS) can be slower on
// downstream combo hubs than baseline descriptor/control setup transactions.
// Keep this aligned with usb-oxide's default control wait budget to avoid
// false timeouts during hub bring-up.
const HUB_CLASS_CONTROL_WAIT_SPINS: usize = 20_000_000;
// Use a fast first pass for hub control and port-status operations; keep a
// slow final attempt to preserve compatibility with slower hubs/keyboards.
const HUB_CLASS_CONTROL_WAIT_SPINS_FAST: usize = 2_000_000;
const HUB_CONTROL_SLOW_TAIL_ATTEMPTS: usize = 1;
const HUB_STATUS_SLOW_TAIL_ATTEMPTS: usize = 1;
const HUB_WAIT_READY_FAST_LOOPS: usize = 16;
const HUB_PORT_STATUS_RETRY_DELAY_MS_FAST: u64 = 10;
const HUB_VERBOSE_STATUS_RETRY_LOGS: bool = false;
const HID_REPORT_DESC_MAX_BYTES: usize = 512;
const HID_REPORT_DESC_WAIT_SPINS: usize = HUB_CLASS_CONTROL_WAIT_SPINS_FAST;
const WAIT_MS_SPINS_PER_MS: usize = 50_000;
const WAIT_MS_MIN_SPINS: usize = 10_000;
const WAIT_MS_MAX_SPINS: usize = 25_000_000;
const USB_PROGRESS_TICK_MS: usize = 1_000;
const USB_PROGRESS_MAX_DOTS: usize = 64;
const WIFI_PROGRESS_LOOP_TICK_LOOPS: usize = 1_000_000;
const WIFI_PROGRESS_EMIT_INTERVAL_TICKS: usize = 64;
const WIFI_PROGRESS_MAX_DOTS: usize = 3;
const PI4_VL805_XHCI_INTX_IRQ: u32 = 143;
const PI4_PCIE_BRIDGE_IRQ: u32 = 147;
const PI4_GENERIC_VTIMER_IRQ: u32 = 27;
const TRUSTED_XHCI_PCIE_SINK_IRQS: [u32; 2] = [PI4_PCIE_BRIDGE_IRQ, PI4_VL805_XHCI_INTX_IRQ];
// The trusted Pi 4 handoff stays polling-driven at the controller level, but
// runtime can still receive PCIe bridge or child INTx delivery around ownership
// transfer. IRQ 27 is the kernel's ARM generic virtual timer PPI on this seL4
// build, not an xHCI/PCIe line, so it must remain diagnostic-only for USB.
const TRUSTED_XHCI_PCIE_SINKS_ENABLED: bool = true;
// Device untyped retype on seL4 is monotonic; retries can only consume more
// device window state without restoring earlier probe addresses.
const KEYBOARD_ATTACH_ATTEMPTS: usize = 2;
const KEYBOARD_RETRY_SPINS: usize = 200_000;
const VL805_PCI_DEV_ADDR: u32 = 0x0010_0000;
// Pin the handed-off xHCI BAR before touching the VL805 ECAM page: seL4 device
// retype is monotonic within a device-untyped, and the Pi 4 ECAM page sits
// above the handed-off BAR in the same PCIe aperture. Live VL805 ECAM reads on
// the safe-mode Pi 4 path still correlate with fatal Pi 4 halts, so keep
// preseed on the map-only path and surface handoff coverage from the
// bootloader BAR contract instead of touching config space directly.
const VL805_CFG_PRESEED_TOUCH_ENABLED: bool = false;
// Local-seat should rediscover the active xHCI MMIO source itself instead of
// trusting a bootloader-exported BAR, but live VL805 config-space reads are
// still unsafe on the current Pi4/seL4 handoff and correlate with the same
// fatal Pi 4 halts. Keep the ECAM window pinned for bounded diagnostics and
// future use, while runtime discovery falls back to the preseeded legacy xHCI
// aliases when the bootloader handoff token is absent.
const VL805_CFG_RUNTIME_TOUCH_ENABLED: bool = false;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Vl805CfgPreseedMode {
    MapOnly,
    ReadMostly,
}

#[inline]
const fn vl805_cfg_preseed_mode(preseed_touch_enabled: bool) -> Vl805CfgPreseedMode {
    if preseed_touch_enabled {
        Vl805CfgPreseedMode::ReadMostly
    } else {
        Vl805CfgPreseedMode::MapOnly
    }
}

#[inline]
const fn vl805_cfg_preseed_needed(
    _preseed_touch_enabled: bool,
    _runtime_touch_enabled: bool,
) -> bool {
    // Even in safe mode, keep the ECAM page pinned so runtime can replay the
    // bootloader's verified PCI command bits before touching the handed-off BAR.
    true
}

const PCI_COMMAND_MEMORY_SPACE: u16 = 1 << 1;
const PCI_COMMAND_BUS_MASTER: u16 = 1 << 2;
const PCI_COMMAND_INTERRUPT_DISABLE: u16 = 1 << 10;
const VL805_PCI_VENDOR_ID: u16 = 0x1106;
const VL805_PCI_DEVICE_ID: u16 = 0x3483;
const VL805_EXPECTED_CLASS_CODE: u32 = 0x0C03_30;
const VL805_ECAM_WINDOW_BYTES: usize = 0x0100_0000;
const PCI_CFG_VENDOR_DEVICE: usize = 0x00;
const PCI_CFG_COMMAND_STATUS: usize = 0x04;
const PCI_CFG_CLASS_REVISION: usize = 0x08;
const PCI_CFG_BAR0: usize = 0x10;
const PCI_CFG_BAR1: usize = 0x14;
const PCI_BAR_IO_SPACE: u32 = 1 << 0;
const PCI_BAR_TYPE_MASK: u32 = 0b110;
const PCI_BAR_TYPE_64: u32 = 0b100;
const PCI_BAR_ADDR_MASK: u64 = !0xFu64;
const FRAMEBUFFER_MAP_EXACT_ATTEMPT_CAP: usize = 2048;
const XHCI_MMIO_MAP_EXACT_ATTEMPT_CAP: usize = 4096;
const EXACT_MAP_LOG_INITIAL_RETRIES: usize = 8;
const EXACT_MAP_LOG_STRIDE: usize = 128;
const FB_BYTES_PER_PIXEL: usize = mem::size_of::<u32>();
const XHCI_MMIO_MAX_BYTES: usize = 2 * 1024 * 1024;
const XHCI_MMIO_INIT_BYTES: usize = PAGE_SIZE;
const XHCI_MMIO_PRESEED_BYTES_FALLBACK: usize = 0x10000;
const XHCI_MMIO_PRESEED_BYTES_MAX: usize = 0x40000;
const XHCI_MMIO_ALIAS_SCAN_STRIDE_BYTES: usize = XHCI_MMIO_PRESEED_BYTES_FALLBACK;
const XHCI_MMIO_ALIAS_SCAN_STEPS: usize =
    XHCI_MMIO_PRESEED_BYTES_MAX / XHCI_MMIO_ALIAS_SCAN_STRIDE_BYTES;
const XHCI_HCI_VERSION_MIN: u16 = 0x0090;
const XHCI_HCI_VERSION_MAX: u16 = 0x0200;
const XHCI_DBOFF_MASK: u32 = !0x3;
const XHCI_RTSOFF_MASK: u32 = !0x1f;
// High-address DMA allocation has repeatedly stalled during Pi4 runtime xHCI
// bring-up. Force low DMA pool probing until high-path allocator faults are
// fully resolved.
const XHCI_FORCE_LOW_DMA_PROBE: bool = true;
// VL805 on Pi4 expects xHCI DMA pointers in the PCIe outbound DMA window
// address space. On bcm2711 that window is identity-mapped for the first 3 GiB
// described by `pcie0 dma-ranges`, so device-visible DMA pointers must stay in
// that low range instead of synthesizing a high `0x4_...` bus alias.
const XHCI_PCIE_DMA_WINDOW_ENABLED: bool = true;
// Per-allocation DMA tracing is useful for bring-up debugging but can add
// heavy UART latency during normal keyboard enumeration.
const XHCI_DMA_VERBOSE_LOGS: bool = false;
// Raw-phys DMA fallback is disabled on Pi4 because it consistently times out
// controller start (stage 0x0272) and adds long startup stalls.
const XHCI_TRY_RAW_PHYS_DMA_FALLBACK: bool = false;
const XHCI_DMA_MAX_BYTES: usize = 8 * 1024 * 1024;
// BCM2711 PCIe cannot DMA above the first 3 GiB (see upstream bcm2711.dtsi
// pcie0 dma-ranges). Keep VL805/xHCI buffers under this ceiling.
const RPI4_PCIE_DMA_LIMIT: usize = 0xC000_0000;
// Pi4 mailbox framebuffers should live in upper low-memory carveouts. Treat
// lower addresses as unsafe to avoid scribbling userspace/kernel RAM when
// firmware returns an unexpected bus alias.
const MIN_SAFE_FB_PHYS: usize = 0x3000_0000;
const MAX_SAFE_FB_PHYS_EXCL: usize = 0x4000_0000;
const MAX_FB_WIDTH: usize = 4096;
const MAX_FB_HEIGHT: usize = 4096;
const MAX_FB_BYTES: usize = 64 * 1024 * 1024;
const MAX_FB_MAP_PAGES: usize = MAX_FB_BYTES / PAGE_SIZE;

static USB_DMA_RANGE_WARNED: AtomicBool = AtomicBool::new(false);
static VL805_CFG_SAFE_MODE_LOGGED: AtomicBool = AtomicBool::new(false);
static VL805_CFG_RUNTIME_GATED_LOGGED: AtomicBool = AtomicBool::new(false);
static XHCI_PRESEED_ALREADY_PINNED_LOGGED: AtomicBool = AtomicBool::new(false);
static KEYBOARD_PRESEED_LOGGED: AtomicBool = AtomicBool::new(false);
static KEYBOARD_RUNTIME_INIT_LOGGED: AtomicBool = AtomicBool::new(false);
static XHCI_DIAG_HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);
static VL805_RUNTIME_RESET_STATE: AtomicU8 = AtomicU8::new(VL805_RUNTIME_RESET_STATE_UNATTEMPTED);
static XHCI_MMIO_DIAG_LOGGED: AtomicBool = AtomicBool::new(false);
static XHCI_MMIO_PIN_REUSE_LOGGED: AtomicBool = AtomicBool::new(false);
static XHCI_DMA_POLICY_LOGGED: AtomicBool = AtomicBool::new(false);
static XHCI_DIAG_LINE_COUNT: AtomicU32 = AtomicU32::new(0);
static XHCI_DIAG_LAST_STAGE: AtomicU32 = AtomicU32::new(0);
static XHCI_DIAG_LAST_A: AtomicUsize = AtomicUsize::new(0);
static XHCI_DIAG_LAST_B: AtomicUsize = AtomicUsize::new(0);
static XHCI_DIAG_LAST_C: AtomicUsize = AtomicUsize::new(0);
static VL805_CFG_VIRT: AtomicUsize = AtomicUsize::new(0);
static VL805_XHCI_MMIO_HINT: AtomicUsize = AtomicUsize::new(0);
static LATEST_USB_PROBE_ROUTE: Mutex<Option<UsbProbePathwaySummary>> = Mutex::new(None);
static PINNED_XHCI_MMIO: Mutex<Option<PinnedMmioWindow>> = Mutex::new(None);
static PINNED_VL805_CFG: Mutex<Option<PinnedMmioWindow>> = Mutex::new(None);
static USB_PROGRESS_ACTIVE: AtomicBool = AtomicBool::new(false);
static USB_PROGRESS_DISPLAY_PTR: AtomicUsize = AtomicUsize::new(0);
static USB_PROGRESS_ELAPSED_MS: AtomicUsize = AtomicUsize::new(0);
static USB_PROGRESS_DOTS: AtomicUsize = AtomicUsize::new(0);
static WIFI_PROGRESS_ACTIVE: AtomicBool = AtomicBool::new(false);
static WIFI_PROGRESS_DISPLAY_PTR: AtomicUsize = AtomicUsize::new(0);
static WIFI_PROGRESS_LOOP_BUDGET: AtomicUsize = AtomicUsize::new(0);
static WIFI_PROGRESS_TICKS: AtomicUsize = AtomicUsize::new(0);

#[inline]
const fn xhci_runtime_init_strategy_prompt_safe(strategy: XhciRuntimeInitStrategy) -> bool {
    match strategy.firmware_handoff {
        // Manual probes may take the reset-owned U-Boot-shaped path, but that
        // lane is gated by the bounded PCIe bridge + VL805 INTx sinks before
        // any runtime ownership touch.
        XhciFirmwareHandoff::ColdStartFromSnapshot => true,
        XhciFirmwareHandoff::ResetlessReinit | XhciFirmwareHandoff::PreserveControllerState => true,
        XhciFirmwareHandoff::None => strategy.seed_stop_state,
    }
}

#[inline]
const fn xhci_dboff_offset(raw: u32) -> u32 {
    raw & XHCI_DBOFF_MASK
}

#[inline]
const fn xhci_rtsoff_offset(raw: u32) -> u32 {
    raw & XHCI_RTSOFF_MASK
}

const XHCI_DIAG_MAX_LINES: u32 = 160;
const VL805_RUNTIME_RESET_STATE_UNATTEMPTED: u8 = 0;
const VL805_RUNTIME_RESET_STATE_NOTIFIED: u8 = 1;
const VL805_RUNTIME_RESET_STATE_SOFT_CONTINUE: u8 = 2;
const VL805_RUNTIME_RESET_STATE_HARD_MAP: u8 = 3;
const VL805_RUNTIME_RESET_STATE_HARD_DMA: u8 = 4;
const VL805_RUNTIME_RESET_STATE_HARD_TIMEOUT: u8 = 5;
const VL805_RUNTIME_RESET_STATE_HARD_PROTOCOL: u8 = 6;
const VL805_RUNTIME_RESET_STATE_POSTED_FALLBACK: u8 = 7;
const VL805_RUNTIME_RESET_STATE_BOOTLOADER_AUTHORIZED: u8 = 8;

fn usb_progress_emit(dots: usize) {
    let ptr = USB_PROGRESS_DISPLAY_PTR.load(Ordering::Acquire);
    if ptr == 0 {
        return;
    }
    let mut line = heapless::String::<160>::new();
    let _ = line.push_str("[cohesix] starting USB subsystem");
    for _ in 0..dots.min(USB_PROGRESS_MAX_DOTS) {
        let _ = line.push('.');
    }
    // SAFETY: The pointer is only published while Pi4LocalSeat::poll_keyboard_bytes
    // runs synchronously on the same runtime path and is cleared on completion.
    unsafe {
        (&mut *(ptr as *mut HdmiTextSink)).write_line(line.as_str());
    }
}

fn usb_progress_begin(display: &mut HdmiTextSink) {
    USB_PROGRESS_ELAPSED_MS.store(0, Ordering::Release);
    USB_PROGRESS_DOTS.store(1, Ordering::Release);
    USB_PROGRESS_DISPLAY_PTR.store(display as *mut _ as usize, Ordering::Release);
    USB_PROGRESS_ACTIVE.store(true, Ordering::Release);
    usb_progress_emit(1);
}

fn usb_progress_advance(ms: u64) {
    if !USB_PROGRESS_ACTIVE.load(Ordering::Acquire) {
        return;
    }
    let ms_usize = ms as usize;
    let previous = USB_PROGRESS_ELAPSED_MS.fetch_add(ms_usize, Ordering::AcqRel);
    let now = previous.saturating_add(ms_usize);
    let target_dots = now / USB_PROGRESS_TICK_MS + 1;
    let current_dots = USB_PROGRESS_DOTS.load(Ordering::Acquire);
    if target_dots > current_dots {
        USB_PROGRESS_DOTS.store(target_dots, Ordering::Release);
        usb_progress_emit(target_dots);
    }
}

fn usb_progress_finish() {
    USB_PROGRESS_ACTIVE.store(false, Ordering::Release);
    USB_PROGRESS_DISPLAY_PTR.store(0, Ordering::Release);
}

fn wifi_progress_dot_count(emissions: usize) -> usize {
    if emissions == 0 {
        0
    } else {
        ((emissions - 1) % WIFI_PROGRESS_MAX_DOTS) + 1
    }
}

fn wifi_progress_dots_for_ticks(ticks: usize) -> usize {
    wifi_progress_dot_count(ticks / WIFI_PROGRESS_EMIT_INTERVAL_TICKS)
}

fn wifi_progress_emit(dots: usize) {
    let ptr = WIFI_PROGRESS_DISPLAY_PTR.load(Ordering::Acquire);
    if ptr == 0 {
        return;
    }
    let mut line = heapless::String::<160>::new();
    let _ = line.push_str("[cohesix] Initializing WiFi");
    for _ in 0..dots.min(WIFI_PROGRESS_MAX_DOTS) {
        let _ = line.push('.');
    }
    // SAFETY: The pointer is published only after the local-seat runtime has
    // moved into its final leaked storage, so the HDMI sink address remains
    // stable for the rest of boot.
    unsafe {
        (&mut *(ptr as *mut HdmiTextSink)).write_line(line.as_str());
    }
}

fn register_wifi_progress_display(display: &mut HdmiTextSink) {
    WIFI_PROGRESS_DISPLAY_PTR.store(display as *mut _ as usize, Ordering::Release);
    if WIFI_PROGRESS_ACTIVE.load(Ordering::Acquire) {
        let ticks = WIFI_PROGRESS_TICKS.load(Ordering::Acquire);
        wifi_progress_emit(wifi_progress_dots_for_ticks(ticks));
    }
}

pub(crate) fn wifi_progress_begin() {
    WIFI_PROGRESS_LOOP_BUDGET.store(0, Ordering::Release);
    WIFI_PROGRESS_TICKS.store(0, Ordering::Release);
    WIFI_PROGRESS_ACTIVE.store(true, Ordering::Release);
    wifi_progress_emit(0);
}

pub(crate) fn wifi_progress_tick() {
    if !WIFI_PROGRESS_ACTIVE.load(Ordering::Acquire) {
        return;
    }
    let tick = WIFI_PROGRESS_TICKS.fetch_add(1, Ordering::AcqRel) + 1;
    if tick % WIFI_PROGRESS_EMIT_INTERVAL_TICKS == 0 {
        wifi_progress_emit(wifi_progress_dots_for_ticks(tick));
    }
}

pub(crate) fn wifi_progress_advance_loops(loops: usize) {
    if loops == 0 || !WIFI_PROGRESS_ACTIVE.load(Ordering::Acquire) {
        return;
    }
    let previous = WIFI_PROGRESS_LOOP_BUDGET.fetch_add(loops, Ordering::AcqRel);
    let current = previous.saturating_add(loops);
    let previous_ticks = previous / WIFI_PROGRESS_LOOP_TICK_LOOPS;
    let current_ticks = current / WIFI_PROGRESS_LOOP_TICK_LOOPS;
    for _ in previous_ticks..current_ticks {
        wifi_progress_tick();
    }
}

pub(crate) fn wifi_progress_finish() {
    WIFI_PROGRESS_ACTIVE.store(false, Ordering::Release);
}

#[inline]
fn wait_ms(ms: u64) {
    if ms == 0 {
        return;
    }
    // Keep hub bring-up delays finite and platform-independent. Accessing
    // architectural timer registers on some Pi4 boot paths can trap/freeze.
    let spins = (ms as usize)
        .saturating_mul(WAIT_MS_SPINS_PER_MS)
        .clamp(WAIT_MS_MIN_SPINS, WAIT_MS_MAX_SPINS);
    for _ in 0..spins {
        spin_loop();
    }
    usb_progress_advance(ms);
}

struct PinnedMmioWindow {
    phys_start: usize,
    length: usize,
    virt_start: usize,
    trusted_for_runtime: bool,
}

/// Optional DT/firmware framebuffer hint for Pi4 HDMI output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pi4FramebufferHint {
    /// Physical base of the framebuffer allocation.
    pub paddr: usize,
    /// Visible width in pixels.
    pub width: usize,
    /// Visible height in pixels.
    pub height: usize,
    /// Bytes per rendered scanline.
    pub pitch: usize,
}

/// Optional platform hints for Pi4 local-seat attachment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Pi4LocalSeatHints {
    /// Optional MMIO base for Pi4 xHCI.
    pub xhci_mmio_hint: Option<usize>,
    /// Optional PCI command state exported by the bootloader for xHCI handoff.
    pub xhci_pci_cmd: Option<u16>,
    /// Whether the bootloader marked the Pi4 xHCI BAR ready for local-seat handoff.
    pub xhci_handoff_ready: bool,
    /// Whether the bootloader masked legacy/MSI/MSI-X interrupt delivery before handoff.
    pub xhci_irq_quiesced: bool,
    /// Whether bootloader post-stop evidence authorizes runtime VL805 reset ownership.
    pub xhci_bootloader_reset_authorized: bool,
    /// Optional xHCI capability snapshot exported by the bootloader handoff.
    pub xhci_capability_snapshot: Option<LocalSeatXhciCapabilitySnapshot>,
    /// Optional xHCI stop-state snapshot exported by the bootloader handoff.
    pub xhci_stop_state_snapshot: Option<LocalSeatXhciStopStateSnapshot>,
    /// Optional DT/firmware framebuffer hint.
    pub framebuffer_hint: Option<Pi4FramebufferHint>,
}

/// Pi4 local-seat backend errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pi4SeatError {
    MailboxMap,
    MailboxDma,
    MailboxProtocol,
    MailboxTimeout,
    FramebufferUnavailable,
    FramebufferMap,
    XhciInit,
    UsbKeyboardMissing,
    UsbKeyboardInit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct XhciDiagSnapshot {
    line_count: usize,
    stage: u16,
    a: u64,
    b: u64,
    c: u64,
}

impl XhciDiagSnapshot {
    #[inline]
    const fn empty() -> Self {
        Self {
            line_count: 0,
            stage: 0,
            a: 0,
            b: 0,
            c: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct UsbXhciDiagStatus {
    pub stage: u16,
    pub tag: Option<&'static str>,
    pub exact_issue: Option<&'static str>,
    pub a: u64,
    pub b: u64,
    pub c: u64,
    pub value_labels: Option<(&'static str, &'static str, &'static str)>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct UsbProbeRouteStatus {
    pub route: &'static str,
    pub pathway_idx: usize,
    pub strategy_idx: usize,
    pub strategy_count: usize,
    pub policy: &'static str,
    pub origin: &'static str,
    pub handoff: &'static str,
    pub seed: &'static str,
    pub halt_guard: &'static str,
    pub current_step: &'static str,
    pub next_step: &'static str,
    pub progress: &'static str,
    pub outcome: &'static str,
    pub prefer_high: bool,
    pub pcie_dma_window: bool,
    pub poll_only: bool,
    pub port: Option<u8>,
    pub connected_mask: u32,
    pub detect_passes: usize,
    pub slow_recheck: bool,
    pub irq27_bound: bool,
    pub bridge_irq_bound: bool,
    pub intx_irq_bound: bool,
    pub controller_gate: &'static str,
    pub diag_fresh: bool,
    pub diag_stage: Option<u16>,
    pub diag_tag: Option<&'static str>,
    pub diag_exact: Option<&'static str>,
    pub diag_a: u64,
    pub diag_b: u64,
    pub diag_c: u64,
    pub diag_value_labels: Option<(&'static str, &'static str, &'static str)>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct UsbProbePreflightStatus {
    pub route: &'static str,
    pub strategy_idx: usize,
    pub strategy_count: usize,
    pub policy: &'static str,
    pub origin: &'static str,
    pub handoff: &'static str,
    pub seed: &'static str,
    pub halt_guard: &'static str,
    pub constructor: &'static str,
    pub pre_reset: &'static str,
    pub legacy: &'static str,
    pub run: &'static str,
    pub publish: &'static str,
    pub post_ready_irq: &'static str,
    pub current_step: &'static str,
    pub next_step: &'static str,
    pub followup_step: &'static str,
    pub prefer_high: bool,
    pub pcie_dma_window: bool,
    pub poll_only: bool,
    pub expected_diag_stage: u16,
    pub expected_diag_tag: Option<&'static str>,
    pub expected_diag_exact: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum UsbProbePathProgress {
    NoController,
    ControllerReady,
    RootPortConnected,
    DeviceAddressed,
    DeviceDescriptor,
    ConfigDescriptor,
    ConfigParsed,
    DeviceConfigured,
    KeyboardReady,
}

impl UsbProbePathProgress {
    #[inline]
    const fn as_str(self) -> &'static str {
        match self {
            Self::NoController => "no-controller",
            Self::ControllerReady => "controller-ready",
            Self::RootPortConnected => "root-port-connected",
            Self::DeviceAddressed => "device-addressed",
            Self::DeviceDescriptor => "device-desc",
            Self::ConfigDescriptor => "config-desc",
            Self::ConfigParsed => "config-parsed",
            Self::DeviceConfigured => "device-configured",
            Self::KeyboardReady => "keyboard-ready",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UsbProbePathOutcome {
    Pending,
    ControllerInitFailed,
    EnumerationDisabledBootloaderOwned,
    NoConnectedPorts,
    AddressFailed,
    DeviceDescFailed,
    ConfigDescFailed,
    ConfigParseFailed,
    InvalidConfigValue,
    SetConfigFailed,
    HidInitFailed,
    NoKeyboardFound,
    KeyboardReady,
}

impl UsbProbePathOutcome {
    #[inline]
    const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::ControllerInitFailed => "controller-init-failed",
            Self::EnumerationDisabledBootloaderOwned => "enumeration-disabled-bootloader-owned",
            Self::NoConnectedPorts => "no-connected-ports",
            Self::AddressFailed => "address-failed",
            Self::DeviceDescFailed => "device-desc-failed",
            Self::ConfigDescFailed => "config-desc-failed",
            Self::ConfigParseFailed => "config-parse-failed",
            Self::InvalidConfigValue => "invalid-config-value",
            Self::SetConfigFailed => "set-config-failed",
            Self::HidInitFailed => "hid-init-failed",
            Self::NoKeyboardFound => "no-keyboard-found",
            Self::KeyboardReady => "keyboard-ready",
        }
    }

    #[inline]
    const fn tie_priority(self) -> u8 {
        match self {
            Self::Pending => 0,
            Self::EnumerationDisabledBootloaderOwned => 1,
            Self::NoConnectedPorts => 2,
            Self::AddressFailed => 3,
            Self::DeviceDescFailed => 4,
            Self::ConfigDescFailed => 5,
            Self::ConfigParseFailed => 6,
            Self::InvalidConfigValue => 7,
            Self::SetConfigFailed => 8,
            Self::NoKeyboardFound => 9,
            Self::HidInitFailed => 10,
            Self::ControllerInitFailed => 11,
            Self::KeyboardReady => 12,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct UsbProbePathwaySummary {
    pathway_idx: usize,
    strategy_idx: usize,
    strategy_count: usize,
    policy: &'static str,
    origin: &'static str,
    handoff: &'static str,
    seed: &'static str,
    halt_guard: &'static str,
    prefer_high: bool,
    pcie_dma_window: bool,
    poll_only: bool,
    progress: UsbProbePathProgress,
    outcome: UsbProbePathOutcome,
    port: Option<u8>,
    connected_mask: u32,
    detect_passes: usize,
    slow_recheck: bool,
    irq27_bound: bool,
    bridge_irq_bound: bool,
    intx_irq_bound: bool,
    controller_gate: &'static str,
    diag: XhciDiagSnapshot,
    diag_fresh: bool,
}

impl UsbProbePathwaySummary {
    #[inline]
    const fn new(
        pathway_idx: usize,
        strategy_idx: usize,
        strategy_count: usize,
        policy: &'static str,
        origin: &'static str,
        handoff: &'static str,
        seed: &'static str,
        halt_guard: &'static str,
        prefer_high: bool,
        pcie_dma_window: bool,
        poll_only: bool,
    ) -> Self {
        Self {
            pathway_idx,
            strategy_idx,
            strategy_count,
            policy,
            origin,
            handoff,
            seed,
            halt_guard,
            prefer_high,
            pcie_dma_window,
            poll_only,
            progress: UsbProbePathProgress::NoController,
            outcome: UsbProbePathOutcome::Pending,
            port: None,
            connected_mask: 0,
            detect_passes: 0,
            slow_recheck: false,
            irq27_bound: false,
            bridge_irq_bound: false,
            intx_irq_bound: false,
            controller_gate: "none",
            diag: XhciDiagSnapshot::empty(),
            diag_fresh: false,
        }
    }

    #[inline]
    fn is_better_than(self, other: Self) -> bool {
        if matches!(self.outcome, UsbProbePathOutcome::Pending) {
            return false;
        }
        if matches!(other.outcome, UsbProbePathOutcome::Pending) {
            return true;
        }
        if self.progress != other.progress {
            return self.progress > other.progress;
        }
        let self_priority = self.outcome.tie_priority();
        let other_priority = other.outcome.tie_priority();
        if self_priority != other_priority {
            return self_priority > other_priority;
        }
        let self_connected = self.connected_mask.count_ones();
        let other_connected = other.connected_mask.count_ones();
        if self_connected != other_connected {
            return self_connected > other_connected;
        }
        if self.diag_fresh != other.diag_fresh {
            return self.diag_fresh;
        }
        false
    }
}

impl Pi4SeatError {
    /// Stable diagnostic token for boot/audit logs.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MailboxMap => "mailbox-map",
            Self::MailboxDma => "mailbox-dma",
            Self::MailboxProtocol => "mailbox-protocol",
            Self::MailboxTimeout => "mailbox-timeout",
            Self::FramebufferUnavailable => "framebuffer-unavailable",
            Self::FramebufferMap => "framebuffer-map",
            Self::XhciInit => "xhci-init",
            Self::UsbKeyboardMissing => "usb-keyboard-missing",
            Self::UsbKeyboardInit => "usb-keyboard-init",
        }
    }
}

#[inline]
const fn keyboard_attach_retry_allowed(
    err: Pi4SeatError,
    attempt: usize,
    max_attempts: usize,
) -> bool {
    attempt < max_attempts && !matches!(err, Pi4SeatError::XhciInit)
}

#[inline]
fn usb_probe_pathway_record(
    summary: &mut UsbProbePathwaySummary,
    progress: UsbProbePathProgress,
    outcome: UsbProbePathOutcome,
    port: Option<u8>,
    connected_mask: u32,
    detect_passes: usize,
    slow_recheck: bool,
    diag: XhciDiagSnapshot,
    diag_fresh: bool,
) {
    let candidate = UsbProbePathwaySummary {
        progress,
        outcome,
        port,
        connected_mask,
        detect_passes,
        slow_recheck,
        diag,
        diag_fresh,
        ..*summary
    };
    if candidate.is_better_than(*summary) {
        *summary = candidate;
    }
}

#[inline]
fn usb_probe_best_pathway_update(
    best: &mut Option<UsbProbePathwaySummary>,
    candidate: UsbProbePathwaySummary,
) {
    if matches!(candidate.outcome, UsbProbePathOutcome::Pending) {
        return;
    }
    if best.is_none_or(|current| candidate.is_better_than(current)) {
        *best = Some(candidate);
    }
}

#[inline]
fn remember_latest_usb_probe_route(summary: &UsbProbePathwaySummary) {
    *LATEST_USB_PROBE_ROUTE.lock() = Some(*summary);
}

#[inline]
fn usb_probe_route_label(summary: UsbProbePathwaySummary) -> &'static str {
    match (summary.handoff, summary.origin) {
        ("none", "resetless-stop-seed") => "trusted-high-bar-stop-seed-primary",
        ("cold-start-from-snapshot", "uboot-fresh-init") => "trusted-high-bar-primary",
        ("cold-start-from-snapshot", "seeded-cold-start") => "trusted-high-bar-seeded-retry",
        ("preserve-controller-state", "stop-state-preserve") => "stop-state-preserve-fallback",
        ("resetless-reinit", "stop-state-resetless-reinit") => "stop-state-resetless-fallback",
        _ => "diagnostic-fallback",
    }
}

#[inline]
fn usb_probe_current_step(summary: UsbProbePathwaySummary) -> &'static str {
    match summary.progress {
        UsbProbePathProgress::NoController if summary.diag.stage == 0x0224 => {
            "skip-stop-revalidation"
        }
        UsbProbePathProgress::NoController => "controller-init",
        UsbProbePathProgress::ControllerReady => "controller-ready",
        UsbProbePathProgress::RootPortConnected => "root-port-detect",
        UsbProbePathProgress::DeviceAddressed => "device-addressed",
        UsbProbePathProgress::DeviceDescriptor => "device-descriptor",
        UsbProbePathProgress::ConfigDescriptor => "config-descriptor",
        UsbProbePathProgress::ConfigParsed => "config-parse",
        UsbProbePathProgress::DeviceConfigured => "device-configure",
        UsbProbePathProgress::KeyboardReady => "keyboard-ready",
    }
}

#[inline]
fn usb_probe_next_step(summary: UsbProbePathwaySummary) -> &'static str {
    match summary.progress {
        UsbProbePathProgress::ControllerReady
            if matches!(
                summary.outcome,
                UsbProbePathOutcome::EnumerationDisabledBootloaderOwned
            ) =>
        {
            "return-to-shell"
        }
        UsbProbePathProgress::NoController
            if summary.diag.stage == 0x0224 && summary.origin == "resetless-stop-seed" =>
        {
            "skip-reset"
        }
        UsbProbePathProgress::NoController if summary.diag.stage == 0x0224 => "reset-post-settle",
        UsbProbePathProgress::NoController => "controller-ready",
        UsbProbePathProgress::ControllerReady => "root-port-detect",
        UsbProbePathProgress::RootPortConnected => "device-addressed",
        UsbProbePathProgress::DeviceAddressed => "device-descriptor",
        UsbProbePathProgress::DeviceDescriptor => "config-descriptor",
        UsbProbePathProgress::ConfigDescriptor => "config-parse",
        UsbProbePathProgress::ConfigParsed => "device-configure",
        UsbProbePathProgress::DeviceConfigured => "keyboard-ready",
        UsbProbePathProgress::KeyboardReady => "none",
    }
}

#[inline]
const fn usb_probe_preflight_current_step() -> &'static str {
    "pre-controller-ready"
}

#[inline]
const fn usb_probe_preflight_next_step(strategy: XhciRuntimeInitStrategy) -> &'static str {
    if xhci_runtime_init_strategy_skips_controller_entry(strategy) {
        "policy-return"
    } else if xhci_runtime_init_strategy_skips_live_halt_read(strategy) {
        "skip-pre-reset"
    } else {
        "pre-reset-scrub"
    }
}

#[inline]
const fn usb_probe_preflight_followup_step(strategy: XhciRuntimeInitStrategy) -> &'static str {
    if xhci_runtime_init_strategy_skips_controller_entry(strategy) {
        "return-to-shell"
    } else if strategy.seed_stop_state
        && matches!(strategy.firmware_handoff, XhciFirmwareHandoff::None)
    {
        "ring-publish"
    } else if strategy.seed_stop_state
        && matches!(
            strategy.firmware_handoff,
            XhciFirmwareHandoff::ColdStartFromSnapshot
        )
    {
        "skip-reset"
    } else {
        "reset-post-settle"
    }
}

#[inline]
const fn usb_probe_preflight_expected_diag_stage(strategy: XhciRuntimeInitStrategy) -> u16 {
    if xhci_runtime_init_strategy_skips_controller_entry(strategy) {
        0
    } else if xhci_runtime_init_strategy_skips_live_halt_read(strategy) {
        0x0204
    } else {
        0x0200
    }
}

#[inline]
fn usb_probe_preflight_status(
    xhci_mmio_hint: Option<usize>,
    xhci_pci_cmd: Option<u16>,
    xhci_handoff_ready: bool,
    xhci_irq_quiesced: bool,
    xhci_bootloader_reset_authorized: bool,
    stop_state_snapshot: Option<LocalSeatXhciStopStateSnapshot>,
    prompt_safe_probe: bool,
) -> Option<UsbProbePreflightStatus> {
    let mut runtime_vl805_reset_state = VL805_RUNTIME_RESET_STATE.load(Ordering::Acquire);
    if matches!(
        runtime_vl805_reset_state,
        VL805_RUNTIME_RESET_STATE_UNATTEMPTED | VL805_RUNTIME_RESET_STATE_SOFT_CONTINUE
    ) && xhci_bootloader_vl805_reset_authorized(
        RPI4_XHCI_MMIO_HIGH_CANDIDATE,
        xhci_mmio_hint,
        xhci_pci_cmd,
        xhci_handoff_ready,
        xhci_irq_quiesced,
        xhci_bootloader_reset_authorized,
        stop_state_snapshot,
    ) {
        runtime_vl805_reset_state = VL805_RUNTIME_RESET_STATE_BOOTLOADER_AUTHORIZED;
    }
    let preferred_handoff = if xhci_firmware_handoff_cold_start_trusted(
        RPI4_XHCI_MMIO_HIGH_CANDIDATE,
        xhci_mmio_hint,
        xhci_pci_cmd,
        xhci_handoff_ready,
        xhci_irq_quiesced,
    ) {
        xhci_preferred_trusted_handoff_mode(runtime_vl805_reset_state)
    } else {
        XhciFirmwareHandoff::None
    };
    let effective_mmio = xhci_mmio_hint.unwrap_or(RPI4_XHCI_MMIO_PRIMARY_CANDIDATE);
    let (strategies, strategy_count) = xhci_runtime_init_strategies(
        preferred_handoff,
        runtime_vl805_reset_state,
        stop_state_snapshot,
    );
    let (strategy_idx, strategy) = strategies[..strategy_count]
        .iter()
        .copied()
        .enumerate()
        .find(|(_, strategy)| {
            !prompt_safe_probe || xhci_runtime_init_strategy_prompt_safe(*strategy)
        })?;
    let summary = UsbProbePathwaySummary::new(
        1,
        strategy_idx + 1,
        strategy_count,
        xhci_runtime_init_strategy_policy_label(strategy),
        xhci_runtime_init_strategy_origin_label(strategy),
        xhci_firmware_handoff_mode_label(strategy.firmware_handoff),
        xhci_runtime_init_strategy_seed_label(strategy),
        xhci_runtime_init_strategy_halt_guard_label(strategy),
        false,
        XHCI_PCIE_DMA_WINDOW_ENABLED,
        xhci_polling_only_runtime(effective_mmio, strategy.firmware_handoff),
    );
    let expected_diag_stage = usb_probe_preflight_expected_diag_stage(strategy);
    Some(UsbProbePreflightStatus {
        route: usb_probe_route_label(summary),
        strategy_idx: strategy_idx + 1,
        strategy_count,
        policy: summary.policy,
        origin: summary.origin,
        handoff: summary.handoff,
        seed: summary.seed,
        halt_guard: summary.halt_guard,
        constructor: xhci_runtime_init_strategy_constructor_label(strategy),
        pre_reset: xhci_runtime_init_strategy_pre_reset_label(strategy),
        legacy: xhci_runtime_init_strategy_legacy_label(strategy),
        run: xhci_runtime_init_strategy_run_label(strategy),
        publish: xhci_runtime_init_strategy_publish_label(strategy),
        post_ready_irq: xhci_runtime_init_strategy_post_ready_irq_label(strategy),
        current_step: usb_probe_preflight_current_step(),
        next_step: usb_probe_preflight_next_step(strategy),
        followup_step: usb_probe_preflight_followup_step(strategy),
        prefer_high: summary.prefer_high,
        pcie_dma_window: summary.pcie_dma_window,
        poll_only: summary.poll_only,
        expected_diag_stage,
        expected_diag_tag: xhci_diag_stage_label(expected_diag_stage),
        expected_diag_exact: xhci_diag_stage_exact_issue_label(expected_diag_stage),
    })
}

#[inline]
const fn runtime_vl805_mailbox_reset_error_allows_cold_init(err: Pi4SeatError) -> bool {
    matches!(
        err,
        Pi4SeatError::MailboxMap
            | Pi4SeatError::MailboxDma
            | Pi4SeatError::MailboxTimeout
            | Pi4SeatError::MailboxProtocol
    )
}

#[inline]
const fn runtime_vl805_mailbox_reset_failure_state(err: Pi4SeatError) -> u8 {
    match err {
        Pi4SeatError::MailboxMap => VL805_RUNTIME_RESET_STATE_HARD_MAP,
        Pi4SeatError::MailboxDma => VL805_RUNTIME_RESET_STATE_HARD_DMA,
        Pi4SeatError::MailboxTimeout => VL805_RUNTIME_RESET_STATE_HARD_TIMEOUT,
        Pi4SeatError::MailboxProtocol => VL805_RUNTIME_RESET_STATE_HARD_PROTOCOL,
        _ => VL805_RUNTIME_RESET_STATE_UNATTEMPTED,
    }
}

#[inline]
const fn runtime_vl805_mailbox_reset_success_detail(
    result: pi4_wifi::Vl805ResetNotifyResult,
) -> &'static str {
    match result {
        pi4_wifi::Vl805ResetNotifyResult::Acked => "mailbox-notify+settle",
        pi4_wifi::Vl805ResetNotifyResult::PostedFallback => "mailbox-posted-fallback+settle",
    }
}

#[inline]
const fn runtime_vl805_mailbox_reset_success_settle_ms(
    result: pi4_wifi::Vl805ResetNotifyResult,
) -> u64 {
    match result {
        pi4_wifi::Vl805ResetNotifyResult::Acked => VL805_MAILBOX_RESET_SETTLE_MS,
        pi4_wifi::Vl805ResetNotifyResult::PostedFallback => VL805_MAILBOX_RESET_POSTED_SETTLE_MS,
    }
}

#[inline]
const fn runtime_vl805_mailbox_reset_success_state(result: pi4_wifi::Vl805ResetNotifyResult) -> u8 {
    match result {
        pi4_wifi::Vl805ResetNotifyResult::Acked => VL805_RUNTIME_RESET_STATE_NOTIFIED,
        pi4_wifi::Vl805ResetNotifyResult::PostedFallback => {
            VL805_RUNTIME_RESET_STATE_POSTED_FALLBACK
        }
    }
}

#[inline]
const fn runtime_vl805_mailbox_reset_completed(state: u8) -> bool {
    matches!(state, VL805_RUNTIME_RESET_STATE_NOTIFIED)
}

#[inline]
const fn runtime_vl805_mailbox_reset_authorizes_hcrst(state: u8) -> bool {
    matches!(state, VL805_RUNTIME_RESET_STATE_NOTIFIED)
}

#[inline]
const fn runtime_vl805_mailbox_reset_allows_trusted_cold_init(state: u8) -> bool {
    matches!(
        state,
        VL805_RUNTIME_RESET_STATE_NOTIFIED | VL805_RUNTIME_RESET_STATE_BOOTLOADER_AUTHORIZED
    )
}

#[inline]
const fn runtime_vl805_mailbox_reset_trusted_cold_init_detail(state: u8) -> &'static str {
    match state {
        VL805_RUNTIME_RESET_STATE_NOTIFIED => "mailbox-reset+trusted-cap-snapshot",
        VL805_RUNTIME_RESET_STATE_BOOTLOADER_AUTHORIZED => {
            "bootloader-reset-authorized+no-touch-cap-snapshot"
        }
        VL805_RUNTIME_RESET_STATE_POSTED_FALLBACK => "mailbox-posted-fallback+no-runtime-ownership",
        VL805_RUNTIME_RESET_STATE_SOFT_CONTINUE => "mailbox-soft-continue+no-runtime-ownership",
        _ => "mailbox-reset-unconfirmed+trusted-cap-snapshot",
    }
}

#[inline]
const fn runtime_vl805_mailbox_reset_state_label(state: u8) -> &'static str {
    match state {
        VL805_RUNTIME_RESET_STATE_UNATTEMPTED => "unattempted",
        VL805_RUNTIME_RESET_STATE_NOTIFIED => "mailbox-acked",
        VL805_RUNTIME_RESET_STATE_SOFT_CONTINUE => "soft-failure",
        VL805_RUNTIME_RESET_STATE_HARD_MAP => "hard-map",
        VL805_RUNTIME_RESET_STATE_HARD_DMA => "hard-dma",
        VL805_RUNTIME_RESET_STATE_HARD_TIMEOUT => "hard-timeout",
        VL805_RUNTIME_RESET_STATE_HARD_PROTOCOL => "hard-protocol",
        VL805_RUNTIME_RESET_STATE_POSTED_FALLBACK => "posted-fallback",
        VL805_RUNTIME_RESET_STATE_BOOTLOADER_AUTHORIZED => "bootloader-authorized",
        _ => "unknown",
    }
}

#[inline]
const fn runtime_vl805_mailbox_reset_handoff_label(state: u8) -> &'static str {
    match state {
        VL805_RUNTIME_RESET_STATE_NOTIFIED => "runtime-owned",
        VL805_RUNTIME_RESET_STATE_BOOTLOADER_AUTHORIZED => "bootloader-owned",
        _ => "runtime-unconfirmed",
    }
}

/// Concrete local-seat backend for Pi 4 (HDMI text + USB keyboard).
pub struct Pi4LocalSeat {
    display: HdmiTextSink,
    keyboard: Option<UsbKeyboard>,
    keyboard_init_attempted: bool,
    prompt_safe_probe_armed: bool,
    xhci_mmio_hint: Option<usize>,
    xhci_pci_cmd: Option<u16>,
    xhci_handoff_ready: bool,
    xhci_irq_quiesced: bool,
    xhci_bootloader_reset_authorized: bool,
    xhci_capability_snapshot: Option<LocalSeatXhciCapabilitySnapshot>,
    xhci_stop_state_snapshot: Option<LocalSeatXhciStopStateSnapshot>,
    xhci_capability_probe: Option<XhciCapProbe>,
    hal_ptr: usize,
}

impl core::fmt::Debug for Pi4LocalSeat {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Pi4LocalSeat").finish_non_exhaustive()
    }
}

impl Pi4LocalSeat {
    /// Initialize the Pi4 local-seat backend.
    pub fn new(hal: &mut KernelHal<'_>, hints: Pi4LocalSeatHints) -> Result<Self, Pi4SeatError> {
        let xhci_mmio_hint = normalize_pi4_xhci_mmio_hint(hints.xhci_mmio_hint);
        let xhci_capability_probe = hints
            .xhci_capability_snapshot
            .and_then(cache_xhci_capability_probe_from_snapshot);
        if let (Some(raw_hint), Some(normalized_hint)) = (hints.xhci_mmio_hint, xhci_mmio_hint) {
            if raw_hint != normalized_hint {
                let mut line = heapless::String::<144>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] xhci hint normalized raw=0x{raw:016x} phys=0x{phys:016x}",
                        raw = raw_hint,
                        phys = normalized_hint
                    ),
                );
                boot_log::force_uart_line(line.as_str());
            }
        }
        boot_log::force_uart_line("[local-seat] pi4 display init begin");
        let mut display = HdmiTextSink::new(hal, hints.framebuffer_hint)?;
        boot_log::force_uart_line("[local-seat] pi4 display init ok");

        let mut display_line = heapless::String::<128>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut display_line,
            format_args!(
                "[local-seat] pi4 display backend={}",
                display.backend_label()
            ),
        );
        boot_log::force_uart_line(display_line.as_str());
        display.write_line("[cohesix] local-seat HDMI online");
        boot_log::force_uart_line("[local-seat] pi4 hdmi banner emitted");

        boot_log::force_uart_line("[local-seat] pi4 keyboard init deferred to runtime");
        Ok(Self {
            display,
            keyboard: None,
            keyboard_init_attempted: false,
            prompt_safe_probe_armed: false,
            xhci_mmio_hint,
            xhci_pci_cmd: hints.xhci_pci_cmd,
            xhci_handoff_ready: hints.xhci_handoff_ready,
            xhci_irq_quiesced: hints.xhci_irq_quiesced,
            xhci_bootloader_reset_authorized: hints.xhci_bootloader_reset_authorized,
            xhci_capability_snapshot: hints.xhci_capability_snapshot,
            xhci_stop_state_snapshot: hints.xhci_stop_state_snapshot,
            xhci_capability_probe,
            hal_ptr: hal as *mut _ as usize,
        })
    }

    /// Preseed keyboard MMIO windows once UART bring-up has completed.
    pub fn preseed_keyboard_mmio(&mut self) {
        let Some(hal) = hal_from_ptr(self.hal_ptr) else {
            return;
        };
        let first_preseed = !KEYBOARD_PRESEED_LOGGED.swap(true, Ordering::AcqRel);
        let cfg_preseed_mode = vl805_cfg_preseed_mode(VL805_CFG_PRESEED_TOUCH_ENABLED);
        let cfg_preseed_needed = vl805_cfg_preseed_needed(
            VL805_CFG_PRESEED_TOUCH_ENABLED,
            VL805_CFG_RUNTIME_TOUCH_ENABLED,
        );
        if first_preseed {
            boot_log::force_uart_line("[local-seat] pi4 keyboard preseed begin");
        }
        if first_preseed {
            boot_log::force_uart_line("[local-seat] pi4 xhci preseed begin");
        }
        // Reserve the handed-off xHCI BAR first. The VL805 ECAM page lives at a
        // higher physical address inside the same device-untyped aperture, so
        // pinning config space first can advance the device cursor past the BAR
        // and make the lower handed-off page appear uncovered.
        prime_pinned_xhci_window(
            hal,
            self.xhci_mmio_hint,
            self.xhci_pci_cmd,
            self.xhci_handoff_ready,
            self.xhci_irq_quiesced,
        );
        log_xhci_handoff_window_state(hal, self.xhci_mmio_hint, self.xhci_pci_cmd);
        if cfg_preseed_needed {
            prime_pinned_vl805_cfg_window(hal, cfg_preseed_mode);
        } else if first_preseed {
            boot_log::force_uart_line(
                "[local-seat] vl805 pci cfg preseed deferred reason=safe-mode-runtime-discovery-disabled",
            );
        }
        if cfg_preseed_needed && matches!(cfg_preseed_mode, Vl805CfgPreseedMode::MapOnly) {
            if first_preseed {
                boot_log::force_uart_line(
                    "[local-seat] vl805 pci cfg preseed mode=map-only reason=irq27-on-ecam-read",
                );
            }
            if !VL805_CFG_SAFE_MODE_LOGGED.swap(true, Ordering::AcqRel) {
                boot_log::force_uart_line(
                    "[local-seat] vl805 pci cfg writes disabled during preseed (safe-mode)",
                );
            }
        }
        log_xhci_firmware_handoff_summary(
            "preseed",
            self.xhci_mmio_hint,
            self.xhci_pci_cmd,
            self.xhci_handoff_ready,
            self.xhci_irq_quiesced,
            xhci_firmware_handoff_cold_start_trusted(
                RPI4_XHCI_MMIO_HIGH_CANDIDATE,
                self.xhci_mmio_hint,
                self.xhci_pci_cmd,
                self.xhci_handoff_ready,
                self.xhci_irq_quiesced,
            ),
        );
        if (!self.xhci_handoff_ready || !self.xhci_irq_quiesced)
            && self.xhci_mmio_hint == Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE)
            && xhci_firmware_handoff_safe(self.xhci_pci_cmd)
        {
            let mut line = heapless::String::<224>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] xhci handoff gate mmio=0x0000000600000000 token={} irq={} cmd_safe=1 action=reject-high-bar",
                    self.xhci_handoff_ready as u8, self.xhci_irq_quiesced as u8,
                ),
            );
            boot_log::force_uart_line(line.as_str());
            let mut detail = heapless::String::<224>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut detail,
                format_args!(
                    "[local-seat] vl805 reset handoff=deferred stage=preseed detail={}",
                    xhci_firmware_handoff_revoked_reason(
                        self.xhci_handoff_ready,
                        self.xhci_irq_quiesced,
                    )
                ),
            );
            boot_log::force_uart_line(detail.as_str());
        }
        if first_preseed {
            boot_log::force_uart_line("[local-seat] pi4 xhci preseed end");
        }
        if first_preseed {
            boot_log::force_uart_line("[local-seat] pi4 keyboard preseed end");
        }
    }

    /// Mirror one rendered line to HDMI.
    pub fn write_line(&mut self, line: &str) {
        self.display.write_line(line);
    }

    /// Publish the stable HDMI sink used for boot-progress banners.
    pub(crate) fn register_boot_progress_display(&mut self) {
        register_wifi_progress_display(&mut self.display);
    }

    /// Mirror raw console input bytes to HDMI without forcing a trailing newline.
    pub fn write_bytes(&mut self, bytes: &[u8]) {
        self.display.write_bytes(bytes);
    }

    /// Scroll the HDMI text viewport by display rows using local keyboard-only controls.
    pub fn scroll_display_rows(&mut self, delta_rows: i8) {
        self.display.scroll_view_rows(delta_rows);
    }

    /// Returns whether the USB keyboard backend is currently online.
    #[must_use]
    pub const fn keyboard_attached(&self) -> bool {
        self.keyboard.is_some()
    }

    /// Arm a bounded prompt-safe keyboard probe for the next manual poll.
    pub fn arm_prompt_safe_probe(&mut self) {
        self.prompt_safe_probe_armed = true;
    }

    /// Predict the first prompt-safe USB probe route before xHCI MMIO starts.
    #[must_use]
    pub(crate) fn keyboard_probe_preflight_status(&self) -> Option<UsbProbePreflightStatus> {
        if self.keyboard.is_some() {
            return None;
        }
        next_usb_probe_preflight_status(
            self.xhci_mmio_hint,
            self.xhci_pci_cmd,
            self.xhci_handoff_ready,
            self.xhci_irq_quiesced,
            self.xhci_bootloader_reset_authorized,
            self.xhci_stop_state_snapshot,
        )
    }

    /// Poll USB keyboard and write canonical bytes into `out`.
    pub fn poll_keyboard_bytes(&mut self, out: &mut [u8]) -> usize {
        let prompt_safe_probe = mem::take(&mut self.prompt_safe_probe_armed);
        if self.keyboard.is_none() && !self.keyboard_init_attempted {
            self.keyboard_init_attempted = true;
            boot_log::force_uart_line("[local-seat] pi4 keyboard runtime init begin");
            if prompt_safe_probe {
                boot_log::force_uart_line(
                    "[local-seat] pi4 keyboard runtime init mode=prompt-safe action=return-to-shell",
                );
            }
            usb_progress_begin(&mut self.display);
            self.preseed_keyboard_mmio();
            boot_log::force_uart_line("[local-seat] pi4 keyboard runtime init after preseed");
            let mut keyboard_error = None;
            if let Some(hal) = hal_from_ptr(self.hal_ptr) {
                let max_attempts = if prompt_safe_probe {
                    1
                } else {
                    KEYBOARD_ATTACH_ATTEMPTS
                };
                for attempt in 1..=max_attempts {
                    let mut line = heapless::String::<144>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut line,
                        format_args!("[local-seat] pi4 keyboard probe attempt={attempt}"),
                    );
                    boot_log::force_uart_line(line.as_str());
                    match UsbKeyboard::new(
                        hal,
                        self.xhci_mmio_hint,
                        self.xhci_pci_cmd,
                        self.xhci_handoff_ready,
                        self.xhci_irq_quiesced,
                        self.xhci_bootloader_reset_authorized,
                        self.xhci_capability_probe,
                        self.xhci_stop_state_snapshot,
                        prompt_safe_probe,
                    ) {
                        Ok(found) => {
                            self.keyboard = Some(found);
                            if attempt > 1 {
                                let mut line = heapless::String::<160>::new();
                                let _ = core::fmt::Write::write_fmt(
                                    &mut line,
                                    format_args!(
                                        "[local-seat] pi4 keyboard attached on retry={attempt}"
                                    ),
                                );
                                boot_log::force_uart_line(line.as_str());
                            }
                            break;
                        }
                        Err(err) => {
                            keyboard_error = Some(err);
                            if !keyboard_attach_retry_allowed(err, attempt, max_attempts) {
                                if matches!(err, Pi4SeatError::XhciInit) {
                                    boot_log::force_uart_line(
                                        "[local-seat] pi4 keyboard retry skipped reason=terminal-xhci-init",
                                    );
                                }
                                break;
                            }
                            if attempt < max_attempts {
                                for _ in 0..KEYBOARD_RETRY_SPINS {
                                    spin_loop();
                                }
                            }
                        }
                    }
                }
            } else {
                keyboard_error = Some(Pi4SeatError::UsbKeyboardInit);
            }
            usb_progress_finish();

            if self.keyboard.is_some() {
                self.display
                    .write_line("[cohesix] local-seat USB keyboard online");
                boot_log::force_uart_line("[local-seat] pi4 keyboard runtime init result=online");
            } else if let Some(err) = keyboard_error {
                if prompt_safe_probe {
                    self.keyboard_init_attempted = false;
                    boot_log::force_uart_line(
                        "[local-seat] pi4 keyboard prompt-safe probe reset state=retry-allowed",
                    );
                }
                if matches!(err, Pi4SeatError::XhciInit) {
                    log_latest_xhci_diag_summary("keyboard-init");
                }
                let mut line = heapless::String::<240>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] pi4 keyboard unavailable detail={} hint=\"UEFI vars: XhciPci=0 XhciReload=1 SystemTableMode=1\"",
                        err.as_str()
                    ),
                );
                boot_log::force_uart_line(line.as_str());
                self.display
                    .write_line("[cohesix] local-seat USB keyboard unavailable");
                boot_log::force_uart_line(
                    "[local-seat] pi4 keyboard runtime init result=unavailable",
                );
            }
        }

        match self.keyboard.as_mut() {
            Some(keyboard) => {
                let written = keyboard.poll_bytes(out);
                let scroll_rows = keyboard.take_pending_display_scroll_rows();
                if scroll_rows != 0 {
                    self.scroll_display_rows(scroll_rows);
                }
                written
            }
            None => 0,
        }
    }
}

struct Mailbox {
    regs_vaddr: usize,
    regs_paddr: usize,
    _regs: Option<crate::sel4::DeviceFrame>,
    request: crate::sel4::RamFrame,
    _prefix_maps: Vec<crate::sel4::DeviceFrame>,
}

#[inline]
const fn should_log_exact_map_retry(attempt: usize, max_attempts: usize) -> bool {
    if attempt < EXACT_MAP_LOG_INITIAL_RETRIES {
        return true;
    }
    if attempt + 1 >= max_attempts {
        return true;
    }
    (attempt + 1) % EXACT_MAP_LOG_STRIDE == 0
}

fn map_device_exact(
    hal: &mut KernelHal<'_>,
    paddr: usize,
    attempt_cap: usize,
    label: &'static str,
    error: Pi4SeatError,
    prefix_maps: &mut Vec<crate::sel4::DeviceFrame>,
) -> Result<crate::sel4::DeviceFrame, Pi4SeatError> {
    let Some(coverage) = hal.device_coverage(paddr, crate::sel4::PAGE_BITS) else {
        let mut line = heapless::String::<192>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] {label} map exact miss paddr=0x{paddr:016x} reason=no-device-coverage"
            ),
        );
        boot_log::force_uart_line(line.as_str());
        return Err(error);
    };
    let span_bytes = coverage.limit.saturating_sub(coverage.base);
    let span_pages = cmp::max(1usize, div_ceil(span_bytes, PAGE_SIZE));
    let max_attempts = cmp::max(1usize, cmp::min(span_pages.saturating_add(1), attempt_cap));

    for attempt in 0..max_attempts {
        let frame = match hal.map_device(paddr) {
            Ok(frame) => frame,
            Err(_) => {
                let mut line = heapless::String::<224>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] {label} map exact fail paddr=0x{paddr:016x} attempt={}/{} base=0x{:08x} limit=0x{:08x}",
                        attempt + 1,
                        max_attempts,
                        coverage.base,
                        coverage.limit
                    ),
                );
                boot_log::force_uart_line(line.as_str());
                return Err(error);
            }
        };
        let actual_paddr = crate::sel4::page_get_address(frame.cap()).map_err(|_| error)?;
        if actual_paddr == paddr {
            if attempt > 0 {
                let mut line = heapless::String::<200>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] {label} map exact after retries={} base=0x{:08x} limit=0x{:08x}",
                        attempt, coverage.base, coverage.limit
                    ),
                );
                boot_log::force_uart_line(line.as_str());
            }
            return Ok(frame);
        }

        if should_log_exact_map_retry(attempt, max_attempts) {
            let mut line = heapless::String::<224>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] {label} map mismatch want=0x{want:08x} got=0x{got:08x} attempt={}/{}",
                    attempt + 1,
                    max_attempts,
                    want = paddr,
                    got = actual_paddr
                ),
            );
            boot_log::force_uart_line(line.as_str());
        }

        if actual_paddr > paddr {
            return Err(error);
        }
        prefix_maps.push(frame);
    }

    Err(error)
}

fn pinned_xhci_window_lookup(phys: usize, size: usize) -> Option<usize> {
    pinned_xhci_window_lookup_with_trust(phys, size, false)
}

fn pinned_xhci_window_lookup_trusted(phys: usize, size: usize) -> Option<usize> {
    pinned_xhci_window_lookup_with_trust(phys, size, true)
}

fn pinned_xhci_window_lookup_with_trust(
    phys: usize,
    size: usize,
    trusted_only: bool,
) -> Option<usize> {
    if size == 0 {
        return None;
    }
    let request_end = phys.checked_add(size)?;
    let pinned = PINNED_XHCI_MMIO.lock();
    let window = pinned.as_ref()?;
    if trusted_only && !window.trusted_for_runtime {
        return None;
    }
    let window_end = window.phys_start.checked_add(window.length)?;
    if phys < window.phys_start || request_end > window_end {
        return None;
    }
    let offset = phys.checked_sub(window.phys_start)?;
    window.virt_start.checked_add(offset)
}

fn pinned_xhci_phys_start_trusted() -> Option<usize> {
    PINNED_XHCI_MMIO.lock().as_ref().and_then(|window| {
        if window.trusted_for_runtime {
            Some(window.phys_start)
        } else {
            None
        }
    })
}

fn pinned_xhci_phys_state() -> Option<(usize, bool)> {
    PINNED_XHCI_MMIO
        .lock()
        .as_ref()
        .map(|window| (window.phys_start, window.trusted_for_runtime))
}

fn pinned_vl805_cfg_lookup(phys: usize, size: usize) -> Option<usize> {
    if size == 0 {
        return None;
    }
    let request_end = phys.checked_add(size)?;
    let pinned = PINNED_VL805_CFG.lock();
    let window = pinned.as_ref()?;
    let window_end = window.phys_start.checked_add(window.length)?;
    if phys < window.phys_start || request_end > window_end {
        return None;
    }
    let offset = phys.checked_sub(window.phys_start)?;
    window.virt_start.checked_add(offset)
}

#[inline]
fn in_vl805_ecam_window(mmio: usize) -> bool {
    for &ecam_base in &VL805_ECAM_BASE_CANDIDATES {
        let ecam_end = ecam_base.saturating_add(VL805_ECAM_WINDOW_BYTES);
        if (ecam_base..ecam_end).contains(&mmio) {
            return true;
        }
    }
    false
}

#[inline]
fn xhci_mmio_candidate_valid(mmio: usize) -> bool {
    if mmio == 0 || (mmio & PAGE_MASK) != 0 {
        return false;
    }
    if mmio == RPI4_XHCI_MMIO_HIGH_CANDIDATE {
        return true;
    }
    if in_vl805_ecam_window(mmio) {
        return false;
    }
    for &ecam_base in &VL805_ECAM_BASE_CANDIDATES {
        let Some(vl805_cfg) = ecam_base.checked_add(VL805_PCI_DEV_ADDR as usize) else {
            continue;
        };
        if mmio == vl805_cfg {
            return false;
        }
    }
    true
}

fn remember_vl805_cfg_virt(config_virt: usize) {
    if config_virt != 0 {
        VL805_CFG_VIRT.store(config_virt, Ordering::Release);
    }
}

fn current_vl805_cfg_virt() -> Option<usize> {
    let cfg = VL805_CFG_VIRT.load(Ordering::Acquire);
    if cfg == 0 {
        None
    } else {
        Some(cfg)
    }
}

#[inline]
fn vl805_cfg_command() -> Option<u16> {
    let cfg_virt = current_vl805_cfg_virt()?;
    Some((pci_cfg_read_u32(cfg_virt, PCI_CFG_COMMAND_STATUS) & 0xffff) as u16)
}

#[inline]
fn vl805_cfg_bus_master_ready() -> bool {
    let Some(command) = vl805_cfg_command() else {
        return false;
    };
    (command & (PCI_COMMAND_MEMORY_SPACE | PCI_COMMAND_BUS_MASTER))
        == (PCI_COMMAND_MEMORY_SPACE | PCI_COMMAND_BUS_MASTER)
}

fn remember_vl805_xhci_mmio_hint(mmio: usize) {
    if mmio != 0 {
        VL805_XHCI_MMIO_HINT.store(mmio, Ordering::Release);
    }
}

fn current_vl805_xhci_mmio_hint() -> Option<usize> {
    let mmio = VL805_XHCI_MMIO_HINT.load(Ordering::Acquire);
    if mmio == 0 {
        None
    } else {
        Some(mmio)
    }
}

#[inline]
fn translate_bcm2711_soc_reg_addr(addr: usize) -> usize {
    if (BCM2711_COMMON_PERIPH_BUS_BASE..BCM2711_COMMON_PERIPH_BUS_BASE + BCM2711_COMMON_PERIPH_SIZE)
        .contains(&addr)
    {
        return BCM2711_COMMON_PERIPH_PHYS_BASE
            .saturating_add(addr.saturating_sub(BCM2711_COMMON_PERIPH_BUS_BASE));
    }
    if (BCM2711_SOC_PERIPH_BUS_BASE..BCM2711_SOC_PERIPH_BUS_BASE + BCM2711_SOC_PERIPH_SIZE)
        .contains(&addr)
    {
        return BCM2711_SOC_PERIPH_PHYS_BASE
            .saturating_add(addr.saturating_sub(BCM2711_SOC_PERIPH_BUS_BASE));
    }
    if (BCM2711_ARM_LOCAL_BUS_BASE..BCM2711_ARM_LOCAL_BUS_BASE + BCM2711_ARM_LOCAL_SIZE)
        .contains(&addr)
    {
        return BCM2711_ARM_LOCAL_PHYS_BASE
            .saturating_add(addr.saturating_sub(BCM2711_ARM_LOCAL_BUS_BASE));
    }
    addr
}

#[inline]
fn normalize_pi4_xhci_mmio_hint(mmio: Option<usize>) -> Option<usize> {
    mmio.map(translate_bcm2711_soc_reg_addr)
}

#[inline]
const fn xhci_mmio_is_legacy_alias(mmio: usize) -> bool {
    mmio == RPI4_XHCI_MMIO_PRIMARY_CANDIDATE || mmio == RPI4_XHCI_MMIO_SECONDARY_CANDIDATE
}

#[inline]
const fn xhci_preseed_allows_static_legacy_fallbacks(_vl805_hint: Option<usize>) -> bool {
    false
}

#[inline]
fn xhci_preseed_pin_only_reason(
    mmio: usize,
    firmware_hint: Option<usize>,
    vl805_hint: Option<usize>,
    xhci_pci_cmd: Option<u16>,
    xhci_handoff_ready: bool,
    xhci_irq_quiesced: bool,
) -> Option<&'static str> {
    if xhci_mmio_is_legacy_alias(mmio) {
        return Some("bcm2835-usb-not-xhci");
    }
    if firmware_hint == Some(mmio) && vl805_hint.is_none() {
        if mmio == RPI4_XHCI_MMIO_HIGH_CANDIDATE
            && xhci_firmware_handoff_safe(xhci_pci_cmd)
            && !xhci_handoff_ready
        {
            return Some("bootloader-handoff-unready");
        }
        if mmio == RPI4_XHCI_MMIO_HIGH_CANDIDATE
            && xhci_firmware_handoff_safe(xhci_pci_cmd)
            && !xhci_irq_quiesced
        {
            return Some("bootloader-handoff-irq-unquiesced");
        }
        return Some("firmware-hint-unverified");
    }
    None
}

#[inline]
const fn xhci_firmware_handoff_safe(xhci_pci_cmd: Option<u16>) -> bool {
    match xhci_pci_cmd {
        Some(cmd) => {
            (cmd & (PCI_COMMAND_MEMORY_SPACE
                | PCI_COMMAND_BUS_MASTER
                | PCI_COMMAND_INTERRUPT_DISABLE))
                == (PCI_COMMAND_MEMORY_SPACE
                    | PCI_COMMAND_BUS_MASTER
                    | PCI_COMMAND_INTERRUPT_DISABLE)
        }
        None => false,
    }
}

#[inline]
fn xhci_firmware_handoff_cold_start_trusted(
    mmio: usize,
    firmware_hint: Option<usize>,
    xhci_pci_cmd: Option<u16>,
    xhci_handoff_ready: bool,
    xhci_irq_quiesced: bool,
) -> bool {
    mmio == RPI4_XHCI_MMIO_HIGH_CANDIDATE
        && firmware_hint == Some(mmio)
        && xhci_firmware_handoff_safe(xhci_pci_cmd)
        && xhci_handoff_ready
        && xhci_irq_quiesced
}

#[inline]
fn xhci_runtime_vl805_mailbox_reset_required(
    mmio: usize,
    firmware_hint: Option<usize>,
    xhci_pci_cmd: Option<u16>,
    xhci_handoff_ready: bool,
    xhci_irq_quiesced: bool,
) -> bool {
    xhci_firmware_handoff_cold_start_trusted(
        mmio,
        firmware_hint,
        xhci_pci_cmd,
        xhci_handoff_ready,
        xhci_irq_quiesced,
    )
}

#[inline]
fn xhci_bootloader_vl805_reset_authorized(
    mmio: usize,
    firmware_hint: Option<usize>,
    xhci_pci_cmd: Option<u16>,
    xhci_handoff_ready: bool,
    xhci_irq_quiesced: bool,
    xhci_bootloader_reset_authorized: bool,
    stop_state_snapshot: Option<LocalSeatXhciStopStateSnapshot>,
) -> bool {
    xhci_bootloader_reset_authorized
        && stop_state_snapshot.is_some()
        && xhci_firmware_handoff_cold_start_trusted(
            mmio,
            firmware_hint,
            xhci_pci_cmd,
            xhci_handoff_ready,
            xhci_irq_quiesced,
        )
}

#[inline]
fn xhci_trusted_handoff_snapshot_allowed(
    mmio: usize,
    firmware_hint: Option<usize>,
    xhci_pci_cmd: Option<u16>,
    xhci_handoff_ready: bool,
    xhci_irq_quiesced: bool,
    runtime_vl805_reset_allows_trusted_snapshot: bool,
) -> bool {
    runtime_vl805_reset_allows_trusted_snapshot
        && xhci_firmware_handoff_cold_start_trusted(
            mmio,
            firmware_hint,
            xhci_pci_cmd,
            xhci_handoff_ready,
            xhci_irq_quiesced,
        )
}

#[inline]
const fn xhci_preferred_trusted_handoff_mode(runtime_vl805_reset_state: u8) -> XhciFirmwareHandoff {
    // Treat the U-Boot handoff as a capability source. The prompt-safe ladder
    // first uses its stop-state seed to skip the fragile live pre-reset reads,
    // then falls back to the unseeded U-Boot-shaped fresh-init lane if needed.
    let _ = runtime_vl805_reset_state;
    XhciFirmwareHandoff::ColdStartFromSnapshot
}

#[inline]
const fn xhci_firmware_handoff_mode_label(firmware_handoff: XhciFirmwareHandoff) -> &'static str {
    match firmware_handoff {
        XhciFirmwareHandoff::None => "none",
        XhciFirmwareHandoff::ColdStartFromSnapshot => "cold-start-from-snapshot",
        XhciFirmwareHandoff::ResetlessReinit => "resetless-reinit",
        XhciFirmwareHandoff::PreserveControllerState => "preserve-controller-state",
    }
}

#[inline]
const fn xhci_runtime_handoff_source_label(
    runtime_vl805_reset: bool,
    using_handoff_snapshot: bool,
    bootloader_reset_authorized: bool,
) -> &'static str {
    if bootloader_reset_authorized && using_handoff_snapshot {
        "fw-handoff-bootloader-owned-snapshot"
    } else if runtime_vl805_reset && using_handoff_snapshot {
        "fw-handoff-runtime-reset-pending-snapshot"
    } else if using_handoff_snapshot {
        "fw-handoff-direct-snapshot"
    } else if runtime_vl805_reset {
        "fw-handoff-runtime-reset-pending-cold-init"
    } else {
        "fw-handoff-cold-start"
    }
}

#[inline]
const fn xhci_firmware_handoff_revoked_reason(
    xhci_handoff_ready: bool,
    xhci_irq_quiesced: bool,
) -> &'static str {
    if !xhci_handoff_ready {
        "fw-handoff-token-missing"
    } else if !xhci_irq_quiesced {
        "fw-handoff-irq-unquiesced"
    } else {
        "fw-handoff-untrusted"
    }
}

#[inline]
fn xhci_firmware_handoff_hint_reason(
    firmware_hint: Option<usize>,
    xhci_pci_cmd: Option<u16>,
    xhci_handoff_ready: bool,
    xhci_irq_quiesced: bool,
) -> &'static str {
    match firmware_hint {
        None => "hint-absent",
        Some(mmio) if mmio != RPI4_XHCI_MMIO_HIGH_CANDIDATE => "not-high-bar",
        Some(_) if !xhci_firmware_handoff_safe(xhci_pci_cmd) => "unsafe-pci-cmd",
        Some(_) if !xhci_handoff_ready => "ready-token-absent",
        Some(_) if !xhci_irq_quiesced => "irq-quiesce-absent",
        Some(_) => "bootloader-handoff-ready",
    }
}

fn log_xhci_firmware_handoff_summary(
    stage: &'static str,
    firmware_hint: Option<usize>,
    xhci_pci_cmd: Option<u16>,
    xhci_handoff_ready: bool,
    xhci_irq_quiesced: bool,
    trusted: bool,
) {
    let mut line = heapless::String::<256>::new();
    let _ = core::fmt::Write::write_fmt(
        &mut line,
        format_args!("[local-seat] xhci handoff summary stage={stage}"),
    );
    let _ = core::fmt::Write::write_str(&mut line, " mmio=");
    match firmware_hint {
        Some(mmio) => {
            let _ = core::fmt::Write::write_fmt(&mut line, format_args!("0x{mmio:016x}"));
        }
        None => {
            let _ = core::fmt::Write::write_str(&mut line, "none");
        }
    }
    let _ = core::fmt::Write::write_str(&mut line, " cmd=");
    match xhci_pci_cmd {
        Some(cmd) => {
            let _ = core::fmt::Write::write_fmt(&mut line, format_args!("0x{cmd:04x}"));
        }
        None => {
            let _ = core::fmt::Write::write_str(&mut line, "absent");
        }
    }
    let _ = core::fmt::Write::write_fmt(
        &mut line,
        format_args!(
            " safe={} token={} irq={} trusted={} reason={}",
            xhci_firmware_handoff_safe(xhci_pci_cmd) as u8,
            xhci_handoff_ready as u8,
            xhci_irq_quiesced as u8,
            trusted as u8,
            xhci_firmware_handoff_hint_reason(
                firmware_hint,
                xhci_pci_cmd,
                xhci_handoff_ready,
                xhci_irq_quiesced,
            ),
        ),
    );
    boot_log::force_uart_line(line.as_str());
}

#[inline]
fn xhci_firmware_handoff_allows_legacy_probe(
    _firmware_hint: Option<usize>,
    _xhci_pci_cmd: Option<u16>,
    _xhci_handoff_ready: bool,
    _xhci_irq_quiesced: bool,
) -> bool {
    false
}

#[inline]
const fn xhci_runtime_allows_alias_scan(
    mmio: usize,
    _verified_vl805_hint: Option<usize>,
    _legacy_mirror_allowed: bool,
) -> bool {
    mmio == RPI4_XHCI_MMIO_HIGH_CANDIDATE
}

#[inline]
fn xhci_runtime_allows_pinned_legacy_fallback(
    _mmio: usize,
    _pinned_xhci_state: Option<(usize, bool)>,
    _firmware_hint: Option<usize>,
    _verified_vl805_hint: Option<usize>,
) -> bool {
    false
}

#[derive(Clone, Copy)]
struct Vl805PciCfgSnapshot {
    vendor_device: u32,
    class_revision: u32,
    command: u16,
    bar0: u32,
    bar1: u32,
    bar_mmio: Option<usize>,
}

fn read_vl805_pci_cfg_snapshot(config_virt: usize) -> Vl805PciCfgSnapshot {
    let vendor_device = pci_cfg_read_u32(config_virt, PCI_CFG_VENDOR_DEVICE);
    let class_revision = pci_cfg_read_u32(config_virt, PCI_CFG_CLASS_REVISION);
    let command = (pci_cfg_read_u32(config_virt, PCI_CFG_COMMAND_STATUS) & 0xffff) as u16;
    let bar0 = pci_cfg_read_u32(config_virt, PCI_CFG_BAR0);
    let bar1 = pci_cfg_read_u32(config_virt, PCI_CFG_BAR1);
    let bar_mmio = decode_pci_mmio_bar(bar0, bar1);
    Vl805PciCfgSnapshot {
        vendor_device,
        class_revision,
        command,
        bar0,
        bar1,
        bar_mmio,
    }
}

fn record_vl805_xhci_mmio_hint(
    snapshot: &Vl805PciCfgSnapshot,
    source: &'static str,
) -> Option<usize> {
    match snapshot.bar_mmio {
        Some(mmio) if xhci_mmio_candidate_valid(mmio) => {
            remember_vl805_xhci_mmio_hint(mmio);
            let mut hint = heapless::String::<208>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut hint,
                format_args!("[local-seat] vl805 pci cfg hint mmio=0x{mmio:016x} source={source}"),
            );
            boot_log::force_uart_line(hint.as_str());
            Some(mmio)
        }
        Some(mmio) => {
            let mut hint = heapless::String::<224>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut hint,
                format_args!(
                    "[local-seat] vl805 pci cfg hint rejected mmio=0x{mmio:016x} source={source} reason=invalid-candidate"
                ),
            );
            boot_log::force_uart_line(hint.as_str());
            None
        }
        None => {
            let mut line = heapless::String::<176>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] vl805 pci cfg hint missing source={source} reason=bar-decode"
                ),
            );
            boot_log::force_uart_line(line.as_str());
            None
        }
    }
}

#[inline]
fn preferred_xhci_runtime_mmio(
    trusted_pinned_mmio: Option<usize>,
    firmware_hint: Option<usize>,
    vl805_pci_mmio: Option<usize>,
    verified_vl805_hint: Option<usize>,
    _legacy_mirror_allowed: bool,
) -> Option<usize> {
    let preferred_verified = match verified_vl805_hint {
        Some(mmio) if xhci_mmio_is_legacy_alias(mmio) => None,
        other => other,
    };
    let preferred_firmware_hint = match firmware_hint {
        Some(mmio) if xhci_mmio_is_legacy_alias(mmio) => None,
        other => other,
    };
    let preferred_trusted_pin = match trusted_pinned_mmio {
        Some(mmio) if xhci_mmio_is_legacy_alias(mmio) => None,
        other => other,
    };

    vl805_pci_mmio
        .or(preferred_verified)
        .or(preferred_firmware_hint)
        .or(preferred_trusted_pin)
}

#[inline]
const fn vl805_runtime_cfg_touch_allowed(runtime_enabled: bool, has_cfg_window: bool) -> bool {
    runtime_enabled && has_cfg_window
}

#[inline]
const fn xhci_safe_mode_skip_command(xhci_pci_cmd: Option<u16>) -> u16 {
    match xhci_pci_cmd {
        Some(cmd) => cmd,
        None => 0,
    }
}

#[inline]
fn xhci_runtime_mmio_candidate_allowed(
    mmio: usize,
    _has_safe_cfg_window: bool,
    _pinned_xhci_state: Option<(usize, bool)>,
    trusted_pinned_mmio: Option<usize>,
    _firmware_hint: Option<usize>,
    verified_vl805_hint: Option<usize>,
    _legacy_mirror_allowed: bool,
) -> bool {
    if mmio == RPI4_XHCI_MMIO_HIGH_CANDIDATE {
        trusted_pinned_mmio == Some(mmio) || verified_vl805_hint == Some(mmio)
    } else if xhci_mmio_is_legacy_alias(mmio) {
        false
    } else {
        true
    }
}

#[inline]
fn xhci_runtime_mmio_has_accessible_window(
    mmio: usize,
    has_device_coverage: bool,
    has_pinned_window: bool,
    pinned_xhci_state: Option<(usize, bool)>,
    trusted_pinned_mmio: Option<usize>,
    firmware_hint: Option<usize>,
    verified_vl805_hint: Option<usize>,
) -> bool {
    has_device_coverage
        || trusted_pinned_mmio == Some(mmio)
        || (has_pinned_window && (firmware_hint == Some(mmio) || verified_vl805_hint == Some(mmio)))
        || (has_pinned_window
            && xhci_runtime_allows_pinned_legacy_fallback(
                mmio,
                pinned_xhci_state,
                firmware_hint,
                verified_vl805_hint,
            ))
}

#[inline]
fn xhci_runtime_candidate_skip_reason(
    mmio: usize,
    _has_safe_cfg_window: bool,
    trusted_pinned_mmio: Option<usize>,
    firmware_hint: Option<usize>,
    verified_vl805_hint: Option<usize>,
    firmware_hint_safe: bool,
    _legacy_mirror_allowed: bool,
) -> &'static str {
    if xhci_mmio_is_legacy_alias(mmio) {
        "legacy-runtime-disabled"
    } else if mmio == RPI4_XHCI_MMIO_HIGH_CANDIDATE
        && firmware_hint == Some(mmio)
        && trusted_pinned_mmio != Some(mmio)
        && verified_vl805_hint != Some(mmio)
    {
        if firmware_hint_safe {
            "fw-handoff-unverified"
        } else {
            "fw-handoff-unsafe"
        }
    } else {
        "no-trusted-source"
    }
}

#[inline]
fn xhci_runtime_candidate_kind(mmio: usize, firmware_hint: Option<usize>) -> &'static str {
    if firmware_hint == Some(mmio) {
        if mmio == RPI4_XHCI_MMIO_HIGH_CANDIDATE {
            "fw-high"
        } else {
            "fw"
        }
    } else if xhci_mmio_is_legacy_alias(mmio) {
        "legacy"
    } else {
        "other"
    }
}

fn log_xhci_runtime_candidate_diag(
    mmio: usize,
    has_safe_cfg_window: bool,
    has_device_coverage: bool,
    has_pinned_window: bool,
    pinned_xhci_state: Option<(usize, bool)>,
    trusted_pinned_mmio: Option<usize>,
    firmware_hint: Option<usize>,
    verified_vl805_hint: Option<usize>,
    firmware_hint_safe: bool,
) {
    let pin_match = matches!(pinned_xhci_state, Some((pinned_mmio, _)) if pinned_mmio == mmio);
    let mut line = heapless::String::<224>::new();
    let _ = core::fmt::Write::write_fmt(
        &mut line,
        format_args!(
            "[local-seat] xhci cand mmio=0x{mmio:016x} kind={} cfg={} cov={} pwin={} pin={} fh={} fs={} vh={}",
            xhci_runtime_candidate_kind(mmio, firmware_hint),
            has_safe_cfg_window as u8,
            has_device_coverage as u8,
            has_pinned_window as u8,
            pin_match as u8,
            (firmware_hint == Some(mmio)) as u8,
            firmware_hint_safe as u8,
            (verified_vl805_hint == Some(mmio)) as u8,
        ),
    );
    boot_log::force_uart_line(line.as_str());
    if trusted_pinned_mmio == Some(mmio) {
        boot_log::force_uart_line(
            "[local-seat] xhci cand trust=trusted-pinned runtime-probe-eligible",
        );
    }
    if mmio == RPI4_XHCI_MMIO_HIGH_CANDIDATE && firmware_hint == Some(mmio) {
        let mut gate = heapless::String::<224>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut gate,
            format_args!(
                "[local-seat] xhci cand gate mmio=0x{mmio:016x} safe={} cfg={} tp={} vv={} pwin={} cov={}",
                firmware_hint_safe as u8,
                has_safe_cfg_window as u8,
                (trusted_pinned_mmio == Some(mmio)) as u8,
                (verified_vl805_hint == Some(mmio)) as u8,
                has_pinned_window as u8,
                has_device_coverage as u8,
            ),
        );
        boot_log::force_uart_line(gate.as_str());
    }
}

fn xhci_diag_hook(stage: u16, a: u64, b: u64, c: u64) {
    XHCI_DIAG_LAST_STAGE.store(u32::from(stage), Ordering::Release);
    XHCI_DIAG_LAST_A.store(a as usize, Ordering::Release);
    XHCI_DIAG_LAST_B.store(b as usize, Ordering::Release);
    XHCI_DIAG_LAST_C.store(c as usize, Ordering::Release);
    let line_no = XHCI_DIAG_LINE_COUNT
        .fetch_add(1, Ordering::Relaxed)
        .saturating_add(1);
    if line_no > XHCI_DIAG_MAX_LINES {
        if line_no == XHCI_DIAG_MAX_LINES.saturating_add(1) {
            boot_log::force_uart_line("[local-seat] xhci.diag suppressed (rate-limited)");
        }
        return;
    }
    let mut line = heapless::String::<224>::new();
    let _ = core::fmt::Write::write_fmt(
        &mut line,
        format_args!("[local-seat] xhci.diag stage=0x{stage:04x}"),
    );
    if let Some(tag) = xhci_diag_stage_label(stage) {
        let _ = core::fmt::Write::write_fmt(&mut line, format_args!(" tag={tag}"));
    }
    if let Some((a_label, b_label, c_label)) = xhci_diag_stage_value_labels(stage) {
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(" {a_label}=0x{a:016x} {b_label}=0x{b:016x} {c_label}=0x{c:016x}"),
        );
    } else {
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(" a=0x{a:016x} b=0x{b:016x} c=0x{c:016x}"),
        );
    }
    boot_log::force_uart_line(line.as_str());
}

#[inline]
fn reset_latest_xhci_diag_snapshot() {
    XHCI_DIAG_LINE_COUNT.store(0, Ordering::Release);
    XHCI_DIAG_LAST_STAGE.store(0, Ordering::Release);
    XHCI_DIAG_LAST_A.store(0, Ordering::Release);
    XHCI_DIAG_LAST_B.store(0, Ordering::Release);
    XHCI_DIAG_LAST_C.store(0, Ordering::Release);
}

#[inline]
fn read_latest_xhci_diag_snapshot() -> XhciDiagSnapshot {
    let line_count = XHCI_DIAG_LINE_COUNT.load(Ordering::Acquire) as usize;
    if line_count == 0 {
        return XhciDiagSnapshot::empty();
    }
    XhciDiagSnapshot {
        line_count,
        stage: XHCI_DIAG_LAST_STAGE.load(Ordering::Acquire) as u16,
        a: XHCI_DIAG_LAST_A.load(Ordering::Acquire) as u64,
        b: XHCI_DIAG_LAST_B.load(Ordering::Acquire) as u64,
        c: XHCI_DIAG_LAST_C.load(Ordering::Acquire) as u64,
    }
}

pub(crate) fn latest_xhci_diag_status() -> Option<UsbXhciDiagStatus> {
    let snapshot = read_latest_xhci_diag_snapshot();
    if snapshot.line_count == 0 {
        return None;
    }
    Some(UsbXhciDiagStatus {
        stage: snapshot.stage,
        tag: xhci_diag_stage_label(snapshot.stage),
        exact_issue: xhci_diag_stage_exact_issue_label(snapshot.stage),
        a: snapshot.a,
        b: snapshot.b,
        c: snapshot.c,
        value_labels: xhci_diag_stage_value_labels(snapshot.stage),
    })
}

pub(crate) fn latest_usb_probe_route_status() -> Option<UsbProbeRouteStatus> {
    let summary = (*LATEST_USB_PROBE_ROUTE.lock())?;
    let diag_stage = (summary.diag.line_count != 0).then_some(summary.diag.stage);
    Some(UsbProbeRouteStatus {
        route: usb_probe_route_label(summary),
        pathway_idx: summary.pathway_idx,
        strategy_idx: summary.strategy_idx,
        strategy_count: summary.strategy_count,
        policy: summary.policy,
        origin: summary.origin,
        handoff: summary.handoff,
        seed: summary.seed,
        halt_guard: summary.halt_guard,
        current_step: usb_probe_current_step(summary),
        next_step: usb_probe_next_step(summary),
        progress: summary.progress.as_str(),
        outcome: summary.outcome.as_str(),
        prefer_high: summary.prefer_high,
        pcie_dma_window: summary.pcie_dma_window,
        poll_only: summary.poll_only,
        port: summary.port,
        connected_mask: summary.connected_mask,
        detect_passes: summary.detect_passes,
        slow_recheck: summary.slow_recheck,
        irq27_bound: summary.irq27_bound,
        bridge_irq_bound: summary.bridge_irq_bound,
        intx_irq_bound: summary.intx_irq_bound,
        controller_gate: summary.controller_gate,
        diag_fresh: summary.diag_fresh,
        diag_stage,
        diag_tag: diag_stage.and_then(xhci_diag_stage_label),
        diag_exact: diag_stage.and_then(xhci_diag_stage_exact_issue_label),
        diag_a: summary.diag.a,
        diag_b: summary.diag.b,
        diag_c: summary.diag.c,
        diag_value_labels: diag_stage.and_then(xhci_diag_stage_value_labels),
    })
}

pub(crate) fn next_usb_probe_preflight_status(
    xhci_mmio_hint: Option<usize>,
    xhci_pci_cmd: Option<u16>,
    xhci_handoff_ready: bool,
    xhci_irq_quiesced: bool,
    xhci_bootloader_reset_authorized: bool,
    stop_state_snapshot: Option<LocalSeatXhciStopStateSnapshot>,
) -> Option<UsbProbePreflightStatus> {
    usb_probe_preflight_status(
        xhci_mmio_hint,
        xhci_pci_cmd,
        xhci_handoff_ready,
        xhci_irq_quiesced,
        xhci_bootloader_reset_authorized,
        stop_state_snapshot,
        true,
    )
}

#[inline]
fn xhci_diag_snapshot_changed(before: XhciDiagSnapshot, after: XhciDiagSnapshot) -> bool {
    before != after
}

fn log_usb_probe_pathway_summary(summary: &UsbProbePathwaySummary) {
    remember_latest_usb_probe_route(summary);
    let mut line = heapless::String::<640>::new();
    let _ = core::fmt::Write::write_fmt(
        &mut line,
        format_args!(
            "[local-seat] usb probe path pathway={} attempt={}/{} outcome={} progress={} policy={} origin={} dma={} bus={} handoff={} seed={} halt_guard={} poll_only={}",
            summary.pathway_idx,
            summary.strategy_idx,
            summary.strategy_count,
            summary.outcome.as_str(),
            summary.progress.as_str(),
            summary.policy,
            summary.origin,
            if summary.prefer_high { "high" } else { "low" },
            if summary.pcie_dma_window {
                "pcie-window"
            } else {
                "phys"
            },
            summary.handoff,
            summary.seed,
            summary.halt_guard,
            if summary.poll_only { "yes" } else { "no" },
        ),
    );
    if let Some(port) = summary.port {
        let _ = core::fmt::Write::write_fmt(&mut line, format_args!(" port={port}"));
    }
    let _ = core::fmt::Write::write_fmt(
        &mut line,
        format_args!(
            " connected_mask=0x{:04x} detect_passes={} slow_recheck={}",
            summary.connected_mask, summary.detect_passes, summary.slow_recheck as u8,
        ),
    );
    if summary.diag.line_count != 0 {
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                " diag_fresh={} diag_stage=0x{:04x}",
                summary.diag_fresh as u8, summary.diag.stage,
            ),
        );
        if let Some(tag) = xhci_diag_stage_label(summary.diag.stage) {
            let _ = core::fmt::Write::write_fmt(&mut line, format_args!(" diag_tag={tag}"));
        }
        if let Some(exact) = xhci_diag_stage_exact_issue_label(summary.diag.stage) {
            let _ = core::fmt::Write::write_fmt(&mut line, format_args!(" diag_exact={exact}"));
        }
    }
    boot_log::force_uart_line(line.as_str());
}

fn log_usb_probe_best_pathway(result: &str, summary: &UsbProbePathwaySummary) {
    remember_latest_usb_probe_route(summary);
    let mut line = heapless::String::<640>::new();
    let _ = core::fmt::Write::write_fmt(
        &mut line,
        format_args!(
            "[local-seat] usb probe best result={} pathway={} outcome={} progress={} policy={} origin={} dma={} bus={} handoff={} seed={} halt_guard={} poll_only={}",
            result,
            summary.pathway_idx,
            summary.outcome.as_str(),
            summary.progress.as_str(),
            summary.policy,
            summary.origin,
            if summary.prefer_high { "high" } else { "low" },
            if summary.pcie_dma_window {
                "pcie-window"
            } else {
                "phys"
            },
            summary.handoff,
            summary.seed,
            summary.halt_guard,
            if summary.poll_only { "yes" } else { "no" },
        ),
    );
    if let Some(port) = summary.port {
        let _ = core::fmt::Write::write_fmt(&mut line, format_args!(" port={port}"));
    }
    let _ = core::fmt::Write::write_fmt(
        &mut line,
        format_args!(
            " connected_mask=0x{:04x} detect_passes={} slow_recheck={}",
            summary.connected_mask, summary.detect_passes, summary.slow_recheck as u8,
        ),
    );
    if summary.diag.line_count != 0 {
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                " diag_fresh={} diag_stage=0x{:04x}",
                summary.diag_fresh as u8, summary.diag.stage,
            ),
        );
        if let Some(tag) = xhci_diag_stage_label(summary.diag.stage) {
            let _ = core::fmt::Write::write_fmt(&mut line, format_args!(" diag_tag={tag}"));
        }
        if let Some(exact) = xhci_diag_stage_exact_issue_label(summary.diag.stage) {
            let _ = core::fmt::Write::write_fmt(&mut line, format_args!(" diag_exact={exact}"));
        }
    }
    boot_log::force_uart_line(line.as_str());
}

fn log_latest_xhci_diag_summary(context: &'static str) {
    if XHCI_DIAG_LINE_COUNT.load(Ordering::Acquire) == 0 {
        return;
    }

    let stage = XHCI_DIAG_LAST_STAGE.load(Ordering::Acquire) as u16;
    let a = XHCI_DIAG_LAST_A.load(Ordering::Acquire) as u64;
    let b = XHCI_DIAG_LAST_B.load(Ordering::Acquire) as u64;
    let c = XHCI_DIAG_LAST_C.load(Ordering::Acquire) as u64;

    let mut line = heapless::String::<320>::new();
    let _ = core::fmt::Write::write_fmt(
        &mut line,
        format_args!("[local-seat] xhci diag summary context={context} stage=0x{stage:04x}"),
    );
    if let Some(tag) = xhci_diag_stage_label(stage) {
        let _ = core::fmt::Write::write_fmt(&mut line, format_args!(" tag={tag}"));
    }
    if let Some(exact) = xhci_diag_stage_exact_issue_label(stage) {
        let _ = core::fmt::Write::write_fmt(&mut line, format_args!(" exact={exact}"));
    }
    if let Some((a_label, b_label, c_label)) = xhci_diag_stage_value_labels(stage) {
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(" {a_label}=0x{a:016x} {b_label}=0x{b:016x} {c_label}=0x{c:016x}"),
        );
    } else {
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(" a=0x{a:016x} b=0x{b:016x} c=0x{c:016x}"),
        );
    }
    boot_log::force_uart_line(line.as_str());
}

#[inline]
const fn xhci_diag_stage_exact_issue_label(stage: u16) -> Option<&'static str> {
    match stage {
        0x0200 => Some("pre-reset-usbcmd-usbsts-read"),
        0x0213 => Some("live-usbsts-read-before-run"),
        0x0215 => Some("live-usbcmd-read-before-run"),
        0x0222 => Some("halt-revalidation-timeout"),
        0x0238 => Some("pre-run-config-store-wedged"),
        0x023b => Some("fresh-rings-reset-required"),
        0x0248 | 0x029e => Some("pre-run-dcbaap-low-store-wedged"),
        0x02a5 => Some("post-run-dcbaap-low-store-wedged"),
        0x0249 | 0x02f6 => Some("pre-run-dcbaap-high-store-wedged"),
        0x02a7 => Some("post-run-dcbaap-high-store-wedged"),
        0x0254 | 0x02b0 => Some("pre-run-crcr-low-store-wedged"),
        0x0255 | 0x02b2 => Some("pre-run-crcr-high-store-wedged"),
        0x0277 | 0x02bb => Some("pre-run-erdp-low-store-wedged"),
        0x0278 | 0x02bd => Some("pre-run-erdp-high-store-wedged"),
        0x02c3 => Some("pre-run-erstsz-store-wedged"),
        0x02c6 => Some("pre-run-erstba-low-store-wedged"),
        0x02c8 => Some("pre-run-erstba-high-store-wedged"),
        0x0269 => Some("pre-run-usbsts-clear-write-wedged"),
        0x0267 => Some("post-ready-imod-write-wedged"),
        0x0268 => Some("post-ready-iman-write-wedged"),
        0x0256 => Some("pre-run-dnctrl-write-wedged"),
        0x02d8 => Some("post-run-dnctrl-write-wedged"),
        0x0319 => Some("post-start-polling-irq-quiesce-timeout"),
        0x0321 => Some("post-start-usbcmd-mask-write-wedged"),
        0x0324 => Some("post-start-imod-write-wedged"),
        0x0326 => Some("post-start-erdp-low-store-wedged"),
        0x0328 => Some("post-start-erdp-high-store-wedged"),
        0x032a => Some("post-start-iman-write-wedged"),
        0x032c => Some("post-start-usbsts-clear-write-wedged"),
        0x0332 => Some("pre-dcbaap-iman-write-wedged"),
        0x02eb => Some("usbcmd-run-barrier-wedged"),
        0x02e9 => Some("usbcmd-run-store-wedged"),
        _ => None,
    }
}

#[inline]
const fn xhci_diag_stage_after_run(stage: u16) -> bool {
    matches!(
        stage,
        0x02a5
            | 0x02a7
            | 0x02cb..=0x02d9
            | 0x02e9
            | 0x02eb
            | 0x0315..=0x0319
            | 0x0320..=0x0330
    )
}

#[inline]
fn xhci_probe_failure_edge_label(
    runtime_vl805_reset_requested: bool,
    firmware_handoff: XhciFirmwareHandoff,
    diag_before: XhciDiagSnapshot,
    diag_after: XhciDiagSnapshot,
) -> &'static str {
    if runtime_vl805_reset_requested {
        "before-mailbox-reset-suppression"
    } else if matches!(firmware_handoff, XhciFirmwareHandoff::ColdStartFromSnapshot)
        && !xhci_diag_snapshot_changed(diag_before, diag_after)
    {
        "trusted-path-selection"
    } else if diag_after.line_count != 0 && xhci_diag_stage_after_run(diag_after.stage) {
        "after-run"
    } else if diag_after.line_count != 0 {
        "first-live-ownership-write"
    } else {
        "controller-init-before-diag"
    }
}

#[inline]
fn log_xhci_probe_failure_edge(
    runtime_vl805_reset_requested: bool,
    strategy: XhciRuntimeInitStrategy,
    diag_before: XhciDiagSnapshot,
    diag_after: XhciDiagSnapshot,
) {
    let edge = xhci_probe_failure_edge_label(
        runtime_vl805_reset_requested,
        strategy.firmware_handoff,
        diag_before,
        diag_after,
    );
    let mut line = heapless::String::<320>::new();
    let _ = core::fmt::Write::write_fmt(
        &mut line,
        format_args!(
            "[local-seat] xhci edge classify=edge={edge} origin={} handoff={}",
            xhci_runtime_init_strategy_origin_label(strategy),
            xhci_firmware_handoff_mode_label(strategy.firmware_handoff),
        ),
    );
    if diag_after.line_count != 0 {
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(" stage=0x{:04x}", diag_after.stage),
        );
        if let Some(tag) = xhci_diag_stage_label(diag_after.stage) {
            let _ = core::fmt::Write::write_fmt(&mut line, format_args!(" tag={tag}"));
        }
        if let Some(exact) = xhci_diag_stage_exact_issue_label(diag_after.stage) {
            let _ = core::fmt::Write::write_fmt(&mut line, format_args!(" exact={exact}"));
        }
    } else {
        let _ = core::fmt::Write::write_str(&mut line, " stage=none");
    }
    boot_log::force_uart_line(line.as_str());
}

#[inline]
const fn xhci_diag_stage_value_labels(
    stage: u16,
) -> Option<(&'static str, &'static str, &'static str)> {
    match stage {
        0x0117 => Some(("handoff", "runtime_mask", "publish_mask")),
        0x0200 => Some(("usbcmd", "usbsts", "mmio")),
        0x0204 => Some(("mmio", "handoff", "seed_flags")),
        0x020d => Some(("usbsts", "imod", "iman")),
        0x020e => Some(("imod", "polling", "trusted")),
        0x020f => Some(("iman", "polling", "trusted")),
        0x0212 => Some(("handoff", "seed_flags", "skip")),
        0x0217 | 0x0218 => Some(("handoff", "seed_flags", "skip")),
        0x023b => Some(("handoff", "seed_flags", "reset_done")),
        0x02f0 => Some(("dcbaa", "cmd_ring", "event_ring")),
        0x02f1 => Some(("erstba", "crcr", "erdp")),
        0x02f2 => Some(("staged_dcbaap", "current_crcr", "staged_erdp")),
        0x02f3 => Some(("staged_erstba", "staged_erstsz", "erstsz")),
        0x02f4 => Some(("publish_mask", "run_usbcmd", "run_mode")),
        0x02f5 => Some(("dcbaap_off", "crcr_off", "int_base")),
        0x02f7 => Some(("policy_mask", "handoff", "seed_flags")),
        0x0316 => Some(("erdp_ack", "iman_ip", "usbsts_clear")),
        0x0317 | 0x0318 | 0x0319 => Some(("attempt", "usbcmd", "usbsts_iman")),
        0x0320 => Some(("usbcmd", "masked_usbcmd", "masked_bits")),
        0x0332 | 0x0333 => Some(("iman", "masked_iman", "seed_flags")),
        _ => None,
    }
}

#[inline]
fn xhci_diag_stage_label(stage: u16) -> Option<&'static str> {
    match stage {
        0x0117 => Some("init-policy-summary"),
        0x0200 => Some("pre-reset-usbcmd-usbsts-read"),
        0x0204 => Some("fw-handoff-skip-pre-quiesce"),
        0x0205 => Some("fw-handoff-usbcmd-mask-write"),
        0x0206 => Some("fw-handoff-usbsts-clear-write"),
        0x0207 => Some("fw-handoff-imod-write"),
        0x0208 => Some("fw-handoff-iman-write"),
        0x0209 => Some("fw-handoff-skip-usbcmd-mask-write"),
        0x020a => Some("fw-handoff-skip-usbsts-clear-write"),
        0x020b => Some("fw-handoff-skip-imod-write"),
        0x020c => Some("fw-handoff-skip-iman-write"),
        0x020d => Some("fw-handoff-trusted-usbsts-clear-skip"),
        0x020e => Some("fw-handoff-trusted-imod-skip"),
        0x020f => Some("fw-handoff-trusted-iman-skip"),
        0x0210 => Some("legacy-ownership-claim-begin"),
        0x0211 => Some("legacy-ownership-claim-done"),
        0x0212 => Some("fw-handoff-skip-legacy-ownership"),
        0x0219 => Some("pre-halt-usbcmd-quiesce-begin"),
        0x021a => Some("pre-halt-usbcmd-write-begin"),
        0x021b => Some("pre-halt-usbcmd-write-done"),
        0x021c => Some("pre-halt-usbcmd-write-skip"),
        0x022a => Some("legacy-control-clear-begin"),
        0x022b => Some("legacy-control-clear-done"),
        0x0217 => Some("stop-revalidation-decision"),
        0x0218 => Some("stop-revalidation-skip-branch"),
        0x0213 => Some("stop-revalidation-usbsts-read-begin"),
        0x0214 => Some("stop-revalidation-usbsts-read"),
        0x0215 => Some("stop-revalidation-usbcmd-read-begin"),
        0x0216 => Some("stop-revalidation-usbcmd-read"),
        0x0220 => Some("stop-revalidation-state"),
        0x0221 => Some("stop-revalidation-run-clear"),
        0x0222 => Some("stop-revalidation-timeout"),
        0x0223 => Some("stop-revalidation-halted"),
        0x0224 => Some("fw-handoff-skip-stop-revalidation"),
        0x0225 => Some("stop-revalidation-ready"),
        0x0226 => Some("reset-pre-usbcmd-read"),
        0x0227 => Some("reset-post-settle-begin"),
        0x0228 => Some("reset-post-settle-done"),
        0x0229 => Some("reset-post-cnr-poll-skip"),
        0x0230 => Some("reset-write"),
        0x023a => Some("reset-write-barrier-done"),
        0x0237 => Some("reset-write-pre-store"),
        0x0235 => Some("reset-write-issued"),
        0x0236 => Some("reset-first-readback"),
        0x023b => Some("fresh-rings-reset-required"),
        0x0238 => Some("config-write-pre-store"),
        0x0239 => Some("config-write-issued"),
        0x0231 => Some("reset-hcrst-timeout"),
        0x0232 => Some("reset-cnr-timeout"),
        0x0233 => Some("reset-complete"),
        0x0234 => Some("fw-handoff-skip-reset"),
        0x0241 => Some("config-read"),
        0x0243 => Some("config-write"),
        0x0242 => Some("dcbaap-readback"),
        0x0245 => Some("fw-handoff-skip-config-write"),
        0x0246 => Some("config-read-begin"),
        0x0247 => Some("dcbaap-readback-begin"),
        0x0244 => Some("dcbaap-write"),
        0x0248 => Some("dcbaap-write-low"),
        0x024a => Some("dcbaap-write-low-done"),
        0x0249 => Some("dcbaap-write-high"),
        0x024b => Some("dcbaap-write-high-done"),
        0x024c => Some("dcbaap-zero-write64"),
        0x024d => Some("dcbaap-zero-write64-done"),
        0x024e => Some("dcbaap-prewrite-read-begin"),
        0x024f => Some("dcbaap-prewrite-read"),
        0x0251 => Some("crcr-read"),
        0x0252 => Some("crcr-write"),
        0x0253 => Some("crcr-read-begin"),
        0x0254 => Some("crcr-write-low"),
        0x0255 => Some("crcr-write-high"),
        0x0256 => Some("dnctrl-write"),
        0x0257 => Some("dcbaap-defer-begin"),
        0x0258 => Some("dcbaap-defer-state"),
        0x0259 => Some("dcbaap-write-split-selected"),
        0x025a => Some("dcbaap-defer-publish"),
        0x0260 => Some("event-ring-base"),
        0x0261 => Some("runtime-ring-read"),
        0x0262 => Some("iman-seed"),
        0x0263 => Some("usbsts-clear-ack"),
        0x0264 => Some("erstsz-write"),
        0x0265 => Some("erstba-write"),
        0x0266 => Some("erdp-write"),
        0x0267 => Some("imod-write"),
        0x0268 => Some("iman-write"),
        0x0269 => Some("usbsts-clear-write"),
        0x026a => Some("usbcmd-run-write"),
        0x026b => Some("skip-imod-write"),
        0x026c => Some("skip-iman-write"),
        0x026d => Some("runtime-ring-read-begin"),
        0x026e => Some("erstba-write-low"),
        0x026f => Some("erstba-write-high"),
        0x02e8 => Some("fw-handoff-trusted-usbcmd-run-skip"),
        0x02ea => Some("usbcmd-run-barrier-done"),
        0x02eb => Some("usbcmd-run-barrier-begin"),
        0x02e9 => Some("usbcmd-run-pre-store"),
        0x02f0 => Some("pre-run-ring-phys"),
        0x02f1 => Some("pre-run-ring-regs"),
        0x02f2 => Some("pre-run-staged-state"),
        0x02f3 => Some("pre-run-erst-state"),
        0x02f4 => Some("pre-run-publish-mask"),
        0x02f5 => Some("pre-run-offsets"),
        0x02f6 => Some("dcbaap-release-only-high-pre-store"),
        0x02f7 => Some("dcbaap-publish-policy"),
        0x0270 => Some("usbsts-run-read"),
        0x0271 => Some("usbcmd-run-read"),
        0x0272 => Some("controller-ready-timeout"),
        0x0273 => Some("controller-ready"),
        0x0274 => Some("usbsts-run-read-begin"),
        0x0275 => Some("usbcmd-run-read-begin"),
        0x0276 => Some("controller-ready-poll-begin"),
        0x0277 => Some("erdp-write-low"),
        0x0278 => Some("erdp-write-high"),
        0x0290 => Some("dcbaap-atomic-write"),
        0x0291 => Some("dcbaap-atomic-write-begin"),
        0x0292 => Some("dcbaap-atomic-write-done"),
        0x0293 => Some("crcr-atomic-write"),
        0x0294 => Some("crcr-atomic-write-begin"),
        0x0295 => Some("crcr-atomic-write-done"),
        0x0296 => Some("erdp-atomic-write"),
        0x0297 => Some("erdp-atomic-write-begin"),
        0x0298 => Some("erdp-atomic-write-done"),
        0x0299 => Some("erstba-atomic-write"),
        0x029a => Some("erstba-atomic-write-begin"),
        0x029b => Some("erstba-atomic-write-done"),
        0x029c => Some("dcbaap-release-only-write"),
        0x029d => Some("dcbaap-release-only-low-barrier-done"),
        0x029e => Some("dcbaap-release-only-low-pre-store"),
        0x029f => Some("dcbaap-release-only-high-barrier-done"),
        0x02a0 => Some("dcbaap-defer-change-mask"),
        0x02a1 => Some("dcbaap-staged-low"),
        0x02a2 => Some("dcbaap-staged-low-done"),
        0x02a3 => Some("dcbaap-staged-high"),
        0x02a4 => Some("dcbaap-staged-high-done"),
        0x02a5 => Some("dcbaap-target-low"),
        0x02a6 => Some("dcbaap-target-low-done"),
        0x02a7 => Some("dcbaap-target-high"),
        0x02a8 => Some("dcbaap-target-high-done"),
        0x02a9 => Some("dcbaap-defer-handoff"),
        0x02aa => Some("crcr-defer-begin"),
        0x02ab => Some("crcr-defer-change-mask"),
        0x02ac => Some("crcr-staged-low"),
        0x02ad => Some("crcr-staged-low-done"),
        0x02ae => Some("crcr-staged-high"),
        0x02af => Some("crcr-staged-high-done"),
        0x02b0 => Some("crcr-target-low"),
        0x02b1 => Some("crcr-target-low-done"),
        0x02b2 => Some("crcr-target-high"),
        0x02b3 => Some("crcr-target-high-done"),
        0x02b4 => Some("crcr-defer-handoff"),
        0x02b5 => Some("erdp-defer-begin"),
        0x02b6 => Some("erdp-defer-change-mask"),
        0x02b7 => Some("erdp-staged-low"),
        0x02b8 => Some("erdp-staged-low-done"),
        0x02b9 => Some("erdp-staged-high"),
        0x02ba => Some("erdp-staged-high-done"),
        0x02bb => Some("erdp-target-low"),
        0x02bc => Some("erdp-target-low-done"),
        0x02bd => Some("erdp-target-high"),
        0x02be => Some("erdp-target-high-done"),
        0x02bf => Some("erdp-defer-handoff"),
        0x02c0 => Some("erst-defer-size"),
        0x02c1 => Some("erst-defer-base"),
        0x02c2 => Some("erstsz-publish-begin"),
        0x02c3 => Some("erstsz-publish-write"),
        0x02c4 => Some("erstsz-publish-write-done"),
        0x02c5 => Some("erstba-publish-begin"),
        0x02c6 => Some("erstba-publish-write"),
        0x02c7 => Some("erstba-publish-write-done"),
        0x02c8 => Some("erstba-publish-high"),
        0x02c9 => Some("erstba-publish-high-done"),
        0x02ca => Some("erstba-defer-handoff"),
        0x02cb => Some("erstsz-post-run-begin"),
        0x02cc => Some("erstsz-post-run-write"),
        0x02cd => Some("erstsz-post-run-write-done"),
        0x02ce => Some("erstba-post-run-begin"),
        0x02cf => Some("erstba-post-run-write"),
        0x02d0 => Some("erstba-post-run-write-done"),
        0x02d1 => Some("erstba-post-run-high"),
        0x02d2 => Some("erstba-post-run-high-done"),
        0x02d3 => Some("erstba-post-run-handoff"),
        0x02d4 => Some("dcbaap-post-run-begin"),
        0x02d5 => Some("dcbaap-post-run-done"),
        0x02d6 => Some("crcr-post-run-begin"),
        0x02d7 => Some("crcr-post-run-done"),
        0x02d8 => Some("dnctrl-post-run-begin"),
        0x02d9 => Some("dnctrl-post-run-done"),
        0x02da => Some("erstsz-publish-skip-preserve"),
        0x0310 => Some("erstba-publish-skip-preserve"),
        0x0311 => Some("erdp-publish-skip-preserve"),
        0x0312 => Some("dcbaap-publish-skip-preserve"),
        0x0313 => Some("crcr-publish-skip-preserve"),
        0x0314 => Some("dnctrl-write-skip-preserve"),
        0x0315 => Some("post-init-polling-irq-quiesce"),
        0x0316 => Some("post-run-polling-irq-quiesce"),
        0x0317 => Some("post-start-polling-irq-state"),
        0x0318 => Some("post-start-polling-irq-settled"),
        0x0319 => Some("post-start-polling-irq-timeout"),
        0x0320 => Some("post-start-usbcmd-mask-state"),
        0x0321 => Some("post-start-usbcmd-mask-write"),
        0x0322 => Some("post-start-usbcmd-mask-write-done"),
        0x0323 => Some("post-start-usbcmd-mask-skip"),
        0x0324 => Some("post-start-imod-write"),
        0x0325 => Some("post-start-imod-write-done"),
        0x0326 => Some("post-start-erdp-write-low"),
        0x0327 => Some("post-start-erdp-write-low-done"),
        0x0328 => Some("post-start-erdp-write-high"),
        0x0329 => Some("post-start-erdp-write-high-done"),
        0x032a => Some("post-start-iman-write"),
        0x032b => Some("post-start-iman-write-done"),
        0x032c => Some("post-start-usbsts-clear-write"),
        0x032d => Some("post-start-usbsts-clear-write-done"),
        0x032e => Some("post-start-erdp-skip-preserve"),
        0x032f => Some("post-start-iman-skip-preserve"),
        0x0330 => Some("post-start-usbsts-clear-skip-preserve"),
        0x0331 => Some("drop-skip-uninitialized"),
        0x0332 => Some("pre-dcbaap-iman-quiesce"),
        0x0333 => Some("pre-dcbaap-iman-quiesce-done"),
        0x0300 => Some("cmd-submit"),
        0x0301 => Some("cmd-completion"),
        0x0302 => Some("cmd-fail"),
        0x0303 => Some("cmd-ring-enqueue"),
        0x0304 => Some("cmd-ccs-expected-ptr"),
        0x0305 => Some("cmd-ccs-mismatch"),
        0x0306 => Some("cmd-fail-state"),
        0x0307 => Some("cmd-timeout"),
        0x0308 => Some("cmd-wait-other-event"),
        0x0309 => Some("cmd-timeout-state"),
        0x030a => Some("cmd-timeout-last-event"),
        _ => None,
    }
}

#[inline]
const fn xhci_root_port_connected(portsc: u32) -> bool {
    (portsc & usb_oxide::regs::PORTSC_CCS) != 0
}

#[inline]
fn xhci_connected_mask_from_portsc(port_statuses: &[u32]) -> u32 {
    let mut mask = 0u32;
    for (port, portsc) in port_statuses.iter().copied().enumerate() {
        if port < u32::BITS as usize && xhci_root_port_connected(portsc) {
            mask |= 1u32 << port;
        }
    }
    mask
}

#[inline]
fn xhci_sample_root_ports(
    ctrl: &XhciCtrl<SeatDma>,
    max_ports: usize,
    port_statuses: &mut [u32; XHCI_MAX_PROBE_PORTS],
) -> u32 {
    for status in port_statuses.iter_mut() {
        *status = 0;
    }
    let sample_ports = cmp::min(max_ports, port_statuses.len());
    for (port, status) in port_statuses.iter_mut().take(sample_ports).enumerate() {
        let xhci_port = port.saturating_add(1);
        {
            let mut line = heapless::String::<160>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] xhci root-port read-begin index={} port={} sample_ports={}",
                    port, xhci_port, sample_ports,
                ),
            );
            boot_log::force_uart_line(line.as_str());
        }
        *status = ctrl.port_status(port as u8);
        {
            let mut line = heapless::String::<160>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] xhci root-port read-done index={} port={} portsc=0x{:08x}",
                    port, xhci_port, *status,
                ),
            );
            boot_log::force_uart_line(line.as_str());
        }
    }
    xhci_connected_mask_from_portsc(&port_statuses[..sample_ports])
}

fn log_xhci_root_port_statuses(port_statuses: &[u32], stage: &str) {
    for (index, portsc) in port_statuses.iter().copied().enumerate() {
        let speed = usb_oxide::regs::portsc_speed(portsc);
        let pls = usb_oxide::regs::portsc_pls(portsc);
        let connected = xhci_root_port_connected(portsc) as u8;
        let enabled = ((portsc & usb_oxide::regs::PORTSC_PED) != 0) as u8;
        let mut line = heapless::String::<224>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] xhci root-port stage={} port={} portsc=0x{portsc:08x} ccs={} ped={} speed={} pls={}",
                stage,
                index + 1,
                connected,
                enabled,
                speed,
                pls,
            ),
        );
        boot_log::force_uart_line(line.as_str());
    }
}

#[inline]
fn usb_address_error_kind(err: UsbError) -> &'static str {
    match err {
        UsbError::DeviceNotFound => "device-not-found",
        UsbError::PortResetTimeout => "port-reset-timeout",
        UsbError::PortEnableTimeout => "port-enable-timeout",
        UsbError::EnableSlotTimeout => "enable-slot-timeout",
        UsbError::AddressDeviceTimeout => "address-device-timeout",
        UsbError::CmdFail(_) => "cmd-fail",
        UsbError::Timeout => "timeout",
        _ => "other",
    }
}

#[derive(Clone, Copy)]
struct XhciCapProbe {
    cap_length: u8,
    hci_version: u16,
    hcs1: u32,
    hcs2: u32,
    hccparams1: u32,
    db_offset: u32,
    rts_offset: u32,
    max_slots: u8,
    max_ports: u8,
    max_scratchpad: u16,
    mmio_size: usize,
}

#[inline]
const fn parse_xhci_capbase(capbase: u32) -> (u8, u16) {
    ((capbase & 0xff) as u8, ((capbase >> 16) & 0xffff) as u16)
}

fn log_xhci_cap_probe_read(mmio_base: usize, reg: &'static str, offset: usize) {
    let mut line = heapless::String::<208>::new();
    let _ = core::fmt::Write::write_fmt(
        &mut line,
        format_args!(
            "[local-seat] xhci cap probe read mmio=0x{mmio:016x} reg={reg} off=0x{offset:03x}",
            mmio = mmio_base,
        ),
    );
    boot_log::force_uart_line(line.as_str());
}

fn probe_xhci_capability_window(
    hal: &mut KernelHal<'_>,
    mmio_base: usize,
) -> Result<XhciCapProbe, &'static str> {
    let mut map_line = heapless::String::<192>::new();
    let _ = core::fmt::Write::write_fmt(
        &mut map_line,
        format_args!(
            "[local-seat] xhci cap probe map-start mmio=0x{mmio:016x} bytes=0x{bytes:05x}",
            mmio = mmio_base,
            bytes = XHCI_MMIO_INIT_BYTES
        ),
    );
    boot_log::force_uart_line(map_line.as_str());

    let dma = SeatDma::new(hal, false, XHCI_PCIE_DMA_WINDOW_ENABLED);
    // SAFETY: Read-only capability probe over candidate xHCI MMIO.
    let init_mmio = unsafe { dma.map_mmio(mmio_base, XHCI_MMIO_INIT_BYTES) }.ok_or("map-init")?;
    let mut mapped_line = heapless::String::<208>::new();
    let _ = core::fmt::Write::write_fmt(
        &mut mapped_line,
        format_args!(
            "[local-seat] xhci cap probe map-ok mmio=0x{mmio:016x} virt=0x{virt:016x}",
            mmio = mmio_base,
            virt = init_mmio
        ),
    );
    boot_log::force_uart_line(mapped_line.as_str());
    // Match usb-oxide's byte/halfword probe sequence. On Pi4, the combined
    // 32-bit CAPBASE read is the exact first runtime xHCI touch still matching
    // the fatal halt signature, so keep the first access width-bounded here.
    log_xhci_cap_probe_read(mmio_base, "caplength", regs::CAPLENGTH);
    // SAFETY: `init_mmio` points to a mapped MMIO page for volatile reads.
    let cap_length = unsafe { ptr::read_volatile(init_mmio as *const u8) };
    log_xhci_cap_probe_read(mmio_base, "hciversion", regs::CAPLENGTH + 0x2);
    // SAFETY: `init_mmio` points to a mapped MMIO page for volatile reads.
    let hci_version =
        unsafe { ptr::read_volatile((init_mmio + regs::CAPLENGTH + 0x2) as *const u16) };
    let cap_base = u32::from(cap_length) | (u32::from(hci_version) << 16);
    let mut cap_line = heapless::String::<224>::new();
    let _ = core::fmt::Write::write_fmt(
        &mut cap_line,
        format_args!(
            "[local-seat] xhci cap probe capbase-ok mmio=0x{mmio:016x} raw=0x{capbase:08x} caplen=0x{caplen:02x} hciver=0x{hciver:04x}",
            mmio = mmio_base,
            capbase = cap_base,
            caplen = cap_length,
            hciver = hci_version,
        ),
    );
    boot_log::force_uart_line(cap_line.as_str());
    log_xhci_cap_probe_read(mmio_base, "hcs1", regs::HCSPARAMS1);
    // SAFETY: `init_mmio` points to a mapped MMIO page for volatile reads.
    let hcs1 = unsafe { ptr::read_volatile((init_mmio + regs::HCSPARAMS1) as *const u32) };
    log_xhci_cap_probe_read(mmio_base, "hcs2", regs::HCSPARAMS2);
    // SAFETY: `init_mmio` points to a mapped MMIO page for volatile reads.
    let hcs2 = unsafe { ptr::read_volatile((init_mmio + regs::HCSPARAMS2) as *const u32) };
    log_xhci_cap_probe_read(mmio_base, "hccparams1", regs::HCCPARAMS1);
    // SAFETY: `init_mmio` points to a mapped MMIO page for volatile reads.
    let hccparams1 = unsafe { ptr::read_volatile((init_mmio + regs::HCCPARAMS1) as *const u32) };
    log_xhci_cap_probe_read(mmio_base, "dboff", regs::DBOFF);
    // SAFETY: `init_mmio` points to a mapped MMIO page for volatile reads.
    let db_offset_raw = unsafe { ptr::read_volatile((init_mmio + regs::DBOFF) as *const u32) };
    let db_offset = xhci_dboff_offset(db_offset_raw);
    log_xhci_cap_probe_read(mmio_base, "rtsoff", regs::RTSOFF);
    // SAFETY: `init_mmio` points to a mapped MMIO page for volatile reads.
    let rts_offset_raw = unsafe { ptr::read_volatile((init_mmio + regs::RTSOFF) as *const u32) };
    let rts_offset = xhci_rtsoff_offset(rts_offset_raw);
    let mut topo_line = heapless::String::<320>::new();
    let _ = core::fmt::Write::write_fmt(
        &mut topo_line,
        format_args!(
            "[local-seat] xhci cap probe regs-ok mmio=0x{mmio:016x} hciver=0x{hciver:04x} hcs1=0x{hcs1:08x} hcs2=0x{hcs2:08x} hcc1=0x{hccparams1:08x} dboff=0x{dboff:08x}/raw=0x{dboff_raw:08x} rtsoff=0x{rtsoff:08x}/raw=0x{rtsoff_raw:08x}",
            mmio = mmio_base,
            hciver = hci_version,
            hcs1 = hcs1,
            hcs2 = hcs2,
            hccparams1 = hccparams1,
            dboff = db_offset,
            dboff_raw = db_offset_raw,
            rtsoff = rts_offset,
            rtsoff_raw = rts_offset_raw,
        ),
    );
    boot_log::force_uart_line(topo_line.as_str());
    // SAFETY: `init_mmio` came from `map_mmio` above.
    unsafe {
        dma.unmap_mmio(init_mmio, XHCI_MMIO_INIT_BYTES);
    }

    let max_slots = (hcs1 & 0xff) as u8;
    let max_ports = ((hcs1 >> 24) & 0xff) as u8;
    let max_scratchpad = (((hcs2 >> 27) & 0x1f) | (((hcs2 >> 21) & 0x1f) << 5)) as u16;
    let mmio_size = (rts_offset as usize + 0x20 + 0x20)
        .max(db_offset as usize + (max_slots as usize + 1) * 4)
        .max(0x10000);
    Ok(XhciCapProbe {
        cap_length,
        hci_version,
        hcs1,
        hcs2,
        hccparams1,
        db_offset,
        rts_offset,
        max_slots,
        max_ports,
        max_scratchpad,
        mmio_size,
    })
}

fn validate_xhci_capability_window(probe: &XhciCapProbe) -> Result<(), &'static str> {
    if probe.cap_length < 0x20
        || (probe.cap_length as usize) >= XHCI_MMIO_INIT_BYTES
        || (probe.cap_length & 0x3) != 0
    {
        return Err("caplength");
    }
    if !(XHCI_HCI_VERSION_MIN..=XHCI_HCI_VERSION_MAX).contains(&probe.hci_version) {
        return Err("hciver");
    }
    if probe.max_slots == 0 || probe.max_ports == 0 || probe.max_scratchpad > 256 {
        return Err("topology");
    }
    if (probe.db_offset & 0x3) != 0 || (probe.rts_offset & 0x1f) != 0 {
        return Err("offset-align");
    }
    if probe.db_offset < probe.cap_length as u32 || probe.rts_offset < probe.cap_length as u32 {
        return Err("offset-range");
    }
    if !(XHCI_MMIO_INIT_BYTES..=XHCI_MMIO_MAX_BYTES).contains(&probe.mmio_size) {
        return Err("span");
    }
    Ok(())
}

#[inline]
const fn xhci_controller_should_apply_brcm_axi_setup(
    mmio: usize,
    firmware_handoff: XhciFirmwareHandoff,
) -> bool {
    if mmio == RPI4_XHCI_MMIO_HIGH_CANDIDATE {
        return false;
    }
    matches!(firmware_handoff, XhciFirmwareHandoff::None)
}

#[inline]
fn xhci_controller_params_from_probe(
    probe: XhciCapProbe,
    mmio: usize,
    firmware_handoff: XhciFirmwareHandoff,
    stop_state_snapshot: Option<LocalSeatXhciStopStateSnapshot>,
) -> XhciControllerParams {
    let runtime_seed_snapshot = match firmware_handoff {
        XhciFirmwareHandoff::None => None,
        _ => stop_state_snapshot.map(xhci_runtime_seed_snapshot_from_stop_state),
    };
    XhciControllerParams {
        cap_length: probe.cap_length,
        hcs1: probe.hcs1,
        hcs2: probe.hcs2,
        hccparams1: probe.hccparams1,
        db_offset: probe.db_offset,
        rts_offset: probe.rts_offset,
        firmware_handoff,
        runtime_seed_snapshot,
        apply_brcm_axi_setup: xhci_controller_should_apply_brcm_axi_setup(mmio, firmware_handoff),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct XhciRuntimeInitStrategy {
    firmware_handoff: XhciFirmwareHandoff,
    seed_stop_state: bool,
}

impl XhciRuntimeInitStrategy {
    #[inline]
    const fn new(firmware_handoff: XhciFirmwareHandoff, seed_stop_state: bool) -> Self {
        Self {
            firmware_handoff,
            seed_stop_state,
        }
    }
}

#[inline]
fn xhci_controller_params_from_probe_with_strategy(
    probe: XhciCapProbe,
    mmio: usize,
    strategy: XhciRuntimeInitStrategy,
    stop_state_snapshot: Option<LocalSeatXhciStopStateSnapshot>,
) -> XhciControllerParams {
    let runtime_seed_snapshot = if strategy.seed_stop_state {
        stop_state_snapshot.map(xhci_runtime_seed_snapshot_from_stop_state)
    } else {
        None
    };
    XhciControllerParams {
        cap_length: probe.cap_length,
        hcs1: probe.hcs1,
        hcs2: probe.hcs2,
        hccparams1: probe.hccparams1,
        db_offset: probe.db_offset,
        rts_offset: probe.rts_offset,
        firmware_handoff: strategy.firmware_handoff,
        runtime_seed_snapshot,
        apply_brcm_axi_setup: xhci_controller_should_apply_brcm_axi_setup(
            mmio,
            strategy.firmware_handoff,
        ) && !strategy.seed_stop_state,
    }
}

const XHCI_RUNTIME_INIT_STRATEGY_MAX: usize = 4;

#[inline]
const fn xhci_runtime_init_strategy_policy_label(
    strategy: XhciRuntimeInitStrategy,
) -> &'static str {
    match (strategy.firmware_handoff, strategy.seed_stop_state) {
        (XhciFirmwareHandoff::PreserveControllerState, _) => "preserve-state",
        (XhciFirmwareHandoff::ColdStartFromSnapshot, true) => "bootloader-owned-pollsafe",
        (XhciFirmwareHandoff::None, true) => "resetless-stop-seed",
        (XhciFirmwareHandoff::ColdStartFromSnapshot, false)
        | (XhciFirmwareHandoff::None, false) => "full-reset-start",
        (XhciFirmwareHandoff::ResetlessReinit, _) => "resetless-reinit",
    }
}

#[inline]
fn xhci_runtime_init_strategy_push(
    strategies: &mut [XhciRuntimeInitStrategy; XHCI_RUNTIME_INIT_STRATEGY_MAX],
    count: &mut usize,
    strategy: XhciRuntimeInitStrategy,
) {
    if *count >= strategies.len() || strategies[..*count].contains(&strategy) {
        return;
    }
    strategies[*count] = strategy;
    *count += 1;
}

#[inline]
fn xhci_runtime_init_strategies(
    preferred_handoff: XhciFirmwareHandoff,
    runtime_vl805_reset_state: u8,
    stop_state_snapshot: Option<LocalSeatXhciStopStateSnapshot>,
) -> (
    [XhciRuntimeInitStrategy; XHCI_RUNTIME_INIT_STRATEGY_MAX],
    usize,
) {
    let mut strategies = [XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::None, false);
        XHCI_RUNTIME_INIT_STRATEGY_MAX];
    let mut count = 0usize;
    let stop_state_seed_available = stop_state_snapshot.is_some();
    if matches!(preferred_handoff, XhciFirmwareHandoff::None) {
        xhci_runtime_init_strategy_push(
            &mut strategies,
            &mut count,
            XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::None, false),
        );
        return (strategies, count);
    }

    if matches!(
        preferred_handoff,
        XhciFirmwareHandoff::PreserveControllerState
    ) {
        if stop_state_seed_available {
            xhci_runtime_init_strategy_push(
                &mut strategies,
                &mut count,
                XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::PreserveControllerState, true),
            );
            xhci_runtime_init_strategy_push(
                &mut strategies,
                &mut count,
                XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::ColdStartFromSnapshot, true),
            );
            xhci_runtime_init_strategy_push(
                &mut strategies,
                &mut count,
                XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::ColdStartFromSnapshot, false),
            );
        } else {
            xhci_runtime_init_strategy_push(
                &mut strategies,
                &mut count,
                XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::ColdStartFromSnapshot, false),
            );
        }
        return (strategies, count);
    }

    if matches!(
        preferred_handoff,
        XhciFirmwareHandoff::ColdStartFromSnapshot
    ) {
        let reset_authorizes_hcrst =
            runtime_vl805_mailbox_reset_authorizes_hcrst(runtime_vl805_reset_state);
        if reset_authorizes_hcrst && stop_state_seed_available {
            xhci_runtime_init_strategy_push(
                &mut strategies,
                &mut count,
                XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::None, true),
            );
        }
        if reset_authorizes_hcrst {
            xhci_runtime_init_strategy_push(
                &mut strategies,
                &mut count,
                XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::ColdStartFromSnapshot, false),
            );
        }
        if stop_state_seed_available {
            xhci_runtime_init_strategy_push(
                &mut strategies,
                &mut count,
                XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::ColdStartFromSnapshot, true),
            );
        }
        return (strategies, count);
    } else {
        xhci_runtime_init_strategy_push(
            &mut strategies,
            &mut count,
            XhciRuntimeInitStrategy::new(preferred_handoff, false),
        );
        if stop_state_seed_available {
            xhci_runtime_init_strategy_push(
                &mut strategies,
                &mut count,
                XhciRuntimeInitStrategy::new(preferred_handoff, true),
            );
        }
    }

    if count == 0 {
        xhci_runtime_init_strategy_push(
            &mut strategies,
            &mut count,
            XhciRuntimeInitStrategy::new(preferred_handoff, false),
        );
    }
    (strategies, count)
}

#[inline]
const fn xhci_runtime_init_strategy_origin_label(
    strategy: XhciRuntimeInitStrategy,
) -> &'static str {
    match (strategy.firmware_handoff, strategy.seed_stop_state) {
        (XhciFirmwareHandoff::None, false) => "live-runtime-default",
        (XhciFirmwareHandoff::None, true) => "resetless-stop-seed",
        (XhciFirmwareHandoff::ColdStartFromSnapshot, false) => "uboot-fresh-init",
        (XhciFirmwareHandoff::ColdStartFromSnapshot, true) => "seeded-cold-start",
        (XhciFirmwareHandoff::ResetlessReinit, true) => "stop-state-resetless-reinit",
        (XhciFirmwareHandoff::PreserveControllerState, true) => "stop-state-preserve",
        (XhciFirmwareHandoff::ResetlessReinit, false) => "resetless-reinit",
        (XhciFirmwareHandoff::PreserveControllerState, false) => "preserve-controller-state",
    }
}

#[inline]
const fn xhci_runtime_init_strategy_seed_label(strategy: XhciRuntimeInitStrategy) -> &'static str {
    if strategy.seed_stop_state {
        "stop-state"
    } else {
        "none"
    }
}

#[inline]
const fn xhci_runtime_init_strategy_halt_guard_label(
    strategy: XhciRuntimeInitStrategy,
) -> &'static str {
    if xhci_runtime_init_strategy_skips_live_halt_read(strategy) {
        "skip-live-halt-read"
    } else {
        "live-halt-read"
    }
}

#[inline]
const fn xhci_runtime_init_strategy_skips_live_halt_read(
    strategy: XhciRuntimeInitStrategy,
) -> bool {
    matches!(
        strategy.firmware_handoff,
        XhciFirmwareHandoff::ResetlessReinit | XhciFirmwareHandoff::PreserveControllerState
    ) || (strategy.seed_stop_state
        && matches!(
            strategy.firmware_handoff,
            XhciFirmwareHandoff::None | XhciFirmwareHandoff::ColdStartFromSnapshot
        ))
}

#[inline]
const fn xhci_runtime_init_strategy_skips_root_port_reads(
    strategy: XhciRuntimeInitStrategy,
) -> bool {
    matches!(
        strategy.firmware_handoff,
        XhciFirmwareHandoff::PreserveControllerState
    ) || (strategy.seed_stop_state
        && matches!(
            strategy.firmware_handoff,
            XhciFirmwareHandoff::ColdStartFromSnapshot
        ))
}

#[inline]
const fn xhci_runtime_init_strategy_skips_controller_entry(
    strategy: XhciRuntimeInitStrategy,
) -> bool {
    strategy.seed_stop_state
        && matches!(
            strategy.firmware_handoff,
            XhciFirmwareHandoff::ColdStartFromSnapshot
        )
}

#[inline]
const fn xhci_runtime_init_strategy_constructor_label(
    strategy: XhciRuntimeInitStrategy,
) -> &'static str {
    if strategy.seed_stop_state
        && matches!(
            strategy.firmware_handoff,
            XhciFirmwareHandoff::ColdStartFromSnapshot
        )
    {
        "pre-halt-usbcmd-quiesce"
    } else if matches!(
        strategy.firmware_handoff,
        XhciFirmwareHandoff::ColdStartFromSnapshot
            | XhciFirmwareHandoff::ResetlessReinit
            | XhciFirmwareHandoff::PreserveControllerState
    ) {
        "trusted-quiesce"
    } else if strategy.seed_stop_state {
        "trusted-quiesce"
    } else {
        "full-quiesce"
    }
}

#[inline]
const fn xhci_runtime_init_strategy_pre_reset_label(
    strategy: XhciRuntimeInitStrategy,
) -> &'static str {
    if strategy.seed_stop_state
        || matches!(
            strategy.firmware_handoff,
            XhciFirmwareHandoff::ResetlessReinit | XhciFirmwareHandoff::PreserveControllerState
        )
    {
        "skip-pre-reset"
    } else {
        "full-pre-reset"
    }
}

#[inline]
const fn xhci_runtime_init_strategy_legacy_label(
    strategy: XhciRuntimeInitStrategy,
) -> &'static str {
    if matches!(
        strategy.firmware_handoff,
        XhciFirmwareHandoff::ColdStartFromSnapshot
            | XhciFirmwareHandoff::ResetlessReinit
            | XhciFirmwareHandoff::PreserveControllerState
    ) || strategy.seed_stop_state
    {
        "skip-legacy"
    } else {
        "claim-legacy"
    }
}

#[inline]
const fn xhci_runtime_init_strategy_run_label(strategy: XhciRuntimeInitStrategy) -> &'static str {
    match (strategy.firmware_handoff, strategy.seed_stop_state) {
        (XhciFirmwareHandoff::PreserveControllerState, _) => "run-skip",
        (XhciFirmwareHandoff::ColdStartFromSnapshot, true) => "run-skip",
        (XhciFirmwareHandoff::ColdStartFromSnapshot, _) | (XhciFirmwareHandoff::None, true) => {
            "run-uboot"
        }
        _ => "run-default",
    }
}

#[inline]
const fn xhci_runtime_init_strategy_publish_label(
    strategy: XhciRuntimeInitStrategy,
) -> &'static str {
    if matches!(
        strategy.firmware_handoff,
        XhciFirmwareHandoff::ResetlessReinit
    ) {
        "rings-post-run"
    } else if strategy.seed_stop_state
        && matches!(
            strategy.firmware_handoff,
            XhciFirmwareHandoff::ColdStartFromSnapshot
        )
    {
        "rings-skip"
    } else {
        "rings-pre-run"
    }
}

#[inline]
const fn xhci_runtime_init_strategy_post_ready_irq_label(
    strategy: XhciRuntimeInitStrategy,
) -> &'static str {
    if matches!(
        strategy.firmware_handoff,
        XhciFirmwareHandoff::ColdStartFromSnapshot
            | XhciFirmwareHandoff::ResetlessReinit
            | XhciFirmwareHandoff::PreserveControllerState
    ) || strategy.seed_stop_state
    {
        "irq-skip"
    } else {
        "irq-zero"
    }
}

#[inline]
const fn xhci_runtime_seed_snapshot_from_stop_state(
    snapshot: LocalSeatXhciStopStateSnapshot,
) -> XhciRuntimeSeedSnapshot {
    XhciRuntimeSeedSnapshot {
        usbcmd: snapshot.usbcmd,
        usbsts: snapshot.usbsts,
        iman0: snapshot.iman0,
        dcbaap: None,
        crcr: None,
        erstba0: None,
        erdp0: None,
        erstsz0: None,
    }
}

#[inline]
const fn xhci_runtime_seed_snapshot_flag_bits(snapshot: Option<XhciRuntimeSeedSnapshot>) -> u8 {
    let mut flags = 0u8;
    if snapshot.is_some() {
        flags |= 1 << 0;
    }
    if let Some(snapshot) = snapshot {
        if snapshot.usbcmd.is_some() || snapshot.usbsts.is_some() || snapshot.iman0.is_some() {
            flags |= 1 << 1;
        }
        if snapshot.dcbaap.is_some()
            || snapshot.crcr.is_some()
            || snapshot.erstba0.is_some()
            || snapshot.erdp0.is_some()
            || snapshot.erstsz0.is_some()
        {
            flags |= 1 << 2;
        }
    }
    flags
}

#[inline]
fn xhci_cap_probe_from_snapshot(snapshot: LocalSeatXhciCapabilitySnapshot) -> XhciCapProbe {
    let max_slots = (snapshot.hcs1 & 0xff) as u8;
    let max_ports = ((snapshot.hcs1 >> 24) & 0xff) as u8;
    let max_scratchpad =
        (((snapshot.hcs2 >> 27) & 0x1f) | (((snapshot.hcs2 >> 21) & 0x1f) << 5)) as u16;
    let db_offset = xhci_dboff_offset(snapshot.db_offset);
    let rts_offset = xhci_rtsoff_offset(snapshot.rts_offset);
    let mmio_size = (rts_offset as usize + 0x20 + 0x20)
        .max(db_offset as usize + (max_slots as usize + 1) * 4)
        .max(0x10000);
    XhciCapProbe {
        cap_length: snapshot.cap_length,
        hci_version: snapshot.hci_version,
        hcs1: snapshot.hcs1,
        hcs2: snapshot.hcs2,
        hccparams1: snapshot.hccparams1,
        db_offset,
        rts_offset,
        max_slots,
        max_ports,
        max_scratchpad,
        mmio_size,
    }
}

fn cache_xhci_capability_probe_from_snapshot(
    snapshot: LocalSeatXhciCapabilitySnapshot,
) -> Option<XhciCapProbe> {
    let probe = xhci_cap_probe_from_snapshot(snapshot);
    if let Err(reason) = validate_xhci_capability_window(&probe) {
        let mut line = heapless::String::<224>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] xhci cap snapshot cache=reject detail={reason} action=drop-runtime-probe"
            ),
        );
        boot_log::force_uart_line(line.as_str());
        return None;
    }
    let mut line = heapless::String::<224>::new();
    let _ = core::fmt::Write::write_fmt(
        &mut line,
        format_args!(
            "[local-seat] xhci cap snapshot cache=ready caplen=0x{caplen:02x} hciver=0x{hciver:04x} span=0x{span:05x}",
            caplen = probe.cap_length,
            hciver = probe.hci_version,
            span = probe.mmio_size,
        ),
    );
    boot_log::force_uart_line(line.as_str());
    Some(probe)
}

fn xhci_alias_scan_candidate(mmio_base: usize, step: usize) -> Option<usize> {
    if step == 0 || step > XHCI_MMIO_ALIAS_SCAN_STEPS {
        return None;
    }
    mmio_base.checked_add(step.checked_mul(XHCI_MMIO_ALIAS_SCAN_STRIDE_BYTES)?)
}

fn probe_xhci_capability_with_alias_scan(
    hal: &mut KernelHal<'_>,
    mmio_base: usize,
) -> Result<(usize, XhciCapProbe), &'static str> {
    let probe = probe_xhci_capability_window(hal, mmio_base)?;
    if validate_xhci_capability_window(&probe).is_ok() {
        return Ok((mmio_base, probe));
    }

    // The Pi4 firmware/UEFI path can expose the VL805 xHCI block at a
    // higher 64 KiB-aligned offset inside the legacy aliases. Probe only
    // aligned controller windows so the recovery path stays bounded and does
    // not thrash arbitrary 4 KiB pages while boot logging is still limited.
    if mmio_base != RPI4_XHCI_MMIO_HIGH_CANDIDATE
        && mmio_base != RPI4_XHCI_MMIO_PRIMARY_CANDIDATE
        && mmio_base != RPI4_XHCI_MMIO_SECONDARY_CANDIDATE
    {
        return Err("cap-invalid");
    }

    let mut scan_line = heapless::String::<176>::new();
    let _ = core::fmt::Write::write_fmt(
        &mut scan_line,
        format_args!(
            "[local-seat] xhci cap scan base=0x{base:016x} steps={steps} stride=0x{stride:05x}",
            base = mmio_base,
            steps = XHCI_MMIO_ALIAS_SCAN_STEPS,
            stride = XHCI_MMIO_ALIAS_SCAN_STRIDE_BYTES,
        ),
    );
    boot_log::force_uart_line(scan_line.as_str());

    for step in 1..=XHCI_MMIO_ALIAS_SCAN_STEPS {
        let Some(candidate) = xhci_alias_scan_candidate(mmio_base, step) else {
            break;
        };
        if !xhci_mmio_candidate_valid(candidate) {
            continue;
        }
        let scanned = match probe_xhci_capability_window(hal, candidate) {
            Ok(scanned) => scanned,
            Err(_) => continue,
        };
        if validate_xhci_capability_window(&scanned).is_ok() {
            let mut line = heapless::String::<224>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] xhci cap relocated base=0x{base:016x} scan=0x{scan:016x} step={} stride=0x{stride:05x}",
                    step,
                    stride = XHCI_MMIO_ALIAS_SCAN_STRIDE_BYTES,
                    base = mmio_base,
                    scan = candidate
                ),
            );
            boot_log::force_uart_line(line.as_str());
            return Ok((candidate, scanned));
        }
    }

    let mut exhausted = heapless::String::<192>::new();
    let _ = core::fmt::Write::write_fmt(
        &mut exhausted,
        format_args!(
            "[local-seat] xhci cap scan exhausted base=0x{base:016x} steps={steps}",
            base = mmio_base,
            steps = XHCI_MMIO_ALIAS_SCAN_STEPS
        ),
    );
    boot_log::force_uart_line(exhausted.as_str());

    Err("cap-invalid")
}

fn prime_pinned_vl805_cfg_window(hal: &mut KernelHal<'_>, mode: Vl805CfgPreseedMode) {
    if PINNED_VL805_CFG.lock().is_some() {
        return;
    }

    let mut prefix_frames = Vec::new();
    for &ecam_base in &VL805_ECAM_BASE_CANDIDATES {
        let Some(config_paddr) = ecam_base.checked_add(VL805_PCI_DEV_ADDR as usize) else {
            continue;
        };
        let config_page = config_paddr & !PAGE_MASK;
        let mut begin = heapless::String::<208>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut begin,
            format_args!(
                "[local-seat] vl805 cfg preseed stage=map-begin ecam=0x{ecam:016x} cfg=0x{cfg:016x}",
                ecam = ecam_base,
                cfg = config_paddr
            ),
        );
        boot_log::force_uart_line(begin.as_str());
        let frame = match map_device_exact(
            hal,
            config_page,
            VL805_PCI_CFG_ATTEMPT_CAP,
            "vl805-preseed",
            Pi4SeatError::XhciInit,
            &mut prefix_frames,
        ) {
            Ok(frame) => frame,
            Err(_) => continue,
        };
        let virt = frame.ptr().as_ptr() as usize;
        let config_page_offset = (VL805_PCI_DEV_ADDR as usize) & PAGE_MASK;
        let config_virt = match virt.checked_add(config_page_offset) {
            Some(cfg) => cfg,
            None => continue,
        };
        let mut mapped = heapless::String::<224>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut mapped,
            format_args!(
                "[local-seat] vl805 cfg preseed stage=map-ok ecam=0x{ecam:016x} cfg=0x{cfg:016x} virt=0x{virt:016x}",
                ecam = ecam_base,
                cfg = config_paddr,
                virt = config_virt
            ),
        );
        boot_log::force_uart_line(mapped.as_str());
        remember_vl805_cfg_virt(config_virt);

        core::mem::forget(frame);

        let mut pinned = PINNED_VL805_CFG.lock();
        *pinned = Some(PinnedMmioWindow {
            phys_start: config_page,
            length: PAGE_SIZE,
            virt_start: virt,
            trusted_for_runtime: false,
        });

        if matches!(mode, Vl805CfgPreseedMode::ReadMostly) {
            boot_log::force_uart_line("[local-seat] vl805 cfg preseed stage=cfg-read-begin");
            let snapshot_before = read_vl805_pci_cfg_snapshot(config_virt);
            let command_before = snapshot_before.command;
            // Keep VL805 INTx masked during bring-up to avoid fatal interrupt storms
            // while still enabling memory decode + DMA for xHCI rings.
            let command_required = command_before
                | PCI_COMMAND_MEMORY_SPACE
                | PCI_COMMAND_BUS_MASTER
                | PCI_COMMAND_INTERRUPT_DISABLE;
            if command_required != command_before {
                pci_cfg_write_u16(config_virt, PCI_CFG_COMMAND_STATUS, command_required);
            }
            let snapshot_after = read_vl805_pci_cfg_snapshot(config_virt);
            let command_after = snapshot_after.command;

            let mut line = heapless::String::<240>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] vl805 pci cfg preseeded ecam=0x{ecam:016x} cfg=0x{cfg:016x} mode=read-mostly cfg_id=0x{cfg_id:08x} class=0x{class:06x} cmd=0x{before:04x}->0x{after:04x} bar0=0x{bar0:08x}",
                    ecam = ecam_base,
                    cfg = config_paddr,
                    cfg_id = snapshot_after.vendor_device,
                    class = (snapshot_after.class_revision >> 8) & 0x00ff_ffff,
                    before = command_before,
                    after = command_after,
                    bar0 = snapshot_after.bar0,
                ),
            );
            boot_log::force_uart_line(line.as_str());
            let mut bar_line = heapless::String::<176>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut bar_line,
                format_args!(
                    "[local-seat] vl805 pci cfg bar bar0=0x{bar0:08x} bar1=0x{bar1:08x}",
                    bar0 = snapshot_after.bar0,
                    bar1 = snapshot_after.bar1
                ),
            );
            boot_log::force_uart_line(bar_line.as_str());
            if let Some(mmio) = record_vl805_xhci_mmio_hint(&snapshot_after, "rw-cfg") {
                let has_coverage = hal.device_coverage(mmio, crate::sel4::PAGE_BITS).is_some();
                let has_pinned = pinned_xhci_window_lookup(mmio, PAGE_SIZE).is_some();
                let mut handoff_line = heapless::String::<240>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut handoff_line,
                    format_args!(
                        "[local-seat] vl805 pci cfg handoff mmio=0x{mmio:016x} coverage={} pinned={} cmd_safe={} source=rw-cfg",
                        if has_coverage { "yes" } else { "no" },
                        if has_pinned { "yes" } else { "no" },
                        xhci_firmware_handoff_safe(Some(command_after)) as u8,
                    ),
                );
                boot_log::force_uart_line(handoff_line.as_str());
            }
            if (command_after & (PCI_COMMAND_MEMORY_SPACE | PCI_COMMAND_BUS_MASTER))
                != (PCI_COMMAND_MEMORY_SPACE | PCI_COMMAND_BUS_MASTER)
            {
                boot_log::force_uart_line(
                    "[local-seat] vl805 pci cfg warning command bits missing after preseed",
                );
            }
        } else {
            let mut line = heapless::String::<176>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] vl805 pci cfg preseeded ecam=0x{ecam:016x} cfg=0x{cfg:016x} mode=map-only cfg-read=deferred reason=irq27-on-ecam-read",
                    ecam = ecam_base,
                    cfg = config_paddr,
                ),
            );
            boot_log::force_uart_line(line.as_str());
        }
        return;
    }

    boot_log::force_uart_line("[local-seat] vl805 pci cfg preseed unavailable");
}

fn prime_pinned_xhci_window(
    hal: &mut KernelHal<'_>,
    xhci_mmio_hint: Option<usize>,
    xhci_pci_cmd: Option<u16>,
    xhci_handoff_ready: bool,
    xhci_irq_quiesced: bool,
) {
    if PINNED_XHCI_MMIO.lock().is_some() {
        if !XHCI_PRESEED_ALREADY_PINNED_LOGGED.swap(true, Ordering::AcqRel) {
            boot_log::force_uart_line("[local-seat] xhci preseed skipped (already pinned)");
        }
        return;
    }

    let vl805_hint = current_vl805_xhci_mmio_hint();
    let mut candidates = [0usize; XHCI_MMIO_CANDIDATE_LIMIT];
    let mut candidate_count = 0usize;
    let mut push_candidate = |mmio: usize| {
        if candidate_count >= candidates.len() {
            return;
        }
        if !xhci_mmio_candidate_valid(mmio) {
            let mut line = heapless::String::<176>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] xhci preseed ignore mmio=0x{mmio:016x} reason=invalid-candidate"
                ),
            );
            boot_log::force_uart_line(line.as_str());
            return;
        }
        if candidates[..candidate_count].contains(&mmio) {
            return;
        }
        candidates[candidate_count] = mmio;
        candidate_count = candidate_count.saturating_add(1);
    };

    if let Some(hint) = vl805_hint {
        push_candidate(hint);
    }
    if let Some(hint) = xhci_mmio_hint {
        push_candidate(hint);
    }
    if let (Some(firmware_hint), Some(vl805_hint)) = (xhci_mmio_hint, vl805_hint) {
        if firmware_hint != vl805_hint {
            let mut line = heapless::String::<224>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] xhci preseed prefer vl805 hint=0x{vl805:016x} over firmware hint=0x{firmware:016x}",
                    vl805 = vl805_hint,
                    firmware = firmware_hint
                ),
            );
            boot_log::force_uart_line(line.as_str());
        }
    }
    if xhci_preseed_allows_static_legacy_fallbacks(vl805_hint) {
        for fallback in RPI4_XHCI_MMIO_PRESEED_CANDIDATES {
            push_candidate(fallback);
        }
    }
    let mut plan = heapless::String::<192>::new();
    let _ = core::fmt::Write::write_fmt(
        &mut plan,
        format_args!(
            "[local-seat] xhci preseed candidates={} hint={} vl805_hint={}",
            candidate_count,
            if xhci_mmio_hint.is_some() {
                "yes"
            } else {
                "no"
            },
            if vl805_hint.is_some() { "yes" } else { "no" }
        ),
    );
    boot_log::force_uart_line(plan.as_str());
    if candidate_count == 0 {
        boot_log::force_uart_line(
            "[local-seat] xhci preseed deferred reason=no-safe-mmio-candidates",
        );
        return;
    }

    for &mmio in &candidates[..candidate_count] {
        // Keep early preseed small so later critical MMIO mappings (UART, etc.)
        // are not starved during bootstrap. usb-oxide computes the runtime MMIO
        // span and this bounded window is enough for capability probing.
        let preseed_lengths = [
            XHCI_MMIO_PRESEED_BYTES_MAX,
            0x20_000,
            XHCI_MMIO_PRESEED_BYTES_FALLBACK,
        ];
        for &length in &preseed_lengths {
            let mut attempt = heapless::String::<200>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut attempt,
                format_args!(
                    "[local-seat] xhci preseed attempt mmio=0x{mmio:016x} bytes=0x{bytes:05x}",
                    bytes = length
                ),
            );
            boot_log::force_uart_line(attempt.as_str());
            if xhci_firmware_handoff_cold_start_trusted(
                mmio,
                xhci_mmio_hint,
                xhci_pci_cmd,
                xhci_handoff_ready,
                xhci_irq_quiesced,
            ) {
                let mut line = heapless::String::<224>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] xhci preseed trusted mmio=0x{mmio:016x} reason=fw-handoff-cold-start cmd=0x{cmd:04x} irq=1",
                        cmd = xhci_pci_cmd.unwrap_or(0),
                    ),
                );
                boot_log::force_uart_line(line.as_str());
                if pin_xhci_mmio_window(hal, mmio, length, true).is_ok() {
                    let mut line = heapless::String::<224>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut line,
                        format_args!(
                            "[local-seat] xhci mmio preseeded mmio=0x{mmio:016x} bytes=0x{bytes:05x} mode=trusted-cold-start",
                            bytes = length
                        ),
                    );
                    boot_log::force_uart_line(line.as_str());
                    boot_log::force_uart_line(
                        "[local-seat] vl805 reset handoff=bootloader-owned stage=preseed detail=chosen-ready-token+irq-quiesced",
                    );
                    return;
                }
                let mut fail = heapless::String::<224>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut fail,
                    format_args!(
                        "[local-seat] xhci preseed failed mmio=0x{mmio:016x} bytes=0x{bytes:05x} mode=trusted-cold-start",
                        bytes = length
                    ),
                );
                boot_log::force_uart_line(fail.as_str());
                continue;
            }
            if let Some(reason) = xhci_preseed_pin_only_reason(
                mmio,
                xhci_mmio_hint,
                vl805_hint,
                xhci_pci_cmd,
                xhci_handoff_ready,
                xhci_irq_quiesced,
            ) {
                let mut line = heapless::String::<224>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] xhci preseed pin-only mmio=0x{mmio:016x} reason={reason}"
                    ),
                );
                boot_log::force_uart_line(line.as_str());
                if pin_xhci_mmio_window(hal, mmio, length, false).is_ok() {
                    let mut line = heapless::String::<208>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut line,
                        format_args!(
                            "[local-seat] xhci mmio preseeded mmio=0x{mmio:016x} bytes=0x{bytes:05x} mode=pin-only",
                            bytes = length
                        ),
                    );
                    boot_log::force_uart_line(line.as_str());
                    return;
                }
                let mut fail = heapless::String::<224>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut fail,
                    format_args!(
                        "[local-seat] xhci preseed failed mmio=0x{mmio:016x} bytes=0x{bytes:05x} mode=pin-only",
                        bytes = length
                    ),
                );
                boot_log::force_uart_line(fail.as_str());
                continue;
            }
            let (validated_mmio, probe) = match probe_xhci_capability_with_alias_scan(hal, mmio) {
                Ok(validated) => validated,
                Err(reason) => {
                    let mut line = heapless::String::<224>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut line,
                        format_args!(
                            "[local-seat] xhci preseed reject mmio=0x{mmio:016x} detail={reason}",
                        ),
                    );
                    boot_log::force_uart_line(line.as_str());
                    continue;
                }
            };
            let pin_length = core::cmp::max(length, probe.mmio_size);
            if validated_mmio != mmio {
                let mut line = heapless::String::<224>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] xhci preseed relocate base=0x{base:016x} pinned=0x{pinned:016x}",
                        base = mmio,
                        pinned = validated_mmio,
                    ),
                );
                boot_log::force_uart_line(line.as_str());
            }
            if pin_xhci_mmio_window(hal, validated_mmio, pin_length, true).is_ok() {
                remember_vl805_xhci_mmio_hint(validated_mmio);
                let mut line = heapless::String::<176>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] xhci mmio preseeded mmio=0x{mmio:016x} bytes=0x{bytes:05x}",
                        mmio = validated_mmio,
                        bytes = pin_length
                    ),
                );
                boot_log::force_uart_line(line.as_str());
                return;
            }
            let mut fail = heapless::String::<200>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut fail,
                format_args!(
                    "[local-seat] xhci preseed failed mmio=0x{mmio:016x} bytes=0x{bytes:05x}",
                    bytes = length
                ),
            );
            boot_log::force_uart_line(fail.as_str());
        }
    }
    boot_log::force_uart_line("[local-seat] xhci preseed exhausted all candidates");
}

fn log_xhci_handoff_window_state(
    hal: &KernelHal<'_>,
    xhci_mmio_hint: Option<usize>,
    xhci_pci_cmd: Option<u16>,
) {
    let Some(mmio) = xhci_mmio_hint else {
        return;
    };
    let has_coverage = hal.device_coverage(mmio, crate::sel4::PAGE_BITS).is_some();
    let has_pinned = pinned_xhci_window_lookup(mmio, PAGE_SIZE).is_some();
    let mut line = heapless::String::<240>::new();
    let _ = core::fmt::Write::write_fmt(
        &mut line,
        format_args!(
            "[local-seat] xhci handoff window mmio=0x{mmio:016x} coverage={} pinned={} cmd_safe={} source=chosen",
            if has_coverage { "yes" } else { "no" },
            if has_pinned { "yes" } else { "no" },
            xhci_firmware_handoff_safe(xhci_pci_cmd) as u8,
        ),
    );
    boot_log::force_uart_line(line.as_str());
}

fn pin_xhci_mmio_window(
    hal: &mut KernelHal<'_>,
    phys_start: usize,
    length: usize,
    trusted_for_runtime: bool,
) -> Result<(), Pi4SeatError> {
    if (phys_start & PAGE_MASK) != 0
        || length < XHCI_MMIO_INIT_BYTES
        || length > XHCI_MMIO_MAX_BYTES
    {
        let mut line = heapless::String::<208>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] xhci preseed reject mmio=0x{mmio:016x} bytes=0x{bytes:05x} reason=invalid-range",
                mmio = phys_start,
                bytes = length
            ),
        );
        boot_log::force_uart_line(line.as_str());
        return Err(Pi4SeatError::XhciInit);
    }
    let mut prefix_frames = Vec::new();
    let first = map_device_exact(
        hal,
        phys_start,
        XHCI_MMIO_MAP_EXACT_ATTEMPT_CAP,
        "xhci-preseed",
        Pi4SeatError::XhciInit,
        &mut prefix_frames,
    )?;
    let first_virt = first.ptr().as_ptr() as usize;
    let page_count = div_ceil(length, PAGE_SIZE);
    if page_count == 0 {
        boot_log::force_uart_line("[local-seat] xhci preseed reject reason=zero-pages");
        return Err(Pi4SeatError::XhciInit);
    }

    let mut frames = Vec::with_capacity(page_count);
    frames.push(first);
    for page in 1..page_count {
        let paddr = phys_start
            .checked_add(page.checked_mul(PAGE_SIZE).ok_or(Pi4SeatError::XhciInit)?)
            .ok_or(Pi4SeatError::XhciInit)?;
        let frame = hal.map_device(paddr).map_err(|_| Pi4SeatError::XhciInit)?;
        let got = frame.ptr().as_ptr() as usize;
        let expected = first_virt
            .checked_add(page.checked_mul(PAGE_SIZE).ok_or(Pi4SeatError::XhciInit)?)
            .ok_or(Pi4SeatError::XhciInit)?;
        if got != expected || frame.paddr() != paddr {
            let mut line = heapless::String::<240>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] xhci preseed page mismatch page={} want_paddr=0x{want_paddr:016x} got_paddr=0x{got_paddr:016x} want_vaddr=0x{want_vaddr:016x} got_vaddr=0x{got_vaddr:016x}",
                    page,
                    want_paddr = paddr,
                    got_paddr = frame.paddr(),
                    want_vaddr = expected,
                    got_vaddr = got
                ),
            );
            boot_log::force_uart_line(line.as_str());
            return Err(Pi4SeatError::XhciInit);
        }
        frames.push(frame);
    }

    // Keep mapped xHCI MMIO frames alive for the lifetime of the root-task so
    // runtime keyboard attach can reuse them even after device coverage
    // metadata is unavailable.
    for frame in frames {
        core::mem::forget(frame);
    }

    let mut pinned = PINNED_XHCI_MMIO.lock();
    *pinned = Some(PinnedMmioWindow {
        phys_start,
        length,
        virt_start: first_virt,
        trusted_for_runtime,
    });
    Ok(())
}

impl Mailbox {
    fn new(hal: &mut KernelHal<'_>) -> Result<Self, Pi4SeatError> {
        let mut regs = None;
        let mut regs_vaddr = 0usize;
        let mut regs_paddr = 0usize;
        let mut prefix_maps = Vec::new();
        if let Some((paddr, vaddr)) = pi4_wifi::pinned_mailbox_regs() {
            regs_paddr = paddr;
            regs_vaddr = vaddr;
            boot_log::force_uart_line(
                "[local-seat] mailbox regs reuse=pinned source=pi4-wifi-preseed",
            );
        } else {
            for &candidate in &MAILBOX_PAGE_PADDR_CANDIDATES {
                if let Ok(mapped) = Self::map_device_exact(hal, candidate, &mut prefix_maps) {
                    regs_vaddr = mapped.ptr().as_ptr() as usize;
                    regs = Some(mapped);
                    regs_paddr = candidate;
                    break;
                }
            }
        }
        if regs_paddr == 0 || regs_vaddr == 0 {
            return Err(Pi4SeatError::MailboxMap);
        }

        let request = hal
            .alloc_dma_frame_low_attr(sel4_sys::seL4_ARM_Page_Uncached)
            .map_err(|_| Pi4SeatError::MailboxDma)?;
        let mut line = heapless::String::<160>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] mailbox regs=0x{regs_paddr:08x} req_paddr=0x{req:08x}",
                req = request.paddr()
            ),
        );
        boot_log::force_uart_line(line.as_str());
        Ok(Self {
            regs_vaddr,
            regs_paddr,
            _regs: regs,
            request,
            _prefix_maps: prefix_maps,
        })
    }

    fn map_device_exact(
        hal: &mut KernelHal<'_>,
        paddr: usize,
        prefix_maps: &mut Vec<crate::sel4::DeviceFrame>,
    ) -> Result<crate::sel4::DeviceFrame, Pi4SeatError> {
        map_device_exact(
            hal,
            paddr,
            MAILBOX_MAP_EXACT_ATTEMPT_CAP,
            "mailbox",
            Pi4SeatError::MailboxMap,
            prefix_maps,
        )
    }

    fn call_tag(
        &mut self,
        tag: u32,
        request_len_bytes: u32,
        payload: &mut [u32],
    ) -> Result<(), Pi4SeatError> {
        let original_payload = payload.to_vec();
        let words = {
            let bytes = self.request.as_mut_slice();
            // SAFETY: The DMA request page is 4-byte aligned and sized to PAGE_SIZE.
            unsafe {
                core::slice::from_raw_parts_mut(bytes.as_mut_ptr().cast::<u32>(), PAGE_SIZE / 4)
            }
        };

        let mut last_err = Pi4SeatError::MailboxProtocol;
        for (alias_index, &alias_base) in VC_BUS_ALIAS_BASES.iter().enumerate() {
            self.encode_request(words, tag, request_len_bytes, &original_payload)?;
            let request_bus =
                phys_to_bus(self.request.paddr(), alias_base).ok_or(Pi4SeatError::MailboxDma)?;

            match self.mailbox_send(request_bus) {
                Ok(()) => {
                    if words[1] != MAILBOX_RESPONSE_SUCCESS {
                        last_err = Pi4SeatError::MailboxProtocol;
                        continue;
                    }
                    if words[2] != tag {
                        last_err = Pi4SeatError::MailboxProtocol;
                        continue;
                    }
                    if (words[4] & MAILBOX_VALUE_RESPONSE) == 0 {
                        last_err = Pi4SeatError::MailboxProtocol;
                        continue;
                    }
                    if alias_index > 0 {
                        let mut line = heapless::String::<192>::new();
                        let _ = core::fmt::Write::write_fmt(
                            &mut line,
                            format_args!(
                                "[local-seat] mailbox alias fallback alias=0x{alias_base:08x}"
                            ),
                        );
                        boot_log::force_uart_line(line.as_str());
                    }
                    payload.copy_from_slice(&words[5..5 + payload.len()]);
                    return Ok(());
                }
                Err(err @ Pi4SeatError::MailboxTimeout)
                | Err(err @ Pi4SeatError::MailboxProtocol) => {
                    last_err = err;
                    if alias_index + 1 == VC_BUS_ALIAS_BASES.len() {
                        return Err(err);
                    }
                }
                Err(err) => return Err(err),
            }
        }

        Err(last_err)
    }

    fn encode_request(
        &self,
        words: &mut [u32],
        tag: u32,
        request_len_bytes: u32,
        payload: &[u32],
    ) -> Result<(), Pi4SeatError> {
        let total_words = 2usize
            .checked_add(3)
            .and_then(|value| value.checked_add(payload.len()))
            .and_then(|value| value.checked_add(1))
            .ok_or(Pi4SeatError::MailboxProtocol)?;
        if total_words > words.len() {
            return Err(Pi4SeatError::MailboxProtocol);
        }

        words[0] = total_words
            .checked_mul(mem::size_of::<u32>())
            .and_then(|bytes| u32::try_from(bytes).ok())
            .ok_or(Pi4SeatError::MailboxProtocol)?;
        words[1] = 0;
        words[2] = tag;
        words[3] = payload
            .len()
            .checked_mul(mem::size_of::<u32>())
            .and_then(|bytes| u32::try_from(bytes).ok())
            .ok_or(Pi4SeatError::MailboxProtocol)?;
        words[4] = request_len_bytes;
        words[5..5 + payload.len()].copy_from_slice(payload);
        words[5 + payload.len()] = 0;

        // Ensure the request header/payload writes are globally observed before
        // handing the request buffer address to VideoCore.
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    fn mailbox_send(&self, data: u32) -> Result<(), Pi4SeatError> {
        // Drain stale responses first so we don't consume an earlier transaction.
        for _ in 0..MAILBOX_DRAIN_LIMIT {
            if self.read_reg(MAILBOX_STATUS0_OFFSET) & MAILBOX_EMPTY != 0 {
                break;
            }
            let _ = self.read_reg(MAILBOX_READ_OFFSET);
        }

        let mut wait = 0usize;
        while self.read_reg(MAILBOX_STATUS1_OFFSET) & MAILBOX_FULL != 0 {
            wait = wait.saturating_add(1);
            if wait >= MAILBOX_WAIT_SPINS {
                self.log_timeout("send-space");
                return Err(Pi4SeatError::MailboxTimeout);
            }
            spin_loop();
        }

        self.write_reg(
            MAILBOX_WRITE_OFFSET,
            (data & !0xF) | (MAILBOX_CHANNEL_PROPERTY & 0xF),
        );

        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

        wait = 0;
        loop {
            while self.read_reg(MAILBOX_STATUS0_OFFSET) & MAILBOX_EMPTY != 0 {
                wait = wait.saturating_add(1);
                if wait >= MAILBOX_WAIT_SPINS {
                    self.log_timeout("recv");
                    return Err(Pi4SeatError::MailboxTimeout);
                }
                spin_loop();
            }
            let value = self.read_reg(MAILBOX_READ_OFFSET);
            if (value & 0xF) == MAILBOX_CHANNEL_PROPERTY {
                if (value & !0xF) != (data & !0xF) {
                    return Err(Pi4SeatError::MailboxProtocol);
                }
                return Ok(());
            }
        }
    }

    fn log_timeout(&self, phase: &str) {
        let mut line = heapless::String::<200>::new();
        let status0 = self.read_reg(MAILBOX_STATUS0_OFFSET);
        let status1 = self.read_reg(MAILBOX_STATUS1_OFFSET);
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] mailbox timeout phase={phase} regs=0x{regs:08x} status0=0x{status0:08x} status1=0x{status1:08x}",
                regs = self.regs_paddr
            ),
        );
        boot_log::force_uart_line(line.as_str());
    }

    fn read_reg(&self, offset: usize) -> u32 {
        let base = self.regs_vaddr;
        let Some(addr) = base.checked_add(offset) else {
            return 0;
        };
        // SAFETY: Register block was mapped as device memory by HAL.
        unsafe { ptr::read_volatile(addr as *const u32) }
    }

    fn write_reg(&self, offset: usize, value: u32) {
        let base = self.regs_vaddr;
        let Some(addr) = base.checked_add(offset) else {
            return;
        };
        // SAFETY: Register block was mapped as device memory by HAL.
        unsafe {
            ptr::write_volatile(addr as *mut u32, value);
        }
    }
}

fn map_pi4_wifi_mailbox_reset_error(err: HalError) -> Pi4SeatError {
    match err {
        HalError::Unsupported("mailbox-dma") => Pi4SeatError::MailboxDma,
        HalError::Unsupported("mailbox-timeout") => Pi4SeatError::MailboxTimeout,
        HalError::Unsupported("device-coverage")
        | HalError::Unsupported("device-map-order")
        | HalError::Unsupported("device-map-exact")
        | HalError::Sel4(_) => Pi4SeatError::MailboxMap,
        _ => Pi4SeatError::MailboxProtocol,
    }
}

fn ensure_runtime_vl805_mailbox_reset(hal: &mut KernelHal<'_>) -> Result<(), Pi4SeatError> {
    match VL805_RUNTIME_RESET_STATE.load(Ordering::Acquire) {
        VL805_RUNTIME_RESET_STATE_NOTIFIED => {
            boot_log::force_uart_line(
                "[local-seat] vl805 reset handoff=runtime-owned stage=runtime detail=skip-already-notified",
            );
            return Ok(());
        }
        VL805_RUNTIME_RESET_STATE_BOOTLOADER_AUTHORIZED => {
            boot_log::force_uart_line(
                "[local-seat] vl805 reset handoff=bootloader-owned stage=runtime detail=post-stop-reset-authorized action=attempt-runtime-mailbox-notify",
            );
        }
        VL805_RUNTIME_RESET_STATE_SOFT_CONTINUE => {
            boot_log::force_uart_line(
                "[local-seat] vl805 reset handoff=runtime-unconfirmed stage=runtime detail=prior-soft-failure action=retry-mailbox-notify",
            );
        }
        VL805_RUNTIME_RESET_STATE_POSTED_FALLBACK => {
            boot_log::force_uart_line(
                "[local-seat] vl805 reset handoff=runtime-unconfirmed stage=runtime detail=prior-posted-fallback action=retry-mailbox-notify",
            );
        }
        VL805_RUNTIME_RESET_STATE_HARD_MAP => {
            return Err(Pi4SeatError::MailboxMap);
        }
        VL805_RUNTIME_RESET_STATE_HARD_DMA => {
            return Err(Pi4SeatError::MailboxDma);
        }
        VL805_RUNTIME_RESET_STATE_HARD_TIMEOUT => {
            return Err(Pi4SeatError::MailboxTimeout);
        }
        VL805_RUNTIME_RESET_STATE_HARD_PROTOCOL => {
            return Err(Pi4SeatError::MailboxProtocol);
        }
        _ => {}
    }

    boot_log::force_uart_line(
        "[local-seat] vl805 reset handoff=runtime-owned stage=runtime action=mailbox-notify",
    );
    match pi4_wifi::notify_vl805_reset(hal).map_err(map_pi4_wifi_mailbox_reset_error) {
        Ok(result) => {
            wait_ms(runtime_vl805_mailbox_reset_success_settle_ms(result));
            let state = runtime_vl805_mailbox_reset_success_state(result);
            VL805_RUNTIME_RESET_STATE.store(state, Ordering::Release);
            let mut line = heapless::String::<224>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] vl805 reset handoff={} stage=runtime detail={}",
                    runtime_vl805_mailbox_reset_handoff_label(state),
                    runtime_vl805_mailbox_reset_success_detail(result),
                ),
            );
            boot_log::force_uart_line(line.as_str());
            Ok(())
        }
        Err(err) if runtime_vl805_mailbox_reset_error_allows_cold_init(err) => {
            wait_ms(VL805_MAILBOX_RESET_SETTLE_MS);
            VL805_RUNTIME_RESET_STATE
                .store(VL805_RUNTIME_RESET_STATE_SOFT_CONTINUE, Ordering::Release);
            let mut line = heapless::String::<224>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] vl805 reset handoff=runtime-unconfirmed stage=runtime detail={} action=no-runtime-ownership",
                    err.as_str()
                ),
            );
            boot_log::force_uart_line(line.as_str());
            Ok(())
        }
        Err(err) => {
            VL805_RUNTIME_RESET_STATE.store(
                runtime_vl805_mailbox_reset_failure_state(err),
                Ordering::Release,
            );
            Err(err)
        }
    }
}

fn authorize_bootloader_vl805_reset(mmio: usize, reason: &'static str) {
    match VL805_RUNTIME_RESET_STATE.load(Ordering::Acquire) {
        VL805_RUNTIME_RESET_STATE_NOTIFIED | VL805_RUNTIME_RESET_STATE_BOOTLOADER_AUTHORIZED => {}
        _ => {
            VL805_RUNTIME_RESET_STATE.store(
                VL805_RUNTIME_RESET_STATE_BOOTLOADER_AUTHORIZED,
                Ordering::Release,
            );
        }
    }
    let mut line = heapless::String::<224>::new();
    let _ = core::fmt::Write::write_fmt(
        &mut line,
        format_args!(
            "[local-seat] vl805 reset handoff=bootloader-owned stage=runtime detail=post-stop-reset-authorized action=no-touch-fallback reason={reason} mmio=0x{mmio:016x}",
        ),
    );
    boot_log::force_uart_line(line.as_str());
}

struct HdmiTextSink {
    width: usize,
    height: usize,
    text_height: usize,
    pitch: usize,
    framebuffer_len: usize,
    cols: usize,
    rows: usize,
    row: usize,
    col: usize,
    framebuffer: *mut u8,
    backend: HdmiBackend,
    scrollback_lines: VecDeque<String>,
    current_line: String,
    scrollback_row_offset: usize,
    mappings: Vec<crate::sel4::DeviceFrame>,
    _prefix_maps: Vec<crate::sel4::DeviceFrame>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HdmiBackend {
    DtbSimpleFramebuffer,
    MailboxProperty,
}

impl HdmiBackend {
    const fn as_str(self) -> &'static str {
        match self {
            Self::DtbSimpleFramebuffer => "dtb-simplefb",
            Self::MailboxProperty => "mailbox-property",
        }
    }
}

impl HdmiTextSink {
    fn new(
        hal: &mut KernelHal<'_>,
        framebuffer_hint: Option<Pi4FramebufferHint>,
    ) -> Result<Self, Pi4SeatError> {
        if let Some(hint) = framebuffer_hint {
            match Self::from_fixed_framebuffer(hal, hint, HdmiBackend::DtbSimpleFramebuffer) {
                Ok(sink) => return Ok(sink),
                Err(err) => {
                    let mut line = heapless::String::<192>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut line,
                        format_args!(
                            "[local-seat] display dtb-simplefb rejected detail={} fallback=mailbox",
                            err.as_str()
                        ),
                    );
                    boot_log::force_uart_line(line.as_str());
                }
            }
        }

        Self::new_from_mailbox(hal)
    }

    fn new_from_mailbox(hal: &mut KernelHal<'_>) -> Result<Self, Pi4SeatError> {
        let mut mailbox = Mailbox::new(hal)?;

        let mut phys = [DEFAULT_FB_WIDTH, DEFAULT_FB_HEIGHT];
        mailbox.call_tag(TAG_SET_PHYSICAL_SIZE, 8, &mut phys)?;

        let mut virt = [DEFAULT_FB_WIDTH, DEFAULT_FB_HEIGHT];
        mailbox.call_tag(TAG_SET_VIRTUAL_SIZE, 8, &mut virt)?;

        let mut depth = [DEFAULT_FB_DEPTH];
        mailbox.call_tag(TAG_SET_DEPTH, 4, &mut depth)?;

        let mut pixel_order = [PIXEL_ORDER_RGB];
        mailbox.call_tag(TAG_SET_PIXEL_ORDER, 4, &mut pixel_order)?;

        let mut alloc = [DEFAULT_FB_ALIGNMENT, 0];
        mailbox.call_tag(TAG_ALLOCATE_BUFFER, 4, &mut alloc)?;

        let fb_bus = alloc[0];
        let fb_size = alloc[1] as usize;
        if fb_bus == 0 || fb_size == 0 {
            return Err(Pi4SeatError::FramebufferUnavailable);
        }

        let mut pitch = [0u32];
        mailbox.call_tag(TAG_GET_PITCH, 0, &mut pitch)?;
        if pitch[0] == 0 {
            return Err(Pi4SeatError::FramebufferUnavailable);
        }

        let fb_phys = bus_to_phys(fb_bus);
        let Some(fb_end) = fb_phys.checked_add(fb_size) else {
            return Err(Pi4SeatError::FramebufferUnavailable);
        };
        let mut line = heapless::String::<224>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] fb mailbox bus=0x{bus:08x} phys=0x{phys:08x} end=0x{end:08x} size={size} pitch={pitch} phys={phys_w}x{phys_h} virt={virt_w}x{virt_h}",
                bus = fb_bus,
                phys = fb_phys,
                end = fb_end,
                size = fb_size,
                pitch = pitch[0],
                phys_w = phys[0],
                phys_h = phys[1],
                virt_w = virt[0],
                virt_h = virt[1],
            ),
        );
        boot_log::force_uart_line(line.as_str());
        if !framebuffer_phys_window_safe(fb_phys, fb_size) {
            let mut reject = heapless::String::<224>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut reject,
                format_args!(
                    "[local-seat] fb mailbox reject phys=[0x{phys:08x}..0x{end:08x}) safe=[0x{safe_start:08x}..0x{safe_end:08x})",
                    phys = fb_phys,
                    end = fb_end,
                    safe_start = MIN_SAFE_FB_PHYS,
                    safe_end = MAX_SAFE_FB_PHYS_EXCL,
                ),
            );
            boot_log::force_uart_line(reject.as_str());
            return Err(Pi4SeatError::FramebufferUnavailable);
        }

        let pitch_bytes = pitch[0] as usize;
        let Some(raw_width) = mailbox_visible_dimension(phys[0] as usize, virt[0] as usize) else {
            return Err(Pi4SeatError::FramebufferUnavailable);
        };
        let Some(raw_height) = mailbox_visible_dimension(phys[1] as usize, virt[1] as usize) else {
            return Err(Pi4SeatError::FramebufferUnavailable);
        };
        let width = clamp_visible_width(raw_width, pitch_bytes);
        let height = clamp_visible_height(raw_height, pitch_bytes, fb_size);
        if width == 0 || height == 0 {
            return Err(Pi4SeatError::FramebufferUnavailable);
        }
        if width != raw_width || height != raw_height {
            let mut clamp = heapless::String::<256>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut clamp,
                format_args!(
                    "[local-seat] fb mailbox viewport clamped raw={}x{} chosen={}x{} pitch={} alloc={}",
                    raw_width, raw_height, width, height, pitch_bytes, fb_size
                ),
            );
            boot_log::force_uart_line(clamp.as_str());
        }

        Self::from_mapped_framebuffer(
            hal,
            fb_phys,
            fb_size,
            width,
            height,
            pitch_bytes,
            HdmiBackend::MailboxProperty,
        )
    }

    fn from_fixed_framebuffer(
        hal: &mut KernelHal<'_>,
        hint: Pi4FramebufferHint,
        backend: HdmiBackend,
    ) -> Result<Self, Pi4SeatError> {
        if hint.width == 0 || hint.height == 0 || hint.pitch == 0 {
            return Err(Pi4SeatError::FramebufferUnavailable);
        }
        let fb_size = hint
            .pitch
            .checked_mul(hint.height)
            .ok_or(Pi4SeatError::FramebufferUnavailable)?;
        Self::from_mapped_framebuffer(
            hal,
            hint.paddr,
            fb_size,
            hint.width,
            hint.height,
            hint.pitch,
            backend,
        )
    }

    fn from_mapped_framebuffer(
        hal: &mut KernelHal<'_>,
        fb_phys: usize,
        fb_size: usize,
        width: usize,
        height: usize,
        pitch: usize,
        backend: HdmiBackend,
    ) -> Result<Self, Pi4SeatError> {
        if fb_phys == 0 || fb_size == 0 || width == 0 || height == 0 || pitch == 0 {
            return Err(Pi4SeatError::FramebufferUnavailable);
        }
        let framebuffer_len = validate_framebuffer_geometry(width, height, pitch, fb_size)
            .ok_or(Pi4SeatError::FramebufferUnavailable)?;

        let page_base = fb_phys & !PAGE_MASK;
        let page_offset = fb_phys & PAGE_MASK;
        let map_len = page_offset
            .checked_add(framebuffer_len)
            .ok_or(Pi4SeatError::FramebufferUnavailable)?;
        let page_count = div_ceil(map_len, PAGE_SIZE);
        if page_count == 0 || page_count > MAX_FB_MAP_PAGES {
            return Err(Pi4SeatError::FramebufferUnavailable);
        }

        let mut prefix_maps = Vec::new();
        let mut mappings = Vec::with_capacity(page_count);
        for page in 0..page_count {
            let paddr = page_base
                .checked_add(
                    page.checked_mul(PAGE_SIZE)
                        .ok_or(Pi4SeatError::FramebufferUnavailable)?,
                )
                .ok_or(Pi4SeatError::FramebufferUnavailable)?;
            let frame = map_device_exact(
                hal,
                paddr,
                FRAMEBUFFER_MAP_EXACT_ATTEMPT_CAP,
                "framebuffer",
                Pi4SeatError::FramebufferMap,
                &mut prefix_maps,
            )?;
            mappings.push(frame);
        }

        let first = mappings
            .first()
            .ok_or(Pi4SeatError::FramebufferMap)?
            .ptr()
            .as_ptr() as usize;
        for (idx, frame) in mappings.iter().enumerate() {
            let expected = first
                .checked_add(
                    idx.checked_mul(PAGE_SIZE)
                        .ok_or(Pi4SeatError::FramebufferMap)?,
                )
                .ok_or(Pi4SeatError::FramebufferMap)?;
            let got = frame.ptr().as_ptr() as usize;
            if got != expected {
                return Err(Pi4SeatError::FramebufferMap);
            }
        }

        let framebuffer = first
            .checked_add(page_offset)
            .ok_or(Pi4SeatError::FramebufferMap)? as *mut u8;
        let rows = text_row_count(height);
        let text_height = text_viewport_height(height, rows);

        let mut sink = Self {
            width,
            height,
            text_height,
            pitch,
            framebuffer_len,
            cols: cmp::max(1, width / CHAR_WIDTH),
            rows,
            row: 0,
            col: 0,
            framebuffer,
            backend,
            scrollback_lines: VecDeque::with_capacity(HDMI_SCROLLBACK_MAX_LINES),
            current_line: String::new(),
            scrollback_row_offset: 0,
            mappings,
            _prefix_maps: prefix_maps,
        };

        sink.clear_screen();
        Ok(sink)
    }

    fn backend_label(&self) -> &'static str {
        self.backend.as_str()
    }

    fn write_line(&mut self, line: &str) {
        self.reset_scrollback_on_live_update();
        for &byte in line.as_bytes() {
            self.put_byte(byte);
        }
        self.newline();
    }

    fn write_bytes(&mut self, bytes: &[u8]) {
        self.reset_scrollback_on_live_update();
        for &byte in bytes {
            self.put_byte(byte);
        }
    }

    fn scroll_view_rows(&mut self, delta_rows: i8) {
        let max_offset = self.max_scrollback_row_offset();
        let requested = if delta_rows >= 0 {
            self.scrollback_row_offset
                .saturating_add(delta_rows as usize)
                .min(max_offset)
        } else {
            self.scrollback_row_offset
                .saturating_sub((-delta_rows) as usize)
        };
        if requested == self.scrollback_row_offset {
            return;
        }
        self.scrollback_row_offset = requested;
        self.render_scrollback_view();
    }

    fn put_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => self.newline(),
            b'\r' => self.col = 0,
            0x08 | 0x7f => {
                self.record_backspace();
                self.backspace();
            }
            b'\t' => {
                for _ in 0..TAB_WIDTH {
                    self.put_byte(b' ');
                }
            }
            _ => {
                self.record_visible_byte(byte);
                if self.col >= self.cols {
                    self.newline();
                }
                self.draw_char(byte);
                self.col = self.col.saturating_add(1);
            }
        }
    }

    fn backspace(&mut self) {
        let (row, col) = text_backspace_target(self.row, self.col, self.cols);
        self.row = row;
        self.col = col;
        self.draw_char(b' ');
    }

    fn newline(&mut self) {
        self.record_newline();
        self.col = 0;
        self.row = self.row.saturating_add(1);
        if self.row >= self.rows {
            self.scroll_up_one_text_row();
            self.row = self.rows.saturating_sub(1);
        }
    }

    fn scroll_up_one_text_row(&mut self) {
        let scroll_pixels = cmp::min(CHAR_HEIGHT, self.text_height);
        if scroll_pixels == 0 {
            return;
        }

        let Some(visible_row_bytes) = self.width.checked_mul(FB_BYTES_PER_PIXEL) else {
            self.clear_screen();
            return;
        };
        let Some(scroll_bytes) = self.pitch.checked_mul(scroll_pixels) else {
            self.clear_screen();
            return;
        };
        let Some(total_bytes) = self.pitch.checked_mul(self.text_height) else {
            self.clear_screen();
            return;
        };
        if total_bytes == 0
            || total_bytes > self.framebuffer_len
            || scroll_bytes >= total_bytes
            || visible_row_bytes == 0
            || visible_row_bytes > self.pitch
        {
            self.clear_screen();
            return;
        }

        let move_rows = self.text_height.saturating_sub(scroll_pixels);
        if visible_row_bytes == self.pitch {
            let move_bytes = total_bytes.saturating_sub(scroll_bytes);
            // SAFETY: The mapped framebuffer is contiguous for `framebuffer_len` bytes,
            // and both source/destination ranges are within that region.
            unsafe {
                ptr::copy(
                    self.framebuffer.add(scroll_bytes),
                    self.framebuffer,
                    move_bytes,
                );
            }
        } else {
            // Copy only visible pixels when pitch includes right-side padding.
            // This reduces per-scroll bandwidth significantly on padded modes.
            for row in 0..move_rows {
                let Some(dst_off) = row.checked_mul(self.pitch) else {
                    self.clear_screen();
                    return;
                };
                let Some(src_row) = row.checked_add(scroll_pixels) else {
                    self.clear_screen();
                    return;
                };
                let Some(src_off) = src_row.checked_mul(self.pitch) else {
                    self.clear_screen();
                    return;
                };
                // SAFETY: Source/destination are inside mapped framebuffer, row regions
                // do not overlap for this direction, and `visible_row_bytes` is bounded.
                unsafe {
                    ptr::copy_nonoverlapping(
                        self.framebuffer.add(src_off),
                        self.framebuffer.add(dst_off),
                        visible_row_bytes,
                    );
                }
            }
        }

        self.fill_rect(
            0,
            self.text_height.saturating_sub(scroll_pixels),
            self.width,
            scroll_pixels,
            BG_COLOR,
        );
    }

    fn draw_char(&mut self, byte: u8) {
        let glyph = BASIC_LEGACY[usize::from(byte.min(0x7F))];
        let x0 = self.col.saturating_mul(CHAR_WIDTH);
        let y0 = self.row.saturating_mul(CHAR_HEIGHT);
        if x0.saturating_add(CHAR_WIDTH) > self.width
            || y0.saturating_add(CHAR_HEIGHT) > self.height
        {
            self.fill_rect(x0, y0, CHAR_WIDTH, CHAR_HEIGHT, BG_COLOR);
            for (gy, bits) in glyph.iter().enumerate() {
                for gx in 0..CHAR_WIDTH {
                    if ((bits >> gx) & 1) == 0 {
                        continue;
                    }
                    let x = x0.saturating_add(gx);
                    let y = y0.saturating_add(gy.saturating_mul(2));
                    self.put_pixel(x, y, FG_COLOR);
                    self.put_pixel(x, y.saturating_add(1), FG_COLOR);
                }
            }
            return;
        }

        self.fill_rect(x0, y0, CHAR_WIDTH, CHAR_HEIGHT, BG_COLOR);
        let Some(x0_bytes) = x0.checked_mul(FB_BYTES_PER_PIXEL) else {
            return;
        };

        for (gy, bits) in glyph.iter().enumerate() {
            let Some(y_upper) = y0.checked_add(gy.saturating_mul(2)) else {
                continue;
            };
            let Some(row0_off) = y_upper
                .checked_mul(self.pitch)
                .and_then(|off| off.checked_add(x0_bytes))
            else {
                continue;
            };
            let Some(row1_off) = row0_off.checked_add(self.pitch) else {
                continue;
            };
            // SAFETY: Bounds were prevalidated for this character cell and offsets
            // stay within mapped framebuffer for both duplicated glyph rows.
            let row0 = unsafe { self.framebuffer.add(row0_off) as *mut u32 };
            // SAFETY: Same as row0 pointer rationale.
            let row1 = unsafe { self.framebuffer.add(row1_off) as *mut u32 };
            for gx in 0..CHAR_WIDTH {
                if ((bits >> gx) & 1) == 0 {
                    continue;
                }
                // SAFETY: `gx < CHAR_WIDTH` and the char cell lies within bounds.
                unsafe {
                    ptr::write(row0.add(gx), FG_COLOR);
                    ptr::write(row1.add(gx), FG_COLOR);
                }
            }
        }
    }

    fn clear_screen(&mut self) {
        self.fill_rect(0, 0, self.width, self.height, BG_COLOR);
    }

    fn reset_scrollback_on_live_update(&mut self) {
        if self.scrollback_row_offset == 0 {
            return;
        }
        self.scrollback_row_offset = 0;
        self.render_scrollback_view();
    }

    fn record_visible_byte(&mut self, byte: u8) {
        if self.current_line.len() >= HDMI_SCROLLBACK_MAX_LINE_BYTES {
            return;
        }
        let _ = self.current_line.push(byte as char);
    }

    fn record_backspace(&mut self) {
        let _ = self.current_line.pop();
    }

    fn record_newline(&mut self) {
        if self.scrollback_lines.len() >= HDMI_SCROLLBACK_MAX_LINES {
            let _ = self.scrollback_lines.pop_front();
        }
        self.scrollback_lines.push_back(self.current_line.clone());
        self.current_line.clear();
    }

    fn max_scrollback_row_offset(&self) -> usize {
        let total_rows = self.collect_scrollback_rows().len();
        total_rows.saturating_sub(self.rows.max(1))
    }

    fn collect_scrollback_rows(&self) -> Vec<String> {
        let mut rows = Vec::new();
        let cols = self.cols.max(1);
        for line in &self.scrollback_lines {
            append_wrapped_scrollback_rows(&mut rows, line, cols);
        }
        append_wrapped_scrollback_rows(&mut rows, &self.current_line, cols);
        if rows.is_empty() {
            rows.push(String::new());
        }
        rows
    }

    fn render_scrollback_view(&mut self) {
        let rows = self.collect_scrollback_rows();
        let visible_rows = self.rows.max(1);
        let total_rows = rows.len();
        let max_offset = total_rows.saturating_sub(visible_rows);
        self.scrollback_row_offset = self.scrollback_row_offset.min(max_offset);
        let end = total_rows.saturating_sub(self.scrollback_row_offset);
        let start = end.saturating_sub(visible_rows);

        self.clear_screen();

        for (display_row, row_text) in rows[start..end].iter().enumerate() {
            for (display_col, byte) in row_text.as_bytes().iter().copied().enumerate() {
                if display_col >= self.cols {
                    break;
                }
                self.row = display_row;
                self.col = display_col;
                self.draw_char(byte);
            }
        }

        let cursor_global_row = total_rows.saturating_sub(1);
        let cursor_col = rows
            .last()
            .map(|line| line.as_bytes().len().min(self.cols))
            .unwrap_or(0);
        if cursor_global_row >= start && cursor_global_row < end {
            self.row = cursor_global_row - start;
            self.col = cursor_col;
        } else {
            self.row = 0;
            self.col = 0;
        }
    }

    fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: u32) {
        let x_end = cmp::min(self.width, x.saturating_add(w));
        let y_end = cmp::min(self.height, y.saturating_add(h));
        if x >= x_end || y >= y_end {
            return;
        }
        let Some(start_bytes) = x.checked_mul(FB_BYTES_PER_PIXEL) else {
            return;
        };
        let pixels = x_end.saturating_sub(x);
        for yy in y..y_end {
            let Some(row_off) = yy
                .checked_mul(self.pitch)
                .and_then(|off| off.checked_add(start_bytes))
            else {
                return;
            };
            let Some(row_end) = row_off.checked_add(pixels.saturating_mul(FB_BYTES_PER_PIXEL))
            else {
                return;
            };
            if row_end > self.framebuffer_len {
                return;
            }
            // SAFETY: Row window lies within mapped framebuffer and contains `pixels`
            // tightly packed u32 pixels.
            let row = unsafe {
                core::slice::from_raw_parts_mut(self.framebuffer.add(row_off) as *mut u32, pixels)
            };
            row.fill(color);
        }
    }

    fn put_pixel(&mut self, x: usize, y: usize, color: u32) {
        if x >= self.width || y >= self.height {
            return;
        }
        let row_off = match y.checked_mul(self.pitch) {
            Some(value) => value,
            None => return,
        };
        let col_off = match x.checked_mul(FB_BYTES_PER_PIXEL) {
            Some(value) => value,
            None => return,
        };
        let byte_off = match row_off.checked_add(col_off) {
            Some(value) => value,
            None => return,
        };
        let end = match byte_off.checked_add(FB_BYTES_PER_PIXEL) {
            Some(value) => value,
            None => return,
        };
        if end > self.framebuffer_len {
            return;
        }
        let addr = match (self.framebuffer as usize).checked_add(byte_off) {
            Some(value) => value as *mut u32,
            None => return,
        };
        // SAFETY: `framebuffer` is a mapped writable frame buffer and bounds were checked.
        unsafe {
            ptr::write(addr, color);
        }
    }
}

impl Drop for HdmiTextSink {
    fn drop(&mut self) {
        let _ = self.mappings.len();
    }
}

#[inline]
fn append_wrapped_scrollback_rows(rows: &mut Vec<String>, line: &str, cols: usize) {
    if cols == 0 {
        return;
    }
    if line.is_empty() {
        rows.push(String::new());
        return;
    }

    let mut start = 0usize;
    while start < line.len() {
        let mut end = cmp::min(start.saturating_add(cols), line.len());
        while end > start && !line.is_char_boundary(end) {
            end = end.saturating_sub(1);
        }
        if end == start {
            break;
        }
        rows.push(String::from(&line[start..end]));
        start = end;
    }
}

fn decode_pci_mmio_bar(bar0: u32, bar1: u32) -> Option<usize> {
    if (bar0 & PCI_BAR_IO_SPACE) != 0 {
        return None;
    }

    let is_64 = (bar0 & PCI_BAR_TYPE_MASK) == PCI_BAR_TYPE_64;
    let base = if is_64 {
        ((bar1 as u64) << 32) | ((bar0 as u64) & PCI_BAR_ADDR_MASK)
    } else {
        (bar0 as u64) & PCI_BAR_ADDR_MASK
    };
    if base == 0 {
        return None;
    }
    usize::try_from(base).ok()
}

fn prepare_vl805_pci(hal: &mut KernelHal<'_>) -> Option<usize> {
    let mut prefix_maps = Vec::new();
    for &ecam_base in &VL805_ECAM_BASE_CANDIDATES {
        let Some(config_paddr) = ecam_base.checked_add(VL805_PCI_DEV_ADDR as usize) else {
            continue;
        };
        let config_page = config_paddr & !PAGE_MASK;
        let config_page_offset = config_paddr & PAGE_MASK;
        let mut _config_frame: Option<crate::sel4::DeviceFrame> = None;
        let mut cfg_src = "mapped";
        let config_page_virt = if let Some(mapped) = pinned_vl805_cfg_lookup(config_page, PAGE_SIZE)
        {
            cfg_src = "pinned";
            mapped
        } else {
            if hal
                .device_coverage(config_page, crate::sel4::PAGE_BITS)
                .is_none()
            {
                let mut line = heapless::String::<176>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] vl805 pci skip ecam=0x{ecam:016x} cfg=0x{cfg:016x} reason=no-device-coverage",
                        ecam = ecam_base,
                        cfg = config_paddr
                    ),
                );
                boot_log::force_uart_line(line.as_str());
                continue;
            }

            let frame = match map_device_exact(
                hal,
                config_page,
                VL805_PCI_CFG_ATTEMPT_CAP,
                "vl805-pci",
                Pi4SeatError::XhciInit,
                &mut prefix_maps,
            ) {
                Ok(frame) => frame,
                Err(_) => {
                    let mut line = heapless::String::<192>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut line,
                        format_args!(
                            "[local-seat] vl805 pci skip ecam=0x{ecam:016x} cfg=0x{cfg:016x} reason=map-exact-failed",
                            ecam = ecam_base,
                            cfg = config_paddr
                        ),
                    );
                    boot_log::force_uart_line(line.as_str());
                    continue;
                }
            };
            let virt = frame.ptr().as_ptr() as usize;
            _config_frame = Some(frame);
            virt
        };
        let Some(config_virt) = config_page_virt.checked_add(config_page_offset) else {
            continue;
        };

        let vendor_device = pci_cfg_read_u32(config_virt, PCI_CFG_VENDOR_DEVICE);
        let vendor_id = (vendor_device & 0xffff) as u16;
        let device_id = ((vendor_device >> 16) & 0xffff) as u16;
        if vendor_id == 0 || vendor_id == 0xffff {
            continue;
        }

        if vendor_id != VL805_PCI_VENDOR_ID || device_id != VL805_PCI_DEVICE_ID {
            let mut id_line = heapless::String::<176>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut id_line,
                format_args!(
                    "[local-seat] vl805 pci id mismatch got={vid:04x}:{did:04x} expected={exp_vid:04x}:{exp_did:04x}",
                    vid = vendor_id,
                    did = device_id,
                    exp_vid = VL805_PCI_VENDOR_ID,
                    exp_did = VL805_PCI_DEVICE_ID
                ),
            );
            boot_log::force_uart_line(id_line.as_str());
            continue;
        }

        let class_revision = pci_cfg_read_u32(config_virt, PCI_CFG_CLASS_REVISION);
        let class_code = (class_revision >> 8) & 0x00ff_ffff;

        if class_code != VL805_EXPECTED_CLASS_CODE {
            let mut class_line = heapless::String::<176>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut class_line,
                format_args!(
                    "[local-seat] vl805 pci class mismatch got=0x{class_code:06x} expected=0x{expected:06x}",
                    expected = VL805_EXPECTED_CLASS_CODE
                ),
            );
            boot_log::force_uart_line(class_line.as_str());
            continue;
        }

        let bar0 = pci_cfg_read_u32(config_virt, PCI_CFG_BAR0);
        let bar1 = pci_cfg_read_u32(config_virt, PCI_CFG_BAR1);
        let mmio = decode_pci_mmio_bar(bar0, bar1);
        let Some(mmio) = mmio else {
            boot_log::force_uart_line("[local-seat] vl805 pci missing BAR0 MMIO address");
            continue;
        };

        if in_vl805_ecam_window(mmio) && mmio != RPI4_XHCI_MMIO_HIGH_CANDIDATE {
            let ecam_end = ecam_base.saturating_add(VL805_ECAM_WINDOW_BYTES);
            let mut reject_line = heapless::String::<192>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut reject_line,
                format_args!(
                    "[local-seat] vl805 pci reject mmio=0x{mmio:016x} reason=bar-points-to-ecam ecam=[0x{base:016x}..0x{end:016x})",
                    base = ecam_base,
                    end = ecam_end
                ),
            );
            boot_log::force_uart_line(reject_line.as_str());
            continue;
        }
        if !xhci_mmio_candidate_valid(mmio) {
            let mut reject_line = heapless::String::<176>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut reject_line,
                format_args!(
                    "[local-seat] vl805 pci reject mmio=0x{mmio:016x} reason=invalid-candidate"
                ),
            );
            boot_log::force_uart_line(reject_line.as_str());
            continue;
        }

        if (mmio & PAGE_MASK) != 0 {
            let mut reject_line = heapless::String::<160>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut reject_line,
                format_args!("[local-seat] vl805 pci reject mmio=0x{mmio:016x} reason=unaligned"),
            );
            boot_log::force_uart_line(reject_line.as_str());
            continue;
        }

        if hal.device_coverage(mmio, crate::sel4::PAGE_BITS).is_none()
            && pinned_xhci_window_lookup(mmio, PAGE_SIZE).is_none()
        {
            let mut reject_line = heapless::String::<176>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut reject_line,
                format_args!(
                    "[local-seat] vl805 pci reject mmio=0x{mmio:016x} reason=no-device-coverage"
                ),
            );
            boot_log::force_uart_line(reject_line.as_str());
            continue;
        }

        let command_before =
            (pci_cfg_read_u32(config_virt, PCI_CFG_COMMAND_STATUS) & 0xffff) as u16;
        // Keep VL805 INTx masked during bring-up to avoid fatal interrupt storms
        // while still enabling memory decode + DMA for xHCI rings.
        let command_required = command_before
            | PCI_COMMAND_MEMORY_SPACE
            | PCI_COMMAND_BUS_MASTER
            | PCI_COMMAND_INTERRUPT_DISABLE;
        if command_required != command_before {
            pci_cfg_write_u16(config_virt, PCI_CFG_COMMAND_STATUS, command_required);
        }
        let command_after = (pci_cfg_read_u32(config_virt, PCI_CFG_COMMAND_STATUS) & 0xffff) as u16;

        let mut line = heapless::String::<288>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] vl805 pci selected ecam=0x{ecam:016x} cfg=0x{cfg:016x} cfg_src={cfg_src} mode=rw-cmd vid:did={vid:04x}:{did:04x} class=0x{class_code:06x} cmd=0x{before:04x}->0x{after:04x} bar0=0x{bar0:08x}",
                ecam = ecam_base,
                cfg = config_paddr,
                vid = vendor_id,
                did = device_id,
                before = command_before,
                after = command_after,
                cfg_src = cfg_src,
            ),
        );
        boot_log::force_uart_line(line.as_str());
        if (command_after & (PCI_COMMAND_MEMORY_SPACE | PCI_COMMAND_BUS_MASTER))
            != (PCI_COMMAND_MEMORY_SPACE | PCI_COMMAND_BUS_MASTER)
        {
            let mut warn_line = heapless::String::<224>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut warn_line,
                format_args!(
                    "[local-seat] vl805 pci command bits missing ecam=0x{ecam:016x} cmd=0x{cmd:04x} required=0x{required:04x}",
                    ecam = ecam_base,
                    cmd = command_after,
                    required = PCI_COMMAND_MEMORY_SPACE | PCI_COMMAND_BUS_MASTER
                ),
            );
            boot_log::force_uart_line(warn_line.as_str());
        }
        remember_vl805_cfg_virt(config_virt);

        return Some(mmio);
    }
    None
}

struct UsbKeyboard {
    hid: HidDevice<SeatDma>,
    _xhci_irq_guard: Option<XhciIrqGuard>,
    last_keys: [u8; 6],
    caps_lock_on: bool,
    poll_error_logged: bool,
    led_error_logged: bool,
    first_report_logged: bool,
    pending_display_scroll_rows: i8,
}

struct XhciIrqGuard {
    root_cnode: sel4_sys::seL4_CPtr,
    bindings: [Option<XhciIrqBinding>; TRUSTED_XHCI_PCIE_SINK_IRQS.len()],
}

#[derive(Clone, Copy)]
struct XhciIrqBinding {
    handler_slot: sel4_sys::seL4_CPtr,
    notification_slot: sel4_sys::seL4_CPtr,
    irq: u32,
    owns_handler: bool,
    shadow: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum XhciIrqInstallPhase {
    PreControllerReady,
    ControllerReady,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct XhciIrqHandlerScanSummary {
    first: Option<sel4_sys::seL4_CPtr>,
    second: Option<sel4_sys::seL4_CPtr>,
    count: usize,
    scan_end: sel4_sys::seL4_CPtr,
}

impl XhciIrqHandlerScanSummary {
    #[inline]
    const fn unique_slot(self) -> Result<Option<sel4_sys::seL4_CPtr>, usize> {
        if self.count > 1 {
            Err(self.count)
        } else {
            Ok(self.first)
        }
    }
}

impl XhciIrqGuard {
    #[inline]
    fn covers_irq(&self, irq: u32) -> bool {
        self.bindings
            .iter()
            .flatten()
            .any(|binding| binding.irq == irq)
    }

    fn log_policy(
        mmio: usize,
        firmware_handoff: XhciFirmwareHandoff,
        phase: XhciIrqInstallPhase,
        require_primary_pcie_irq: bool,
    ) {
        let sink_mode = xhci_irq_sink_mode(mmio, firmware_handoff, phase, require_primary_pcie_irq);
        let mut line = heapless::String::<256>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] xhci irq policy mmio=0x{mmio:016x} stage={} mode={} reason={}",
                xhci_irq_phase_label(phase),
                match sink_mode {
                    XhciIrqSinkMode::Disabled => "poll-only",
                    XhciIrqSinkMode::TrustedPcieSinks => {
                        "poll-only+pcie-bridge+intx-sink"
                    }
                },
                xhci_irq_policy_reason(firmware_handoff),
            ),
        );
        boot_log::force_uart_line(line.as_str());
    }

    fn install_binding(
        env: &mut crate::sel4::KernelEnv<'_>,
        root_cnode: sel4_sys::seL4_CPtr,
        depth: u8,
        mmio: usize,
        irq: u32,
        shadow: bool,
    ) -> Result<Option<XhciIrqBinding>, &'static str> {
        let requested_handler_slot = env.allocate_slot();
        let mut line = heapless::String::<240>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] xhci irq {}request irq={} mmio=0x{mmio:016x} slot=0x{slot:04x} depth={depth}",
                if shadow { "shadow " } else { "sink " },
                irq,
                slot = requested_handler_slot,
            ),
        );
        boot_log::force_uart_line(line.as_str());

        let get_err = crate::sel4::irq_control_get_level_handler(
            irq as sel4_sys::seL4_Word,
            root_cnode,
            requested_handler_slot,
            depth,
        );
        let (handler_slot, owns_handler) = if shadow {
            if let Some(reason) = xhci_shadow_irq_handoff_contract_reason(get_err) {
                let _ = crate::sel4::cnode_delete(root_cnode, requested_handler_slot, depth);
                let mut line = heapless::String::<256>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] xhci irq shadow unavailable irq={} mmio=0x{mmio:016x} err={} ({}) reason={reason}",
                        irq,
                        get_err,
                        crate::sel4::error_name(get_err),
                    ),
                );
                boot_log::force_uart_line(line.as_str());
                return Err(reason);
            }
            (requested_handler_slot, true)
        } else {
            let handler_scan = if get_err == sel4_sys::seL4_RevokeFirst {
                let summary = xhci_irq_handler_scan_summary(env);
                let mut line = heapless::String::<256>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] xhci irq sink scan irq={} mmio=0x{mmio:016x} matches={} first=0x{:04x} second=0x{:04x} end=0x{:04x} after=get-revoke-first",
                        irq,
                        summary.count,
                        summary.first.unwrap_or(0),
                        summary.second.unwrap_or(0),
                        summary.scan_end,
                    ),
                );
                boot_log::force_uart_line(line.as_str());
                Some(summary)
            } else {
                None
            };
            let existing_handler = handler_scan
                .map(XhciIrqHandlerScanSummary::unique_slot)
                .unwrap_or_else(|| unique_existing_irq_handler_slot(env));
            match resolve_xhci_irq_handler_acquisition(get_err, existing_handler) {
                Ok(()) => (requested_handler_slot, true),
                Err("irq-handler-ambiguous") => {
                    let _ = crate::sel4::cnode_delete(root_cnode, requested_handler_slot, depth);
                    let mut line = heapless::String::<224>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut line,
                        format_args!(
                            "[local-seat] xhci irq sink ambiguous existing handlers irq={} mmio=0x{mmio:016x} matches={count} after=get-revoke-first",
                            irq,
                            count = match unique_existing_irq_handler_slot(env) {
                                Err(count) => count,
                                _ => 0,
                            },
                        ),
                    );
                    boot_log::force_uart_line(line.as_str());
                    return Err("irq-handler-ambiguous");
                }
                Err("irq-get-revoke-first-no-handler") => {
                    let _ = crate::sel4::cnode_delete(root_cnode, requested_handler_slot, depth);
                    let mut line = heapless::String::<256>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut line,
                        format_args!(
                            "[local-seat] xhci irq sink get saw existing owner irq={} mmio=0x{mmio:016x} err={} ({}) but no reusable handler cap was found",
                            irq,
                            get_err,
                            crate::sel4::error_name(get_err),
                        ),
                    );
                    boot_log::force_uart_line(line.as_str());
                    return Err("irq-get-revoke-first-no-handler");
                }
                Err(reason) => {
                    let _ = crate::sel4::cnode_delete(root_cnode, requested_handler_slot, depth);
                    let mut line = heapless::String::<256>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut line,
                        format_args!(
                            "[local-seat] xhci irq sink get failed irq={} mmio=0x{mmio:016x} err={} ({}) reason={reason}",
                            irq,
                            get_err,
                            crate::sel4::error_name(get_err),
                        ),
                    );
                    boot_log::force_uart_line(line.as_str());
                    return Err(reason);
                }
            }
        };

        let notification_slot = match env.alloc_notification() {
            Ok(slot) => slot,
            Err(_) => {
                if owns_handler {
                    let _ = crate::sel4::cnode_delete(root_cnode, handler_slot, depth);
                }
                return if shadow {
                    Err("irq-shadow-notification")
                } else {
                    Err("notification-retype")
                };
            }
        };
        let bind_err = crate::sel4::irq_handler_set_notification(handler_slot, notification_slot);
        if bind_err != sel4_sys::seL4_NoError {
            let _ = crate::sel4::irq_handler_clear(handler_slot);
            let _ = crate::sel4::cnode_delete(root_cnode, notification_slot, depth);
            if owns_handler {
                let _ = crate::sel4::cnode_delete(root_cnode, handler_slot, depth);
            }
            let mut line = heapless::String::<224>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] xhci irq {}bind failed irq={} mmio=0x{mmio:016x} err={} ({})",
                    if shadow { "shadow " } else { "sink " },
                    irq,
                    bind_err,
                    crate::sel4::error_name(bind_err),
                ),
            );
            boot_log::force_uart_line(line.as_str());
            return if shadow {
                Err("irq-shadow-set-notification")
            } else {
                Err("irq-set-notification")
            };
        }

        let mut line = heapless::String::<224>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] xhci irq {}armed irq={} mmio=0x{mmio:016x} handler=0x{handler:04x} notif=0x{notif:04x} owned={owned}",
                if shadow { "shadow " } else { "sink " },
                irq,
                handler = handler_slot,
                notif = notification_slot,
                owned = if owns_handler { 1 } else { 0 },
            ),
        );
        boot_log::force_uart_line(line.as_str());

        Ok(Some(XhciIrqBinding {
            handler_slot,
            notification_slot,
            irq,
            owns_handler,
            shadow,
        }))
    }

    fn install(
        hal: &mut KernelHal<'_>,
        mmio: usize,
        firmware_handoff: XhciFirmwareHandoff,
        phase: XhciIrqInstallPhase,
        require_primary_pcie_irq: bool,
    ) -> Result<Option<Self>, &'static str> {
        Self::log_policy(mmio, firmware_handoff, phase, require_primary_pcie_irq);
        let sink_mode = xhci_irq_sink_mode(mmio, firmware_handoff, phase, require_primary_pcie_irq);

        if matches!(sink_mode, XhciIrqSinkMode::Disabled) {
            return Ok(None);
        }

        let env = hal.as_env_mut();
        let depth = crate::sel4::word_bits() as u8;
        let root_cnode = env.init_cnode_cap();

        let mut bindings = [None; TRUSTED_XHCI_PCIE_SINK_IRQS.len()];
        match sink_mode {
            XhciIrqSinkMode::TrustedPcieSinks => {
                for (index, irq) in TRUSTED_XHCI_PCIE_SINK_IRQS.iter().copied().enumerate() {
                    match Self::install_binding(env, root_cnode, depth, mmio, irq, false) {
                        Ok(binding) => bindings[index] = binding,
                        Err(err) if xhci_trusted_irq_soft_ignore_reason(irq, err) => {
                            let mut line = heapless::String::<256>::new();
                            let _ = core::fmt::Write::write_fmt(
                                &mut line,
                                format_args!(
                                    "[local-seat] xhci irq sink degraded irq={} mmio=0x{mmio:016x} detail={err} action=continue-with-partial-sinks",
                                    irq,
                                ),
                            );
                            boot_log::force_uart_line(line.as_str());
                        }
                        Err(err) => {
                            for binding in bindings.iter().flatten() {
                                let _ = crate::sel4::irq_handler_clear(binding.handler_slot);
                                let _ = crate::sel4::cnode_delete(
                                    root_cnode,
                                    binding.notification_slot,
                                    depth,
                                );
                                if binding.owns_handler {
                                    let _ = crate::sel4::cnode_delete(
                                        root_cnode,
                                        binding.handler_slot,
                                        depth,
                                    );
                                }
                            }
                            return Err(err);
                        }
                    }
                }
            }
            XhciIrqSinkMode::Disabled => {}
        };

        Ok(Some(Self {
            root_cnode,
            bindings,
        }))
    }
}

impl Drop for XhciIrqGuard {
    fn drop(&mut self) {
        let depth = crate::sel4::word_bits() as u8;
        for binding in self.bindings.iter().flatten() {
            let clear_result = crate::sel4::irq_handler_clear(binding.handler_slot);
            let delete_notification_result =
                crate::sel4::cnode_delete(self.root_cnode, binding.notification_slot, depth);
            let delete_handler_result = if binding.owns_handler {
                crate::sel4::cnode_delete(self.root_cnode, binding.handler_slot, depth)
            } else {
                sel4_sys::seL4_NoError
            };
            let mut line = heapless::String::<240>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] xhci irq {}drop irq={} handler=0x{:04x} notif=0x{:04x} clear={} ({}) delete_handler={} ({}) delete_notif={} ({}) owned={}",
                    if binding.shadow { "shadow " } else { "sink " },
                    binding.irq,
                    binding.handler_slot,
                    binding.notification_slot,
                    clear_result,
                    crate::sel4::error_name(clear_result),
                    delete_handler_result,
                    crate::sel4::error_name(delete_handler_result),
                    delete_notification_result,
                    crate::sel4::error_name(delete_notification_result),
                    if binding.owns_handler { 1 } else { 0 },
                ),
            );
            boot_log::force_uart_line(line.as_str());
        }
    }
}

#[inline]
fn unique_existing_irq_handler_slot(
    env: &crate::sel4::KernelEnv<'_>,
) -> Result<Option<sel4_sys::seL4_CPtr>, usize> {
    xhci_irq_handler_scan_summary(env).unique_slot()
}

#[inline]
fn xhci_irq_handler_scan_summary(env: &crate::sel4::KernelEnv<'_>) -> XhciIrqHandlerScanSummary {
    let mut first = None;
    let mut second = None;
    let mut count = 0usize;
    let scan_end = 1usize
        .checked_shl(env.bootinfo().init_cnode_bits() as u32)
        .unwrap_or(env.bootinfo().empty.end as usize) as sel4_sys::seL4_CPtr;
    let mut slot: sel4_sys::seL4_CPtr = 0;
    while slot < scan_end {
        let tag = crate::sel4::CapTag::from_raw(crate::sel4::debug_cap_identify(slot));
        if matches!(tag, Some(crate::sel4::CapTag::IrqHandler)) {
            count = count.saturating_add(1);
            if first.is_none() {
                first = Some(slot);
            } else if second.is_none() {
                second = Some(slot);
            }
        }
        slot = slot.saturating_add(1);
    }
    XhciIrqHandlerScanSummary {
        first,
        second,
        count,
        scan_end,
    }
}

#[inline]
fn resolve_xhci_irq_handler_acquisition(
    get_err: sel4_sys::seL4_Error,
    existing: Result<Option<sel4_sys::seL4_CPtr>, usize>,
) -> Result<(), &'static str> {
    if get_err == sel4_sys::seL4_NoError {
        return Ok(());
    }
    if get_err != sel4_sys::seL4_RevokeFirst {
        return Err("irq-get-handler");
    }
    match existing {
        Ok(Some(_)) => Err("irq-get-revoke-first-owned"),
        Ok(None) => Err("irq-get-revoke-first-no-handler"),
        Err(_) => Err("irq-handler-ambiguous"),
    }
}

#[inline]
const fn xhci_shadow_irq_handoff_contract_reason(
    get_err: sel4_sys::seL4_Error,
) -> Option<&'static str> {
    if get_err == sel4_sys::seL4_NoError {
        None
    } else if get_err == sel4_sys::seL4_RevokeFirst {
        Some("irq-shadow-owned")
    } else {
        Some("irq-shadow-get-handler")
    }
}

#[inline]
const fn xhci_runtime_uses_trusted_handoff(firmware_handoff: XhciFirmwareHandoff) -> bool {
    !matches!(firmware_handoff, XhciFirmwareHandoff::None)
}

#[inline]
const fn xhci_irq_policy_reason(firmware_handoff: XhciFirmwareHandoff) -> &'static str {
    match firmware_handoff {
        XhciFirmwareHandoff::None => "runtime-default",
        XhciFirmwareHandoff::ColdStartFromSnapshot => "fw-handoff-cold-start-from-snapshot",
        XhciFirmwareHandoff::ResetlessReinit => "fw-handoff-resetless-reinit",
        XhciFirmwareHandoff::PreserveControllerState => "fw-handoff-preserve",
    }
}

#[inline]
const fn xhci_polling_only_runtime(mmio: usize, firmware_handoff: XhciFirmwareHandoff) -> bool {
    mmio == RPI4_XHCI_MMIO_HIGH_CANDIDATE
        && (xhci_runtime_uses_trusted_handoff(firmware_handoff)
            || matches!(firmware_handoff, XhciFirmwareHandoff::None))
}

#[inline]
const fn xhci_irq_phase_label(phase: XhciIrqInstallPhase) -> &'static str {
    match phase {
        XhciIrqInstallPhase::PreControllerReady => "pre-controller-ready",
        XhciIrqInstallPhase::ControllerReady => "controller-ready",
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum XhciIrqSinkMode {
    Disabled,
    TrustedPcieSinks,
}

#[inline]
const fn xhci_irq_sink_mode(
    mmio: usize,
    firmware_handoff: XhciFirmwareHandoff,
    phase: XhciIrqInstallPhase,
    require_primary_pcie_irq: bool,
) -> XhciIrqSinkMode {
    if xhci_polling_only_runtime(mmio, firmware_handoff) && TRUSTED_XHCI_PCIE_SINKS_ENABLED {
        match phase {
            XhciIrqInstallPhase::PreControllerReady if require_primary_pcie_irq => {
                XhciIrqSinkMode::TrustedPcieSinks
            }
            XhciIrqInstallPhase::ControllerReady => {
                // Match the known-good U-Boot-style path: once RUN has reached
                // controller-ready, enumeration is poll-driven. Re-requesting
                // IRQ 27 here is the kernel timer PPI, not part of xHCI
                // polling, and requesting it can steal or disturb kernel time.
                XhciIrqSinkMode::Disabled
            }
            XhciIrqInstallPhase::PreControllerReady => XhciIrqSinkMode::Disabled,
        }
    } else {
        XhciIrqSinkMode::Disabled
    }
}

#[inline]
const fn xhci_irq_sink_needed(
    mmio: usize,
    firmware_handoff: XhciFirmwareHandoff,
    phase: XhciIrqInstallPhase,
    require_primary_pcie_irq: bool,
) -> bool {
    !matches!(
        xhci_irq_sink_mode(mmio, firmware_handoff, phase, require_primary_pcie_irq),
        XhciIrqSinkMode::Disabled
    )
}

#[inline]
fn xhci_trusted_irq_soft_ignore_reason(_irq: u32, _reason: &'static str) -> bool {
    false
}

#[inline]
fn xhci_irq_guard_satisfies_phase(
    guard: &XhciIrqGuard,
    mmio: usize,
    firmware_handoff: XhciFirmwareHandoff,
    phase: XhciIrqInstallPhase,
    require_primary_pcie_irq: bool,
) -> bool {
    match xhci_irq_sink_mode(mmio, firmware_handoff, phase, require_primary_pcie_irq) {
        XhciIrqSinkMode::Disabled => true,
        XhciIrqSinkMode::TrustedPcieSinks => {
            let _ = mmio;
            let _ = firmware_handoff;
            let _ = phase;
            TRUSTED_XHCI_PCIE_SINK_IRQS
                .iter()
                .copied()
                .all(|irq| guard.covers_irq(irq))
        }
    }
}

#[inline]
const fn xhci_runtime_init_strategy_requires_primary_pcie_irq(
    mmio: usize,
    strategy: XhciRuntimeInitStrategy,
) -> bool {
    mmio == RPI4_XHCI_MMIO_HIGH_CANDIDATE
        && matches!(
            strategy.firmware_handoff,
            XhciFirmwareHandoff::ColdStartFromSnapshot | XhciFirmwareHandoff::None
        )
        && (!strategy.seed_stop_state
            || matches!(strategy.firmware_handoff, XhciFirmwareHandoff::None))
}

#[derive(Clone, Copy)]
struct HubPortStatus {
    status: u16,
    change: u16,
}

#[derive(Clone, Copy, Debug)]
enum HubPortStatusReadError {
    Control(UsbError),
    ShortTransfer {
        transferred: usize,
        bytes: [u8; HUB_PORT_STATUS_BYTES],
    },
}

#[derive(Clone, Copy)]
struct HubInterfaceInfo {
    protocol: u8,
    multi_tt: bool,
    interface_number: u8,
}

enum HubChildProbeResult {
    Keyboard(HidDevice<SeatDma>),
    ProbedNoKeyboard,
    Failed,
}

impl HubPortStatus {
    #[inline]
    const fn connected(self) -> bool {
        (self.status & (1 << 0)) != 0
    }

    #[inline]
    const fn enabled(self) -> bool {
        (self.status & (1 << 1)) != 0
    }

    #[inline]
    const fn reset(self) -> bool {
        (self.status & (1 << 4)) != 0
    }

    #[inline]
    const fn powered(self) -> bool {
        (self.status & (1 << 8)) != 0
    }

    #[inline]
    const fn low_speed(self) -> bool {
        (self.status & (1 << 9)) != 0
    }

    #[inline]
    const fn high_speed(self) -> bool {
        (self.status & (1 << 10)) != 0
    }

    #[inline]
    const fn raw_bytes(self) -> [u8; HUB_PORT_STATUS_BYTES] {
        [
            (self.status & 0x00ff) as u8,
            ((self.status >> 8) & 0x00ff) as u8,
            (self.change & 0x00ff) as u8,
            ((self.change >> 8) & 0x00ff) as u8,
        ]
    }
}

impl UsbKeyboard {
    fn new(
        hal: &mut KernelHal<'_>,
        xhci_mmio_hint: Option<usize>,
        xhci_pci_cmd: Option<u16>,
        xhci_handoff_ready: bool,
        xhci_irq_quiesced: bool,
        xhci_bootloader_reset_authorized: bool,
        xhci_capability_probe: Option<XhciCapProbe>,
        xhci_stop_state_snapshot: Option<LocalSeatXhciStopStateSnapshot>,
        prompt_safe_probe: bool,
    ) -> Result<Self, Pi4SeatError> {
        if !KEYBOARD_RUNTIME_INIT_LOGGED.swap(true, Ordering::AcqRel) {
            boot_log::force_uart_line("[local-seat] usb keyboard init path entered");
        }
        if prompt_safe_probe {
            boot_log::force_uart_line(
                "[local-seat] usb keyboard init mode=prompt-safe detail=prefer-firmware-snapshot-paths",
            );
        }
        if !XHCI_DIAG_HOOK_INSTALLED.swap(true, Ordering::AcqRel) {
            set_xhci_diag_hook(Some(xhci_diag_hook));
            boot_log::force_uart_line("[local-seat] xhci diag hook installed");
        }
        let cfg_window_present = current_vl805_cfg_virt().is_some();
        let runtime_cfg_touch_enabled =
            vl805_runtime_cfg_touch_allowed(VL805_CFG_RUNTIME_TOUCH_ENABLED, cfg_window_present);
        if runtime_cfg_touch_enabled {
            if cfg_window_present {
                boot_log::force_uart_line("[local-seat] vl805 cfg present stage=usb-init-entry");
            } else {
                boot_log::force_uart_line("[local-seat] vl805 cfg missing stage=usb-init-entry");
            }
        } else if VL805_CFG_RUNTIME_TOUCH_ENABLED {
            if !VL805_CFG_RUNTIME_GATED_LOGGED.swap(true, Ordering::AcqRel) {
                boot_log::force_uart_line(
                    "[local-seat] vl805 pci cfg runtime preflight skipped reason=no-safe-cfg-window",
                );
            }
        } else if !VL805_CFG_SAFE_MODE_LOGGED.swap(true, Ordering::AcqRel) {
            boot_log::force_uart_line("[local-seat] vl805 pci cfg touch disabled (safe-mode)");
        }

        let trusted_pinned_xhci_mmio = pinned_xhci_phys_start_trusted();
        let (vl805_pci_mmio, pci_cfg_ready) = if runtime_cfg_touch_enabled {
            let vl805_pci_mmio = prepare_vl805_pci(hal);
            if vl805_pci_mmio.is_none() {
                boot_log::force_uart_line("[local-seat] vl805 pci preflight=none");
            }
            let pci_cfg_ready = vl805_cfg_bus_master_ready();
            if !pci_cfg_ready {
                let mut line = heapless::String::<192>::new();
                let command = vl805_cfg_command().unwrap_or(0);
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] vl805 cfg unavailable; xhci probe forced no-bus-master cmd=0x{command:04x}"
                    ),
                );
                boot_log::force_uart_line(line.as_str());
            }
            (vl805_pci_mmio, pci_cfg_ready)
        } else {
            (None, false)
        };

        let pinned_xhci_state = pinned_xhci_phys_state();
        let verified_vl805_hint = current_vl805_xhci_mmio_hint();
        let has_safe_cfg_window = current_vl805_cfg_virt().is_some();
        let firmware_hint_safe = xhci_firmware_handoff_safe(xhci_pci_cmd);
        let mut source_line = heapless::String::<256>::new();
        let _ = core::fmt::Write::write_str(&mut source_line, "[local-seat] xhci source fw=");
        match xhci_mmio_hint {
            Some(mmio) => {
                let _ =
                    core::fmt::Write::write_fmt(&mut source_line, format_args!("0x{mmio:016x}"));
            }
            None => {
                let _ = core::fmt::Write::write_str(&mut source_line, "none");
            }
        }
        let _ = core::fmt::Write::write_str(&mut source_line, " pin=");
        match pinned_xhci_state {
            Some((mmio, trusted)) => {
                let _ = core::fmt::Write::write_fmt(
                    &mut source_line,
                    format_args!("0x{mmio:016x}/t{}", trusted as u8),
                );
            }
            None => {
                let _ = core::fmt::Write::write_str(&mut source_line, "none");
            }
        }
        let _ = core::fmt::Write::write_str(&mut source_line, " ver=");
        match verified_vl805_hint {
            Some(mmio) => {
                let _ =
                    core::fmt::Write::write_fmt(&mut source_line, format_args!("0x{mmio:016x}"));
            }
            None => {
                let _ = core::fmt::Write::write_str(&mut source_line, "none");
            }
        }
        let _ = core::fmt::Write::write_str(&mut source_line, " pci=");
        match vl805_pci_mmio {
            Some(mmio) => {
                let _ =
                    core::fmt::Write::write_fmt(&mut source_line, format_args!("0x{mmio:016x}"));
            }
            None => {
                let _ = core::fmt::Write::write_str(&mut source_line, "none");
            }
        }
        let _ = core::fmt::Write::write_str(&mut source_line, " cmd=");
        match xhci_pci_cmd {
            Some(cmd) => {
                let _ = core::fmt::Write::write_fmt(
                    &mut source_line,
                    format_args!("0x{cmd:04x}/safe{}", firmware_hint_safe as u8),
                );
            }
            None => {
                let _ = core::fmt::Write::write_str(&mut source_line, "none/safe0");
            }
        }
        let _ = core::fmt::Write::write_fmt(
            &mut source_line,
            format_args!(
                " handoff={} irq={} reset_auth={}",
                xhci_handoff_ready as u8,
                xhci_irq_quiesced as u8,
                xhci_bootloader_reset_authorized as u8,
            ),
        );
        boot_log::force_uart_line(source_line.as_str());
        let trusted_fw_handoff_high_bar = xhci_firmware_handoff_cold_start_trusted(
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            xhci_mmio_hint,
            xhci_pci_cmd,
            xhci_handoff_ready,
            xhci_irq_quiesced,
        );
        let runtime_vl805_reset_strategy = xhci_runtime_vl805_mailbox_reset_required(
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            xhci_mmio_hint,
            xhci_pci_cmd,
            xhci_handoff_ready,
            xhci_irq_quiesced,
        );
        let runtime_handoff_snapshot_available = xhci_capability_probe.is_some()
            && xhci_trusted_handoff_snapshot_allowed(
                RPI4_XHCI_MMIO_HIGH_CANDIDATE,
                xhci_mmio_hint,
                xhci_pci_cmd,
                xhci_handoff_ready,
                xhci_irq_quiesced,
                false,
            );
        if trusted_fw_handoff_high_bar {
            if has_safe_cfg_window {
                let command = xhci_safe_mode_skip_command(xhci_pci_cmd);
                let mut line = heapless::String::<224>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] vl805 handoff pci-cmd replay skipped reason=runtime-ecam-unsafe exported=0x{command:04x}"
                    ),
                );
                boot_log::force_uart_line(line.as_str());
            } else {
                boot_log::force_uart_line(
                    "[local-seat] vl805 handoff pci-cmd replay skipped reason=cfg-window-absent",
                );
            }
        }
        let effective_trusted_pinned_xhci_mmio = trusted_pinned_xhci_mmio.filter(|&mmio| {
            !matches!(mmio, RPI4_XHCI_MMIO_HIGH_CANDIDATE) || trusted_fw_handoff_high_bar
        });
        if trusted_pinned_xhci_mmio == Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE)
            && effective_trusted_pinned_xhci_mmio.is_none()
        {
            let mut line = heapless::String::<224>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] xhci runtime trust revoked reason={}",
                    xhci_firmware_handoff_revoked_reason(xhci_handoff_ready, xhci_irq_quiesced)
                ),
            );
            boot_log::force_uart_line(line.as_str());
        }
        let legacy_mirror_allowed = xhci_firmware_handoff_allows_legacy_probe(
            xhci_mmio_hint,
            xhci_pci_cmd,
            xhci_handoff_ready,
            xhci_irq_quiesced,
        ) && verified_vl805_hint.is_none()
            && vl805_pci_mmio.is_none()
            && effective_trusted_pinned_xhci_mmio != Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE);
        log_xhci_firmware_handoff_summary(
            "runtime",
            xhci_mmio_hint,
            xhci_pci_cmd,
            xhci_handoff_ready,
            xhci_irq_quiesced,
            effective_trusted_pinned_xhci_mmio == Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE)
                && trusted_fw_handoff_high_bar,
        );
        if xhci_mmio_hint == Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE) {
            let mut line = heapless::String::<224>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] xhci handoff gate mmio=0x{mmio:016x} token={} irq={} cmd_safe={} trusted-pin={}",
                    xhci_handoff_ready as u8,
                    xhci_irq_quiesced as u8,
                    firmware_hint_safe as u8,
                    (effective_trusted_pinned_xhci_mmio == Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE))
                        as u8,
                    mmio = RPI4_XHCI_MMIO_HIGH_CANDIDATE,
                ),
            );
            boot_log::force_uart_line(line.as_str());
        }
        if effective_trusted_pinned_xhci_mmio == Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE)
            && trusted_fw_handoff_high_bar
        {
            let mut line = heapless::String::<224>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] xhci runtime trust source={} mmio=0x{mmio:016x} cmd=0x{cmd:04x} token=1 irq=1",
                    xhci_runtime_handoff_source_label(
                        runtime_vl805_reset_strategy,
                        runtime_handoff_snapshot_available,
                        xhci_bootloader_reset_authorized,
                    ),
                    mmio = RPI4_XHCI_MMIO_HIGH_CANDIDATE,
                    cmd = xhci_pci_cmd.unwrap_or(0),
                ),
            );
            boot_log::force_uart_line(line.as_str());
        }
        let mut candidates = [0usize; XHCI_MMIO_CANDIDATE_LIMIT];
        let mut candidate_count = 0usize;
        let mut consider_candidate = |mmio: usize| -> bool {
            if candidate_count >= candidates.len() {
                return false;
            }
            let has_device_coverage = hal.device_coverage(mmio, crate::sel4::PAGE_BITS).is_some();
            let has_pinned_window = pinned_xhci_window_lookup(mmio, PAGE_SIZE).is_some();
            if !xhci_runtime_mmio_candidate_allowed(
                mmio,
                has_safe_cfg_window,
                pinned_xhci_state,
                effective_trusted_pinned_xhci_mmio,
                xhci_mmio_hint,
                verified_vl805_hint,
                legacy_mirror_allowed,
            ) {
                log_xhci_runtime_candidate_diag(
                    mmio,
                    has_safe_cfg_window,
                    has_device_coverage,
                    has_pinned_window,
                    pinned_xhci_state,
                    effective_trusted_pinned_xhci_mmio,
                    xhci_mmio_hint,
                    verified_vl805_hint,
                    firmware_hint_safe,
                );
                let reason = xhci_runtime_candidate_skip_reason(
                    mmio,
                    has_safe_cfg_window,
                    effective_trusted_pinned_xhci_mmio,
                    xhci_mmio_hint,
                    verified_vl805_hint,
                    firmware_hint_safe,
                    legacy_mirror_allowed,
                );
                let mut line = heapless::String::<208>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] xhci candidate skipped mmio=0x{mmio:016x} reason={reason}"
                    ),
                );
                boot_log::force_uart_line(line.as_str());
                return false;
            }
            if !xhci_mmio_candidate_valid(mmio) {
                let mut line = heapless::String::<184>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] xhci candidate ignored mmio=0x{mmio:016x} reason=invalid-candidate"
                    ),
                );
                boot_log::force_uart_line(line.as_str());
                return false;
            }
            if !xhci_runtime_mmio_has_accessible_window(
                mmio,
                has_device_coverage,
                has_pinned_window,
                pinned_xhci_state,
                trusted_pinned_xhci_mmio,
                xhci_mmio_hint,
                verified_vl805_hint,
            ) {
                log_xhci_runtime_candidate_diag(
                    mmio,
                    has_safe_cfg_window,
                    has_device_coverage,
                    has_pinned_window,
                    pinned_xhci_state,
                    effective_trusted_pinned_xhci_mmio,
                    xhci_mmio_hint,
                    verified_vl805_hint,
                    firmware_hint_safe,
                );
                let mut line = heapless::String::<224>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] xhci candidate skipped mmio=0x{mmio:016x} reason=no-accessible-window trusted-pin={}",
                        if effective_trusted_pinned_xhci_mmio == Some(mmio) {
                            "yes"
                        } else {
                            "no"
                        }
                    ),
                );
                boot_log::force_uart_line(line.as_str());
                return false;
            }
            if candidates[..candidate_count].contains(&mmio) {
                return true;
            }
            candidates[candidate_count] = mmio;
            candidate_count = candidate_count.saturating_add(1);
            true
        };
        match preferred_xhci_runtime_mmio(
            effective_trusted_pinned_xhci_mmio,
            xhci_mmio_hint,
            vl805_pci_mmio,
            verified_vl805_hint,
            legacy_mirror_allowed,
        ) {
            Some(preferred_mmio) => {
                if preferred_mmio == RPI4_XHCI_MMIO_HIGH_CANDIDATE
                    && effective_trusted_pinned_xhci_mmio == Some(preferred_mmio)
                    && xhci_firmware_handoff_cold_start_trusted(
                        preferred_mmio,
                        xhci_mmio_hint,
                        xhci_pci_cmd,
                        xhci_handoff_ready,
                        xhci_irq_quiesced,
                    )
                {
                    let mut line = heapless::String::<224>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut line,
                        format_args!(
                            "[local-seat] xhci runtime preferred source={} mmio=0x{mmio:016x}",
                            xhci_runtime_handoff_source_label(
                                runtime_vl805_reset_strategy,
                                runtime_handoff_snapshot_available,
                                xhci_bootloader_reset_authorized,
                            ),
                            mmio = preferred_mmio
                        ),
                    );
                    boot_log::force_uart_line(line.as_str());
                }
                consider_candidate(preferred_mmio);
                if let Some(hint) = xhci_mmio_hint {
                    if hint != preferred_mmio {
                        let hint_retained = consider_candidate(hint);
                        let mut line = heapless::String::<208>::new();
                        let _ = core::fmt::Write::write_fmt(
                            &mut line,
                            format_args!(
                                "[local-seat] xhci hint {} hint=0x{hint:016x} preferred=0x{preferred:016x} safe={}",
                                if hint_retained { "retained" } else { "ignored" },
                                firmware_hint_safe as u8,
                                preferred = preferred_mmio
                            ),
                        );
                        boot_log::force_uart_line(line.as_str());
                    }
                }
                if let Some(remembered_mmio) = verified_vl805_hint {
                    if remembered_mmio != preferred_mmio {
                        consider_candidate(remembered_mmio);
                    }
                }
                for fallback in RPI4_XHCI_MMIO_FALLBACKS {
                    if fallback != preferred_mmio {
                        consider_candidate(fallback);
                    }
                }
            }
            None => {
                if let Some(hint) = xhci_mmio_hint {
                    consider_candidate(hint);
                }
                if let Some(hint) = verified_vl805_hint {
                    consider_candidate(hint);
                }
                for fallback in RPI4_XHCI_MMIO_FALLBACKS {
                    consider_candidate(fallback);
                }
            }
        }
        let mut summary = heapless::String::<224>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut summary,
            format_args!(
                "[local-seat] xhci runtime candidates={} hint={} pci_cfg_ready={}",
                candidate_count,
                if xhci_mmio_hint.is_some() {
                    "yes"
                } else {
                    "no"
                },
                if pci_cfg_ready { "yes" } else { "no" }
            ),
        );
        boot_log::force_uart_line(summary.as_str());
        for (index, &mmio) in candidates[..candidate_count].iter().enumerate() {
            let mut line = heapless::String::<192>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] xhci runtime candidate[{}]=0x{mmio:016x}",
                    index
                ),
            );
            boot_log::force_uart_line(line.as_str());
        }
        if candidate_count == 0 {
            boot_log::force_uart_line("[local-seat] xhci runtime candidate set empty");
            if xhci_mmio_hint == Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE)
                && xhci_firmware_handoff_safe(xhci_pci_cmd)
                && (!xhci_handoff_ready || !xhci_irq_quiesced)
            {
                let mut line = heapless::String::<224>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] xhci runtime blocked action=reject-untrusted-high-bar reason={}",
                        xhci_firmware_handoff_revoked_reason(xhci_handoff_ready, xhci_irq_quiesced,)
                    ),
                );
                boot_log::force_uart_line(line.as_str());
            }
        }
        if !XHCI_DMA_POLICY_LOGGED.swap(true, Ordering::AcqRel) {
            let mut line = heapless::String::<224>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] xhci dma probe policy={} bus_addrs={}",
                    if XHCI_FORCE_LOW_DMA_PROBE {
                        "low-only"
                    } else {
                        "low-then-high"
                    },
                    if XHCI_PCIE_DMA_WINDOW_ENABLED && XHCI_TRY_RAW_PHYS_DMA_FALLBACK {
                        "pcie-window-then-phys"
                    } else if XHCI_PCIE_DMA_WINDOW_ENABLED {
                        "pcie-window-only"
                    } else {
                        "phys-only"
                    }
                ),
            );
            boot_log::force_uart_line(line.as_str());
        }

        let mut saw_controller = false;
        let mut saw_keyboard_init_error = false;
        let mut best_probe_pathway = None;
        for &mmio_base in &candidates[..candidate_count] {
            let (
                effective_mmio,
                cap_probe,
                firmware_handoff,
                runtime_vl805_reset_state,
                runtime_vl805_reset_requested,
            ) = {
                let runtime_vl805_reset_requested = xhci_runtime_vl805_mailbox_reset_required(
                    mmio_base,
                    xhci_mmio_hint,
                    xhci_pci_cmd,
                    xhci_handoff_ready,
                    xhci_irq_quiesced,
                );
                let bootloader_reset_authorized = xhci_bootloader_vl805_reset_authorized(
                    mmio_base,
                    xhci_mmio_hint,
                    xhci_pci_cmd,
                    xhci_handoff_ready,
                    xhci_irq_quiesced,
                    xhci_bootloader_reset_authorized,
                    xhci_stop_state_snapshot,
                );
                let mut runtime_vl805_reset = false;
                let mut runtime_vl805_reset_state =
                    VL805_RUNTIME_RESET_STATE.load(Ordering::Acquire);
                if runtime_vl805_reset_requested {
                    if let Err(err) = ensure_runtime_vl805_mailbox_reset(hal) {
                        if bootloader_reset_authorized {
                            authorize_bootloader_vl805_reset(mmio_base, err.as_str());
                        } else {
                            let mut line = heapless::String::<224>::new();
                            let _ = core::fmt::Write::write_fmt(
                                &mut line,
                                format_args!(
                                    "[local-seat] vl805 reset handoff=runtime-unconfirmed stage=runtime detail={} action=skip-candidate mmio=0x{mmio:016x}",
                                    err.as_str(),
                                    mmio = mmio_base,
                                ),
                            );
                            boot_log::force_uart_line(line.as_str());
                            continue;
                        }
                    }
                    runtime_vl805_reset_state = VL805_RUNTIME_RESET_STATE.load(Ordering::Acquire);
                    if bootloader_reset_authorized
                        && !runtime_vl805_mailbox_reset_completed(runtime_vl805_reset_state)
                    {
                        authorize_bootloader_vl805_reset(
                            mmio_base,
                            runtime_vl805_mailbox_reset_state_label(runtime_vl805_reset_state),
                        );
                        runtime_vl805_reset_state =
                            VL805_RUNTIME_RESET_STATE.load(Ordering::Acquire);
                    }
                    runtime_vl805_reset =
                        runtime_vl805_mailbox_reset_completed(runtime_vl805_reset_state);
                    if !runtime_vl805_mailbox_reset_allows_trusted_cold_init(
                        runtime_vl805_reset_state,
                    ) {
                        let mut line = heapless::String::<224>::new();
                        let _ = core::fmt::Write::write_fmt(
                            &mut line,
                            format_args!(
                                "[local-seat] vl805 reset handoff=runtime-unconfirmed stage=runtime detail=mailbox-reset-unconfirmed action=skip-candidate mmio=0x{mmio:016x}",
                                mmio = mmio_base,
                            ),
                        );
                        boot_log::force_uart_line(line.as_str());
                        continue;
                    }
                }

                let trusted_handoff_probe = xhci_capability_probe.filter(|_| {
                    xhci_trusted_handoff_snapshot_allowed(
                        mmio_base,
                        xhci_mmio_hint,
                        xhci_pci_cmd,
                        xhci_handoff_ready,
                        xhci_irq_quiesced,
                        runtime_vl805_mailbox_reset_allows_trusted_cold_init(
                            runtime_vl805_reset_state,
                        ),
                    )
                });
                let preferred_firmware_handoff = if trusted_handoff_probe.is_some() {
                    let handoff_mode =
                        xhci_preferred_trusted_handoff_mode(runtime_vl805_reset_state);
                    if runtime_vl805_reset {
                        let mut line = heapless::String::<224>::new();
                        let _ = core::fmt::Write::write_fmt(
                            &mut line,
                            format_args!(
                                "[local-seat] xhci handoff=runtime-owned stage=runtime detail=mailbox-reset+trusted-cap-snapshot action=fresh-init-from-cap-snapshot mode={}",
                                xhci_firmware_handoff_mode_label(handoff_mode),
                            ),
                        );
                        boot_log::force_uart_line(line.as_str());
                    } else if runtime_vl805_reset_requested {
                        let mut line = heapless::String::<224>::new();
                        let _ = core::fmt::Write::write_fmt(
                            &mut line,
                            format_args!(
                                "[local-seat] xhci handoff={} stage=runtime detail={} action=no-touch-cap-snapshot mode={}",
                                runtime_vl805_mailbox_reset_handoff_label(
                                    runtime_vl805_reset_state
                                ),
                                runtime_vl805_mailbox_reset_trusted_cold_init_detail(
                                    runtime_vl805_reset_state
                                ),
                                xhci_firmware_handoff_mode_label(handoff_mode),
                            ),
                        );
                        boot_log::force_uart_line(line.as_str());
                    } else {
                        let mut line = heapless::String::<224>::new();
                        let _ = core::fmt::Write::write_fmt(
                            &mut line,
                            format_args!(
                                "[local-seat] vl805 reset handoff=bootloader-owned stage=runtime detail=mailbox-not-required action=no-touch-cap-snapshot mode={}",
                                xhci_firmware_handoff_mode_label(handoff_mode),
                            ),
                        );
                        boot_log::force_uart_line(line.as_str());
                        line.clear();
                        let _ = core::fmt::Write::write_fmt(
                            &mut line,
                            format_args!(
                                "[local-seat] xhci handoff=bootloader-owned stage=runtime detail=trusted-post-stop-cap-snapshot action=no-touch-cap-snapshot mode={}",
                                xhci_firmware_handoff_mode_label(handoff_mode),
                            ),
                        );
                        boot_log::force_uart_line(line.as_str());
                    }
                    let mut line = heapless::String::<256>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut line,
                        format_args!(
                            "[local-seat] xhci runtime selected detail={} preferred_mode={} reset_done={} snapshot={}",
                            if runtime_vl805_reset_requested {
                                runtime_vl805_mailbox_reset_trusted_cold_init_detail(
                                    runtime_vl805_reset_state,
                                )
                            } else {
                                "bootloader-trusted-cap-snapshot"
                            },
                            xhci_firmware_handoff_mode_label(handoff_mode),
                            runtime_vl805_reset as u8,
                            trusted_handoff_probe.is_some() as u8,
                        ),
                    );
                    boot_log::force_uart_line(line.as_str());
                    handoff_mode
                } else {
                    XhciFirmwareHandoff::None
                };

                let raw_probe = if let Some(raw_probe) = trusted_handoff_probe {
                    let mut line = heapless::String::<352>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut line,
                        format_args!(
                            "[local-seat] xhci cap snapshot mmio=0x{mmio:016x} caplen=0x{caplen:02x} hciver=0x{hciver:04x} hcs1=0x{hcs1:08x} hcs2=0x{hcs2:08x} hcc1=0x{hccparams1:08x} dboff=0x{dboff:08x} rtsoff=0x{rtsoff:08x} slots={} ports={} scratch={} span=0x{span:05x}",
                            raw_probe.max_slots,
                            raw_probe.max_ports,
                            raw_probe.max_scratchpad,
                            mmio = mmio_base,
                            caplen = raw_probe.cap_length,
                            hciver = raw_probe.hci_version,
                            hcs1 = raw_probe.hcs1,
                            hcs2 = raw_probe.hcs2,
                            hccparams1 = raw_probe.hccparams1,
                            dboff = raw_probe.db_offset,
                            rtsoff = raw_probe.rts_offset,
                            span = raw_probe.mmio_size,
                        ),
                    );
                    boot_log::force_uart_line(line.as_str());
                    if let Err(reason) = validate_xhci_capability_window(&raw_probe) {
                        let mut invalid = heapless::String::<224>::new();
                        let _ = core::fmt::Write::write_fmt(
                            &mut invalid,
                            format_args!(
                                "[local-seat] xhci cap snapshot invalid mmio=0x{mmio:016x} detail={reason} action=reject-without-live-probe",
                                mmio = mmio_base,
                            ),
                        );
                        boot_log::force_uart_line(invalid.as_str());
                        continue;
                    }
                    raw_probe
                } else {
                    match probe_xhci_capability_window(hal, mmio_base) {
                        Ok(probe) => probe,
                        Err(reason) => {
                            let mut line = heapless::String::<192>::new();
                            let _ = core::fmt::Write::write_fmt(
                                &mut line,
                                format_args!(
                                    "[local-seat] xhci cap probe failed mmio=0x{mmio:016x} detail={reason}",
                                    mmio = mmio_base
                                ),
                            );
                            boot_log::force_uart_line(line.as_str());
                            continue;
                        }
                    }
                };

                let mut cap_line = heapless::String::<352>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut cap_line,
                    format_args!(
                        "[local-seat] xhci cap mmio=0x{mmio:016x} caplen=0x{caplen:02x} hciver=0x{hciver:04x} hcs1=0x{hcs1:08x} hcs2=0x{hcs2:08x} hcc1=0x{hccparams1:08x} dboff=0x{dboff:08x} rtsoff=0x{rtsoff:08x} slots={} ports={} scratch={} span=0x{span:05x}",
                        raw_probe.max_slots,
                        raw_probe.max_ports,
                        raw_probe.max_scratchpad,
                        mmio = mmio_base,
                        caplen = raw_probe.cap_length,
                        hciver = raw_probe.hci_version,
                        hcs1 = raw_probe.hcs1,
                        hcs2 = raw_probe.hcs2,
                        hccparams1 = raw_probe.hccparams1,
                        dboff = raw_probe.db_offset,
                        rtsoff = raw_probe.rts_offset,
                        span = raw_probe.mmio_size,
                    ),
                );
                boot_log::force_uart_line(cap_line.as_str());

                match validate_xhci_capability_window(&raw_probe) {
                    Ok(()) => (
                        mmio_base,
                        raw_probe,
                        preferred_firmware_handoff,
                        runtime_vl805_reset_state,
                        runtime_vl805_reset_requested,
                    ),
                    Err(reason) => {
                        if !xhci_runtime_allows_alias_scan(
                            mmio_base,
                            verified_vl805_hint,
                            legacy_mirror_allowed,
                        ) {
                            let mut line = heapless::String::<208>::new();
                            let _ = core::fmt::Write::write_fmt(
                                &mut line,
                                format_args!(
                                    "[local-seat] xhci candidate rejected mmio=0x{mmio:016x} detail={reason} mode=no-alias-scan",
                                    mmio = mmio_base
                                ),
                            );
                            boot_log::force_uart_line(line.as_str());
                            continue;
                        }
                        match probe_xhci_capability_with_alias_scan(hal, mmio_base) {
                            Ok((candidate, scanned_probe)) => {
                                let mut line = heapless::String::<224>::new();
                                let _ = core::fmt::Write::write_fmt(
                                    &mut line,
                                    format_args!(
                                        "[local-seat] xhci candidate shifted base=0x{base:016x} selected=0x{selected:016x}",
                                        base = mmio_base,
                                        selected = candidate
                                    ),
                                );
                                boot_log::force_uart_line(line.as_str());
                                (
                                    candidate,
                                    scanned_probe,
                                    XhciFirmwareHandoff::None,
                                    VL805_RUNTIME_RESET_STATE_UNATTEMPTED,
                                    runtime_vl805_reset_requested,
                                )
                            }
                            Err(_) => {
                                let mut line = heapless::String::<208>::new();
                                let _ = core::fmt::Write::write_fmt(
                                    &mut line,
                                    format_args!(
                                        "[local-seat] xhci candidate rejected mmio=0x{mmio:016x} detail={reason}",
                                        mmio = mmio_base
                                    ),
                                );
                                boot_log::force_uart_line(line.as_str());
                                continue;
                            }
                        }
                    }
                }
            };
            let (init_strategies, init_strategy_count) = xhci_runtime_init_strategies(
                firmware_handoff,
                runtime_vl805_reset_state,
                xhci_stop_state_snapshot,
            );

            // Keep probe order deterministic so field logs are directly
            // comparable across boots.
            let dma_probe_order: &[bool] = if XHCI_FORCE_LOW_DMA_PROBE {
                &[false]
            } else {
                &[false, true]
            };
            let mut pathway_idx = 0usize;
            for (strategy_idx, strategy) in init_strategies[..init_strategy_count]
                .iter()
                .copied()
                .enumerate()
            {
                if prompt_safe_probe && !xhci_runtime_init_strategy_prompt_safe(strategy) {
                    let mut line = heapless::String::<320>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut line,
                        format_args!(
                            "[local-seat] xhci prompt-safe skip attempt={}/{} policy={} origin={} handoff={} seed={} reason=requires-live-reset-path",
                            strategy_idx + 1,
                            init_strategy_count,
                            xhci_runtime_init_strategy_policy_label(strategy),
                            xhci_runtime_init_strategy_origin_label(strategy),
                            xhci_firmware_handoff_mode_label(strategy.firmware_handoff),
                            xhci_runtime_init_strategy_seed_label(strategy),
                        ),
                    );
                    boot_log::force_uart_line(line.as_str());
                    continue;
                }
                let controller_params = xhci_controller_params_from_probe_with_strategy(
                    cap_probe,
                    effective_mmio,
                    strategy,
                    xhci_stop_state_snapshot,
                );
                let seed_flags =
                    xhci_runtime_seed_snapshot_flag_bits(controller_params.runtime_seed_snapshot);
                let mut params_line = heapless::String::<384>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut params_line,
                    format_args!(
                        "[local-seat] xhci probe params attempt={}/{} policy={} origin={} mode={} halt_guard={} ctor={} pre={} legacy={} run={} publish={} post_ready={} seed_flags=0x{seed_flags:02x} snapshot={} stop_seed={} ring_seed={} axi_setup={}",
                        strategy_idx + 1,
                        init_strategy_count,
                        xhci_runtime_init_strategy_policy_label(strategy),
                        xhci_runtime_init_strategy_origin_label(strategy),
                        xhci_firmware_handoff_mode_label(strategy.firmware_handoff),
                        xhci_runtime_init_strategy_halt_guard_label(strategy),
                        xhci_runtime_init_strategy_constructor_label(strategy),
                        xhci_runtime_init_strategy_pre_reset_label(strategy),
                        xhci_runtime_init_strategy_legacy_label(strategy),
                        xhci_runtime_init_strategy_run_label(strategy),
                        xhci_runtime_init_strategy_publish_label(strategy),
                        xhci_runtime_init_strategy_post_ready_irq_label(strategy),
                        (seed_flags & 0x01) != 0,
                        (seed_flags & 0x02) != 0,
                        (seed_flags & 0x04) != 0,
                        controller_params.apply_brcm_axi_setup,
                    ),
                );
                boot_log::force_uart_line(params_line.as_str());
                if xhci_runtime_init_strategy_skips_controller_entry(strategy) {
                    pathway_idx = pathway_idx.saturating_add(1);
                    let policy_label = xhci_runtime_init_strategy_policy_label(strategy);
                    let origin_label = xhci_runtime_init_strategy_origin_label(strategy);
                    let handoff_label = xhci_firmware_handoff_mode_label(strategy.firmware_handoff);
                    let seed_label = xhci_runtime_init_strategy_seed_label(strategy);
                    let halt_guard_label = xhci_runtime_init_strategy_halt_guard_label(strategy);
                    let poll_only =
                        xhci_polling_only_runtime(effective_mmio, strategy.firmware_handoff);
                    let mut pathway_summary = UsbProbePathwaySummary::new(
                        pathway_idx,
                        strategy_idx + 1,
                        init_strategy_count,
                        policy_label,
                        origin_label,
                        handoff_label,
                        seed_label,
                        halt_guard_label,
                        false,
                        XHCI_PCIE_DMA_WINDOW_ENABLED,
                        poll_only,
                    );
                    usb_probe_pathway_record(
                        &mut pathway_summary,
                        UsbProbePathProgress::ControllerReady,
                        UsbProbePathOutcome::EnumerationDisabledBootloaderOwned,
                        None,
                        0,
                        0,
                        false,
                        XhciDiagSnapshot::empty(),
                        false,
                    );
                    let mut line = heapless::String::<320>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut line,
                        format_args!(
                            "[local-seat] xhci probe skipped mmio=0x{mmio:016x} attempt={}/{} policy={} origin={} reason=bootloader-owned-no-fresh-ownership action=return-to-shell",
                            strategy_idx + 1,
                            init_strategy_count,
                            policy_label,
                            origin_label,
                            mmio = effective_mmio,
                        ),
                    );
                    boot_log::force_uart_line(line.as_str());
                    log_usb_probe_pathway_summary(&pathway_summary);
                    usb_probe_best_pathway_update(&mut best_probe_pathway, pathway_summary);
                    saw_controller = true;
                    continue;
                }
                let requires_primary_pcie_irq =
                    xhci_runtime_init_strategy_requires_primary_pcie_irq(effective_mmio, strategy);
                let mut xhci_irq_guard = match XhciIrqGuard::install(
                    hal,
                    effective_mmio,
                    strategy.firmware_handoff,
                    XhciIrqInstallPhase::PreControllerReady,
                    requires_primary_pcie_irq,
                ) {
                    Ok(guard) => guard,
                    Err(reason) => {
                        let mut line = heapless::String::<224>::new();
                        let _ = core::fmt::Write::write_fmt(
                            &mut line,
                            format_args!(
                                "[local-seat] xhci irq sink unavailable mmio=0x{mmio:016x} stage={} detail={reason} action=defer-to-controller-gate",
                                xhci_irq_phase_label(XhciIrqInstallPhase::PreControllerReady),
                                mmio = effective_mmio
                            ),
                        );
                        boot_log::force_uart_line(line.as_str());
                        None
                    }
                };
                let irq27_bound = xhci_irq_guard
                    .as_ref()
                    .is_some_and(|guard| guard.covers_irq(PI4_GENERIC_VTIMER_IRQ));
                let bridge_irq_bound = xhci_irq_guard
                    .as_ref()
                    .is_some_and(|guard| guard.covers_irq(PI4_PCIE_BRIDGE_IRQ));
                let intx_irq_bound = xhci_irq_guard
                    .as_ref()
                    .is_some_and(|guard| guard.covers_irq(PI4_VL805_XHCI_INTX_IRQ));
                let bounded_pcie_sinks_bound = bridge_irq_bound && intx_irq_bound;
                let controller_gate = if requires_primary_pcie_irq && !bounded_pcie_sinks_bound {
                    "pcie-sinks-unbound"
                } else {
                    "none"
                };
                if requires_primary_pcie_irq && !bounded_pcie_sinks_bound {
                    let next_origin = if strategy_idx + 1 < init_strategy_count {
                        xhci_runtime_init_strategy_origin_label(init_strategies[strategy_idx + 1])
                    } else {
                        "none"
                    };
                    let mut line = heapless::String::<320>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut line,
                        format_args!(
                            "[local-seat] xhci trusted fresh-init skip mmio=0x{mmio:016x} attempt={}/{} origin={} reason=pcie-sinks-unbound fallback={next_origin}",
                            strategy_idx + 1,
                            init_strategy_count,
                            xhci_runtime_init_strategy_origin_label(strategy),
                            mmio = effective_mmio,
                        ),
                    );
                    boot_log::force_uart_line(line.as_str());
                    continue;
                }
                for &prefer_high in dma_probe_order {
                    let dma_bus_modes: &[bool] =
                        if XHCI_PCIE_DMA_WINDOW_ENABLED && XHCI_TRY_RAW_PHYS_DMA_FALLBACK {
                            &[true, false]
                        } else if XHCI_PCIE_DMA_WINDOW_ENABLED {
                            &[true]
                        } else {
                            &[false]
                        };
                    for (bus_mode_idx, &pcie_dma_window) in dma_bus_modes.iter().enumerate() {
                        pathway_idx = pathway_idx.saturating_add(1);
                        let policy_label = xhci_runtime_init_strategy_policy_label(strategy);
                        let origin_label = xhci_runtime_init_strategy_origin_label(strategy);
                        let handoff_label =
                            xhci_firmware_handoff_mode_label(strategy.firmware_handoff);
                        let seed_label = xhci_runtime_init_strategy_seed_label(strategy);
                        let halt_guard_label =
                            xhci_runtime_init_strategy_halt_guard_label(strategy);
                        let poll_only =
                            xhci_polling_only_runtime(effective_mmio, strategy.firmware_handoff);
                        reset_latest_xhci_diag_snapshot();
                        let diag_before = read_latest_xhci_diag_snapshot();
                        let mut pathway_summary = UsbProbePathwaySummary::new(
                            pathway_idx,
                            strategy_idx + 1,
                            init_strategy_count,
                            policy_label,
                            origin_label,
                            handoff_label,
                            seed_label,
                            halt_guard_label,
                            prefer_high,
                            pcie_dma_window,
                            poll_only,
                        );
                        pathway_summary.irq27_bound = irq27_bound;
                        pathway_summary.bridge_irq_bound = bridge_irq_bound;
                        pathway_summary.intx_irq_bound = intx_irq_bound;
                        pathway_summary.controller_gate = controller_gate;
                        let mut probe_line = heapless::String::<352>::new();
                        let _ = core::fmt::Write::write_fmt(
                            &mut probe_line,
                            format_args!(
                                "[local-seat] xhci probe begin mmio=0x{mmio:016x} attempt={}/{} policy={} origin={} dma={} bus={} handoff={} seed={} poll_only={} axi_setup={}",
                                strategy_idx + 1,
                                init_strategy_count,
                                policy_label,
                                origin_label,
                                if prefer_high { "high" } else { "low" },
                                if pcie_dma_window {
                                    "pcie-window"
                                } else {
                                    "phys"
                                },
                                xhci_irq_policy_reason(strategy.firmware_handoff),
                                seed_label,
                                if poll_only { "yes" } else { "no" },
                                controller_params.apply_brcm_axi_setup,
                                mmio = effective_mmio
                            ),
                        );
                        boot_log::force_uart_line(probe_line.as_str());

                        let dma = SeatDma::new(hal, prefer_high, pcie_dma_window);
                        let ctrl = match XhciCtrl::new_with_params(
                            effective_mmio,
                            dma,
                            controller_params,
                        ) {
                            Ok(ctrl) => {
                                saw_controller = true;
                                Arc::new(ctrl)
                            }
                            Err(err) => {
                                let diag_after = read_latest_xhci_diag_snapshot();
                                usb_probe_pathway_record(
                                    &mut pathway_summary,
                                    UsbProbePathProgress::NoController,
                                    UsbProbePathOutcome::ControllerInitFailed,
                                    None,
                                    0,
                                    0,
                                    false,
                                    diag_after,
                                    xhci_diag_snapshot_changed(diag_before, diag_after),
                                );
                                let mut line = heapless::String::<320>::new();
                                let _ = core::fmt::Write::write_fmt(
                                    &mut line,
                                    format_args!(
                                        "[local-seat] xhci probe failed mmio=0x{mmio:016x} attempt={}/{} origin={} dma={} bus={} handoff={} seed={} detail={err:?}",
                                        strategy_idx + 1,
                                        init_strategy_count,
                                        xhci_runtime_init_strategy_origin_label(strategy),
                                        if prefer_high { "high" } else { "low" },
                                        if pcie_dma_window {
                                            "pcie-window"
                                        } else {
                                            "phys"
                                        },
                                        xhci_irq_policy_reason(strategy.firmware_handoff),
                                        xhci_runtime_init_strategy_seed_label(strategy),
                                        mmio = effective_mmio
                                    ),
                                );
                                boot_log::force_uart_line(line.as_str());
                                log_xhci_probe_failure_edge(
                                    runtime_vl805_reset_requested,
                                    strategy,
                                    diag_before,
                                    diag_after,
                                );
                                log_latest_xhci_diag_summary("probe-new");
                                log_usb_probe_pathway_summary(&pathway_summary);
                                usb_probe_best_pathway_update(
                                    &mut best_probe_pathway,
                                    pathway_summary,
                                );
                                if bus_mode_idx + 1 < dma_bus_modes.len() {
                                    let next_bus = if dma_bus_modes[bus_mode_idx + 1] {
                                        "pcie-window"
                                    } else {
                                        "phys"
                                    };
                                    let mut fallback_line = heapless::String::<320>::new();
                                    let _ = core::fmt::Write::write_fmt(
                                        &mut fallback_line,
                                        format_args!(
                                            "[local-seat] xhci probe fallback mmio=0x{mmio:016x} attempt={}/{} origin={} dma={} from_bus={} to_bus={} reason={err:?}",
                                            strategy_idx + 1,
                                            init_strategy_count,
                                            xhci_runtime_init_strategy_origin_label(strategy),
                                            if prefer_high { "high" } else { "low" },
                                            if pcie_dma_window {
                                                "pcie-window"
                                            } else {
                                                "phys"
                                            },
                                            next_bus,
                                            mmio = effective_mmio
                                        ),
                                    );
                                    boot_log::force_uart_line(fallback_line.as_str());
                                } else if strategy_idx + 1 < init_strategy_count {
                                    let next_strategy = init_strategies[strategy_idx + 1];
                                    let mut fallback_line = heapless::String::<352>::new();
                                    let _ = core::fmt::Write::write_fmt(
                                        &mut fallback_line,
                                        format_args!(
                                            "[local-seat] xhci probe fallback mmio=0x{mmio:016x} from_attempt={}/{} to_origin={} next_handoff={} next_seed={} reason={err:?}",
                                            strategy_idx + 1,
                                            init_strategy_count,
                                            xhci_runtime_init_strategy_origin_label(next_strategy),
                                            xhci_irq_policy_reason(next_strategy.firmware_handoff),
                                            xhci_runtime_init_strategy_seed_label(next_strategy),
                                            mmio = effective_mmio
                                        ),
                                    );
                                    boot_log::force_uart_line(fallback_line.as_str());
                                }
                                continue;
                            }
                        };
                        if xhci_irq_sink_needed(
                            effective_mmio,
                            strategy.firmware_handoff,
                            XhciIrqInstallPhase::ControllerReady,
                            requires_primary_pcie_irq,
                        ) && xhci_irq_guard.as_ref().map_or(true, |guard| {
                            !xhci_irq_guard_satisfies_phase(
                                guard,
                                effective_mmio,
                                strategy.firmware_handoff,
                                XhciIrqInstallPhase::ControllerReady,
                                requires_primary_pcie_irq,
                            )
                        }) {
                            if let Some(existing_guard) = xhci_irq_guard.as_ref() {
                                let mut line = heapless::String::<288>::new();
                                let _ = core::fmt::Write::write_fmt(
                                    &mut line,
                                    format_args!(
                                        "[local-seat] xhci irq sink retry mmio=0x{mmio:016x} stage={} action=reinstall-bounded-sinks has_irq27={} has_bridge={} has_intx={}",
                                        xhci_irq_phase_label(XhciIrqInstallPhase::ControllerReady),
                                        existing_guard.covers_irq(PI4_GENERIC_VTIMER_IRQ) as u8,
                                        existing_guard.covers_irq(PI4_PCIE_BRIDGE_IRQ) as u8,
                                        existing_guard.covers_irq(PI4_VL805_XHCI_INTX_IRQ) as u8,
                                        mmio = effective_mmio,
                                    ),
                                );
                                boot_log::force_uart_line(line.as_str());
                            }
                            match XhciIrqGuard::install(
                                hal,
                                effective_mmio,
                                strategy.firmware_handoff,
                                XhciIrqInstallPhase::ControllerReady,
                                requires_primary_pcie_irq,
                            ) {
                                Ok(Some(guard)) => xhci_irq_guard = Some(guard),
                                Ok(None) => {}
                                Err(reason) => {
                                    let mut line = heapless::String::<224>::new();
                                    let _ = core::fmt::Write::write_fmt(
                                        &mut line,
                                        format_args!(
                                            "[local-seat] xhci irq sink unavailable mmio=0x{mmio:016x} stage={} detail={reason} action=fallback-poll-only",
                                            xhci_irq_phase_label(
                                                XhciIrqInstallPhase::ControllerReady
                                            ),
                                            mmio = effective_mmio
                                        ),
                                    );
                                    boot_log::force_uart_line(line.as_str());
                                }
                            }
                        }

                        let max_ports = cmp::min(ctrl.max_ports() as usize, XHCI_MAX_PROBE_PORTS);
                        let mut connected_mask = 0u32;
                        let mut port_statuses = [0u32; XHCI_MAX_PROBE_PORTS];
                        let mut detect_passes_used = 1usize;
                        let bootloader_port_reads_toxic =
                            xhci_runtime_init_strategy_skips_root_port_reads(strategy);
                        if bootloader_port_reads_toxic {
                            boot_log::force_uart_line(
                                "[local-seat] xhci root-port sample skipped reason=bootloader-owned-portsc-toxic",
                            );
                        } else {
                            {
                                let mut line = heapless::String::<160>::new();
                                let _ = core::fmt::Write::write_fmt(
                                    &mut line,
                                    format_args!(
                                        "[local-seat] xhci root-port sample begin ports={} passes={}",
                                        max_ports, XHCI_PORT_DETECT_PASSES,
                                    ),
                                );
                                boot_log::force_uart_line(line.as_str());
                            }
                            for pass in 0..XHCI_PORT_DETECT_PASSES {
                                detect_passes_used = pass.saturating_add(1);
                                connected_mask = xhci_sample_root_ports(
                                    ctrl.as_ref(),
                                    max_ports,
                                    &mut port_statuses,
                                );
                                if connected_mask != 0 || pass + 1 >= XHCI_PORT_DETECT_PASSES {
                                    break;
                                }
                                for _ in 0..XHCI_PORT_DETECT_SETTLE_SPINS {
                                    spin_loop();
                                }
                            }
                        }
                        let mut slow_recheck_used = false;
                        if connected_mask == 0 && max_ports != 0 && !bootloader_port_reads_toxic {
                            log_xhci_root_port_statuses(&port_statuses[..max_ports], "detect-zero");
                            wait_ms(XHCI_PORT_DETECT_FINAL_WAIT_MS);
                            slow_recheck_used = true;
                            detect_passes_used = detect_passes_used.saturating_add(1);
                            connected_mask = xhci_sample_root_ports(
                                ctrl.as_ref(),
                                max_ports,
                                &mut port_statuses,
                            );
                            log_xhci_root_port_statuses(
                                &port_statuses[..max_ports],
                                if connected_mask == 0 {
                                    "detect-slow-zero"
                                } else {
                                    "detect-slow-hit"
                                },
                            );
                        }

                        let mut line = heapless::String::<224>::new();
                        let _ = core::fmt::Write::write_fmt(
                            &mut line,
                            format_args!(
                                "[local-seat] xhci online mmio=0x{mmio:016x} dma={} bus={} ports={} ctx={} connected_mask=0x{mask:04x} detect_passes={} slow_recheck={}",
                                if prefer_high { "high" } else { "low" },
                                if pcie_dma_window {
                                    "pcie-window"
                                } else {
                                    "phys"
                                },
                                max_ports,
                                ctrl.context_size_bytes(),
                                detect_passes_used,
                                slow_recheck_used as u8,
                                mmio = effective_mmio,
                                mask = connected_mask,
                            ),
                        );
                        boot_log::force_uart_line(line.as_str());

                        let mut attempt_recorded = false;
                        for port in 0..max_ports {
                            if (connected_mask & (1u32 << port)) == 0 {
                                continue;
                            }

                            let mut device = match UsbDevice::new(ctrl.clone(), port as u8) {
                                Ok(device) => device,
                                Err(err) => {
                                    let diag_after = read_latest_xhci_diag_snapshot();
                                    usb_probe_pathway_record(
                                        &mut pathway_summary,
                                        UsbProbePathProgress::RootPortConnected,
                                        UsbProbePathOutcome::AddressFailed,
                                        Some((port + 1) as u8),
                                        connected_mask,
                                        detect_passes_used,
                                        slow_recheck_used,
                                        diag_after,
                                        xhci_diag_snapshot_changed(diag_before, diag_after),
                                    );
                                    attempt_recorded = true;
                                    let mut kind_line = heapless::String::<192>::new();
                                    let _ = core::fmt::Write::write_fmt(
                                        &mut kind_line,
                                        format_args!(
                                            "[local-seat] usb root-enum classify port={} stage=address kind={} dma={} bus={}",
                                            port + 1,
                                            usb_address_error_kind(err),
                                            if prefer_high { "high" } else { "low" },
                                            if pcie_dma_window {
                                                "pcie-window"
                                            } else {
                                                "phys"
                                            },
                                        ),
                                    );
                                    boot_log::force_uart_line(kind_line.as_str());

                                    let mut line = heapless::String::<192>::new();
                                    let _ = core::fmt::Write::write_fmt(
                                        &mut line,
                                        format_args!(
                                            "[local-seat] usb root-enum failed port={} stage=address dma={} bus={} detail={err:?}",
                                            port + 1,
                                            if prefer_high { "high" } else { "low" },
                                            if pcie_dma_window {
                                                "pcie-window"
                                            } else {
                                                "phys"
                                            },
                                        ),
                                    );
                                    boot_log::force_uart_line(line.as_str());
                                    if matches!(
                                        err,
                                        UsbError::EnableSlotTimeout
                                            | UsbError::AddressDeviceTimeout
                                            | UsbError::CmdFail(_)
                                    ) {
                                        if let UsbError::CmdFail(code) = err {
                                            let mut cmd_line = heapless::String::<224>::new();
                                            let _ = core::fmt::Write::write_fmt(
                                                &mut cmd_line,
                                                format_args!(
                                                    "[local-seat] usb cmd completion detail port={} code={} (0x{code:02x}) name={}",
                                                    port + 1,
                                                    code,
                                                    completion::name(code),
                                                ),
                                            );
                                            boot_log::force_uart_line(cmd_line.as_str());
                                        }
                                        let diag = ctrl.command_diag_for_port(port as u8);
                                        let mut summary = heapless::String::<224>::new();
                                        let _ = core::fmt::Write::write_fmt(
                                            &mut summary,
                                            format_args!(
                                                "[local-seat] xhci timeout diag port={} usbcmd=0x{usbcmd:08x} usbsts=0x{usbsts:08x} portsc=0x{portsc:08x}",
                                                port + 1,
                                                usbcmd = diag.usbcmd,
                                                usbsts = diag.usbsts,
                                                portsc = diag.portsc,
                                            ),
                                        );
                                        boot_log::force_uart_line(summary.as_str());

                                        let mut regs0 = heapless::String::<192>::new();
                                        let _ = core::fmt::Write::write_fmt(
                                            &mut regs0,
                                            format_args!(
                                                "[local-seat] xhci timeout regs crcr=0x{crcr:016x} dcbaap=0x{dcbaap:016x} iman=0x{iman:08x}",
                                                crcr = diag.crcr,
                                                dcbaap = diag.dcbaap,
                                                iman = diag.iman,
                                            ),
                                        );
                                        boot_log::force_uart_line(regs0.as_str());

                                        let mut regs1 = heapless::String::<192>::new();
                                        let _ = core::fmt::Write::write_fmt(
                                            &mut regs1,
                                            format_args!(
                                                "[local-seat] xhci timeout regs erdp=0x{erdp:016x} erstba=0x{erstba:016x}",
                                                erdp = diag.erdp,
                                                erstba = diag.erstba,
                                            ),
                                        );
                                        boot_log::force_uart_line(regs1.as_str());
                                    }
                                    continue;
                                }
                            };

                            let device_desc = match device.get_device_descriptor() {
                                Ok(desc) => desc,
                                Err(err) => {
                                    let diag_after = read_latest_xhci_diag_snapshot();
                                    usb_probe_pathway_record(
                                        &mut pathway_summary,
                                        UsbProbePathProgress::DeviceAddressed,
                                        UsbProbePathOutcome::DeviceDescFailed,
                                        Some((port + 1) as u8),
                                        connected_mask,
                                        detect_passes_used,
                                        slow_recheck_used,
                                        diag_after,
                                        xhci_diag_snapshot_changed(diag_before, diag_after),
                                    );
                                    attempt_recorded = true;
                                    let mut line = heapless::String::<192>::new();
                                    let _ = core::fmt::Write::write_fmt(
                                        &mut line,
                                        format_args!(
                                            "[local-seat] usb root-enum failed port={} stage=device-desc detail={err:?}",
                                            port + 1
                                        ),
                                    );
                                    boot_log::force_uart_line(line.as_str());
                                    continue;
                                }
                            };

                            let config_blob = match device.get_config_descriptor(0) {
                                Ok(config_blob) => config_blob,
                                Err(err) => {
                                    let diag_after = read_latest_xhci_diag_snapshot();
                                    usb_probe_pathway_record(
                                        &mut pathway_summary,
                                        UsbProbePathProgress::DeviceDescriptor,
                                        UsbProbePathOutcome::ConfigDescFailed,
                                        Some((port + 1) as u8),
                                        connected_mask,
                                        detect_passes_used,
                                        slow_recheck_used,
                                        diag_after,
                                        xhci_diag_snapshot_changed(diag_before, diag_after),
                                    );
                                    attempt_recorded = true;
                                    let mut line = heapless::String::<192>::new();
                                    let _ = core::fmt::Write::write_fmt(
                                        &mut line,
                                        format_args!(
                                            "[local-seat] usb root-enum failed port={} stage=config-desc detail={err:?}",
                                            port + 1
                                        ),
                                    );
                                    boot_log::force_uart_line(line.as_str());
                                    continue;
                                }
                            };
                            let Some(config) = read_config_desc(&config_blob) else {
                                let diag_after = read_latest_xhci_diag_snapshot();
                                usb_probe_pathway_record(
                                    &mut pathway_summary,
                                    UsbProbePathProgress::ConfigDescriptor,
                                    UsbProbePathOutcome::ConfigParseFailed,
                                    Some((port + 1) as u8),
                                    connected_mask,
                                    detect_passes_used,
                                    slow_recheck_used,
                                    diag_after,
                                    xhci_diag_snapshot_changed(diag_before, diag_after),
                                );
                                attempt_recorded = true;
                                let total_len = if config_blob.len() >= 4 {
                                    u16::from_le_bytes([config_blob[2], config_blob[3]])
                                } else {
                                    0
                                };
                                let mut detail = heapless::String::<224>::new();
                                let _ = core::fmt::Write::write_fmt(
                                    &mut detail,
                                    format_args!(
                                        "[local-seat] usb cfg parse detail port={} len={} b0=0x{:02x} b1=0x{:02x} total=0x{:04x}",
                                        port + 1,
                                        config_blob.len(),
                                        config_blob.get(0).copied().unwrap_or(0),
                                        config_blob.get(1).copied().unwrap_or(0),
                                        total_len
                                    ),
                                );
                                boot_log::force_uart_line(detail.as_str());

                                let mut line = heapless::String::<160>::new();
                                let _ = core::fmt::Write::write_fmt(
                                    &mut line,
                                    format_args!(
                                        "[local-seat] usb root-enum failed port={} stage=config-parse",
                                        port + 1
                                    ),
                                );
                                boot_log::force_uart_line(line.as_str());
                                continue;
                            };
                            let Some(config_value) = config_value_for_set(config) else {
                                let diag_after = read_latest_xhci_diag_snapshot();
                                usb_probe_pathway_record(
                                    &mut pathway_summary,
                                    UsbProbePathProgress::ConfigParsed,
                                    UsbProbePathOutcome::InvalidConfigValue,
                                    Some((port + 1) as u8),
                                    connected_mask,
                                    detect_passes_used,
                                    slow_recheck_used,
                                    diag_after,
                                    xhci_diag_snapshot_changed(diag_before, diag_after),
                                );
                                attempt_recorded = true;
                                let mut line = heapless::String::<224>::new();
                                let _ = core::fmt::Write::write_fmt(
                                    &mut line,
                                    format_args!(
                                        "[local-seat] usb root-enum failed port={} stage=set-config-value detail=invalid bConfigurationValue=0x{:02x} iConfiguration=0x{:02x}",
                                        port + 1,
                                        config.configuration_value(),
                                        config.configuration_string_index()
                                    ),
                                );
                                boot_log::force_uart_line(line.as_str());
                                continue;
                            };
                            if let Err(err) = device.set_configuration(config_value) {
                                let diag_after = read_latest_xhci_diag_snapshot();
                                usb_probe_pathway_record(
                                    &mut pathway_summary,
                                    UsbProbePathProgress::ConfigParsed,
                                    UsbProbePathOutcome::SetConfigFailed,
                                    Some((port + 1) as u8),
                                    connected_mask,
                                    detect_passes_used,
                                    slow_recheck_used,
                                    diag_after,
                                    xhci_diag_snapshot_changed(diag_before, diag_after),
                                );
                                attempt_recorded = true;
                                let mut line = heapless::String::<192>::new();
                                let _ = core::fmt::Write::write_fmt(
                                    &mut line,
                                    format_args!(
                                        "[local-seat] usb root-enum failed port={} stage=set-config({}) detail={err:?}",
                                        port + 1,
                                        config_value
                                    ),
                                );
                                boot_log::force_uart_line(line.as_str());
                                continue;
                            }

                            let keyboard_init_error_before = saw_keyboard_init_error;
                            let device = Arc::new(device);
                            if let Some(hid) = Self::probe_device_for_keyboard(
                                device,
                                device_desc,
                                &config_blob,
                                HUB_ENUM_MAX_DEPTH,
                                &mut saw_keyboard_init_error,
                            ) {
                                let diag_after = read_latest_xhci_diag_snapshot();
                                usb_probe_pathway_record(
                                    &mut pathway_summary,
                                    UsbProbePathProgress::KeyboardReady,
                                    UsbProbePathOutcome::KeyboardReady,
                                    Some((port + 1) as u8),
                                    connected_mask,
                                    detect_passes_used,
                                    slow_recheck_used,
                                    diag_after,
                                    xhci_diag_snapshot_changed(diag_before, diag_after),
                                );
                                log_usb_probe_pathway_summary(&pathway_summary);
                                log_usb_probe_best_pathway("keyboard-ready", &pathway_summary);
                                hid.device().ctrl().host().seal_runtime();
                                return Ok(Self {
                                    hid,
                                    _xhci_irq_guard: xhci_irq_guard,
                                    last_keys: [0; 6],
                                    caps_lock_on: false,
                                    poll_error_logged: false,
                                    led_error_logged: false,
                                    first_report_logged: false,
                                    pending_display_scroll_rows: 0,
                                });
                            }
                            let diag_after = read_latest_xhci_diag_snapshot();
                            usb_probe_pathway_record(
                                &mut pathway_summary,
                                UsbProbePathProgress::DeviceConfigured,
                                if saw_keyboard_init_error != keyboard_init_error_before {
                                    UsbProbePathOutcome::HidInitFailed
                                } else {
                                    UsbProbePathOutcome::NoKeyboardFound
                                },
                                Some((port + 1) as u8),
                                connected_mask,
                                detect_passes_used,
                                slow_recheck_used,
                                diag_after,
                                xhci_diag_snapshot_changed(diag_before, diag_after),
                            );
                            attempt_recorded = true;
                        }
                        if !attempt_recorded {
                            let diag_after = read_latest_xhci_diag_snapshot();
                            let outcome = if bootloader_port_reads_toxic {
                                UsbProbePathOutcome::EnumerationDisabledBootloaderOwned
                            } else {
                                UsbProbePathOutcome::NoConnectedPorts
                            };
                            usb_probe_pathway_record(
                                &mut pathway_summary,
                                UsbProbePathProgress::ControllerReady,
                                outcome,
                                None,
                                connected_mask,
                                detect_passes_used,
                                slow_recheck_used,
                                diag_after,
                                xhci_diag_snapshot_changed(diag_before, diag_after),
                            );
                        }
                        log_usb_probe_pathway_summary(&pathway_summary);
                        usb_probe_best_pathway_update(&mut best_probe_pathway, pathway_summary);
                    }
                }
            }
        }
        let final_error = if saw_keyboard_init_error {
            Pi4SeatError::UsbKeyboardInit
        } else if saw_controller {
            Pi4SeatError::UsbKeyboardMissing
        } else {
            Pi4SeatError::XhciInit
        };
        if let Some(best_probe_pathway) = best_probe_pathway {
            log_usb_probe_best_pathway(final_error.as_str(), &best_probe_pathway);
        }
        return Err(final_error);
    }

    fn probe_device_for_keyboard(
        device: Arc<UsbDevice<SeatDma>>,
        device_desc: DeviceDesc,
        config_blob: &[u8],
        depth_remaining: usize,
        saw_keyboard_init_error: &mut bool,
    ) -> Option<HidDevice<SeatDma>> {
        if let Some(hid) =
            Self::attach_hid_keyboard(device.clone(), config_blob, saw_keyboard_init_error)
        {
            return Some(hid);
        }
        if depth_remaining == 0 {
            return None;
        }

        let Some(hub_info) = Self::hub_interface_info(device_desc, config_blob) else {
            let mut line = heapless::String::<192>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] usb non-hid/non-hub slot={} class=0x{:02x} subclass=0x{:02x} proto=0x{:02x}",
                    device.slot_id(),
                    device_desc.device_class,
                    device_desc.device_subclass,
                    device_desc.device_protocol
                ),
            );
            boot_log::force_uart_line(line.as_str());
            return None;
        };

        let mut line = heapless::String::<192>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] usb hub detected slot={} route=0x{route:05x} root_port={} iface={} protocol={}",
                device.slot_id(),
                device.root_hub_port(),
                hub_info.interface_number,
                hub_info.protocol,
                route = device.route(),
            ),
        );
        boot_log::force_uart_line(line.as_str());

        Self::scan_hub_children(
            device,
            hub_info.protocol,
            hub_info.multi_tt,
            hub_info.interface_number,
            depth_remaining.saturating_sub(1),
            saw_keyboard_init_error,
        )
    }

    fn attach_hid_keyboard(
        device: Arc<UsbDevice<SeatDma>>,
        config_blob: &[u8],
        saw_keyboard_init_error: &mut bool,
    ) -> Option<HidDevice<SeatDma>> {
        let interfaces = find_hid_interfaces(config_blob);
        let mut protocol_none_candidates = Vec::<(
            usb_oxide::InterfaceDesc,
            usb_oxide::EndpointDesc,
            Option<bool>,
        )>::new();
        let mut strict_keyboard_candidates = 0usize;

        for attach_rank in 0..=1 {
            for (iface, ep_in) in &interfaces {
                let Some(candidate_rank) =
                    hid_keyboard_attach_rank(iface.interface_subclass, iface.interface_protocol)
                else {
                    continue;
                };
                if candidate_rank == 2 {
                    if attach_rank == 0 {
                        let hint = Self::hid_report_descriptor_keyboard_hint(
                            device.as_ref(),
                            config_blob,
                            iface.interface_number,
                        );
                        if hint == Some(false) {
                            let mut line = heapless::String::<256>::new();
                            let _ = core::fmt::Write::write_fmt(
                                &mut line,
                                format_args!(
                                    "[local-seat] usb hid candidate skip slot={} iface={} ep=0x{:02x} source=protocol-none-fallback reason=report-desc-not-keyboard",
                                    device.slot_id(),
                                    iface.interface_number,
                                    ep_in.endpoint_address,
                                ),
                            );
                            boot_log::force_uart_line(line.as_str());
                        }
                        protocol_none_candidates.push((*iface, *ep_in, hint));
                    }
                    continue;
                }
                strict_keyboard_candidates = strict_keyboard_candidates.saturating_add(1);
                if candidate_rank != attach_rank {
                    continue;
                }
                let source = hid_keyboard_attach_source(candidate_rank);
                let require_boot_switch = candidate_rank != 0;
                // Boot keyboards are already configured by `HidDevice::from_interface`;
                // only relaxed candidates need the compatibility coercion path.
                let force_keyboard_mode =
                    hid_keyboard_candidate_requires_force_mode(candidate_rank);
                let track_failures = candidate_rank != 2;
                if let Some(hid) = Self::try_attach_hid_keyboard_candidate(
                    device.clone(),
                    *iface,
                    *ep_in,
                    source,
                    require_boot_switch,
                    force_keyboard_mode,
                    track_failures,
                    saw_keyboard_init_error,
                ) {
                    return Some(hid);
                }
            }
        }

        for prefer_hint in [true, false] {
            for (iface, ep_in, hint) in &protocol_none_candidates {
                if matches!(hint, Some(false)) {
                    continue;
                }
                if prefer_hint {
                    if !matches!(hint, Some(true)) {
                        continue;
                    }
                } else if hint.is_some() {
                    continue;
                }
                if let Some(hid) = Self::try_attach_hid_keyboard_candidate(
                    device.clone(),
                    *iface,
                    *ep_in,
                    hid_keyboard_attach_source(2),
                    true,
                    true,
                    false,
                    saw_keyboard_init_error,
                ) {
                    return Some(hid);
                }
            }
        }
        // Last-resort: report-descriptor hints can be wrong on some vendor
        // keyboards. If no strict keyboard protocol candidate exists, still
        // attempt protocol-none interfaces that were hinted as non-keyboard.
        if strict_keyboard_candidates == 0 {
            for (iface, ep_in, hint) in &protocol_none_candidates {
                if !matches!(hint, Some(false)) {
                    continue;
                }
                if let Some(hid) = Self::try_attach_hid_keyboard_candidate(
                    device.clone(),
                    *iface,
                    *ep_in,
                    "protocol-none-last-resort",
                    true,
                    true,
                    false,
                    saw_keyboard_init_error,
                ) {
                    return Some(hid);
                }
            }
        }
        None
    }

    fn hid_report_desc_length_for_interface(
        config_blob: &[u8],
        interface_number: u8,
    ) -> Option<u16> {
        let mut offset = 0usize;
        let mut in_target_interface = false;
        while offset + 2 <= config_blob.len() {
            let len = config_blob[offset] as usize;
            let dtype = config_blob[offset + 1];
            if len == 0 || offset + len > config_blob.len() {
                break;
            }

            if dtype == desc_type::INTERFACE && len >= mem::size_of::<usb_oxide::InterfaceDesc>() {
                // SAFETY: Interface descriptor bytes may be unaligned in the
                // configuration blob.
                let iface = unsafe {
                    ptr::read_unaligned(
                        config_blob
                            .as_ptr()
                            .add(offset)
                            .cast::<usb_oxide::InterfaceDesc>(),
                    )
                };
                in_target_interface = iface.interface_class == class::HID
                    && iface.interface_number == interface_number;
            } else if in_target_interface && dtype == desc_type::HID && len >= 9 {
                // SAFETY: HID descriptor bytes may be unaligned in the
                // configuration blob.
                let hid_desc = unsafe {
                    ptr::read_unaligned(config_blob.as_ptr().add(offset).cast::<HidDesc>())
                };
                if hid_desc.report_desc_type == desc_type::HID_REPORT
                    && hid_desc.report_desc_length > 0
                {
                    return Some(hid_desc.report_desc_length);
                }
            }

            offset += len;
        }
        None
    }

    fn hid_usage_indicates_keyboard(usage_page: u32, usage: u32) -> bool {
        (usage_page == 0x01 && usage == 0x06)
            || (usage_page == 0x07 && (0x04..=0xE7).contains(&usage))
    }

    fn hid_usage_range_indicates_keyboard(usage_page: u32, min_usage: u32, max_usage: u32) -> bool {
        if max_usage < min_usage {
            return false;
        }
        if usage_page == 0x01 {
            return min_usage <= 0x06 && max_usage >= 0x06;
        }
        if usage_page == 0x07 {
            return min_usage <= 0xE7 && max_usage >= 0x04;
        }
        false
    }

    fn hid_report_descriptor_is_keyboard(report_desc: &[u8]) -> bool {
        let mut offset = 0usize;
        let mut usage_page = 0u32;
        let mut local_keyboard_usage = false;
        let mut local_usage_min: Option<(u32, u32)> = None;
        while offset < report_desc.len() {
            let prefix = report_desc[offset];
            offset = offset.saturating_add(1);
            if prefix == 0xFE {
                if offset + 2 > report_desc.len() {
                    break;
                }
                let long_size = report_desc[offset] as usize;
                offset = offset.saturating_add(2);
                if offset + long_size > report_desc.len() {
                    break;
                }
                offset = offset.saturating_add(long_size);
                continue;
            }

            let data_len = match prefix & 0x03 {
                0 => 0usize,
                1 => 1usize,
                2 => 2usize,
                _ => 4usize,
            };
            if offset + data_len > report_desc.len() {
                break;
            }

            let data = match data_len {
                0 => 0u32,
                1 => report_desc[offset] as u32,
                2 => u16::from_le_bytes([report_desc[offset], report_desc[offset + 1]]) as u32,
                _ => u32::from_le_bytes([
                    report_desc[offset],
                    report_desc[offset + 1],
                    report_desc[offset + 2],
                    report_desc[offset + 3],
                ]),
            };
            offset = offset.saturating_add(data_len);

            let item_type = (prefix >> 2) & 0x03;
            let tag = (prefix >> 4) & 0x0f;
            match item_type {
                0x01 => {
                    if tag == 0x00 {
                        usage_page = data;
                    }
                }
                0x02 => match tag {
                    0x00 => {
                        if Self::hid_usage_indicates_keyboard(usage_page, data) {
                            local_keyboard_usage = true;
                        }
                    }
                    0x01 => {
                        local_usage_min = Some((usage_page, data));
                    }
                    0x02 => {
                        if let Some((range_page, min_usage)) = local_usage_min.take() {
                            if Self::hid_usage_range_indicates_keyboard(range_page, min_usage, data)
                            {
                                local_keyboard_usage = true;
                            }
                        }
                    }
                    _ => {}
                },
                0x00 => {
                    if tag == 0x0A {
                        // Application Collection
                        if (data & 0xff) == 0x01 && local_keyboard_usage {
                            return true;
                        }
                    }
                    // Local items apply only to the next main item.
                    local_keyboard_usage = false;
                    local_usage_min = None;
                }
                _ => {}
            }
        }
        false
    }

    fn hid_report_descriptor_keyboard_hint(
        device: &UsbDevice<SeatDma>,
        config_blob: &[u8],
        interface_number: u8,
    ) -> Option<bool> {
        let report_len = Self::hid_report_desc_length_for_interface(config_blob, interface_number)
            .map(|len| len as usize)
            .unwrap_or(HID_REPORT_DESC_MAX_BYTES);
        let request_len = cmp::min(report_len.max(64), HID_REPORT_DESC_MAX_BYTES);
        if request_len == 0 {
            return None;
        }

        let mut report_desc = alloc::vec![0u8; request_len];
        let setup = SetupPacket::new(
            0x81,
            request::GET_DESCRIPTOR,
            (desc_type::HID_REPORT as u16) << 8,
            interface_number as u16,
            request_len as u16,
        );
        let transferred = match device.control_transfer_with_wait_spins(
            &setup,
            Some(report_desc.as_mut_slice()),
            HID_REPORT_DESC_WAIT_SPINS,
        ) {
            Ok(transferred) => transferred,
            Err(err) => {
                let mut line = heapless::String::<256>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] usb hid report-desc read failed slot={} iface={} detail={err:?}",
                        device.slot_id(),
                        interface_number
                    ),
                );
                boot_log::force_uart_line(line.as_str());
                return None;
            }
        };
        let used = cmp::min(transferred, report_desc.len());
        if used == 0 {
            return None;
        }
        let is_keyboard = Self::hid_report_descriptor_is_keyboard(&report_desc[..used]);
        let mut line = heapless::String::<256>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] usb hid report-desc slot={} iface={} bytes={} keyboard_hint={}",
                device.slot_id(),
                interface_number,
                used,
                is_keyboard as u8,
            ),
        );
        boot_log::force_uart_line(line.as_str());
        Some(is_keyboard)
    }

    fn try_attach_hid_keyboard_candidate(
        device: Arc<UsbDevice<SeatDma>>,
        iface: usb_oxide::InterfaceDesc,
        ep_in: usb_oxide::EndpointDesc,
        source: &str,
        require_boot_switch: bool,
        force_keyboard_mode: bool,
        track_failures: bool,
        saw_keyboard_init_error: &mut bool,
    ) -> Option<HidDevice<SeatDma>> {
        let mut bypassed_boot_subclass = false;
        let mut hid = match HidDevice::from_interface(device.clone(), &iface, &ep_in) {
            Ok(hid) => hid,
            Err(primary_err)
                if iface.interface_subclass == hid_subclass::BOOT
                    && iface.interface_protocol == hid_protocol::KEYBOARD =>
            {
                // Some keyboards reject early SET_PROTOCOL during constructor
                // attach. Retry without Boot subclass auto-switch and handle
                // protocol switching in this function's compatibility path.
                let mut relaxed_iface = iface;
                relaxed_iface.interface_subclass = hid_subclass::NONE;
                match HidDevice::from_interface(device.clone(), &relaxed_iface, &ep_in) {
                    Ok(hid) => {
                        bypassed_boot_subclass = true;
                        let mut line = heapless::String::<288>::new();
                        let _ = core::fmt::Write::write_fmt(
                            &mut line,
                            format_args!(
                                "[local-seat] usb hid attach retry slot={} iface={} ep=0x{:02x} source={} mode=boot-subclass-bypass detail={primary_err:?}",
                                device.slot_id(),
                                iface.interface_number,
                                ep_in.endpoint_address,
                                source
                            ),
                        );
                        boot_log::force_uart_line(line.as_str());
                        hid
                    }
                    Err(err) => {
                        if track_failures {
                            *saw_keyboard_init_error = true;
                        }
                        let mut line = heapless::String::<320>::new();
                        let _ = core::fmt::Write::write_fmt(
                            &mut line,
                            format_args!(
                                "[local-seat] usb hid attach failed slot={} iface={} ep=0x{:02x} source={} detail={primary_err:?} retry={err:?}",
                                device.slot_id(),
                                iface.interface_number,
                                ep_in.endpoint_address,
                                source,
                            ),
                        );
                        boot_log::force_uart_line(line.as_str());
                        return None;
                    }
                }
            }
            Err(err) => {
                if track_failures {
                    *saw_keyboard_init_error = true;
                }
                let mut line = heapless::String::<256>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] usb hid attach failed slot={} iface={} ep=0x{:02x} source={} detail={err:?}",
                        device.slot_id(),
                        iface.interface_number,
                        ep_in.endpoint_address,
                        source,
                    ),
                );
                boot_log::force_uart_line(line.as_str());
                return None;
            }
        };
        let mut report_layout = "boot";
        if require_boot_switch || bypassed_boot_subclass {
            match hid.set_protocol(0) {
                Ok(()) => {}
                Err(_) if force_keyboard_mode => {
                    // Non-boot keyboard candidates may only support report protocol.
                    // Keep a compatibility path that treats the first input byte
                    // as report ID.
                    report_layout = "report-id";
                    let mut line = heapless::String::<288>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut line,
                        format_args!(
                            "[local-seat] usb hid boot-switch failed slot={} iface={} ep=0x{:02x} source={} fallback=report-id",
                            device.slot_id(),
                            iface.interface_number,
                            ep_in.endpoint_address,
                            source,
                        ),
                    );
                    boot_log::force_uart_line(line.as_str());
                    hid = hid.into_forced_keyboard_with_report_id();
                }
                Err(_) => {
                    let mut line = heapless::String::<256>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut line,
                        format_args!(
                            "[local-seat] usb hid boot-switch failed slot={} iface={} ep=0x{:02x} source={}",
                            device.slot_id(),
                            iface.interface_number,
                            ep_in.endpoint_address,
                            source,
                        ),
                    );
                    boot_log::force_uart_line(line.as_str());
                    if track_failures {
                        *saw_keyboard_init_error = true;
                    }
                    return None;
                }
            }
        }
        if force_keyboard_mode && report_layout == "boot" {
            hid = hid.into_forced_keyboard();
        }
        if hid.queue_read().is_err() {
            if track_failures {
                *saw_keyboard_init_error = true;
            }
            return None;
        }
        let mut line = heapless::String::<256>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] usb hid keyboard ready slot={} iface={} ep=0x{:02x} source={} layout={} subclass=0x{:02x} protocol=0x{:02x}",
                device.slot_id(),
                iface.interface_number,
                ep_in.endpoint_address,
                source,
                report_layout,
                iface.interface_subclass,
                iface.interface_protocol
            ),
        );
        boot_log::force_uart_line(line.as_str());
        Some(hid)
    }

    fn hub_interface_info(device_desc: DeviceDesc, config_blob: &[u8]) -> Option<HubInterfaceInfo> {
        let mut offset = 0usize;
        while offset + 2 <= config_blob.len() {
            let len = config_blob[offset] as usize;
            let dtype = config_blob[offset + 1];
            if len == 0 || offset + len > config_blob.len() {
                break;
            }

            if dtype == desc_type::INTERFACE && len >= 9 {
                // SAFETY: Descriptor bytes may be unaligned in the config blob.
                let iface = unsafe {
                    ptr::read_unaligned(
                        config_blob
                            .as_ptr()
                            .add(offset)
                            .cast::<usb_oxide::InterfaceDesc>(),
                    )
                };
                if iface.interface_class == class::HUB {
                    return Some(HubInterfaceInfo {
                        protocol: iface.interface_protocol,
                        multi_tt: iface.interface_protocol == hub_protocol::HI_SPEED_MULTI_TT,
                        interface_number: iface.interface_number,
                    });
                }
            }
            offset += len;
        }
        if device_desc.device_class == class::HUB {
            let protocol = device_desc.device_protocol;
            return Some(HubInterfaceInfo {
                protocol,
                multi_tt: protocol == hub_protocol::HI_SPEED_MULTI_TT,
                interface_number: 0,
            });
        }
        None
    }

    fn scan_hub_children(
        device: Arc<UsbDevice<SeatDma>>,
        hub_protocol_code: u8,
        hub_multi_tt: bool,
        hub_interface_number: u8,
        depth_remaining: usize,
        saw_keyboard_init_error: &mut bool,
    ) -> Option<HidDevice<SeatDma>> {
        if hub_protocol_code == hub_protocol::SUPER_SPEED {
            let hub_depth = cmp::min(route_depth(device.route()), 4);
            match device.set_hub_depth(hub_depth) {
                Ok(()) => {
                    let mut line = heapless::String::<192>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut line,
                        format_args!(
                            "[local-seat] ss hub depth set slot={} route=0x{route:05x} depth={}",
                            device.slot_id(),
                            hub_depth,
                            route = device.route(),
                        ),
                    );
                    boot_log::force_uart_line(line.as_str());
                }
                Err(err) => {
                    let mut line = heapless::String::<224>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut line,
                        format_args!(
                            "[local-seat] ss hub depth set failed slot={} route=0x{route:05x} depth={} detail={err:?}",
                            device.slot_id(),
                            hub_depth,
                            route = device.route(),
                        ),
                    );
                    boot_log::force_uart_line(line.as_str());
                }
            }
        }

        let hub_desc = match Self::read_hub_descriptor(
            device.as_ref(),
            hub_protocol_code,
            hub_interface_number,
        ) {
            Some(desc) => desc,
            None => {
                let mut line = heapless::String::<192>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] hub enum failed slot={} stage=hub-desc proto={}",
                        device.slot_id(),
                        hub_protocol_code
                    ),
                );
                boot_log::force_uart_line(line.as_str());
                return None;
            }
        };
        let max_ports = cmp::min(hub_desc.num_ports as usize, HUB_MAX_DOWNSTREAM_PORTS);
        if max_ports == 0 {
            return None;
        }
        let hub_tt_think_time = hub_desc.tt_think_time();
        match device.configure_hub(max_ports as u8, hub_multi_tt) {
            Ok(()) => {
                let mut line = heapless::String::<224>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] hub slot configured slot={} iface={} ports={} mtt={}",
                        device.slot_id(),
                        hub_interface_number,
                        max_ports,
                        hub_multi_tt as u8
                    ),
                );
                boot_log::force_uart_line(line.as_str());
            }
            Err(err) => {
                let mut line = heapless::String::<256>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] hub slot config failed slot={} iface={} ports={} mtt={} detail={err:?}",
                        device.slot_id(),
                        hub_interface_number,
                        max_ports,
                        hub_multi_tt as u8
                    ),
                );
                boot_log::force_uart_line(line.as_str());
            }
        }

        let mut hub_line = heapless::String::<224>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut hub_line,
            format_args!(
                "[local-seat] hub desc slot={} iface={} ports={} pwr_mode={} pwr2good={} mtt={} ttt={}",
                device.slot_id(),
                hub_interface_number,
                max_ports,
                hub_desc.power_switching_mode(),
                hub_desc.pwr_on_2_pwr_good,
                hub_multi_tt as u8,
                hub_tt_think_time & 0x03,
            ),
        );
        boot_log::force_uart_line(hub_line.as_str());
        let hub_chars = hub_desc.hub_characteristics;
        let mut hub_raw_line = heapless::String::<192>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut hub_raw_line,
            format_args!(
                "[local-seat] hub desc raw slot={} iface={} proto={} chars=0x{:04x}",
                device.slot_id(),
                hub_interface_number,
                hub_protocol_code,
                hub_chars,
            ),
        );
        boot_log::force_uart_line(hub_raw_line.as_str());

        boot_log::force_uart_line("[local-seat] hub settle begin");
        wait_ms(HUB_POST_CONFIG_SETTLE_MS);
        boot_log::force_uart_line("[local-seat] hub settle end");

        boot_log::force_uart_line("[local-seat] hub power stage begin");
        Self::hub_power_on_ports(
            device.as_ref(),
            max_ports,
            hub_interface_number,
            hub_desc.power_switching_mode(),
            hub_desc.pwr_on_2_pwr_good,
        );
        boot_log::force_uart_line("[local-seat] hub power stage end");

        let reset_feature = Self::hub_reset_feature(hub_protocol_code);
        let mut attempted_ports = 0usize;
        let mut unavailable_pre_reset = 0usize;
        let mut disconnected_pre_reset = 0usize;
        let mut disconnected_unpowered_pre_reset = 0usize;
        let mut disconnected_recovered = 0usize;
        let mut reset_feature_failed = 0usize;
        let mut ready_timeout = 0usize;
        let mut blind_probe_attempted = 0usize;
        let blind_probe_succeeded = 0usize;

        for downstream in 1..=max_ports {
            let downstream_port = downstream as u8;
            attempted_ports = attempted_ports.saturating_add(1);

            let pre_status = Self::hub_port_status_with_retry(
                device.as_ref(),
                hub_interface_number,
                downstream_port,
                "pre-reset",
            );
            if let Some(status) = pre_status {
                Self::log_hub_port_status(device.slot_id(), downstream_port, "pre-reset", status);
                Self::clear_hub_port_change_bits(
                    device.as_ref(),
                    hub_interface_number,
                    downstream_port,
                    status,
                    hub_protocol_code,
                );
                if !status.connected() {
                    disconnected_pre_reset = disconnected_pre_reset.saturating_add(1);
                    if !status.powered() {
                        disconnected_unpowered_pre_reset =
                            disconnected_unpowered_pre_reset.saturating_add(1);
                    }
                    if Self::recover_disconnected_hub_port(
                        device.as_ref(),
                        hub_interface_number,
                        hub_protocol_code,
                        hub_desc.power_switching_mode(),
                        hub_desc.pwr_on_2_pwr_good,
                        downstream_port,
                        reset_feature,
                        status,
                    ) {
                        disconnected_recovered = disconnected_recovered.saturating_add(1);
                        if let Some(status_after_recovery) = Self::hub_port_status_with_retry(
                            device.as_ref(),
                            hub_interface_number,
                            downstream_port,
                            "post-disconnected-recovery",
                        ) {
                            Self::log_hub_port_status(
                                device.slot_id(),
                                downstream_port,
                                "post-disconnected-recovery",
                                status_after_recovery,
                            );
                            Self::clear_hub_port_change_bits(
                                device.as_ref(),
                                hub_interface_number,
                                downstream_port,
                                status_after_recovery,
                                hub_protocol_code,
                            );
                        } else {
                            Self::log_hub_port_status_unavailable(
                                device.slot_id(),
                                downstream_port,
                                "post-disconnected-recovery",
                            );
                        }
                    } else {
                        if HUB_ENABLE_DISCONNECTED_BLIND_PROBE {
                            blind_probe_attempted = blind_probe_attempted.saturating_add(1);
                            match Self::probe_hub_child_without_port_status(
                                &device,
                                downstream_port,
                                hub_multi_tt,
                                hub_tt_think_time,
                                depth_remaining,
                                saw_keyboard_init_error,
                                "disconnected-pre-reset",
                            ) {
                                HubChildProbeResult::Keyboard(hid) => return Some(hid),
                                HubChildProbeResult::ProbedNoKeyboard => {
                                    continue;
                                }
                                HubChildProbeResult::Failed => {}
                            }
                        }
                        Self::log_hub_port_exact_fault(
                            device.slot_id(),
                            hub_interface_number,
                            downstream_port,
                            "disconnected-pre-reset-no-recovery",
                            status,
                            hub_desc.power_switching_mode(),
                        );
                        Self::log_hub_port_terminal(
                            device.slot_id(),
                            downstream_port,
                            "disconnected-pre-reset",
                        );
                        continue;
                    }
                }
            } else {
                unavailable_pre_reset = unavailable_pre_reset.saturating_add(1);
                Self::log_hub_port_status_unavailable(
                    device.slot_id(),
                    downstream_port,
                    "pre-reset",
                );
                let prepared = Self::hub_prepare_port_without_status(
                    device.as_ref(),
                    hub_interface_number,
                    hub_protocol_code,
                    hub_desc.power_switching_mode(),
                    hub_desc.pwr_on_2_pwr_good,
                    downstream_port,
                    reset_feature,
                );
                if !prepared {
                    let mut line = heapless::String::<256>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut line,
                        format_args!(
                            "[local-seat] hub blind-probe fallback slot={} port={} reason=prepare-failed source=pre-status-unavailable",
                            device.slot_id(),
                            downstream_port,
                        ),
                    );
                    boot_log::force_uart_line(line.as_str());
                }
                let blind_source = if prepared {
                    "pre-status-unavailable"
                } else {
                    "pre-status-unavailable-no-prepare"
                };
                if !prepared || !HUB_ENABLE_UNAVAILABLE_BLIND_PROBE {
                    Self::log_hub_port_terminal(device.slot_id(), downstream_port, blind_source);
                    continue;
                }
                blind_probe_attempted = blind_probe_attempted.saturating_add(1);
                match Self::probe_hub_child_without_port_status(
                    &device,
                    downstream_port,
                    hub_multi_tt,
                    hub_tt_think_time,
                    depth_remaining,
                    saw_keyboard_init_error,
                    blind_source,
                ) {
                    HubChildProbeResult::Keyboard(hid) => return Some(hid),
                    HubChildProbeResult::ProbedNoKeyboard => {
                        continue;
                    }
                    HubChildProbeResult::Failed => {}
                }
                Self::log_hub_port_terminal(device.slot_id(), downstream_port, blind_source);
                continue;
            }

            if !Self::hub_set_feature_with_retry(
                device.as_ref(),
                hub_interface_number,
                downstream_port,
                reset_feature,
                "set-feature",
                HUB_SET_FEATURE_RETRIES,
            ) {
                if let Some(status) = Self::hub_port_status_with_retry(
                    device.as_ref(),
                    hub_interface_number,
                    downstream_port,
                    "set-feature-fail",
                ) {
                    Self::log_hub_port_status(
                        device.slot_id(),
                        downstream_port,
                        "set-feature-fail",
                        status,
                    );
                } else {
                    Self::log_hub_port_status_unavailable(
                        device.slot_id(),
                        downstream_port,
                        "set-feature-fail",
                    );
                }
                reset_feature_failed = reset_feature_failed.saturating_add(1);
                Self::log_hub_port_terminal(
                    device.slot_id(),
                    downstream_port,
                    "reset-feature-failed",
                );
                continue;
            }
            wait_ms(HUB_RESET_SETTLE_MS);
            if let Some(status) = Self::hub_port_status_with_retry(
                device.as_ref(),
                hub_interface_number,
                downstream_port,
                "post-reset-delay",
            ) {
                Self::log_hub_port_status(
                    device.slot_id(),
                    downstream_port,
                    "post-reset-delay",
                    status,
                );
            } else {
                Self::log_hub_port_status_unavailable(
                    device.slot_id(),
                    downstream_port,
                    "post-reset-delay",
                );
            }

            let Some(status) = Self::wait_hub_port_ready(
                device.as_ref(),
                hub_interface_number,
                downstream_port,
                hub_protocol_code,
            ) else {
                ready_timeout = ready_timeout.saturating_add(1);
                Self::log_hub_port_terminal(device.slot_id(), downstream_port, "ready-timeout");
                continue;
            };
            Self::clear_hub_port_change_bits(
                device.as_ref(),
                hub_interface_number,
                downstream_port,
                status,
                hub_protocol_code,
            );
            if let Some(status_after_clear) = Self::hub_port_status_with_retry(
                device.as_ref(),
                hub_interface_number,
                downstream_port,
                "post-clear-change",
            ) {
                Self::log_hub_port_status(
                    device.slot_id(),
                    downstream_port,
                    "post-clear-change",
                    status_after_clear,
                );
            } else {
                Self::log_hub_port_status_unavailable(
                    device.slot_id(),
                    downstream_port,
                    "post-clear-change",
                );
            }

            let Some(route) = append_route_segment(device.route(), downstream_port) else {
                Self::log_hub_port_terminal(device.slot_id(), downstream_port, "route-overflow");
                continue;
            };
            let child_speed = Self::speed_from_hub_port_status(status, hub_protocol_code);
            match Self::probe_hub_child_with_route_and_speed(
                &device,
                downstream_port,
                route,
                child_speed,
                hub_multi_tt,
                hub_tt_think_time,
                depth_remaining,
                saw_keyboard_init_error,
                "status-path",
            ) {
                HubChildProbeResult::Keyboard(hid) => return Some(hid),
                HubChildProbeResult::ProbedNoKeyboard => continue,
                HubChildProbeResult::Failed => {
                    let mut line = heapless::String::<256>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut line,
                        format_args!(
                            "[local-seat] hub status-path fallback slot={} port={} route=0x{route:05x} speed={} detail=address-fail",
                            device.slot_id(),
                            downstream_port,
                            child_speed,
                        ),
                    );
                    boot_log::force_uart_line(line.as_str());
                    let rearmed_primary_speed = match Self::rearm_hub_port_after_address_failure(
                        device.as_ref(),
                        hub_interface_number,
                        downstream_port,
                        hub_protocol_code,
                        "status-path",
                    ) {
                        Some(rearmed_status) => {
                            let observed =
                                Self::speed_from_hub_port_status(rearmed_status, hub_protocol_code);
                            let mut speed_line = heapless::String::<256>::new();
                            let _ = core::fmt::Write::write_fmt(
                                &mut speed_line,
                                format_args!(
                                    "[local-seat] hub fallback speed-refresh slot={} port={} route=0x{route:05x} prev={} observed={}",
                                    device.slot_id(),
                                    downstream_port,
                                    child_speed,
                                    observed
                                ),
                            );
                            boot_log::force_uart_line(speed_line.as_str());
                            observed
                        }
                        None => child_speed,
                    };
                    if HUB_ENABLE_SPEED_FALLBACK_REPROBE {
                        match Self::probe_hub_child_with_speed_fallback(
                            &device,
                            downstream_port,
                            route,
                            rearmed_primary_speed,
                            hub_multi_tt,
                            hub_tt_think_time,
                            hub_interface_number,
                            hub_protocol_code,
                            depth_remaining,
                            saw_keyboard_init_error,
                        ) {
                            HubChildProbeResult::Keyboard(hid) => return Some(hid),
                            HubChildProbeResult::ProbedNoKeyboard | HubChildProbeResult::Failed => {
                                continue;
                            }
                        }
                    } else {
                        continue;
                    }
                }
            }
        }

        let mut summary = heapless::String::<256>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut summary,
            format_args!(
                "[local-seat] hub scan summary slot={} iface={} attempted_ports={} pre_status_unavailable={} disconnected_pre_reset={} disconnected_unpowered_pre_reset={} disconnected_recovered={} reset_feature_failed={} ready_timeout={} blind_probe_attempted={} blind_probe_succeeded={}",
                device.slot_id(),
                hub_interface_number,
                attempted_ports,
                unavailable_pre_reset,
                disconnected_pre_reset,
                disconnected_unpowered_pre_reset,
                disconnected_recovered,
                reset_feature_failed,
                ready_timeout,
                blind_probe_attempted,
                blind_probe_succeeded
            ),
        );
        boot_log::force_uart_line(summary.as_str());
        if attempted_ports > 0
            && disconnected_pre_reset == attempted_ports
            && disconnected_recovered == 0
        {
            let reason = if disconnected_unpowered_pre_reset == attempted_ports {
                "all-ports-unpowered-pre-reset"
            } else {
                "all-ports-disconnected-pre-reset"
            };
            let mut line = heapless::String::<256>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] hub exact-fault slot={} iface={} reason={} mode={} attempted_ports={}",
                    device.slot_id(),
                    hub_interface_number,
                    reason,
                    hub_desc.power_switching_mode(),
                    attempted_ports,
                ),
            );
            boot_log::force_uart_line(line.as_str());
        }

        None
    }

    fn probe_hub_child_without_port_status(
        device: &Arc<UsbDevice<SeatDma>>,
        downstream_port: u8,
        hub_multi_tt: bool,
        hub_tt_think_time: u8,
        depth_remaining: usize,
        saw_keyboard_init_error: &mut bool,
        source: &str,
    ) -> HubChildProbeResult {
        let Some(route) = append_route_segment(device.route(), downstream_port) else {
            Self::log_hub_port_terminal(device.slot_id(), downstream_port, "route-overflow");
            return HubChildProbeResult::Failed;
        };

        const SPEED_CANDIDATES: [u8; 3] = [
            usb_oxide::regs::SPEED_HIGH,
            usb_oxide::regs::SPEED_FULL,
            usb_oxide::regs::SPEED_LOW,
        ];
        for child_speed in SPEED_CANDIDATES {
            let mut line = heapless::String::<256>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] hub blind-probe slot={} port={} route=0x{route:05x} speed={} source={}",
                    device.slot_id(),
                    downstream_port,
                    child_speed,
                    source,
                ),
            );
            boot_log::force_uart_line(line.as_str());

            match Self::probe_hub_child_with_route_and_speed(
                device,
                downstream_port,
                route,
                child_speed,
                hub_multi_tt,
                hub_tt_think_time,
                depth_remaining,
                saw_keyboard_init_error,
                source,
            ) {
                HubChildProbeResult::Keyboard(hid) => return HubChildProbeResult::Keyboard(hid),
                HubChildProbeResult::ProbedNoKeyboard => {
                    return HubChildProbeResult::ProbedNoKeyboard;
                }
                HubChildProbeResult::Failed => {}
            }
        }

        let mut line = heapless::String::<224>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] hub blind-probe failed slot={} port={} route=0x{route:05x}",
                device.slot_id(),
                downstream_port
            ),
        );
        boot_log::force_uart_line(line.as_str());
        HubChildProbeResult::Failed
    }

    fn probe_hub_child_with_speed_fallback(
        device: &Arc<UsbDevice<SeatDma>>,
        downstream_port: u8,
        route: u32,
        primary_speed: u8,
        hub_multi_tt: bool,
        hub_tt_think_time: u8,
        hub_interface_number: u8,
        hub_protocol_code: u8,
        depth_remaining: usize,
        saw_keyboard_init_error: &mut bool,
    ) -> HubChildProbeResult {
        let mut retry_line = heapless::String::<256>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut retry_line,
            format_args!(
                "[local-seat] hub status-retry slot={} port={} route=0x{route:05x} speed={}",
                device.slot_id(),
                downstream_port,
                primary_speed
            ),
        );
        boot_log::force_uart_line(retry_line.as_str());
        match Self::probe_hub_child_with_route_and_speed(
            device,
            downstream_port,
            route,
            primary_speed,
            hub_multi_tt,
            hub_tt_think_time,
            depth_remaining,
            saw_keyboard_init_error,
            "status-retry",
        ) {
            HubChildProbeResult::Keyboard(hid) => return HubChildProbeResult::Keyboard(hid),
            HubChildProbeResult::ProbedNoKeyboard => return HubChildProbeResult::ProbedNoKeyboard,
            HubChildProbeResult::Failed => {}
        }

        const SPEED_CANDIDATES: [u8; 3] = [
            usb_oxide::regs::SPEED_HIGH,
            usb_oxide::regs::SPEED_FULL,
            usb_oxide::regs::SPEED_LOW,
        ];
        for child_speed in SPEED_CANDIDATES {
            if child_speed == primary_speed {
                continue;
            }

            let observed_speed = Self::rearm_hub_port_after_address_failure(
                device.as_ref(),
                hub_interface_number,
                downstream_port,
                hub_protocol_code,
                "status-fallback",
            )
            .map(|status| Self::speed_from_hub_port_status(status, hub_protocol_code));
            if let Some(observed) = observed_speed {
                let mut line = heapless::String::<256>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] hub fallback candidate slot={} port={} route=0x{route:05x} candidate_speed={} observed_speed={} primary_speed={}",
                        device.slot_id(),
                        downstream_port,
                        child_speed,
                        observed,
                        primary_speed
                    ),
                );
                boot_log::force_uart_line(line.as_str());
            } else {
                let mut line = heapless::String::<256>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] hub fallback candidate slot={} port={} route=0x{route:05x} candidate_speed={} detail=rearm-unavailable primary_speed={}",
                        device.slot_id(),
                        downstream_port,
                        child_speed,
                        primary_speed
                    ),
                );
                boot_log::force_uart_line(line.as_str());
            }

            let mut line = heapless::String::<256>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] hub status-fallback slot={} port={} route=0x{route:05x} speed={} primary_speed={}",
                    device.slot_id(),
                    downstream_port,
                    child_speed,
                    primary_speed,
                ),
            );
            boot_log::force_uart_line(line.as_str());

            match Self::probe_hub_child_with_route_and_speed(
                device,
                downstream_port,
                route,
                child_speed,
                hub_multi_tt,
                hub_tt_think_time,
                depth_remaining,
                saw_keyboard_init_error,
                "status-fallback",
            ) {
                HubChildProbeResult::Keyboard(hid) => return HubChildProbeResult::Keyboard(hid),
                HubChildProbeResult::ProbedNoKeyboard => {
                    return HubChildProbeResult::ProbedNoKeyboard;
                }
                HubChildProbeResult::Failed => {}
            }
        }

        let mut line = heapless::String::<256>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] hub status-fallback failed slot={} port={} route=0x{route:05x} primary_speed={}",
                device.slot_id(),
                downstream_port,
                primary_speed,
            ),
        );
        boot_log::force_uart_line(line.as_str());
        HubChildProbeResult::Failed
    }

    fn probe_hub_child_with_route_and_speed(
        device: &Arc<UsbDevice<SeatDma>>,
        downstream_port: u8,
        route: u32,
        child_speed: u8,
        hub_multi_tt: bool,
        hub_tt_think_time: u8,
        depth_remaining: usize,
        saw_keyboard_init_error: &mut bool,
        source: &str,
    ) -> HubChildProbeResult {
        let mut tt_port_raw = 0u8;
        let mut tt_ttt_raw = 0u8;
        let tt_context = if (child_speed == usb_oxide::regs::SPEED_LOW
            || child_speed == usb_oxide::regs::SPEED_FULL)
            && device.speed() == usb_oxide::regs::SPEED_HIGH
        {
            tt_port_raw = downstream_port;
            tt_ttt_raw = hub_tt_think_time & 0x03;
            let (tt_port, tt_ttt) =
                normalize_hub_tt_profile(downstream_port, hub_multi_tt, hub_tt_think_time);
            if tt_port != tt_port_raw || tt_ttt != tt_ttt_raw {
                let mut line = heapless::String::<320>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] hub tt normalized slot={} port={} route=0x{route:05x} root_port={} speed={} source={} mtt={} tt_port_raw={} tt_port={} ttt_raw={} ttt={}",
                        device.slot_id(),
                        downstream_port,
                        device.root_hub_port(),
                        child_speed,
                        source,
                        hub_multi_tt as u8,
                        tt_port_raw,
                        tt_port,
                        tt_ttt_raw,
                        tt_ttt
                    ),
                );
                boot_log::force_uart_line(line.as_str());
            }
            Some(TtContext {
                hub_slot_id: device.slot_id(),
                downstream_port: tt_port,
                tt_think_time: tt_ttt,
                multi_tt: hub_multi_tt,
            })
        } else {
            None
        };
        let (tt_slot, tt_port, tt_ttt, tt_multi, tt_port_orig, tt_ttt_orig) =
            if let Some(tt) = tt_context {
                (
                    tt.hub_slot_id as u16,
                    tt.downstream_port as u16,
                    tt.tt_think_time as u16,
                    tt.multi_tt as u16,
                    tt_port_raw as u16,
                    tt_ttt_raw as u16,
                )
            } else {
                (0xffff, 0xffff, 0xffff, 0, 0xffff, 0xffff)
            };
        let mut begin_line = heapless::String::<320>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut begin_line,
            format_args!(
                "[local-seat] hub child address begin slot={} port={} source={} speed={} route=0x{route:05x} root_port={} tt=slot:{} port:{} ttt:{} mtt:{} raw_port:{} raw_ttt:{}",
                device.slot_id(),
                downstream_port,
                source,
                child_speed,
                device.root_hub_port(),
                tt_slot,
                tt_port,
                tt_ttt,
                tt_multi,
                tt_port_orig,
                tt_ttt_orig,
            ),
        );
        boot_log::force_uart_line(begin_line.as_str());
        let mut begin_tt_line = heapless::String::<224>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut begin_tt_line,
            format_args!(
                "[local-seat] hub child tt slot={} port={} speed={} src={} tt={}:{}/{} mtt={} raw={}/{}",
                device.slot_id(),
                downstream_port,
                child_speed,
                source,
                tt_slot,
                tt_port,
                tt_ttt,
                tt_multi,
                tt_port_orig,
                tt_ttt_orig,
            ),
        );
        boot_log::force_uart_line(begin_tt_line.as_str());

        let mut child = match UsbDevice::new_routed(
            device.ctrl().clone(),
            route,
            device.root_hub_port(),
            child_speed,
            tt_context,
        ) {
            Ok(child) => child,
            Err(err) => {
                if let UsbError::CmdFail(code) = err {
                    let mut cmd_line = heapless::String::<240>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut cmd_line,
                        format_args!(
                            "[local-seat] hub child address cmd-fail slot={} port={} speed={} source={} code={} name={}",
                            device.slot_id(),
                            downstream_port,
                            child_speed,
                            source,
                            code,
                            completion::name(code),
                        ),
                    );
                    boot_log::force_uart_line(cmd_line.as_str());
                    let mut cmd_tt_line = heapless::String::<256>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut cmd_tt_line,
                        format_args!(
                            "[local-seat] hub child cmdfail-tt slot={} port={} speed={} src={} route=0x{route:05x} root_port={} tt={}:{}/{} mtt={} raw={}/{}",
                            device.slot_id(),
                            downstream_port,
                            child_speed,
                            source,
                            device.root_hub_port(),
                            tt_slot,
                            tt_port,
                            tt_ttt,
                            tt_multi,
                            tt_port_orig,
                            tt_ttt_orig,
                        ),
                    );
                    boot_log::force_uart_line(cmd_tt_line.as_str());
                }
                let mut line = heapless::String::<320>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] hub child failed slot={} port={} stage=address speed={} source={} route=0x{route:05x} root_port={} tt=slot:{} port:{} ttt:{} mtt:{} raw_port:{} raw_ttt:{} detail={err:?}",
                        device.slot_id(),
                        downstream_port,
                        child_speed,
                        source,
                        device.root_hub_port(),
                        tt_slot,
                        tt_port,
                        tt_ttt,
                        tt_multi,
                        tt_port_orig,
                        tt_ttt_orig,
                    ),
                );
                boot_log::force_uart_line(line.as_str());
                Self::log_hub_port_terminal(device.slot_id(), downstream_port, "address-fail");
                return HubChildProbeResult::Failed;
            }
        };

        let child_desc = match child.get_device_descriptor() {
            Ok(desc) => desc,
            Err(err) => {
                let mut line = heapless::String::<272>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] hub child failed slot={} port={} stage=device-desc speed={} source={} detail={err:?}",
                        device.slot_id(),
                        downstream_port,
                        child_speed,
                        source
                    ),
                );
                boot_log::force_uart_line(line.as_str());
                Self::log_hub_port_terminal(device.slot_id(), downstream_port, "device-desc-fail");
                return HubChildProbeResult::Failed;
            }
        };
        let child_config_blob = match child.get_config_descriptor(0) {
            Ok(config_blob) => config_blob,
            Err(err) => {
                let mut line = heapless::String::<272>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] hub child failed slot={} port={} stage=config-desc speed={} source={} detail={err:?}",
                        device.slot_id(),
                        downstream_port,
                        child_speed,
                        source
                    ),
                );
                boot_log::force_uart_line(line.as_str());
                Self::log_hub_port_terminal(device.slot_id(), downstream_port, "config-desc-fail");
                return HubChildProbeResult::Failed;
            }
        };
        let Some(config) = read_config_desc(&child_config_blob) else {
            let total_len = if child_config_blob.len() >= 4 {
                u16::from_le_bytes([child_config_blob[2], child_config_blob[3]])
            } else {
                0
            };
            let mut detail = heapless::String::<256>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut detail,
                format_args!(
                    "[local-seat] hub cfg parse detail slot={} port={} speed={} source={} len={} b0=0x{:02x} b1=0x{:02x} total=0x{:04x}",
                    device.slot_id(),
                    downstream_port,
                    child_speed,
                    source,
                    child_config_blob.len(),
                    child_config_blob.get(0).copied().unwrap_or(0),
                    child_config_blob.get(1).copied().unwrap_or(0),
                    total_len
                ),
            );
            boot_log::force_uart_line(detail.as_str());

            let mut line = heapless::String::<224>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] hub child failed slot={} port={} stage=config-parse speed={} source={}",
                    device.slot_id(),
                    downstream_port,
                    child_speed,
                    source
                ),
            );
            boot_log::force_uart_line(line.as_str());
            Self::log_hub_port_terminal(device.slot_id(), downstream_port, "config-parse-fail");
            return HubChildProbeResult::Failed;
        };
        let Some(config_value) = config_value_for_set(config) else {
            let mut line = heapless::String::<272>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] hub child failed slot={} port={} stage=set-config-value speed={} source={} detail=invalid bConfigurationValue=0x{:02x} iConfiguration=0x{:02x}",
                    device.slot_id(),
                    downstream_port,
                    child_speed,
                    source,
                    config.configuration_value(),
                    config.configuration_string_index()
                ),
            );
            boot_log::force_uart_line(line.as_str());
            Self::log_hub_port_terminal(device.slot_id(), downstream_port, "set-config-value-fail");
            return HubChildProbeResult::Failed;
        };
        if let Err(err) = child.set_configuration(config_value) {
            let mut line = heapless::String::<272>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] hub child failed slot={} port={} stage=set-config({}) speed={} source={} detail={err:?}",
                    device.slot_id(),
                    downstream_port,
                    config_value,
                    child_speed,
                    source
                ),
            );
            boot_log::force_uart_line(line.as_str());
            Self::log_hub_port_terminal(device.slot_id(), downstream_port, "set-config-fail");
            return HubChildProbeResult::Failed;
        }

        let child = Arc::new(child);
        if let Some(hid) = Self::probe_device_for_keyboard(
            child,
            child_desc,
            &child_config_blob,
            depth_remaining,
            saw_keyboard_init_error,
        ) {
            Self::log_hub_port_terminal(device.slot_id(), downstream_port, "keyboard-found");
            return HubChildProbeResult::Keyboard(hid);
        }
        Self::log_hub_port_terminal(device.slot_id(), downstream_port, "not-keyboard");
        HubChildProbeResult::ProbedNoKeyboard
    }

    fn rearm_hub_port_after_address_failure(
        device: &UsbDevice<SeatDma>,
        hub_interface_number: u8,
        downstream_port: u8,
        hub_protocol_code: u8,
        source: &str,
    ) -> Option<HubPortStatus> {
        let reset_feature = Self::hub_reset_feature(hub_protocol_code);
        let mut begin = heapless::String::<224>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut begin,
            format_args!(
                "[local-seat] hub rearm begin slot={} iface={} port={} source={}",
                device.slot_id(),
                hub_interface_number,
                downstream_port,
                source
            ),
        );
        boot_log::force_uart_line(begin.as_str());

        if !Self::hub_set_feature_with_retry(
            device,
            hub_interface_number,
            downstream_port,
            reset_feature,
            "rearm-set-feature",
            HUB_SET_FEATURE_RETRIES,
        ) {
            let mut fail = heapless::String::<224>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut fail,
                format_args!(
                    "[local-seat] hub rearm fail slot={} iface={} port={} source={} stage=set-feature",
                    device.slot_id(),
                    hub_interface_number,
                    downstream_port,
                    source
                ),
            );
            boot_log::force_uart_line(fail.as_str());
            return None;
        }

        wait_ms(HUB_RESET_SETTLE_MS);
        let Some(status) = Self::wait_hub_port_ready(
            device,
            hub_interface_number,
            downstream_port,
            hub_protocol_code,
        ) else {
            let mut fail = heapless::String::<224>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut fail,
                format_args!(
                    "[local-seat] hub rearm fail slot={} iface={} port={} source={} stage=wait-ready",
                    device.slot_id(),
                    hub_interface_number,
                    downstream_port,
                    source
                ),
            );
            boot_log::force_uart_line(fail.as_str());
            return None;
        };
        Self::clear_hub_port_change_bits(
            device,
            hub_interface_number,
            downstream_port,
            status,
            hub_protocol_code,
        );
        let final_status = Self::hub_port_status_with_retry(
            device,
            hub_interface_number,
            downstream_port,
            "rearm",
        )
        .unwrap_or(status);
        Self::log_hub_port_status(device.slot_id(), downstream_port, "rearm", final_status);
        Some(final_status)
    }

    fn read_hub_descriptor(
        device: &UsbDevice<SeatDma>,
        hub_protocol_code: u8,
        hub_interface_number: u8,
    ) -> Option<HubDesc> {
        let mut blob = [0u8; HUB_DESC_MAX_BYTES];
        let descriptor_type = if hub_protocol_code == hub_protocol::SUPER_SPEED {
            desc_type::SS_HUB
        } else {
            desc_type::HUB
        };
        let descriptor_value = (descriptor_type as u16) << 8;
        let request_len = blob.len() as u16;
        let primary_setup = SetupPacket::new(
            0xA0,
            request::GET_DESCRIPTOR,
            descriptor_value,
            // Linux and USB hub class requests issue hub descriptor reads
            // against the hub device with wIndex=0.
            0,
            request_len,
        );
        let transferred = match device.control_transfer(&primary_setup, Some(&mut blob)) {
            Ok(transferred) => transferred,
            Err(primary_err) => {
                let Some(alt_index) = Self::hub_interface_windex(hub_interface_number) else {
                    let mut line = heapless::String::<288>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut line,
                        format_args!(
                            "[local-seat] hub desc read fail slot={} iface={} req=(bm=0xa0,b=0x{:02x},wValue=0x{:04x},wIndex=0x{:04x},wLen=0x{:04x}) detail={primary_err:?}",
                            device.slot_id(),
                            hub_interface_number,
                            request::GET_DESCRIPTOR,
                            descriptor_value,
                            0,
                            request_len
                        ),
                    );
                    boot_log::force_uart_line(line.as_str());
                    return None;
                };
                let alt_setup = SetupPacket::new(
                    0xA0,
                    request::GET_DESCRIPTOR,
                    descriptor_value,
                    alt_index,
                    request_len,
                );
                match device.control_transfer(&alt_setup, Some(&mut blob)) {
                    Ok(transferred) => {
                        let mut line = heapless::String::<256>::new();
                        let _ = core::fmt::Write::write_fmt(
                            &mut line,
                            format_args!(
                                "[local-seat] hub desc fallback slot={} iface={} wIndex=0x{:04x} detail=primary-failed({primary_err:?})",
                                device.slot_id(),
                                hub_interface_number,
                                alt_index
                            ),
                        );
                        boot_log::force_uart_line(line.as_str());
                        transferred
                    }
                    Err(err) => {
                        let mut line = heapless::String::<320>::new();
                        let _ = core::fmt::Write::write_fmt(
                            &mut line,
                            format_args!(
                                "[local-seat] hub desc read fail slot={} iface={} req=(bm=0xa0,b=0x{:02x},wValue=0x{:04x},wIndex=0x{:04x}/0x{:04x},wLen=0x{:04x}) detail={err:?}",
                                device.slot_id(),
                                hub_interface_number,
                                request::GET_DESCRIPTOR,
                                descriptor_value,
                                0,
                                alt_index,
                                request_len
                            ),
                        );
                        boot_log::force_uart_line(line.as_str());
                        return None;
                    }
                }
            }
        };
        let mut raw_line = heapless::String::<320>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut raw_line,
            format_args!(
                "[local-seat] hub desc raw slot={} iface={} len={} bytes={:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x}",
                device.slot_id(),
                hub_interface_number,
                transferred,
                blob[0],
                blob[1],
                blob[2],
                blob[3],
                blob[4],
                blob[5],
                blob[6],
                blob[7],
                blob[8],
                blob[9],
                blob[10],
                blob[11]
            ),
        );
        boot_log::force_uart_line(raw_line.as_str());
        if transferred < mem::size_of::<HubDesc>() {
            let mut line = heapless::String::<224>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] hub desc short slot={} iface={} need={} got={}",
                    device.slot_id(),
                    hub_interface_number,
                    mem::size_of::<HubDesc>(),
                    transferred
                ),
            );
            boot_log::force_uart_line(line.as_str());
            return None;
        }
        // SAFETY: Hub descriptor bytes may be unaligned in the transfer buffer.
        Some(unsafe { ptr::read_unaligned(blob.as_ptr().cast::<HubDesc>()) })
    }

    #[inline]
    const fn hub_port_index(port: u8) -> u16 {
        // Port-recipient hub requests encode the downstream port in wIndex.
        port as u16
    }

    #[inline]
    const fn hub_port_index_with_interface(interface_number: u8, port: u8) -> u16 {
        ((interface_number as u16) << 8) | (port as u16)
    }

    #[inline]
    const fn hub_interface_windex(interface_number: u8) -> Option<u16> {
        if interface_number == 0 {
            None
        } else {
            Some(interface_number as u16)
        }
    }

    fn hub_port_index_candidates(
        interface_number: u8,
        port: u8,
    ) -> ([u16; HUB_PORT_INDEX_CANDIDATES_MAX], usize) {
        let mut candidates = [0u16; HUB_PORT_INDEX_CANDIDATES_MAX];
        let mut count = 0usize;

        let primary = Self::hub_port_index(port);
        candidates[count] = primary;
        count += 1;

        let explicit = Self::hub_port_index_with_interface(interface_number, port);
        if explicit != primary {
            candidates[count] = explicit;
            count += 1;
        }

        for iface in 1..=HUB_PORT_IFACE_FALLBACK_MAX {
            let candidate = Self::hub_port_index_with_interface(iface, port);
            let mut seen = false;
            for existing in candidates.iter().take(count).copied() {
                if existing == candidate {
                    seen = true;
                    break;
                }
            }
            if !seen {
                candidates[count] = candidate;
                count += 1;
            }
        }

        (candidates, count)
    }

    #[inline]
    const fn should_try_hub_index_fallback(err: UsbError) -> bool {
        if !HUB_INDEX_FALLBACK_ON_STALL_ONLY {
            return true;
        }
        matches!(err, UsbError::Stall)
    }

    fn hub_set_feature(
        device: &UsbDevice<SeatDma>,
        hub_interface_number: u8,
        feature: u16,
        port: u8,
        wait_spins: usize,
    ) -> core::result::Result<(), UsbError> {
        let (candidates, candidate_count) =
            Self::hub_port_index_candidates(hub_interface_number, port);
        let primary_index = candidates[0];
        let primary_setup = SetupPacket::new(0x23, request::SET_FEATURE, feature, primary_index, 0);
        let primary_err =
            match device.control_transfer_with_wait_spins(&primary_setup, None, wait_spins) {
                Ok(_) => return Ok(()),
                Err(err) => err,
            };
        if candidate_count <= 1 || !Self::should_try_hub_index_fallback(primary_err) {
            return Err(primary_err);
        }

        let mut last_err = primary_err;
        for index in candidates.iter().take(candidate_count).copied().skip(1) {
            let setup = SetupPacket::new(0x23, request::SET_FEATURE, feature, index, 0);
            match device.control_transfer_with_wait_spins(&setup, None, wait_spins) {
                Ok(_) => {
                    let mut line = heapless::String::<256>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut line,
                        format_args!(
                            "[local-seat] hub ctl fallback slot={} iface={} port={} stage=set-feature wIndex=0x{:04x} detail=primary-failed({primary_err:?})",
                            device.slot_id(),
                            hub_interface_number,
                            port,
                            index
                        ),
                    );
                    boot_log::force_uart_line(line.as_str());
                    return Ok(());
                }
                Err(err) => {
                    last_err = err;
                }
            }
        }
        Err(last_err)
    }

    #[inline]
    const fn hub_reset_feature(hub_protocol_code: u8) -> u16 {
        if hub_protocol_code == hub_protocol::SUPER_SPEED {
            hub_feature::BH_PORT_RESET
        } else {
            hub_feature::PORT_RESET
        }
    }

    #[inline]
    const fn hub_mode_supports_port_power(power_mode: u8) -> bool {
        power_mode == 0 || power_mode == 1
    }

    #[inline]
    const fn hub_should_eager_port_power(power_mode: u8) -> bool {
        HUB_EAGER_INDIVIDUAL_PORT_POWER && power_mode == 1
    }

    #[inline]
    const fn hub_should_power_port_during_blind_prepare(power_mode: u8) -> bool {
        Self::hub_mode_supports_port_power(power_mode)
    }

    #[inline]
    const fn hub_should_probe_status_after_power_kick(power_mode: u8) -> bool {
        power_mode != 1
    }

    #[inline]
    const fn hub_post_power_wait_ms(power_mode: u8, pwr_on_2_pwr_good: u8) -> u64 {
        let pwr_good_ms = (pwr_on_2_pwr_good as u64).saturating_mul(2);
        let minimum = if power_mode == 1 {
            HUB_POWER_SETTLE_MIN_MS
        } else {
            HUB_PORT_STATUS_QUICK_RETRY_DELAY_MS
        };
        if pwr_good_ms > minimum {
            pwr_good_ms
        } else {
            minimum
        }
    }

    fn recover_disconnected_hub_port(
        device: &UsbDevice<SeatDma>,
        hub_interface_number: u8,
        hub_protocol_code: u8,
        hub_power_mode: u8,
        pwr_on_2_pwr_good: u8,
        port: u8,
        reset_feature: u16,
        pre_status: HubPortStatus,
    ) -> bool {
        let mut begin = heapless::String::<256>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut begin,
            format_args!(
                "[local-seat] hub disconnected recover slot={} iface={} port={} mode={} pre_status=0x{:04x} pre_change=0x{:04x} pre_pwr={}",
                device.slot_id(),
                hub_interface_number,
                port,
                hub_power_mode,
                pre_status.status,
                pre_status.change,
                pre_status.powered() as u8,
            ),
        );
        boot_log::force_uart_line(begin.as_str());

        let mut power_kick_sent = false;
        let mut post_power_status = None;
        if !pre_status.powered() && Self::hub_mode_supports_port_power(hub_power_mode) {
            power_kick_sent = Self::hub_set_feature_with_retry(
                device,
                hub_interface_number,
                port,
                hub_feature::PORT_POWER,
                "disconnected-kick-power",
                HUB_DISCONNECTED_RECOVERY_POWER_RETRIES,
            );
            if power_kick_sent {
                let wait_ms_after_power =
                    Self::hub_post_power_wait_ms(hub_power_mode, pwr_on_2_pwr_good);
                wait_ms(wait_ms_after_power);
                if Self::hub_should_probe_status_after_power_kick(hub_power_mode) {
                    post_power_status = Self::hub_port_status_with_retry(
                        device,
                        hub_interface_number,
                        port,
                        "disconnected-post-power",
                    );
                    if let Some(status) = post_power_status {
                        Self::log_hub_port_status(
                            device.slot_id(),
                            port,
                            "disconnected-post-power",
                            status,
                        );
                        Self::clear_hub_port_change_bits(
                            device,
                            hub_interface_number,
                            port,
                            status,
                            hub_protocol_code,
                        );
                        if status.connected() {
                            Self::log_hub_port_exact_fault(
                                device.slot_id(),
                                hub_interface_number,
                                port,
                                "recovered-after-power-kick",
                                status,
                                hub_power_mode,
                            );
                            return true;
                        }
                    } else {
                        Self::log_hub_port_status_unavailable(
                            device.slot_id(),
                            port,
                            "disconnected-post-power",
                        );
                    }
                } else {
                    let mut line = heapless::String::<256>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut line,
                        format_args!(
                            "[local-seat] hub power kick deferred-status slot={} iface={} port={} mode={} wait_ms={}",
                            device.slot_id(),
                            hub_interface_number,
                            port,
                            hub_power_mode,
                            wait_ms_after_power,
                        ),
                    );
                    boot_log::force_uart_line(line.as_str());
                }
            }
        }

        let reset_sent = Self::hub_set_feature_with_retry(
            device,
            hub_interface_number,
            port,
            reset_feature,
            "disconnected-force-reset",
            1,
        );
        if reset_sent {
            wait_ms(HUB_RESET_SETTLE_MS);
        }
        let post_reset_status = Self::hub_port_status_with_retry(
            device,
            hub_interface_number,
            port,
            "disconnected-post-reset",
        );
        if let Some(status) = post_reset_status {
            Self::log_hub_port_status(device.slot_id(), port, "disconnected-post-reset", status);
            Self::clear_hub_port_change_bits(
                device,
                hub_interface_number,
                port,
                status,
                hub_protocol_code,
            );
            if status.connected() {
                Self::log_hub_port_exact_fault(
                    device.slot_id(),
                    hub_interface_number,
                    port,
                    "recovered-after-force-reset",
                    status,
                    hub_power_mode,
                );
                return true;
            }
        } else {
            Self::log_hub_port_status_unavailable(
                device.slot_id(),
                port,
                "disconnected-post-reset",
            );
        }

        let (post_power_raw_status, post_power_raw_change) = post_power_status
            .map(|status| (status.status, status.change))
            .unwrap_or((0xffff, 0xffff));
        let (post_reset_raw_status, post_reset_raw_change) = post_reset_status
            .map(|status| (status.status, status.change))
            .unwrap_or((0xffff, 0xffff));
        let mut line = heapless::String::<384>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] hub exact-fault slot={} iface={} port={} reason=disconnected-stuck mode={} pre=0x{:04x}/0x{:04x} post_power=0x{:04x}/0x{:04x} post_reset=0x{:04x}/0x{:04x} power_kick_sent={} reset_sent={}",
                device.slot_id(),
                hub_interface_number,
                port,
                hub_power_mode,
                pre_status.status,
                pre_status.change,
                post_power_raw_status,
                post_power_raw_change,
                post_reset_raw_status,
                post_reset_raw_change,
                power_kick_sent as u8,
                reset_sent as u8,
            ),
        );
        boot_log::force_uart_line(line.as_str());
        false
    }

    fn hub_power_on_ports(
        device: &UsbDevice<SeatDma>,
        max_ports: usize,
        hub_interface_number: u8,
        power_mode: u8,
        pwr_on_2_pwr_good: u8,
    ) {
        let mut powered_ports = 0usize;
        let mut power_short_circuit = false;
        // Eager per-port power can stall some downstream hubs during bring-up.
        // Default to deferred power and keep this path behind an explicit opt-in.
        if Self::hub_should_eager_port_power(power_mode) {
            for downstream in 1..=max_ports {
                let port = downstream as u8;
                if Self::hub_set_feature_with_retry(
                    device,
                    hub_interface_number,
                    port,
                    hub_feature::PORT_POWER,
                    "set-power",
                    HUB_SET_FEATURE_RETRIES,
                ) {
                    powered_ports = powered_ports.saturating_add(1);
                    continue;
                }
                power_short_circuit = true;
                let skipped_ports = max_ports.saturating_sub(downstream);
                let mut line = heapless::String::<256>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] hub power control short-circuit slot={} iface={} mode={} failed_port={} accepted_ports={} skipped_ports={}",
                        device.slot_id(),
                        hub_interface_number,
                        power_mode,
                        port,
                        powered_ports,
                        skipped_ports
                    ),
                );
                boot_log::force_uart_line(line.as_str());
                break;
            }
        } else {
            let reason = match power_mode {
                0 => "ganged-port-power-deferred",
                1 => "individual-port-power-deferred",
                _ => "no-port-power-switching",
            };
            let mut line = heapless::String::<224>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] hub power control skipped slot={} iface={} mode={} reason={}",
                    device.slot_id(),
                    hub_interface_number,
                    power_mode,
                    reason
                ),
            );
            boot_log::force_uart_line(line.as_str());
        }

        let mut line = heapless::String::<224>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] hub power control slot={} iface={} mode={} requested_ports={} accepted_ports={}",
                device.slot_id(),
                hub_interface_number,
                power_mode,
                max_ports,
                powered_ports
            ),
        );
        boot_log::force_uart_line(line.as_str());
        if power_short_circuit {
            let mut line = heapless::String::<224>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] hub power control fallback slot={} iface={} mode={} detail=continue-without-full-port-power",
                    device.slot_id(),
                    hub_interface_number,
                    power_mode
                ),
            );
            boot_log::force_uart_line(line.as_str());
        }

        let pwr_good_ms = (pwr_on_2_pwr_good as u64).saturating_mul(2);
        wait_ms(cmp::max(pwr_good_ms, HUB_POWER_SETTLE_MIN_MS));
    }

    fn hub_set_feature_with_retry(
        device: &UsbDevice<SeatDma>,
        hub_interface_number: u8,
        port: u8,
        feature: u16,
        stage: &str,
        attempts: usize,
    ) -> bool {
        let max_attempts = attempts.max(1);
        for attempt in 1..=max_attempts {
            let wait_spins =
                hub_retry_wait_spins(attempt, max_attempts, HUB_CONTROL_SLOW_TAIL_ATTEMPTS);
            match Self::hub_set_feature(device, hub_interface_number, feature, port, wait_spins) {
                Ok(()) => {
                    if attempt > 1 {
                        let mut line = heapless::String::<224>::new();
                        let _ = core::fmt::Write::write_fmt(
                            &mut line,
                            format_args!(
                                "[local-seat] hub ctl retry-ok slot={} iface={} port={} stage={} feature=0x{:04x} attempt={}/{} wait={}",
                                device.slot_id(),
                                hub_interface_number,
                                port,
                                stage,
                                feature,
                                attempt,
                                max_attempts,
                                if wait_spins == HUB_CLASS_CONTROL_WAIT_SPINS_FAST {
                                    "fast"
                                } else {
                                    "slow"
                                }
                            ),
                        );
                        boot_log::force_uart_line(line.as_str());
                    }
                    return true;
                }
                Err(err) => {
                    Self::log_hub_control_error(
                        device.slot_id(),
                        hub_interface_number,
                        port,
                        feature,
                        stage,
                        err,
                    );
                    if attempt < max_attempts {
                        wait_ms(HUB_SET_FEATURE_RETRY_DELAY_MS);
                    }
                }
            }
        }
        false
    }

    fn hub_prepare_port_without_status(
        device: &UsbDevice<SeatDma>,
        hub_interface_number: u8,
        hub_protocol_code: u8,
        hub_power_mode: u8,
        pwr_on_2_pwr_good: u8,
        port: u8,
        reset_feature: u16,
    ) -> bool {
        let mut prepared = false;
        if Self::hub_should_power_port_during_blind_prepare(hub_power_mode)
            && Self::hub_set_feature_with_retry(
                device,
                hub_interface_number,
                port,
                hub_feature::PORT_POWER,
                "blind-set-power",
                1,
            )
        {
            prepared = true;
            wait_ms(Self::hub_post_power_wait_ms(
                hub_power_mode,
                pwr_on_2_pwr_good,
            ));
        }

        if Self::hub_set_feature_with_retry(
            device,
            hub_interface_number,
            port,
            reset_feature,
            "blind-reset",
            HUB_BLIND_PREPARE_RESET_RETRIES,
        ) {
            prepared = true;
            wait_ms(HUB_RESET_SETTLE_MS);
            let reset_change_feature = if hub_protocol_code == hub_protocol::SUPER_SPEED {
                hub_feature::C_BH_PORT_RESET
            } else {
                hub_feature::C_PORT_RESET
            };
            let _ =
                Self::hub_clear_feature(device, hub_interface_number, reset_change_feature, port);
        }

        let mut line = heapless::String::<224>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] hub blind-prepare slot={} iface={} port={} prepared={} reset_feature=0x{:04x}",
                device.slot_id(),
                hub_interface_number,
                port,
                prepared as u8,
                reset_feature
            ),
        );
        boot_log::force_uart_line(line.as_str());
        prepared
    }

    fn hub_clear_feature(
        device: &UsbDevice<SeatDma>,
        hub_interface_number: u8,
        feature: u16,
        port: u8,
    ) -> bool {
        let (candidates, candidate_count) =
            Self::hub_port_index_candidates(hub_interface_number, port);
        let primary_index = candidates[0];
        let primary_setup =
            SetupPacket::new(0x23, request::CLEAR_FEATURE, feature, primary_index, 0);
        let primary_err = match device.control_transfer_with_wait_spins(
            &primary_setup,
            None,
            HUB_CLASS_CONTROL_WAIT_SPINS,
        ) {
            Ok(_) => return true,
            Err(err) => err,
        };
        if candidate_count <= 1 || !Self::should_try_hub_index_fallback(primary_err) {
            return false;
        }
        for index in candidates.iter().take(candidate_count).copied().skip(1) {
            let setup = SetupPacket::new(0x23, request::CLEAR_FEATURE, feature, index, 0);
            if device
                .control_transfer_with_wait_spins(&setup, None, HUB_CLASS_CONTROL_WAIT_SPINS)
                .is_ok()
            {
                let mut line = heapless::String::<224>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] hub ctl fallback slot={} iface={} port={} stage=clear-feature wIndex=0x{:04x}",
                        device.slot_id(),
                        hub_interface_number,
                        port,
                        index
                    ),
                );
                boot_log::force_uart_line(line.as_str());
                return true;
            }
        }
        false
    }

    fn hub_port_status_read(
        device: &UsbDevice<SeatDma>,
        hub_interface_number: u8,
        port: u8,
        wait_spins: usize,
    ) -> Result<HubPortStatus, HubPortStatusReadError> {
        let (candidates, candidate_count) =
            Self::hub_port_index_candidates(hub_interface_number, port);
        let mut bytes = [0u8; HUB_PORT_STATUS_BYTES];
        let primary_index = candidates[0];
        let primary_setup = SetupPacket::new(
            0xA3,
            request::GET_STATUS,
            0,
            primary_index,
            HUB_PORT_STATUS_BYTES as u16,
        );
        let mut primary_control_err = None;
        let mut first_short_transfer = None;
        match device.control_transfer_with_wait_spins(&primary_setup, Some(&mut bytes), wait_spins)
        {
            Ok(transferred) => {
                if transferred >= HUB_PORT_STATUS_BYTES {
                    return Ok(HubPortStatus {
                        status: u16::from_le_bytes([bytes[0], bytes[1]]),
                        change: u16::from_le_bytes([bytes[2], bytes[3]]),
                    });
                }
                first_short_transfer = Some(transferred);
            }
            Err(err) => {
                primary_control_err = Some(err);
            }
        }

        let allow_fallback = if candidate_count <= 1 {
            false
        } else if let Some(err) = primary_control_err {
            Self::should_try_hub_index_fallback(err)
        } else {
            true
        };
        if !allow_fallback {
            if let Some(transferred) = first_short_transfer {
                return Err(HubPortStatusReadError::ShortTransfer { transferred, bytes });
            }
            if let Some(err) = primary_control_err {
                return Err(HubPortStatusReadError::Control(err));
            }
            return Err(HubPortStatusReadError::Control(UsbError::Stall));
        }

        let mut fallback_control_err = None;
        for index in candidates.iter().take(candidate_count).copied().skip(1) {
            let setup = SetupPacket::new(
                0xA3,
                request::GET_STATUS,
                0,
                index,
                HUB_PORT_STATUS_BYTES as u16,
            );
            match device.control_transfer_with_wait_spins(&setup, Some(&mut bytes), wait_spins) {
                Ok(transferred) => {
                    if transferred >= HUB_PORT_STATUS_BYTES {
                        let mut line = heapless::String::<256>::new();
                        let _ = core::fmt::Write::write_fmt(
                            &mut line,
                            format_args!(
                                "[local-seat] hub status fallback slot={} iface={} port={} wIndex=0x{:04x}",
                                device.slot_id(),
                                hub_interface_number,
                                port,
                                index
                            ),
                        );
                        boot_log::force_uart_line(line.as_str());
                        return Ok(HubPortStatus {
                            status: u16::from_le_bytes([bytes[0], bytes[1]]),
                            change: u16::from_le_bytes([bytes[2], bytes[3]]),
                        });
                    }
                    if first_short_transfer.is_none() {
                        first_short_transfer = Some(transferred);
                    }
                }
                Err(err) => {
                    if fallback_control_err.is_none() {
                        fallback_control_err = Some(err);
                    }
                }
            }
        }
        if let Some(transferred) = first_short_transfer {
            return Err(HubPortStatusReadError::ShortTransfer { transferred, bytes });
        }
        let control_err = match fallback_control_err.or(primary_control_err) {
            Some(err) => err,
            None => UsbError::Stall,
        };
        Err(HubPortStatusReadError::Control(control_err))
    }

    fn hub_port_status(
        device: &UsbDevice<SeatDma>,
        hub_interface_number: u8,
        port: u8,
    ) -> Option<HubPortStatus> {
        Self::hub_port_status_read(
            device,
            hub_interface_number,
            port,
            HUB_CLASS_CONTROL_WAIT_SPINS,
        )
        .ok()
    }

    fn hub_port_status_with_retry(
        device: &UsbDevice<SeatDma>,
        hub_interface_number: u8,
        port: u8,
        stage: &str,
    ) -> Option<HubPortStatus> {
        for attempt in 1..=HUB_PORT_STATUS_QUICK_RETRIES {
            let wait_spins = hub_retry_wait_spins(
                attempt,
                HUB_PORT_STATUS_QUICK_RETRIES,
                HUB_STATUS_SLOW_TAIL_ATTEMPTS,
            );
            match Self::hub_port_status_read(device, hub_interface_number, port, wait_spins) {
                Ok(status) => {
                    if HUB_VERBOSE_STATUS_RETRY_LOGS || attempt > 1 {
                        let raw = status.raw_bytes();
                        let mut line = heapless::String::<256>::new();
                        let _ = core::fmt::Write::write_fmt(
                            &mut line,
                            format_args!(
                                "[local-seat] hub status read slot={} iface={} port={} stage={} attempt={}/{} wait={} status=0x{:04x} change=0x{:04x} raw={:02x}{:02x}{:02x}{:02x}",
                                device.slot_id(),
                                hub_interface_number,
                                port,
                                stage,
                                attempt,
                                HUB_PORT_STATUS_QUICK_RETRIES,
                                if wait_spins == HUB_CLASS_CONTROL_WAIT_SPINS_FAST {
                                    "fast"
                                } else {
                                    "slow"
                                },
                                status.status,
                                status.change,
                                raw[0],
                                raw[1],
                                raw[2],
                                raw[3]
                            ),
                        );
                        boot_log::force_uart_line(line.as_str());
                    }
                    return Some(status);
                }
                Err(err) => {
                    if HUB_VERBOSE_STATUS_RETRY_LOGS || attempt == HUB_PORT_STATUS_QUICK_RETRIES {
                        Self::log_hub_status_read_error(
                            device.slot_id(),
                            hub_interface_number,
                            port,
                            stage,
                            attempt,
                            HUB_PORT_STATUS_QUICK_RETRIES,
                            err,
                        );
                    }
                }
            }
            if attempt < HUB_PORT_STATUS_QUICK_RETRIES {
                wait_ms(HUB_PORT_STATUS_QUICK_RETRY_DELAY_MS);
            }
        }
        None
    }

    fn wait_hub_port_ready(
        device: &UsbDevice<SeatDma>,
        hub_interface_number: u8,
        port: u8,
        hub_protocol_code: u8,
    ) -> Option<HubPortStatus> {
        let mut first_seen = None;
        let mut last_seen = None;
        let mut attempts_executed = 0usize;
        for attempt in 0..HUB_PORT_STATUS_RETRY_LOOPS {
            attempts_executed = attempt.saturating_add(1);
            let fast_phase = attempt < HUB_WAIT_READY_FAST_LOOPS;
            let wait_spins = if fast_phase {
                HUB_CLASS_CONTROL_WAIT_SPINS_FAST
            } else {
                HUB_CLASS_CONTROL_WAIT_SPINS
            };
            if let Some(status) =
                Self::hub_port_status_read(device, hub_interface_number, port, wait_spins).ok()
            {
                if first_seen.is_none() {
                    first_seen = Some(status);
                }
                last_seen = Some(status);
                let ready = if hub_protocol_code == hub_protocol::SUPER_SPEED {
                    status.connected() && !status.reset()
                } else {
                    status.connected() && status.enabled() && !status.reset()
                };
                if ready {
                    let mut line = heapless::String::<224>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut line,
                        format_args!(
                            "[local-seat] hub wait ready slot={} iface={} port={} attempt={}/{} status=0x{:04x} change=0x{:04x}",
                            device.slot_id(),
                            hub_interface_number,
                            port,
                            attempt + 1,
                            HUB_PORT_STATUS_RETRY_LOOPS,
                            status.status,
                            status.change
                        ),
                    );
                    boot_log::force_uart_line(line.as_str());
                    return Some(status);
                }
            }
            if fast_phase {
                if first_seen.is_none() && attempts_executed >= HUB_WAIT_READY_FAST_LOOPS {
                    break;
                }
                wait_ms(HUB_PORT_STATUS_RETRY_DELAY_MS_FAST);
            } else {
                wait_ms(HUB_PORT_STATUS_RETRY_DELAY_MS);
            }
        }
        let mut line = heapless::String::<256>::new();
        match (first_seen, last_seen) {
            (Some(first), Some(last)) => {
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] hub wait timeout slot={} iface={} port={} attempts={} first=0x{:04x}/0x{:04x} last=0x{:04x}/0x{:04x}",
                        device.slot_id(),
                        hub_interface_number,
                        port,
                        attempts_executed,
                        first.status,
                        first.change,
                        last.status,
                        last.change
                    ),
                );
            }
            _ => {
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] hub wait timeout slot={} iface={} port={} attempts={} detail=no-status",
                        device.slot_id(),
                        hub_interface_number,
                        port,
                        attempts_executed
                    ),
                );
            }
        }
        boot_log::force_uart_line(line.as_str());
        None
    }

    fn log_hub_port_status(slot_id: u8, port: u8, stage: &str, status: HubPortStatus) {
        let mut line = heapless::String::<256>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] hub port slot={} port={} stage={} status=0x{:04x} change=0x{:04x} conn={} ena={} rst={} pwr={} ls={} hs={}",
                slot_id,
                port,
                stage,
                status.status,
                status.change,
                status.connected() as u8,
                status.enabled() as u8,
                status.reset() as u8,
                status.powered() as u8,
                status.low_speed() as u8,
                status.high_speed() as u8
            ),
        );
        boot_log::force_uart_line(line.as_str());
    }

    fn log_hub_port_status_unavailable(slot_id: u8, port: u8, stage: &str) {
        let mut line = heapless::String::<192>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] hub port slot={} port={} stage={} status=unavailable",
                slot_id, port, stage
            ),
        );
        boot_log::force_uart_line(line.as_str());
    }

    fn log_hub_port_terminal(slot_id: u8, port: u8, reason: &str) {
        let mut line = heapless::String::<192>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] hub port terminal slot={} port={} reason={}",
                slot_id, port, reason
            ),
        );
        boot_log::force_uart_line(line.as_str());
    }

    fn log_hub_port_exact_fault(
        slot_id: u8,
        hub_interface_number: u8,
        port: u8,
        reason: &str,
        status: HubPortStatus,
        hub_power_mode: u8,
    ) {
        let mut line = heapless::String::<288>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] hub exact-fault slot={} iface={} port={} reason={} mode={} status=0x{:04x} change=0x{:04x} conn={} ena={} rst={} pwr={}",
                slot_id,
                hub_interface_number,
                port,
                reason,
                hub_power_mode,
                status.status,
                status.change,
                status.connected() as u8,
                status.enabled() as u8,
                status.reset() as u8,
                status.powered() as u8,
            ),
        );
        boot_log::force_uart_line(line.as_str());
    }

    fn log_hub_status_read_error(
        slot_id: u8,
        hub_interface_number: u8,
        port: u8,
        stage: &str,
        attempt: usize,
        max_attempts: usize,
        err: HubPortStatusReadError,
    ) {
        let w_index = Self::hub_port_index(port);
        let mut line = heapless::String::<320>::new();
        match err {
            HubPortStatusReadError::Control(control_err) => {
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] hub status read fail slot={} iface={} port={} stage={} attempt={}/{} req=(bm=0xa3,b=0x{:02x},wValue=0x0000,wIndex=0x{:04x},wLen=0x{:04x}) detail={control_err:?}",
                        slot_id,
                        hub_interface_number,
                        port,
                        stage,
                        attempt,
                        max_attempts,
                        request::GET_STATUS,
                        w_index,
                        HUB_PORT_STATUS_BYTES as u16
                    ),
                );
            }
            HubPortStatusReadError::ShortTransfer { transferred, bytes } => {
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] hub status read fail slot={} iface={} port={} stage={} attempt={}/{} req=(bm=0xa3,b=0x{:02x},wValue=0x0000,wIndex=0x{:04x},wLen=0x{:04x}) detail=short-transfer got={} raw={:02x}{:02x}{:02x}{:02x}",
                        slot_id,
                        hub_interface_number,
                        port,
                        stage,
                        attempt,
                        max_attempts,
                        request::GET_STATUS,
                        w_index,
                        HUB_PORT_STATUS_BYTES as u16,
                        transferred,
                        bytes[0],
                        bytes[1],
                        bytes[2],
                        bytes[3]
                    ),
                );
            }
        }
        boot_log::force_uart_line(line.as_str());
    }

    fn log_hub_control_error(
        slot_id: u8,
        hub_interface_number: u8,
        port: u8,
        feature: u16,
        stage: &str,
        err: UsbError,
    ) {
        let mut line = heapless::String::<224>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] hub ctl fail slot={} iface={} port={} stage={} feature=0x{:04x} detail={err:?}",
                slot_id, hub_interface_number, port, stage, feature
            ),
        );
        boot_log::force_uart_line(line.as_str());
    }

    fn clear_hub_port_change_bits(
        device: &UsbDevice<SeatDma>,
        hub_interface_number: u8,
        port: u8,
        status: HubPortStatus,
        hub_protocol_code: u8,
    ) {
        if (status.change & (1 << 0)) != 0 {
            let _ = Self::hub_clear_feature(
                device,
                hub_interface_number,
                hub_feature::C_PORT_CONNECTION,
                port,
            );
        }
        if (status.change & (1 << 1)) != 0 {
            let _ = Self::hub_clear_feature(
                device,
                hub_interface_number,
                hub_feature::C_PORT_ENABLE,
                port,
            );
        }
        if (status.change & (1 << 2)) != 0 {
            let _ = Self::hub_clear_feature(
                device,
                hub_interface_number,
                hub_feature::C_PORT_SUSPEND,
                port,
            );
        }
        if (status.change & (1 << 3)) != 0 {
            let _ = Self::hub_clear_feature(
                device,
                hub_interface_number,
                hub_feature::C_PORT_OVER_CURRENT,
                port,
            );
        }
        if (status.change & (1 << 4)) != 0 {
            let reset_change_feature = if hub_protocol_code == hub_protocol::SUPER_SPEED {
                hub_feature::C_BH_PORT_RESET
            } else {
                hub_feature::C_PORT_RESET
            };
            let _ =
                Self::hub_clear_feature(device, hub_interface_number, reset_change_feature, port);
        }
        if hub_protocol_code == hub_protocol::SUPER_SPEED && (status.change & (1 << 5)) != 0 {
            let _ = Self::hub_clear_feature(
                device,
                hub_interface_number,
                hub_feature::C_PORT_LINK_STATE,
                port,
            );
        }
        if hub_protocol_code == hub_protocol::SUPER_SPEED && (status.change & (1 << 6)) != 0 {
            let _ = Self::hub_clear_feature(
                device,
                hub_interface_number,
                hub_feature::C_PORT_CONFIG_ERROR,
                port,
            );
        }
    }

    fn speed_from_hub_port_status(status: HubPortStatus, hub_protocol_code: u8) -> u8 {
        if hub_protocol_code == hub_protocol::SUPER_SPEED {
            usb_oxide::regs::SPEED_SUPER
        } else if status.high_speed() {
            usb_oxide::regs::SPEED_HIGH
        } else if status.low_speed() {
            usb_oxide::regs::SPEED_LOW
        } else {
            usb_oxide::regs::SPEED_FULL
        }
    }

    fn poll_bytes(&mut self, out: &mut [u8]) -> usize {
        if out.is_empty() {
            return 0;
        }

        let report = match self.hid.poll_keyboard_checked() {
            Ok(Some(report)) => {
                self.poll_error_logged = false;
                if !self.first_report_logged {
                    let mut line = heapless::String::<224>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut line,
                        format_args!(
                            "[local-seat] usb hid first report shift={} keys={:02x},{:02x},{:02x},{:02x},{:02x},{:02x}",
                            report.shift() as u8,
                            report.keys[0],
                            report.keys[1],
                            report.keys[2],
                            report.keys[3],
                            report.keys[4],
                            report.keys[5]
                        ),
                    );
                    boot_log::force_uart_line(line.as_str());
                    self.first_report_logged = true;
                }
                report
            }
            Ok(None) => return 0,
            Err(err) => {
                if !self.poll_error_logged {
                    let mut line = heapless::String::<192>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut line,
                        format_args!(
                            "[local-seat] pi4 keyboard read queue failed detail=usb-queue-read err={err:?}"
                        ),
                    );
                    boot_log::force_uart_line(line.as_str());
                    self.poll_error_logged = true;
                }
                return 0;
            }
        };

        let shift = report.shift();
        let mut written = 0usize;
        for key in report.keys {
            if key == 0 || self.last_keys.contains(&key) {
                continue;
            }
            let scroll_delta = keyboard_display_scroll_delta_for_key(key);
            if scroll_delta != 0 {
                self.pending_display_scroll_rows = self
                    .pending_display_scroll_rows
                    .saturating_add(scroll_delta);
                continue;
            }
            if key == scancode::CAPS_LOCK {
                self.caps_lock_on = !self.caps_lock_on;
                let leds = if self.caps_lock_on { led::CAPS_LOCK } else { 0 };
                if let Err(err) = self.hid.set_leds(leds) {
                    if !self.led_error_logged {
                        let mut line = heapless::String::<224>::new();
                        let _ = core::fmt::Write::write_fmt(
                            &mut line,
                            format_args!(
                                "[local-seat] pi4 keyboard led sync failed caps={} detail={err:?}",
                                self.caps_lock_on as u8
                            ),
                        );
                        boot_log::force_uart_line(line.as_str());
                        self.led_error_logged = true;
                    }
                } else {
                    self.led_error_logged = false;
                }
                continue;
            }
            if let Some(ch) = keyboard_scancode_to_char(key, shift) {
                if written >= out.len() {
                    break;
                }
                let mut effective = ch;
                if self.caps_lock_on && effective.is_ascii_alphabetic() {
                    effective = if shift {
                        effective.to_ascii_lowercase()
                    } else {
                        effective.to_ascii_uppercase()
                    };
                }
                out[written] = effective as u8;
                written = written.saturating_add(1);
            }
        }

        self.last_keys = report.keys;
        written
    }

    fn take_pending_display_scroll_rows(&mut self) -> i8 {
        let delta = self.pending_display_scroll_rows;
        self.pending_display_scroll_rows = 0;
        delta
    }
}

struct SeatDma {
    state: Mutex<SeatDmaState>,
}

// SAFETY: The root-task event loop is single-threaded on this path and all
// interior mutability in `SeatDma` is synchronized by the internal `Mutex`.
unsafe impl Send for SeatDma {}

// SAFETY: Same reasoning as `Send`; callers only access mutable state through
// methods that lock `state`.
unsafe impl Sync for SeatDma {}

struct SeatDmaState {
    hal_ptr: usize,
    prefer_high: bool,
    pcie_dma_window: bool,
    sealed: bool,
    regions: Vec<PhysRegion>,
}

enum RegionBacking {
    Dma(Vec<crate::sel4::RamFrame>),
    Mmio(Vec<crate::sel4::DeviceFrame>),
}

struct PhysRegion {
    virt_start: usize,
    phys_start: usize,
    length: usize,
    size: usize,
    align: usize,
    backing: RegionBacking,
}

impl SeatDma {
    fn new(hal: &mut KernelHal<'_>, prefer_high: bool, pcie_dma_window: bool) -> Self {
        Self {
            state: Mutex::new(SeatDmaState {
                hal_ptr: hal as *mut _ as usize,
                prefer_high,
                pcie_dma_window,
                sealed: false,
                regions: Vec::new(),
            }),
        }
    }

    fn seal_runtime(&self) {
        let mut state = self.state.lock();
        state.sealed = true;
        state.hal_ptr = 0;
    }

    fn alloc_dma_locked(state: &mut SeatDmaState, size: usize, align: usize) -> Option<usize> {
        if state.sealed || size == 0 {
            let mut line = heapless::String::<176>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] xhci dma alloc reject size=0x{size:x} align=0x{align:x} reason={}",
                    if state.sealed { "sealed" } else { "zero-size" }
                ),
            );
            boot_log::force_uart_line(line.as_str());
            return None;
        }
        if size > XHCI_DMA_MAX_BYTES {
            let mut line = heapless::String::<192>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] xhci dma alloc reject size=0x{size:x} align=0x{align:x} reason=too-large limit=0x{limit:x}",
                    limit = XHCI_DMA_MAX_BYTES
                ),
            );
            boot_log::force_uart_line(line.as_str());
            return None;
        }
        if !align.is_power_of_two() {
            let mut line = heapless::String::<176>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] xhci dma alloc reject size=0x{size:x} align=0x{align:x} reason=bad-align"
                ),
            );
            boot_log::force_uart_line(line.as_str());
            return None;
        }

        let page_count = div_ceil(size, PAGE_SIZE);
        let hal = hal_from_ptr(state.hal_ptr)?;
        if XHCI_DMA_VERBOSE_LOGS {
            let mut begin_line = heapless::String::<208>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut begin_line,
                format_args!(
                    "[local-seat] xhci dma alloc begin size=0x{size:x} align=0x{align:x} pages={} mode={}",
                    page_count,
                    if state.prefer_high { "high" } else { "low" }
                ),
            );
            boot_log::force_uart_line(begin_line.as_str());
        }

        let mut frames = Vec::with_capacity(page_count);
        let mut expected_phys = 0usize;
        let mut expected_virt = 0usize;
        for idx in 0..page_count {
            if XHCI_DMA_VERBOSE_LOGS {
                let mut req_line = heapless::String::<192>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut req_line,
                    format_args!(
                        "[local-seat] xhci dma frame request idx={} pages={} mode={}",
                        idx,
                        page_count,
                        if state.prefer_high { "high" } else { "low" }
                    ),
                );
                boot_log::force_uart_line(req_line.as_str());
            }
            let frame = match if state.prefer_high {
                hal.alloc_dma_frame_attr(sel4_sys::seL4_ARM_Page_Uncached)
            } else {
                hal.alloc_dma_frame_low_attr(sel4_sys::seL4_ARM_Page_Uncached)
            } {
                Ok(frame) => frame,
                Err(err) => {
                    let mut line = heapless::String::<224>::new();
                    match err {
                        crate::hal::HalError::Sel4(sel4_err) => {
                            let _ = core::fmt::Write::write_fmt(
                                &mut line,
                                format_args!(
                                    "[local-seat] xhci dma frame alloc failed idx={} mode={} err={} ({})",
                                    idx,
                                    if state.prefer_high { "high" } else { "low" },
                                    sel4_err,
                                    crate::sel4::error_name(sel4_err),
                                ),
                            );
                        }
                        _ => {
                            let _ = core::fmt::Write::write_fmt(
                                &mut line,
                                format_args!(
                                    "[local-seat] xhci dma frame alloc failed idx={} mode={} err={err:?}",
                                    idx,
                                    if state.prefer_high { "high" } else { "low" },
                                ),
                            );
                        }
                    }
                    boot_log::force_uart_line(line.as_str());
                    return None;
                }
            };
            let phys = frame.paddr();
            let virt = frame.ptr().as_ptr() as usize;
            if XHCI_DMA_VERBOSE_LOGS {
                let mut got_line = heapless::String::<224>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut got_line,
                    format_args!(
                        "[local-seat] xhci dma frame ready idx={} paddr=0x{phys:016x} vaddr=0x{virt:016x}",
                        idx
                    ),
                );
                boot_log::force_uart_line(got_line.as_str());
            }
            if phys >= RPI4_PCIE_DMA_LIMIT {
                if !USB_DMA_RANGE_WARNED.swap(true, Ordering::AcqRel) {
                    let mut line = heapless::String::<192>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut line,
                        format_args!(
                            "[local-seat] xhci dma outside pcie window paddr=0x{phys:016x} limit=0x{limit:08x}",
                            limit = RPI4_PCIE_DMA_LIMIT
                        ),
                    );
                    boot_log::force_uart_line(line.as_str());
                }
                return None;
            }
            if idx == 0 {
                expected_phys = phys;
                expected_virt = virt;
            } else {
                let off = idx.checked_mul(PAGE_SIZE)?;
                let next_phys = expected_phys.checked_add(off)?;
                let next_virt = expected_virt.checked_add(off)?;
                if phys != next_phys || virt != next_virt {
                    let mut line = heapless::String::<224>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut line,
                        format_args!(
                            "[local-seat] xhci dma frame reject idx={} reason=non-contiguous got_paddr=0x{phys:016x} want_paddr=0x{want_phys:016x} got_vaddr=0x{virt:016x} want_vaddr=0x{want_virt:016x}",
                            idx,
                            want_phys = next_phys,
                            want_virt = next_virt,
                        ),
                    );
                    boot_log::force_uart_line(line.as_str());
                    return None;
                }
            }
            frames.push(frame);
        }

        if (expected_virt & (align - 1)) != 0 {
            let mut line = heapless::String::<208>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] xhci dma alloc reject size=0x{size:x} align=0x{align:x} vaddr=0x{vaddr:016x} reason=unaligned",
                    vaddr = expected_virt
                ),
            );
            boot_log::force_uart_line(line.as_str());
            return None;
        }

        state.regions.push(PhysRegion {
            virt_start: expected_virt,
            phys_start: expected_phys,
            length: page_count.checked_mul(PAGE_SIZE)?,
            size,
            align,
            backing: RegionBacking::Dma(frames),
        });
        if XHCI_DMA_VERBOSE_LOGS {
            let mut done_line = heapless::String::<224>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut done_line,
                format_args!(
                    "[local-seat] xhci dma alloc done size=0x{size:x} align=0x{align:x} pages={} paddr=0x{paddr:016x} vaddr=0x{vaddr:016x}",
                    page_count,
                    paddr = expected_phys,
                    vaddr = expected_virt
                ),
            );
            boot_log::force_uart_line(done_line.as_str());
        }
        Some(expected_virt)
    }

    fn map_mmio_locked(state: &mut SeatDmaState, phys: usize, size: usize) -> Option<usize> {
        if state.sealed || size == 0 {
            if !XHCI_MMIO_DIAG_LOGGED.swap(true, Ordering::AcqRel) {
                let mut line = heapless::String::<192>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] xhci mmio map reject phys=0x{phys:016x} size=0x{size:06x} reason={}",
                        if state.sealed { "sealed" } else { "zero-size" }
                    ),
                );
                boot_log::force_uart_line(line.as_str());
            }
            return None;
        }
        if size > XHCI_MMIO_MAX_BYTES {
            let mut line = heapless::String::<208>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] xhci mmio map reject phys=0x{phys:016x} size=0x{size:06x} reason=too-large limit=0x{limit:06x}",
                    limit = XHCI_MMIO_MAX_BYTES
                ),
            );
            boot_log::force_uart_line(line.as_str());
            return None;
        }
        if let Some(mapped) = pinned_xhci_window_lookup(phys, size) {
            if !XHCI_MMIO_PIN_REUSE_LOGGED.swap(true, Ordering::AcqRel) {
                let mut line = heapless::String::<224>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] xhci mmio map reuse phys=0x{phys:016x} size=0x{size:06x} virt=0x{virt:016x} source={}",
                        if pinned_xhci_window_lookup_trusted(phys, size).is_some() {
                            "pinned-trusted"
                        } else {
                            "pinned"
                        },
                        virt = mapped
                    ),
                );
                boot_log::force_uart_line(line.as_str());
            }
            return Some(mapped);
        }
        let request_end = phys.checked_add(size)?;
        if let Some(window) = PINNED_XHCI_MMIO.lock().as_ref() {
            let window_end = window.phys_start.checked_add(window.length)?;
            if phys >= window.phys_start && phys < window_end && request_end > window_end {
                let mut line = heapless::String::<240>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] xhci mmio pinned-insufficient phys=0x{phys:016x} size=0x{size:06x} pinned=[0x{start:016x}..0x{end:016x})",
                        start = window.phys_start,
                        end = window_end
                    ),
                );
                boot_log::force_uart_line(line.as_str());
            }
        }

        let mut extend_index: Option<usize> = None;
        for (index, region) in state.regions.iter().enumerate() {
            let RegionBacking::Mmio(_) = &region.backing else {
                continue;
            };
            let mapped_start = region.phys_start;
            let mapped_end = mapped_start.checked_add(region.length)?;
            if phys >= mapped_start && request_end <= mapped_end {
                let offset = phys.checked_sub(mapped_start)?;
                return region.virt_start.checked_add(offset);
            }
            if phys == mapped_start && request_end > mapped_end {
                extend_index = Some(index);
            }
        }

        if let Some(index) = extend_index {
            let hal = hal_from_ptr(state.hal_ptr)?;
            let region = state.regions.get_mut(index)?;
            let mapped_base_phys = region.phys_start & !PAGE_MASK;
            let mapped_base_virt = region
                .virt_start
                .checked_sub(region.phys_start & PAGE_MASK)?;
            let RegionBacking::Mmio(frames) = &mut region.backing else {
                return None;
            };

            let mut mapped_end =
                mapped_base_phys.checked_add(frames.len().checked_mul(PAGE_SIZE)?)?;
            while mapped_end < request_end {
                let frame = match hal.map_device(mapped_end) {
                    Ok(frame) => frame,
                    Err(_) => {
                        let mut line = heapless::String::<224>::new();
                        let _ = core::fmt::Write::write_fmt(
                            &mut line,
                            format_args!(
                                "[local-seat] xhci mmio extend failed base=0x{base:016x} need_end=0x{need:016x} fail_page=0x{page:016x}",
                                base = mapped_base_phys,
                                need = request_end,
                                page = mapped_end
                            ),
                        );
                        boot_log::force_uart_line(line.as_str());
                        return None;
                    }
                };
                let expected_virt =
                    mapped_base_virt.checked_add(frames.len().checked_mul(PAGE_SIZE)?)?;
                let actual_virt = frame.ptr().as_ptr() as usize;
                if actual_virt != expected_virt {
                    let mut line = heapless::String::<240>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut line,
                        format_args!(
                            "[local-seat] xhci mmio extend mismatch page=0x{page:016x} want_vaddr=0x{want:016x} got_vaddr=0x{got:016x}",
                            page = mapped_end,
                            want = expected_virt,
                            got = actual_virt
                        ),
                    );
                    boot_log::force_uart_line(line.as_str());
                    return None;
                }
                frames.push(frame);
                mapped_end = mapped_end.checked_add(PAGE_SIZE)?;
            }

            region.length = request_end.saturating_sub(region.phys_start);
            let offset = phys.checked_sub(region.phys_start)?;
            return region.virt_start.checked_add(offset);
        }

        let page_base = phys & !PAGE_MASK;
        let page_offset = phys & PAGE_MASK;
        let map_len = page_offset.checked_add(size)?;
        let page_count = div_ceil(map_len, PAGE_SIZE);
        let hal = hal_from_ptr(state.hal_ptr)?;

        let mut prefix_frames = Vec::new();
        let mut mapped_frames = Vec::with_capacity(page_count);
        let mut first_virt = 0usize;
        for idx in 0..page_count {
            let page_phys = page_base.checked_add(idx.checked_mul(PAGE_SIZE)?)?;
            let frame = map_device_exact(
                hal,
                page_phys,
                XHCI_MMIO_MAP_EXACT_ATTEMPT_CAP,
                "xhci mmio",
                Pi4SeatError::XhciInit,
                &mut prefix_frames,
            )
            .ok()?;
            let virt = frame.ptr().as_ptr() as usize;
            if idx == 0 {
                first_virt = virt;
            } else {
                let next_virt = first_virt.checked_add(idx.checked_mul(PAGE_SIZE)?)?;
                if virt != next_virt {
                    let mut line = heapless::String::<240>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut line,
                        format_args!(
                            "[local-seat] xhci mmio map mismatch page={} page_phys=0x{page_phys:016x} want_vaddr=0x{want:016x} got_vaddr=0x{got:016x}",
                            idx,
                            want = next_virt,
                            got = virt
                        ),
                    );
                    boot_log::force_uart_line(line.as_str());
                    return None;
                }
            }
            mapped_frames.push(frame);
        }

        let virt = first_virt.checked_add(page_offset)?;
        let mut frames = prefix_frames;
        frames.extend(mapped_frames);
        state.regions.push(PhysRegion {
            virt_start: virt,
            phys_start: phys,
            length: size,
            size,
            align: PAGE_SIZE,
            backing: RegionBacking::Mmio(frames),
        });
        Some(virt)
    }

    fn virt_to_phys_locked(state: &SeatDmaState, va: usize) -> usize {
        for region in &state.regions {
            let start = region.virt_start;
            let Some(end) = start.checked_add(region.length) else {
                continue;
            };
            if (start..end).contains(&va) {
                let offset = va - start;
                if let Some(phys) = region.phys_start.checked_add(offset) {
                    return match &region.backing {
                        RegionBacking::Dma(_) => {
                            if state.pcie_dma_window {
                                pcie_dma_bus_addr(phys).unwrap_or(phys)
                            } else {
                                phys
                            }
                        }
                        RegionBacking::Mmio(_) => phys,
                    };
                }
            }
        }
        va
    }

    fn share_for_device_locked(
        state: &SeatDmaState,
        vaddr: usize,
        len: usize,
        label: &'static str,
    ) -> Result<(), DmaShareError> {
        let end = vaddr.checked_add(len).ok_or(DmaShareError)?;
        for region in &state.regions {
            let RegionBacking::Dma(_) = &region.backing else {
                continue;
            };
            let start = region.virt_start;
            let Some(region_end) = start.checked_add(region.length) else {
                continue;
            };
            if vaddr < start || end > region_end {
                continue;
            }
            let offset = vaddr.checked_sub(start).ok_or(DmaShareError)?;
            let phys = region.phys_start.checked_add(offset).ok_or(DmaShareError)?;
            dma::pin(vaddr, phys, len, label).map_err(|_| DmaShareError)?;
            return Ok(());
        }
        Err(DmaShareError)
    }
}

impl Dma for SeatDma {
    unsafe fn alloc(&self, size: usize, align: usize) -> Option<usize> {
        let mut state = self.state.lock();
        Self::alloc_dma_locked(&mut state, size, align)
    }

    unsafe fn free(&self, addr: usize, size: usize, align: usize) {
        let mut state = self.state.lock();
        if state.sealed {
            return;
        }

        if let Some(index) = state.regions.iter().position(|region| {
            region.virt_start == addr && region.size == size && region.align == align
        }) {
            let region = state.regions.swap_remove(index);
            match region.backing {
                RegionBacking::Dma(frames) => {
                    let _ = frames.len();
                }
                RegionBacking::Mmio(frames) => {
                    let _ = frames.len();
                }
            }
        }
    }

    unsafe fn map_mmio(&self, phys: usize, size: usize) -> Option<usize> {
        let mut state = self.state.lock();
        Self::map_mmio_locked(&mut state, phys, size)
    }

    unsafe fn unmap_mmio(&self, _virt: usize, _size: usize) {
        // Mappings remain pinned for the lifetime of the backend.
    }

    fn virt_to_phys(&self, va: usize) -> usize {
        let state = self.state.lock();
        Self::virt_to_phys_locked(&state, va)
    }

    fn share_for_device(
        &self,
        vaddr: usize,
        len: usize,
        label: &'static str,
    ) -> Result<(), DmaShareError> {
        let state = self.state.lock();
        Self::share_for_device_locked(&state, vaddr, len, label)
    }

    fn page_size(&self) -> usize {
        PAGE_SIZE
    }
}

fn hal_from_ptr(ptr: usize) -> Option<&'static mut KernelHal<'static>> {
    if ptr == 0 {
        return None;
    }

    // SAFETY: `ptr` originates from a live `&mut KernelHal` during backend
    // construction and is only used before `seal_runtime` clears it.
    Some(unsafe { &mut *(ptr as *mut KernelHal<'static>) })
}

#[inline]
fn pci_cfg_read_u32(base: usize, offset: usize) -> u32 {
    let Some(addr) = base.checked_add(offset) else {
        return 0;
    };
    // SAFETY: `base` points to a mapped PCI config page in `prepare_vl805_pci`.
    unsafe { ptr::read_volatile(addr as *const u32) }
}

#[inline]
fn pci_cfg_read_u16(base: usize, offset: usize) -> u16 {
    let Some(addr) = base.checked_add(offset) else {
        return 0;
    };
    // SAFETY: `base` points to a mapped PCI config page in `prepare_vl805_pci`.
    unsafe { ptr::read_volatile(addr as *const u16) }
}

#[inline]
fn pci_cfg_read_u8(base: usize, offset: usize) -> u8 {
    let Some(addr) = base.checked_add(offset) else {
        return 0;
    };
    // SAFETY: `base` points to a mapped PCI config page in `prepare_vl805_pci`.
    unsafe { ptr::read_volatile(addr as *const u8) }
}

#[inline]
fn pci_cfg_write_u32(base: usize, offset: usize, value: u32) {
    let Some(addr) = base.checked_add(offset) else {
        return;
    };
    // SAFETY: `base` points to a mapped PCI config page in `prepare_vl805_pci`.
    unsafe {
        ptr::write_volatile(addr as *mut u32, value);
    }
}

#[inline]
fn pci_cfg_write_u16(base: usize, offset: usize, value: u16) {
    let Some(addr) = base.checked_add(offset) else {
        return;
    };
    // SAFETY: `base` points to a mapped PCI config page in `prepare_vl805_pci`.
    unsafe {
        ptr::write_volatile(addr as *mut u16, value);
    }
}

#[inline]
fn phys_to_bus(phys: usize, alias_base: u32) -> Option<u32> {
    if phys > VC_BUS_MASK as usize {
        return None;
    }
    Some((phys as u32 & VC_BUS_MASK) | alias_base)
}

#[inline]
fn pcie_dma_bus_addr(phys: usize) -> Option<usize> {
    if !XHCI_PCIE_DMA_WINDOW_ENABLED {
        return Some(phys);
    }
    if phys >= RPI4_PCIE_DMA_LIMIT {
        return None;
    }
    Some(phys)
}

#[inline]
fn bus_to_phys(bus: u32) -> usize {
    (bus & VC_BUS_MASK) as usize
}

#[inline]
fn framebuffer_phys_window_safe(fb_phys: usize, fb_size: usize) -> bool {
    let Some(fb_end) = fb_phys.checked_add(fb_size) else {
        return false;
    };
    fb_phys >= MIN_SAFE_FB_PHYS && fb_end <= MAX_SAFE_FB_PHYS_EXCL
}

#[inline]
const fn mailbox_visible_dimension(phys: usize, virt: usize) -> Option<usize> {
    if phys == 0 {
        if virt == 0 {
            None
        } else {
            Some(virt)
        }
    } else if virt == 0 {
        Some(phys)
    } else if phys < virt {
        Some(phys)
    } else {
        Some(virt)
    }
}

#[inline]
const fn clamp_visible_width(width: usize, pitch: usize) -> usize {
    if width == 0 || pitch == 0 {
        return 0;
    }
    let pitch_pixels = pitch / FB_BYTES_PER_PIXEL;
    if pitch_pixels == 0 {
        return 0;
    }
    if width > pitch_pixels {
        pitch_pixels
    } else {
        width
    }
}

#[inline]
const fn clamp_visible_height(height: usize, pitch: usize, fb_size: usize) -> usize {
    if height == 0 || pitch == 0 || fb_size == 0 {
        return 0;
    }
    let max_rows = fb_size / pitch;
    if max_rows == 0 {
        return 0;
    }
    if height > max_rows {
        max_rows
    } else {
        height
    }
}

#[inline]
const fn div_ceil(value: usize, divisor: usize) -> usize {
    if value == 0 {
        0
    } else {
        1 + ((value - 1) / divisor)
    }
}

#[inline]
fn append_route_segment(route: u32, downstream_port: u8) -> Option<u32> {
    let depth = route_depth(route) as u32;
    if depth >= 5 {
        return None;
    }
    let segment = cmp::min(downstream_port, 15) as u32;
    Some((route & 0x000f_ffff) | (segment << (depth * 4)))
}

#[inline]
fn route_depth(route: u32) -> u8 {
    let mut depth = 0u8;
    let mut rem = route & 0x000f_ffff;
    while rem != 0 {
        depth = depth.saturating_add(1);
        rem >>= 4;
    }
    depth
}

#[inline]
const fn hid_keyboard_attach_rank(interface_subclass: u8, interface_protocol: u8) -> Option<u8> {
    if interface_subclass == hid_subclass::BOOT && interface_protocol == hid_protocol::KEYBOARD {
        // Strict USB HID Boot keyboard interface (preferred).
        Some(0)
    } else if interface_protocol == hid_protocol::KEYBOARD {
        // Keyboard protocol advertised but not boot subclass.
        Some(1)
    } else if interface_protocol == hid_protocol::NONE {
        // Legacy/vendor keyboards may hide behind protocol NONE; attempt only
        // after explicit keyboard protocol candidates fail.
        Some(2)
    } else {
        None
    }
}

#[inline]
const fn hid_keyboard_attach_source(rank: u8) -> &'static str {
    match rank {
        0 => "boot-keyboard",
        1 => "keyboard-protocol",
        2 => "protocol-none-fallback",
        _ => "unknown",
    }
}

#[inline]
const fn hid_keyboard_candidate_requires_force_mode(rank: u8) -> bool {
    rank != 0
}

#[inline]
const fn text_backspace_target(row: usize, col: usize, cols: usize) -> (usize, usize) {
    if cols == 0 {
        return (0, 0);
    }
    if col > 0 {
        (row, col - 1)
    } else if row > 0 {
        (row - 1, cols - 1)
    } else {
        (0, 0)
    }
}

#[inline]
const fn text_row_count(height: usize) -> usize {
    let rows = height / CHAR_HEIGHT;
    if rows == 0 {
        1
    } else {
        rows
    }
}

#[inline]
const fn text_viewport_height(height: usize, rows: usize) -> usize {
    let viewport = rows.saturating_mul(CHAR_HEIGHT);
    if viewport > height {
        height
    } else {
        viewport
    }
}

#[inline]
fn keyboard_scancode_to_char(key: u8, shift: bool) -> Option<char> {
    if key == scancode::KP_ENTER {
        Some('\n')
    } else {
        scancode_to_ascii(key, shift)
    }
}

#[inline]
const fn keyboard_display_scroll_delta_for_key(key: u8) -> i8 {
    match key {
        scancode::UP_ARROW => 1,
        scancode::DOWN_ARROW => -1,
        _ => 0,
    }
}

#[inline]
const fn normalize_hub_tt_profile(
    downstream_port: u8,
    hub_multi_tt: bool,
    hub_tt_think_time: u8,
) -> (u8, u8) {
    // xHCI TT Port Number semantics:
    // - Single-TT hub: must be 1
    // - Multi-TT hub:  downstream port number
    let tt_port = if hub_multi_tt {
        if downstream_port == 0 {
            1
        } else {
            downstream_port
        }
    } else {
        1
    };
    let tt_ttt_raw = hub_tt_think_time & 0x03;
    let tt_ttt = if tt_ttt_raw > 2 { 2 } else { tt_ttt_raw };
    (tt_port, tt_ttt)
}

#[inline]
const fn hub_retry_wait_spins(
    attempt: usize,
    max_attempts: usize,
    slow_tail_attempts: usize,
) -> usize {
    if max_attempts <= 1 {
        return HUB_CLASS_CONTROL_WAIT_SPINS;
    }
    let slow_tail = if slow_tail_attempts >= max_attempts {
        max_attempts.saturating_sub(1)
    } else {
        slow_tail_attempts
    };
    if slow_tail == 0 {
        return HUB_CLASS_CONTROL_WAIT_SPINS_FAST;
    }
    let slow_start = max_attempts.saturating_sub(slow_tail).saturating_add(1);
    if attempt >= slow_start {
        HUB_CLASS_CONTROL_WAIT_SPINS
    } else {
        HUB_CLASS_CONTROL_WAIT_SPINS_FAST
    }
}

#[inline]
fn read_config_desc(config_blob: &[u8]) -> Option<ConfigDesc> {
    if config_blob.len() < mem::size_of::<ConfigDesc>() {
        return None;
    }
    // SAFETY: The descriptor bytes may be unaligned in the returned USB blob.
    Some(unsafe { ptr::read_unaligned(config_blob.as_ptr().cast::<ConfigDesc>()) })
}

#[inline]
fn config_value_for_set(config: ConfigDesc) -> Option<u8> {
    // USB SET_CONFIGURATION expects bConfigurationValue, not iConfiguration.
    let value = config.configuration_value();
    if value == 0 {
        None
    } else {
        Some(value)
    }
}

fn validate_framebuffer_geometry(
    width: usize,
    height: usize,
    pitch: usize,
    fb_size: usize,
) -> Option<usize> {
    if width == 0 || height == 0 || pitch == 0 || fb_size == 0 {
        return None;
    }
    if width > MAX_FB_WIDTH || height > MAX_FB_HEIGHT || fb_size > MAX_FB_BYTES {
        return None;
    }
    let min_pitch = width.checked_mul(FB_BYTES_PER_PIXEL)?;
    if pitch < min_pitch {
        return None;
    }
    let required = pitch.checked_mul(height)?;
    if required == 0 || required > fb_size || required > MAX_FB_BYTES {
        return None;
    }
    Some(required)
}

#[cfg(test)]
mod tests {
    use alloc::{string::String, vec::Vec};

    use super::{
        append_wrapped_scrollback_rows, clamp_visible_height, clamp_visible_width,
        config_value_for_set, decode_pci_mmio_bar, hid_keyboard_attach_rank,
        hid_keyboard_attach_source, hid_keyboard_candidate_requires_force_mode,
        hub_retry_wait_spins, hub_should_eager_port_power, keyboard_attach_retry_allowed,
        keyboard_display_scroll_delta_for_key, keyboard_scancode_to_char,
        mailbox_visible_dimension, normalize_hub_tt_profile, normalize_pi4_xhci_mmio_hint,
        parse_xhci_capbase, runtime_vl805_mailbox_reset_allows_trusted_cold_init,
        runtime_vl805_mailbox_reset_error_allows_cold_init, text_backspace_target, text_row_count,
        text_viewport_height, translate_bcm2711_soc_reg_addr, vl805_cfg_preseed_mode,
        vl805_cfg_preseed_needed, vl805_runtime_cfg_touch_allowed, xhci_connected_mask_from_portsc,
        xhci_controller_params_from_probe, xhci_controller_params_from_probe_with_strategy,
        xhci_diag_stage_label, xhci_diag_stage_value_labels, xhci_firmware_handoff_hint_reason,
        xhci_irq_sink_needed, xhci_preseed_allows_static_legacy_fallbacks,
        xhci_preseed_pin_only_reason, xhci_root_port_connected, xhci_runtime_init_strategies,
        xhci_runtime_init_strategy_policy_label, xhci_runtime_init_strategy_prompt_safe,
        xhci_runtime_mmio_candidate_allowed, xhci_runtime_mmio_has_accessible_window,
        xhci_safe_mode_skip_command, ConfigDesc, LocalSeatXhciStopStateSnapshot, Pi4SeatError,
        UsbKeyboard, UsbProbePathOutcome, UsbProbePathProgress, UsbProbePathwaySummary,
        Vl805CfgPreseedMode, XhciCapProbe, XhciDiagSnapshot, XhciFirmwareHandoff,
        XhciIrqInstallPhase, XhciRuntimeInitStrategy, HUB_PORT_IFACE_FALLBACK_MAX,
        RPI4_XHCI_MMIO_HIGH_CANDIDATE, RPI4_XHCI_MMIO_PRIMARY_CANDIDATE,
        XHCI_MMIO_ALIAS_SCAN_STEPS,
    };
    use super::{
        hid_protocol, hid_subclass, scancode, CHAR_HEIGHT, HUB_CLASS_CONTROL_WAIT_SPINS,
        HUB_CLASS_CONTROL_WAIT_SPINS_FAST, RPI4_XHCI_MMIO_SECONDARY_CANDIDATE,
    };

    #[test]
    fn decode_pci_mmio_bar_rejects_io_bar() {
        assert_eq!(decode_pci_mmio_bar(0x0000_0001, 0), None);
    }

    #[test]
    fn decode_pci_mmio_bar_decodes_32bit_bar() {
        assert_eq!(decode_pci_mmio_bar(0xFE98_0000, 0), Some(0xFE98_0000));
    }

    #[test]
    fn decode_pci_mmio_bar_decodes_64bit_bar() {
        let low = 0x0000_0004;
        let high = 0x0000_0006;
        assert_eq!(decode_pci_mmio_bar(low, high), Some(0x0000_0006_0000_0000));
    }

    #[test]
    fn parse_xhci_capbase_extracts_caplen_and_version() {
        assert_eq!(parse_xhci_capbase(0x0010_0040), (0x40, 0x0010));
    }

    #[test]
    fn xhci_root_port_connected_uses_ccs_bit() {
        assert!(!xhci_root_port_connected(0));
        assert!(xhci_root_port_connected(usb_oxide::regs::PORTSC_CCS));
        assert!(xhci_root_port_connected(
            usb_oxide::regs::PORTSC_CCS | usb_oxide::regs::PORTSC_PED
        ));
    }

    #[test]
    fn xhci_connected_mask_from_portsc_sets_bits_for_connected_ports_only() {
        let statuses = [
            usb_oxide::regs::PORTSC_CCS,
            0,
            usb_oxide::regs::PORTSC_CCS | usb_oxide::regs::PORTSC_PED,
        ];
        assert_eq!(xhci_connected_mask_from_portsc(&statuses), 0b0101);
    }

    #[test]
    fn keyboard_attach_retry_stops_after_terminal_xhci_failure() {
        assert!(keyboard_attach_retry_allowed(
            Pi4SeatError::UsbKeyboardInit,
            1,
            2
        ));
        assert!(!keyboard_attach_retry_allowed(Pi4SeatError::XhciInit, 1, 2));
        assert!(!keyboard_attach_retry_allowed(
            Pi4SeatError::UsbKeyboardInit,
            2,
            2
        ));
    }

    #[test]
    fn prompt_safe_runtime_init_strategy_allows_resetless_stop_seed_start() {
        assert!(xhci_runtime_init_strategy_prompt_safe(
            XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::None, true),
        ));
        assert!(xhci_runtime_init_strategy_prompt_safe(
            XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::ColdStartFromSnapshot, true),
        ));
        assert!(xhci_runtime_init_strategy_prompt_safe(
            XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::ColdStartFromSnapshot, false),
        ));
        assert!(xhci_runtime_init_strategy_prompt_safe(
            XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::PreserveControllerState, true),
        ));
        assert!(!xhci_runtime_init_strategy_prompt_safe(
            XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::None, false),
        ));
    }

    #[test]
    fn usb_probe_pathway_prefers_deeper_progress() {
        let shallow = UsbProbePathwaySummary {
            progress: UsbProbePathProgress::RootPortConnected,
            outcome: UsbProbePathOutcome::AddressFailed,
            ..UsbProbePathwaySummary::new(
                1,
                1,
                3,
                "preserve-state",
                "stop-state-preserve",
                "preserve",
                "stop-state",
                "skip-live-halt-read",
                false,
                true,
                true,
            )
        };
        let deep = UsbProbePathwaySummary {
            progress: UsbProbePathProgress::DeviceConfigured,
            outcome: UsbProbePathOutcome::NoKeyboardFound,
            ..shallow
        };
        assert!(deep.is_better_than(shallow));
        assert!(!shallow.is_better_than(deep));
    }

    #[test]
    fn usb_probe_pathway_prefers_hid_failure_over_generic_no_keyboard_on_tie() {
        let generic = UsbProbePathwaySummary {
            progress: UsbProbePathProgress::DeviceConfigured,
            outcome: UsbProbePathOutcome::NoKeyboardFound,
            diag: XhciDiagSnapshot::empty(),
            ..UsbProbePathwaySummary::new(
                2,
                1,
                3,
                "preserve-state",
                "stop-state-preserve",
                "preserve",
                "stop-state",
                "skip-live-halt-read",
                false,
                true,
                true,
            )
        };
        let hid_failure = UsbProbePathwaySummary {
            outcome: UsbProbePathOutcome::HidInitFailed,
            ..generic
        };
        assert!(hid_failure.is_better_than(generic));
        assert!(!generic.is_better_than(hid_failure));
    }

    #[test]
    fn runtime_vl805_mailbox_reset_failures_continue_cold_init() {
        assert!(runtime_vl805_mailbox_reset_error_allows_cold_init(
            Pi4SeatError::MailboxTimeout
        ));
        assert!(runtime_vl805_mailbox_reset_error_allows_cold_init(
            Pi4SeatError::MailboxProtocol
        ));
        assert!(runtime_vl805_mailbox_reset_error_allows_cold_init(
            Pi4SeatError::MailboxMap
        ));
        assert!(runtime_vl805_mailbox_reset_error_allows_cold_init(
            Pi4SeatError::MailboxDma
        ));
    }

    #[test]
    fn runtime_vl805_mailbox_reset_failures_are_hard_states() {
        assert_eq!(
            runtime_vl805_mailbox_reset_failure_state(Pi4SeatError::MailboxTimeout),
            VL805_RUNTIME_RESET_STATE_HARD_TIMEOUT
        );
        assert_eq!(
            runtime_vl805_mailbox_reset_failure_state(Pi4SeatError::MailboxProtocol),
            VL805_RUNTIME_RESET_STATE_HARD_PROTOCOL
        );
    }

    #[test]
    fn runtime_vl805_mailbox_reset_success_details_cover_ack_and_fallback() {
        assert_eq!(
            runtime_vl805_mailbox_reset_success_detail(pi4_wifi::Vl805ResetNotifyResult::Acked),
            "mailbox-notify+settle"
        );
        assert_eq!(
            runtime_vl805_mailbox_reset_success_detail(
                pi4_wifi::Vl805ResetNotifyResult::PostedFallback
            ),
            "mailbox-posted-fallback+settle"
        );
    }

    #[test]
    fn runtime_vl805_mailbox_reset_posted_fallback_uses_longer_settle_budget() {
        assert_eq!(
            runtime_vl805_mailbox_reset_success_settle_ms(pi4_wifi::Vl805ResetNotifyResult::Acked),
            VL805_MAILBOX_RESET_SETTLE_MS
        );
        assert_eq!(
            runtime_vl805_mailbox_reset_success_settle_ms(
                pi4_wifi::Vl805ResetNotifyResult::PostedFallback
            ),
            VL805_MAILBOX_RESET_POSTED_SETTLE_MS
        );
        assert!(VL805_MAILBOX_RESET_POSTED_SETTLE_MS > VL805_MAILBOX_RESET_SETTLE_MS);
    }

    #[test]
    fn runtime_vl805_mailbox_reset_ack_is_only_runtime_ownership_boundary() {
        assert_eq!(
            runtime_vl805_mailbox_reset_success_state(pi4_wifi::Vl805ResetNotifyResult::Acked),
            VL805_RUNTIME_RESET_STATE_NOTIFIED
        );
        assert_eq!(
            runtime_vl805_mailbox_reset_success_state(
                pi4_wifi::Vl805ResetNotifyResult::PostedFallback
            ),
            VL805_RUNTIME_RESET_STATE_POSTED_FALLBACK
        );
        assert!(runtime_vl805_mailbox_reset_completed(
            VL805_RUNTIME_RESET_STATE_NOTIFIED
        ));
        assert!(!runtime_vl805_mailbox_reset_completed(
            VL805_RUNTIME_RESET_STATE_BOOTLOADER_AUTHORIZED
        ));
        assert!(!runtime_vl805_mailbox_reset_completed(
            VL805_RUNTIME_RESET_STATE_POSTED_FALLBACK
        ));
        assert!(!runtime_vl805_mailbox_reset_completed(
            VL805_RUNTIME_RESET_STATE_SOFT_CONTINUE
        ));
        assert!(runtime_vl805_mailbox_reset_authorizes_hcrst(
            VL805_RUNTIME_RESET_STATE_NOTIFIED
        ));
        assert!(!runtime_vl805_mailbox_reset_authorizes_hcrst(
            VL805_RUNTIME_RESET_STATE_BOOTLOADER_AUTHORIZED
        ));
        assert!(!runtime_vl805_mailbox_reset_authorizes_hcrst(
            VL805_RUNTIME_RESET_STATE_UNATTEMPTED
        ));
        assert!(!runtime_vl805_mailbox_reset_authorizes_hcrst(
            VL805_RUNTIME_RESET_STATE_POSTED_FALLBACK
        ));
        assert!(!runtime_vl805_mailbox_reset_authorizes_hcrst(
            VL805_RUNTIME_RESET_STATE_SOFT_CONTINUE
        ));
        assert!(runtime_vl805_mailbox_reset_allows_trusted_cold_init(
            VL805_RUNTIME_RESET_STATE_NOTIFIED
        ));
        assert!(runtime_vl805_mailbox_reset_allows_trusted_cold_init(
            VL805_RUNTIME_RESET_STATE_BOOTLOADER_AUTHORIZED
        ));
        assert!(!runtime_vl805_mailbox_reset_allows_trusted_cold_init(
            VL805_RUNTIME_RESET_STATE_POSTED_FALLBACK
        ));
        assert!(!runtime_vl805_mailbox_reset_allows_trusted_cold_init(
            VL805_RUNTIME_RESET_STATE_SOFT_CONTINUE
        ));
        assert_eq!(
            runtime_vl805_mailbox_reset_handoff_label(VL805_RUNTIME_RESET_STATE_NOTIFIED),
            "runtime-owned"
        );
        assert_eq!(
            runtime_vl805_mailbox_reset_handoff_label(
                VL805_RUNTIME_RESET_STATE_BOOTLOADER_AUTHORIZED
            ),
            "bootloader-owned"
        );
        assert_eq!(
            runtime_vl805_mailbox_reset_handoff_label(VL805_RUNTIME_RESET_STATE_POSTED_FALLBACK),
            "runtime-unconfirmed"
        );
    }

    #[test]
    fn config_value_for_set_uses_b_configuration_value() {
        let config = ConfigDesc {
            config_value: 1,
            configuration: 42,
            ..ConfigDesc::default()
        };
        assert_eq!(config_value_for_set(config), Some(1));
    }

    #[test]
    fn config_value_for_set_rejects_zero_b_configuration_value() {
        let config = ConfigDesc {
            config_value: 0,
            configuration: 1,
            ..ConfigDesc::default()
        };
        assert_eq!(config_value_for_set(config), None);
    }

    #[test]
    fn vl805_runtime_cfg_touch_requires_safe_cfg_window() {
        assert!(!vl805_runtime_cfg_touch_allowed(true, false));
        assert!(vl805_runtime_cfg_touch_allowed(true, true));
    }

    #[test]
    fn vl805_runtime_cfg_touch_respects_global_disable() {
        assert!(!vl805_runtime_cfg_touch_allowed(false, true));
    }

    #[test]
    fn xhci_safe_mode_skip_command_prefers_bootloader_export() {
        assert_eq!(xhci_safe_mode_skip_command(None), 0);
        assert_eq!(xhci_safe_mode_skip_command(Some(0x0546)), 0x0546);
    }

    #[test]
    fn xhci_runtime_init_strategies_lead_with_reset_owned_stop_seed_after_mailbox_ack() {
        let (strategies, count) = xhci_runtime_init_strategies(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            super::VL805_RUNTIME_RESET_STATE_NOTIFIED,
            Some(LocalSeatXhciStopStateSnapshot {
                usbcmd: Some(0),
                usbsts: Some(1),
                iman0: Some(0),
            }),
        );
        assert_eq!(count, 3);
        assert_eq!(
            strategies[0],
            XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::None, true)
        );
        assert_eq!(
            strategies[1],
            XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::ColdStartFromSnapshot, false)
        );
        assert_eq!(
            strategies[2],
            XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::ColdStartFromSnapshot, true)
        );
    }

    #[test]
    fn xhci_runtime_init_strategies_keep_bootloader_auth_pollsafe() {
        let (strategies, count) = xhci_runtime_init_strategies(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            super::VL805_RUNTIME_RESET_STATE_BOOTLOADER_AUTHORIZED,
            Some(LocalSeatXhciStopStateSnapshot {
                usbcmd: Some(0),
                usbsts: Some(1),
                iman0: Some(0),
            }),
        );
        assert_eq!(count, 1);
        assert_eq!(
            strategies[0],
            XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::ColdStartFromSnapshot, true)
        );
    }

    #[test]
    fn xhci_runtime_init_strategies_suppress_hcrst_after_posted_mailbox_fallback() {
        let (strategies, count) = xhci_runtime_init_strategies(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            super::VL805_RUNTIME_RESET_STATE_POSTED_FALLBACK,
            Some(LocalSeatXhciStopStateSnapshot {
                usbcmd: Some(0),
                usbsts: Some(1),
                iman0: Some(0),
            }),
        );
        assert_eq!(count, 1);
        assert_eq!(
            strategies[0],
            XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::ColdStartFromSnapshot, true)
        );
    }

    #[test]
    fn xhci_runtime_init_strategies_suppress_hcrst_after_soft_mailbox_failure() {
        let (strategies, count) = xhci_runtime_init_strategies(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            super::VL805_RUNTIME_RESET_STATE_SOFT_CONTINUE,
            Some(LocalSeatXhciStopStateSnapshot {
                usbcmd: Some(0),
                usbsts: Some(1),
                iman0: Some(0),
            }),
        );
        assert_eq!(count, 1);
        assert_eq!(
            strategies[0],
            XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::ColdStartFromSnapshot, true)
        );
    }

    #[test]
    fn xhci_runtime_init_strategy_policy_labels_cover_pi4_runtime_categories() {
        assert_eq!(
            xhci_runtime_init_strategy_policy_label(XhciRuntimeInitStrategy::new(
                XhciFirmwareHandoff::ColdStartFromSnapshot,
                true,
            )),
            "bootloader-owned-pollsafe"
        );
        assert_eq!(
            xhci_runtime_init_strategy_policy_label(XhciRuntimeInitStrategy::new(
                XhciFirmwareHandoff::ColdStartFromSnapshot,
                false,
            )),
            "full-reset-start"
        );
        assert_eq!(
            xhci_runtime_init_strategy_policy_label(XhciRuntimeInitStrategy::new(
                XhciFirmwareHandoff::None,
                true,
            )),
            "resetless-stop-seed"
        );
        assert_eq!(
            xhci_runtime_init_strategy_policy_label(XhciRuntimeInitStrategy::new(
                XhciFirmwareHandoff::PreserveControllerState,
                true,
            )),
            "preserve-state"
        );
    }

    #[test]
    fn xhci_runtime_init_strategy_post_ready_irq_labels_cover_polling_skip_paths() {
        assert_eq!(
            super::xhci_runtime_init_strategy_post_ready_irq_label(XhciRuntimeInitStrategy::new(
                XhciFirmwareHandoff::ColdStartFromSnapshot,
                false,
            )),
            "irq-skip"
        );
        assert_eq!(
            super::xhci_runtime_init_strategy_post_ready_irq_label(XhciRuntimeInitStrategy::new(
                XhciFirmwareHandoff::ColdStartFromSnapshot,
                true,
            )),
            "irq-skip"
        );
        assert_eq!(
            super::xhci_runtime_init_strategy_post_ready_irq_label(XhciRuntimeInitStrategy::new(
                XhciFirmwareHandoff::PreserveControllerState,
                true,
            )),
            "irq-skip"
        );
        assert_eq!(
            super::xhci_runtime_init_strategy_post_ready_irq_label(XhciRuntimeInitStrategy::new(
                XhciFirmwareHandoff::None,
                true,
            )),
            "irq-skip"
        );
        assert_eq!(
            super::xhci_runtime_init_strategy_post_ready_irq_label(XhciRuntimeInitStrategy::new(
                XhciFirmwareHandoff::ResetlessReinit,
                true,
            )),
            "irq-skip"
        );
    }

    #[test]
    fn xhci_runtime_init_strategy_publish_labels_match_actual_pi4_publish_order() {
        assert_eq!(
            xhci_runtime_init_strategy_publish_label(XhciRuntimeInitStrategy::new(
                XhciFirmwareHandoff::ColdStartFromSnapshot,
                true,
            )),
            "rings-skip"
        );
        assert_eq!(
            xhci_runtime_init_strategy_publish_label(XhciRuntimeInitStrategy::new(
                XhciFirmwareHandoff::PreserveControllerState,
                true,
            )),
            "rings-pre-run"
        );
        assert_eq!(
            xhci_runtime_init_strategy_publish_label(XhciRuntimeInitStrategy::new(
                XhciFirmwareHandoff::ResetlessReinit,
                true,
            )),
            "rings-post-run"
        );
    }

    #[test]
    fn xhci_runtime_init_strategy_run_labels_match_actual_pi4_run_policy() {
        assert_eq!(
            xhci_runtime_init_strategy_run_label(XhciRuntimeInitStrategy::new(
                XhciFirmwareHandoff::ColdStartFromSnapshot,
                true,
            )),
            "run-skip"
        );
        assert_eq!(
            xhci_runtime_init_strategy_run_label(XhciRuntimeInitStrategy::new(
                XhciFirmwareHandoff::ColdStartFromSnapshot,
                false,
            )),
            "run-uboot"
        );
        assert_eq!(
            xhci_runtime_init_strategy_run_label(XhciRuntimeInitStrategy::new(
                XhciFirmwareHandoff::PreserveControllerState,
                true,
            )),
            "run-skip"
        );
    }

    #[test]
    fn xhci_runtime_init_strategy_origin_labels_distinguish_resetless_stop_seed_start() {
        assert_eq!(
            super::xhci_runtime_init_strategy_origin_label(XhciRuntimeInitStrategy::new(
                XhciFirmwareHandoff::None,
                true,
            )),
            "resetless-stop-seed"
        );
        assert_eq!(
            super::xhci_runtime_init_strategy_origin_label(XhciRuntimeInitStrategy::new(
                XhciFirmwareHandoff::ColdStartFromSnapshot,
                false,
            )),
            "uboot-fresh-init"
        );
    }

    #[test]
    fn usb_probe_route_labels_distinguish_primary_and_fallback_paths() {
        let stop_seed_primary = UsbProbePathwaySummary::new(
            0,
            0,
            3,
            "resetless-stop-seed",
            "resetless-stop-seed",
            "none",
            "stop-state",
            "skip-live-halt-read",
            true,
            true,
            true,
        );
        let uboot_primary = UsbProbePathwaySummary::new(
            1,
            1,
            3,
            "full-reset-start",
            "uboot-fresh-init",
            "cold-start-from-snapshot",
            "none",
            "skip-live-halt-read",
            true,
            true,
            true,
        );
        let preserve = UsbProbePathwaySummary::new(
            2,
            2,
            3,
            "preserve-state",
            "stop-state-preserve",
            "preserve-controller-state",
            "stop-state",
            "stop-state-seed",
            true,
            true,
            true,
        );
        assert_eq!(
            super::usb_probe_route_label(stop_seed_primary),
            "trusted-high-bar-stop-seed-primary"
        );
        assert_eq!(
            super::usb_probe_route_label(uboot_primary),
            "trusted-high-bar-primary"
        );
        assert_eq!(
            super::usb_probe_route_label(preserve),
            "stop-state-preserve-fallback"
        );
    }

    #[test]
    fn latest_usb_probe_route_status_preserves_pathway_and_diag_freshness() {
        let summary = UsbProbePathwaySummary {
            progress: UsbProbePathProgress::ControllerReady,
            outcome: UsbProbePathOutcome::NoConnectedPorts,
            irq27_bound: false,
            bridge_irq_bound: true,
            intx_irq_bound: true,
            controller_gate: "none",
            diag: XhciDiagSnapshot {
                line_count: 1,
                stage: 0x0311,
                a: 0x220,
                b: 0x0402_3000,
                c: 1,
            },
            diag_fresh: true,
            ..UsbProbePathwaySummary::new(
                1,
                2,
                3,
                "preserve-state",
                "stop-state-preserve",
                "preserve-controller-state",
                "stop-state",
                "stop-state-seed",
                true,
                true,
                true,
            )
        };
        super::remember_latest_usb_probe_route(&summary);
        let route = super::latest_usb_probe_route_status().expect("route status must exist");
        assert_eq!(route.route, "stop-state-preserve-fallback");
        assert_eq!(route.pathway_idx, 1);
        assert!(route.diag_fresh);
        assert!(!route.irq27_bound);
        assert!(route.bridge_irq_bound);
        assert!(route.intx_irq_bound);
        assert_eq!(route.controller_gate, "none");
        assert_eq!(route.diag_stage, Some(0x0311));
        assert_eq!(route.diag_tag, Some("erdp-publish-skip-preserve"));
        assert_eq!(route.diag_a, 0x220);
        assert_eq!(route.diag_b, 0x0402_3000);
        assert_eq!(route.diag_c, 1);
        assert_eq!(route.diag_value_labels, None);
    }

    #[test]
    fn usb_probe_next_step_uses_reset_post_settle_after_skip_stop_revalidation() {
        let summary = UsbProbePathwaySummary {
            diag: XhciDiagSnapshot {
                line_count: 1,
                stage: 0x0224,
                a: 0,
                b: 0,
                c: 0,
            },
            ..UsbProbePathwaySummary::new(
                0,
                0,
                3,
                "full-reset-start",
                "seeded-cold-start",
                "cold-start-from-snapshot",
                "stop-state",
                "skip-live-halt-read",
                true,
                true,
                true,
            )
        };
        assert_eq!(
            super::usb_probe_current_step(summary),
            "skip-stop-revalidation"
        );
        assert_eq!(super::usb_probe_next_step(summary), "reset-post-settle");
    }

    #[test]
    fn usb_probe_preflight_status_keeps_bootloader_auth_pollsafe() {
        let status = super::usb_probe_preflight_status(
            Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
            Some(0x0546),
            true,
            true,
            true,
            Some(LocalSeatXhciStopStateSnapshot {
                usbcmd: Some(0),
                usbsts: Some(1),
                iman0: Some(0),
            }),
            true,
        )
        .expect("trusted prompt-safe path should be classified");
        assert_eq!(status.route, "trusted-high-bar-seeded-retry");
        assert_eq!(status.strategy_idx, 1);
        assert_eq!(status.strategy_count, 1);
        assert_eq!(status.policy, "bootloader-owned-pollsafe");
        assert_eq!(status.origin, "seeded-cold-start");
        assert_eq!(status.handoff, "cold-start-from-snapshot");
        assert_eq!(status.seed, "stop-state");
        assert_eq!(status.halt_guard, "skip-live-halt-read");
        assert_eq!(status.constructor, "pre-halt-usbcmd-quiesce");
        assert_eq!(status.pre_reset, "skip-pre-reset");
        assert_eq!(status.legacy, "skip-legacy");
        assert_eq!(status.run, "run-skip");
        assert_eq!(status.publish, "rings-skip");
        assert_eq!(status.post_ready_irq, "irq-skip");
        assert_eq!(status.current_step, "pre-controller-ready");
        assert_eq!(status.next_step, "policy-return");
        assert_eq!(status.followup_step, "return-to-shell");
        assert!(!status.prefer_high);
        assert!(status.pcie_dma_window);
        assert!(status.poll_only);
        assert_eq!(status.expected_diag_stage, 0);
        assert_eq!(status.expected_diag_tag, None);
        assert_eq!(status.expected_diag_exact, None);
    }

    #[test]
    fn xhci_runtime_init_strategy_halt_guard_labels_match_trusted_and_seeded_paths() {
        assert_eq!(
            super::xhci_runtime_init_strategy_halt_guard_label(XhciRuntimeInitStrategy::new(
                XhciFirmwareHandoff::ColdStartFromSnapshot,
                false,
            )),
            "live-halt-read"
        );
        assert_eq!(
            super::xhci_runtime_init_strategy_halt_guard_label(XhciRuntimeInitStrategy::new(
                XhciFirmwareHandoff::None,
                true,
            )),
            "skip-live-halt-read"
        );
        assert_eq!(
            super::xhci_runtime_init_strategy_halt_guard_label(XhciRuntimeInitStrategy::new(
                XhciFirmwareHandoff::ColdStartFromSnapshot,
                true,
            )),
            "skip-live-halt-read"
        );
        assert_eq!(
            super::xhci_runtime_init_strategy_halt_guard_label(XhciRuntimeInitStrategy::new(
                XhciFirmwareHandoff::PreserveControllerState,
                false,
            )),
            "skip-live-halt-read"
        );
    }

    #[test]
    fn xhci_runtime_init_strategy_legacy_labels_keep_seeded_retry_poll_only() {
        assert_eq!(
            super::xhci_runtime_init_strategy_legacy_label(XhciRuntimeInitStrategy::new(
                XhciFirmwareHandoff::ColdStartFromSnapshot,
                false,
            )),
            "skip-legacy"
        );
        assert_eq!(
            super::xhci_runtime_init_strategy_legacy_label(XhciRuntimeInitStrategy::new(
                XhciFirmwareHandoff::ColdStartFromSnapshot,
                true,
            )),
            "skip-legacy"
        );
        assert_eq!(
            super::xhci_runtime_init_strategy_legacy_label(XhciRuntimeInitStrategy::new(
                XhciFirmwareHandoff::None,
                true,
            )),
            "skip-legacy"
        );
    }

    #[test]
    fn usb_probe_preflight_status_keeps_seeded_retry_on_skip_stop_revalidation() {
        let summary = UsbProbePathwaySummary::new(
            0,
            2,
            2,
            "bootloader-owned-pollsafe",
            "seeded-cold-start",
            "cold-start-from-snapshot",
            "stop-state",
            super::xhci_runtime_init_strategy_halt_guard_label(XhciRuntimeInitStrategy::new(
                XhciFirmwareHandoff::ColdStartFromSnapshot,
                true,
            )),
            true,
            true,
            true,
        );
        assert_eq!(summary.halt_guard, "skip-live-halt-read");
        assert_eq!(
            super::usb_probe_current_step(summary),
            "skip-stop-revalidation"
        );
        assert_eq!(super::usb_probe_next_step(summary), "reset-post-settle");
    }

    #[test]
    fn xhci_controller_params_preserve_hccparams1_for_runtime_init() {
        let params = xhci_controller_params_from_probe(
            XhciCapProbe {
                cap_length: 0x40,
                hci_version: 0x0100,
                hcs1: 32u32 | (8u32 << 24),
                hcs2: 0,
                hccparams1: 1 << 2,
                db_offset: 0x1000,
                rts_offset: 0x2000,
                max_slots: 32,
                max_ports: 8,
                max_scratchpad: 0,
                mmio_size: 0x10000,
            },
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            XhciFirmwareHandoff::PreserveControllerState,
            None,
        );
        assert_eq!(params.hccparams1, 1 << 2);
        assert_eq!(params.cap_length, 0x40);
        assert_eq!(params.db_offset, 0x1000);
        assert_eq!(
            params.firmware_handoff,
            XhciFirmwareHandoff::PreserveControllerState
        );
        assert!(params.runtime_seed_snapshot.is_none());
    }

    #[test]
    fn xhci_controller_params_strategy_can_skip_stop_state_seed_for_uboot_fresh_init() {
        let params = xhci_controller_params_from_probe_with_strategy(
            XhciCapProbe {
                cap_length: 0x40,
                hci_version: 0x0100,
                hcs1: 32u32 | (8u32 << 24),
                hcs2: 0,
                hccparams1: 1 << 2,
                db_offset: 0x1000,
                rts_offset: 0x2000,
                max_slots: 32,
                max_ports: 8,
                max_scratchpad: 0,
                mmio_size: 0x10000,
            },
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::ColdStartFromSnapshot, false),
            Some(LocalSeatXhciStopStateSnapshot {
                usbcmd: Some(0),
                usbsts: Some(1),
                iman0: Some(0),
            }),
        );
        assert_eq!(
            params.firmware_handoff,
            XhciFirmwareHandoff::ColdStartFromSnapshot
        );
        assert!(params.runtime_seed_snapshot.is_none());
    }

    #[test]
    fn xhci_controller_params_keep_standalone_init_unseeded() {
        let params = xhci_controller_params_from_probe(
            XhciCapProbe {
                cap_length: 0x40,
                hci_version: 0x0100,
                hcs1: 32u32 | (8u32 << 24),
                hcs2: 0,
                hccparams1: 1 << 2,
                db_offset: 0x1000,
                rts_offset: 0x2000,
                max_slots: 32,
                max_ports: 8,
                max_scratchpad: 0,
                mmio_size: 0x10000,
            },
            RPI4_XHCI_MMIO_PRIMARY_CANDIDATE,
            XhciFirmwareHandoff::None,
            None,
        );
        assert_eq!(params.firmware_handoff, XhciFirmwareHandoff::None);
        assert!(params.runtime_seed_snapshot.is_none());
        assert!(params.apply_brcm_axi_setup);
    }

    #[test]
    fn xhci_controller_params_disable_axi_setup_for_high_bar_none_retry() {
        let params = xhci_controller_params_from_probe(
            XhciCapProbe {
                cap_length: 0x40,
                hci_version: 0x0100,
                hcs1: 32u32 | (8u32 << 24),
                hcs2: 0,
                hccparams1: 1 << 2,
                db_offset: 0x1000,
                rts_offset: 0x2000,
                max_slots: 32,
                max_ports: 8,
                max_scratchpad: 0,
                mmio_size: 0x10000,
            },
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            XhciFirmwareHandoff::None,
            None,
        );
        assert_eq!(params.firmware_handoff, XhciFirmwareHandoff::None);
        assert!(!params.apply_brcm_axi_setup);
    }

    #[test]
    fn xhci_controller_params_keep_cold_start_mode_without_runtime_seed() {
        let params = xhci_controller_params_from_probe(
            XhciCapProbe {
                cap_length: 0x40,
                hci_version: 0x0100,
                hcs1: 32u32 | (8u32 << 24),
                hcs2: 0,
                hccparams1: 1 << 2,
                db_offset: 0x1000,
                rts_offset: 0x2000,
                max_slots: 32,
                max_ports: 8,
                max_scratchpad: 0,
                mmio_size: 0x10000,
            },
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            None,
        );
        assert_eq!(
            params.firmware_handoff,
            XhciFirmwareHandoff::ColdStartFromSnapshot
        );
        assert!(params.runtime_seed_snapshot.is_none());
        assert!(!params.apply_brcm_axi_setup);
    }

    #[test]
    fn xhci_controller_params_seed_trusted_cold_start_from_stop_state_snapshot() {
        let params = xhci_controller_params_from_probe(
            XhciCapProbe {
                cap_length: 0x40,
                hci_version: 0x0100,
                hcs1: 32u32 | (8u32 << 24),
                hcs2: 0,
                hccparams1: 1 << 2,
                db_offset: 0x1000,
                rts_offset: 0x2000,
                max_slots: 32,
                max_ports: 8,
                max_scratchpad: 0,
                mmio_size: 0x10000,
            },
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            Some(LocalSeatXhciStopStateSnapshot {
                usbcmd: Some(0),
                usbsts: Some(1),
                iman0: Some(0),
            }),
        );
        let snapshot = params
            .runtime_seed_snapshot
            .expect("trusted cold-start should seed stop-state snapshot");
        assert_eq!(snapshot.usbcmd, Some(0));
        assert_eq!(snapshot.usbsts, Some(1));
        assert_eq!(snapshot.iman0, Some(0));
        assert_eq!(snapshot.dcbaap, None);
        assert_eq!(snapshot.crcr, None);
        assert_eq!(snapshot.erstba0, None);
        assert_eq!(snapshot.erdp0, None);
        assert_eq!(snapshot.erstsz0, None);
    }

    #[test]
    fn xhci_controller_params_seed_preserved_state_from_stop_state_snapshot() {
        let params = xhci_controller_params_from_probe(
            XhciCapProbe {
                cap_length: 0x40,
                hci_version: 0x0100,
                hcs1: 32u32 | (8u32 << 24),
                hcs2: 0,
                hccparams1: 1 << 2,
                db_offset: 0x1000,
                rts_offset: 0x2000,
                max_slots: 32,
                max_ports: 8,
                max_scratchpad: 0,
                mmio_size: 0x10000,
            },
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            XhciFirmwareHandoff::PreserveControllerState,
            Some(LocalSeatXhciStopStateSnapshot {
                usbcmd: Some(0),
                usbsts: Some(1),
                iman0: Some(0),
            }),
        );
        let snapshot = params
            .runtime_seed_snapshot
            .expect("preserved handoff should seed stop-state snapshot");
        assert_eq!(snapshot.usbcmd, Some(0));
        assert_eq!(snapshot.usbsts, Some(1));
        assert_eq!(snapshot.iman0, Some(0));
        assert_eq!(snapshot.dcbaap, None);
        assert_eq!(snapshot.crcr, None);
        assert_eq!(snapshot.erstba0, None);
        assert_eq!(snapshot.erdp0, None);
        assert_eq!(snapshot.erstsz0, None);
        assert!(!params.apply_brcm_axi_setup);
    }

    #[test]
    fn xhci_controller_params_strategy_disables_axi_setup_for_trusted_handoffs() {
        let probe = XhciCapProbe {
            cap_length: 0x40,
            hci_version: 0x0100,
            hcs1: 32u32 | (8u32 << 24),
            hcs2: 0,
            hccparams1: 1 << 2,
            db_offset: 0x1000,
            rts_offset: 0x2000,
            max_slots: 32,
            max_ports: 8,
            max_scratchpad: 0,
            mmio_size: 0x10000,
        };
        let live = xhci_controller_params_from_probe_with_strategy(
            probe,
            RPI4_XHCI_MMIO_PRIMARY_CANDIDATE,
            XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::None, false),
            None,
        );
        assert!(live.apply_brcm_axi_setup);

        let trusted = xhci_controller_params_from_probe_with_strategy(
            probe,
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::ColdStartFromSnapshot, true),
            Some(LocalSeatXhciStopStateSnapshot {
                usbcmd: Some(0),
                usbsts: Some(1),
                iman0: Some(0),
            }),
        );
        assert!(!trusted.apply_brcm_axi_setup);
    }

    #[test]
    fn xhci_cap_probe_from_snapshot_preserves_handoff_snapshot_fields() {
        let probe = xhci_cap_probe_from_snapshot(LocalSeatXhciCapabilitySnapshot {
            cap_length: 0x40,
            hci_version: 0x0100,
            hcs1: 32u32 | (8u32 << 24),
            hcs2: 0,
            hccparams1: 1 << 2,
            db_offset: 0x1003,
            rts_offset: 0x201f,
        });
        assert_eq!(probe.cap_length, 0x40);
        assert_eq!(probe.hci_version, 0x0100);
        assert_eq!(probe.hccparams1, 1 << 2);
        assert_eq!(probe.max_slots, 32);
        assert_eq!(probe.max_ports, 8);
        assert_eq!(probe.db_offset, 0x1000);
        assert_eq!(probe.rts_offset, 0x2000);
        assert_eq!(probe.mmio_size, 0x10000);
    }

    #[test]
    fn cache_xhci_capability_probe_from_snapshot_accepts_valid_snapshot() {
        let probe = cache_xhci_capability_probe_from_snapshot(LocalSeatXhciCapabilitySnapshot {
            cap_length: 0x40,
            hci_version: 0x0100,
            hcs1: 32u32 | (8u32 << 24),
            hcs2: 0,
            hccparams1: 1 << 2,
            db_offset: 0x1000,
            rts_offset: 0x2000,
        })
        .expect("valid snapshot should cache into a runtime probe");
        assert_eq!(probe.cap_length, 0x40);
        assert_eq!(probe.hci_version, 0x0100);
        assert_eq!(probe.max_slots, 32);
        assert_eq!(probe.max_ports, 8);
    }

    #[test]
    fn cache_xhci_capability_probe_from_snapshot_rejects_invalid_snapshot() {
        assert!(
            cache_xhci_capability_probe_from_snapshot(LocalSeatXhciCapabilitySnapshot {
                cap_length: 0x10,
                hci_version: 0x0100,
                hcs1: 32u32 | (8u32 << 24),
                hcs2: 0,
                hccparams1: 1 << 2,
                db_offset: 0x1000,
                rts_offset: 0x2000,
            })
            .is_none()
        );
    }

    #[test]
    fn xhci_diag_stage_labels_cover_runtime_write_fault_markers() {
        assert_eq!(
            xhci_diag_stage_label(0x0205),
            Some("fw-handoff-usbcmd-mask-write")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0209),
            Some("fw-handoff-skip-usbcmd-mask-write")
        );
        assert_eq!(
            xhci_diag_stage_label(0x020a),
            Some("fw-handoff-skip-usbsts-clear-write")
        );
        assert_eq!(
            xhci_diag_stage_label(0x020b),
            Some("fw-handoff-skip-imod-write")
        );
        assert_eq!(
            xhci_diag_stage_label(0x020c),
            Some("fw-handoff-skip-iman-write")
        );
        assert_eq!(
            xhci_diag_stage_label(0x020d),
            Some("fw-handoff-trusted-usbsts-clear-skip")
        );
        assert_eq!(
            xhci_diag_stage_label(0x020e),
            Some("fw-handoff-trusted-imod-skip")
        );
        assert_eq!(
            xhci_diag_stage_label(0x020f),
            Some("fw-handoff-trusted-iman-skip")
        );
        assert_eq!(xhci_diag_stage_label(0x0117), Some("init-policy-summary"));
        assert_eq!(
            xhci_diag_stage_label(0x0200),
            Some("pre-reset-usbcmd-usbsts-read")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0210),
            Some("legacy-ownership-claim-begin")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0211),
            Some("legacy-ownership-claim-done")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0212),
            Some("fw-handoff-skip-legacy-ownership")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0217),
            Some("stop-revalidation-decision")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0218),
            Some("stop-revalidation-skip-branch")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0213),
            Some("stop-revalidation-usbsts-read-begin")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0214),
            Some("stop-revalidation-usbsts-read")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0215),
            Some("stop-revalidation-usbcmd-read-begin")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0216),
            Some("stop-revalidation-usbcmd-read")
        );
        assert_eq!(xhci_diag_stage_label(0x0208), Some("fw-handoff-iman-write"));
        assert_eq!(
            xhci_diag_stage_label(0x0220),
            Some("stop-revalidation-state")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0221),
            Some("stop-revalidation-run-clear")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0222),
            Some("stop-revalidation-timeout")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0223),
            Some("stop-revalidation-halted")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0224),
            Some("fw-handoff-skip-stop-revalidation")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0225),
            Some("stop-revalidation-ready")
        );
        assert_eq!(xhci_diag_stage_label(0x0226), Some("reset-pre-usbcmd-read"));
        assert_eq!(
            xhci_diag_stage_label(0x0227),
            Some("reset-post-settle-begin")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0228),
            Some("reset-post-settle-done")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0229),
            Some("reset-post-cnr-poll-skip")
        );
        assert_eq!(xhci_diag_stage_label(0x0230), Some("reset-write"));
        assert_eq!(
            xhci_diag_stage_label(0x023a),
            Some("reset-write-barrier-done")
        );
        assert_eq!(xhci_diag_stage_label(0x0235), Some("reset-write-issued"));
        assert_eq!(xhci_diag_stage_label(0x0236), Some("reset-first-readback"));
        assert_eq!(xhci_diag_stage_label(0x0237), Some("reset-write-pre-store"));
        assert_eq!(
            xhci_diag_stage_label(0x0238),
            Some("config-write-pre-store")
        );
        assert_eq!(xhci_diag_stage_label(0x0239), Some("config-write-issued"));
        assert_eq!(xhci_diag_stage_label(0x0231), Some("reset-hcrst-timeout"));
        assert_eq!(xhci_diag_stage_label(0x0232), Some("reset-cnr-timeout"));
        assert_eq!(xhci_diag_stage_label(0x0233), Some("reset-complete"));
        assert_eq!(xhci_diag_stage_label(0x0241), Some("config-read"));
        assert_eq!(xhci_diag_stage_label(0x0242), Some("dcbaap-readback"));
        assert_eq!(xhci_diag_stage_label(0x0243), Some("config-write"));
        assert_eq!(
            xhci_diag_stage_label(0x0245),
            Some("fw-handoff-skip-config-write")
        );
        assert_eq!(xhci_diag_stage_label(0x0246), Some("config-read-begin"));
        assert_eq!(xhci_diag_stage_label(0x0247), Some("dcbaap-readback-begin"));
        assert_eq!(xhci_diag_stage_label(0x0248), Some("dcbaap-write-low"));
        assert_eq!(xhci_diag_stage_label(0x024a), Some("dcbaap-write-low-done"));
        assert_eq!(xhci_diag_stage_label(0x0249), Some("dcbaap-write-high"));
        assert_eq!(
            xhci_diag_stage_label(0x024b),
            Some("dcbaap-write-high-done")
        );
        assert_eq!(xhci_diag_stage_label(0x024c), Some("dcbaap-zero-write64"));
        assert_eq!(
            xhci_diag_stage_label(0x024d),
            Some("dcbaap-zero-write64-done")
        );
        assert_eq!(
            xhci_diag_stage_label(0x024e),
            Some("dcbaap-prewrite-read-begin")
        );
        assert_eq!(xhci_diag_stage_label(0x024f), Some("dcbaap-prewrite-read"));
        assert_eq!(xhci_diag_stage_label(0x0244), Some("dcbaap-write"));
        assert_eq!(xhci_diag_stage_label(0x0251), Some("crcr-read"));
        assert_eq!(xhci_diag_stage_label(0x0252), Some("crcr-write"));
        assert_eq!(xhci_diag_stage_label(0x0253), Some("crcr-read-begin"));
        assert_eq!(xhci_diag_stage_label(0x0254), Some("crcr-write-low"));
        assert_eq!(xhci_diag_stage_label(0x0255), Some("crcr-write-high"));
        assert_eq!(xhci_diag_stage_label(0x0256), Some("dnctrl-write"));
        assert_eq!(xhci_diag_stage_label(0x0257), Some("dcbaap-defer-begin"));
        assert_eq!(xhci_diag_stage_label(0x0258), Some("dcbaap-defer-state"));
        assert_eq!(
            xhci_diag_stage_label(0x0259),
            Some("dcbaap-write-split-selected")
        );
        assert_eq!(xhci_diag_stage_label(0x025a), Some("dcbaap-defer-publish"));
        assert_eq!(xhci_diag_stage_label(0x0260), Some("event-ring-base"));
        assert_eq!(xhci_diag_stage_label(0x0261), Some("runtime-ring-read"));
        assert_eq!(xhci_diag_stage_label(0x0262), Some("iman-seed"));
        assert_eq!(xhci_diag_stage_label(0x0263), Some("usbsts-clear-ack"));
        assert_eq!(xhci_diag_stage_label(0x026b), Some("skip-imod-write"));
        assert_eq!(xhci_diag_stage_label(0x026c), Some("skip-iman-write"));
        assert_eq!(
            xhci_diag_stage_label(0x026d),
            Some("runtime-ring-read-begin")
        );
        assert_eq!(xhci_diag_stage_label(0x026e), Some("erstba-write-low"));
        assert_eq!(xhci_diag_stage_label(0x026f), Some("erstba-write-high"));
        assert_eq!(xhci_diag_stage_label(0x0266), Some("erdp-write"));
        assert_eq!(xhci_diag_stage_label(0x026a), Some("usbcmd-run-write"));
        assert_eq!(
            xhci_diag_stage_label(0x02e8),
            Some("fw-handoff-trusted-usbcmd-run-skip")
        );
        assert_eq!(xhci_diag_stage_label(0x02e9), Some("usbcmd-run-pre-store"));
        assert_eq!(xhci_diag_stage_label(0x02f0), Some("pre-run-ring-phys"));
        assert_eq!(xhci_diag_stage_label(0x02f1), Some("pre-run-ring-regs"));
        assert_eq!(xhci_diag_stage_label(0x02f2), Some("pre-run-staged-state"));
        assert_eq!(xhci_diag_stage_label(0x02f3), Some("pre-run-erst-state"));
        assert_eq!(xhci_diag_stage_label(0x02f4), Some("pre-run-publish-mask"));
        assert_eq!(xhci_diag_stage_label(0x02f5), Some("pre-run-offsets"));
        assert_eq!(
            xhci_diag_stage_label(0x02f6),
            Some("dcbaap-release-only-high-pre-store")
        );
        assert_eq!(xhci_diag_stage_label(0x02f7), Some("dcbaap-publish-policy"));
        assert_eq!(
            xhci_diag_stage_label(0x0332),
            Some("pre-dcbaap-iman-quiesce")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0333),
            Some("pre-dcbaap-iman-quiesce-done")
        );
        assert_eq!(xhci_diag_stage_label(0x0270), Some("usbsts-run-read"));
        assert_eq!(xhci_diag_stage_label(0x0271), Some("usbcmd-run-read"));
        assert_eq!(
            xhci_diag_stage_label(0x0272),
            Some("controller-ready-timeout")
        );
        assert_eq!(xhci_diag_stage_label(0x0273), Some("controller-ready"));
        assert_eq!(xhci_diag_stage_label(0x0274), Some("usbsts-run-read-begin"));
        assert_eq!(xhci_diag_stage_label(0x0275), Some("usbcmd-run-read-begin"));
        assert_eq!(
            xhci_diag_stage_label(0x0276),
            Some("controller-ready-poll-begin")
        );
        assert_eq!(xhci_diag_stage_label(0x0277), Some("erdp-write-low"));
        assert_eq!(xhci_diag_stage_label(0x0278), Some("erdp-write-high"));
        assert_eq!(xhci_diag_stage_label(0x0290), Some("dcbaap-atomic-write"));
        assert_eq!(
            xhci_diag_stage_label(0x0291),
            Some("dcbaap-atomic-write-begin")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0292),
            Some("dcbaap-atomic-write-done")
        );
        assert_eq!(xhci_diag_stage_label(0x0293), Some("crcr-atomic-write"));
        assert_eq!(
            xhci_diag_stage_label(0x0294),
            Some("crcr-atomic-write-begin")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0295),
            Some("crcr-atomic-write-done")
        );
        assert_eq!(xhci_diag_stage_label(0x0296), Some("erdp-atomic-write"));
        assert_eq!(
            xhci_diag_stage_label(0x0297),
            Some("erdp-atomic-write-begin")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0298),
            Some("erdp-atomic-write-done")
        );
        assert_eq!(xhci_diag_stage_label(0x0299), Some("erstba-atomic-write"));
        assert_eq!(
            xhci_diag_stage_label(0x029a),
            Some("erstba-atomic-write-begin")
        );
        assert_eq!(
            xhci_diag_stage_label(0x029b),
            Some("erstba-atomic-write-done")
        );
        assert_eq!(
            xhci_diag_stage_label(0x029c),
            Some("dcbaap-release-only-write")
        );
        assert_eq!(
            xhci_diag_stage_label(0x029d),
            Some("dcbaap-release-only-low-barrier-done")
        );
        assert_eq!(
            xhci_diag_stage_label(0x029e),
            Some("dcbaap-release-only-low-pre-store")
        );
        assert_eq!(
            xhci_diag_stage_label(0x029f),
            Some("dcbaap-release-only-high-barrier-done")
        );
        assert_eq!(
            xhci_diag_stage_label(0x02a0),
            Some("dcbaap-defer-change-mask")
        );
        assert_eq!(xhci_diag_stage_label(0x02a1), Some("dcbaap-staged-low"));
        assert_eq!(
            xhci_diag_stage_label(0x02a2),
            Some("dcbaap-staged-low-done")
        );
        assert_eq!(xhci_diag_stage_label(0x02a3), Some("dcbaap-staged-high"));
        assert_eq!(
            xhci_diag_stage_label(0x02a4),
            Some("dcbaap-staged-high-done")
        );
        assert_eq!(xhci_diag_stage_label(0x02a5), Some("dcbaap-target-low"));
        assert_eq!(
            xhci_diag_stage_label(0x02a6),
            Some("dcbaap-target-low-done")
        );
        assert_eq!(xhci_diag_stage_label(0x02a7), Some("dcbaap-target-high"));
        assert_eq!(
            xhci_diag_stage_label(0x02a8),
            Some("dcbaap-target-high-done")
        );
        assert_eq!(xhci_diag_stage_label(0x02a9), Some("dcbaap-defer-handoff"));
        assert_eq!(xhci_diag_stage_label(0x02aa), Some("crcr-defer-begin"));
        assert_eq!(
            xhci_diag_stage_label(0x02ab),
            Some("crcr-defer-change-mask")
        );
        assert_eq!(xhci_diag_stage_label(0x02ac), Some("crcr-staged-low"));
        assert_eq!(xhci_diag_stage_label(0x02ad), Some("crcr-staged-low-done"));
        assert_eq!(xhci_diag_stage_label(0x02ae), Some("crcr-staged-high"));
        assert_eq!(xhci_diag_stage_label(0x02af), Some("crcr-staged-high-done"));
        assert_eq!(xhci_diag_stage_label(0x02b0), Some("crcr-target-low"));
        assert_eq!(xhci_diag_stage_label(0x02b1), Some("crcr-target-low-done"));
        assert_eq!(xhci_diag_stage_label(0x02b2), Some("crcr-target-high"));
        assert_eq!(xhci_diag_stage_label(0x02b3), Some("crcr-target-high-done"));
        assert_eq!(xhci_diag_stage_label(0x02b4), Some("crcr-defer-handoff"));
        assert_eq!(xhci_diag_stage_label(0x02b5), Some("erdp-defer-begin"));
        assert_eq!(
            xhci_diag_stage_label(0x02b6),
            Some("erdp-defer-change-mask")
        );
        assert_eq!(xhci_diag_stage_label(0x02b7), Some("erdp-staged-low"));
        assert_eq!(xhci_diag_stage_label(0x02b8), Some("erdp-staged-low-done"));
        assert_eq!(xhci_diag_stage_label(0x02b9), Some("erdp-staged-high"));
        assert_eq!(xhci_diag_stage_label(0x02ba), Some("erdp-staged-high-done"));
        assert_eq!(xhci_diag_stage_label(0x02bb), Some("erdp-target-low"));
        assert_eq!(xhci_diag_stage_label(0x02bc), Some("erdp-target-low-done"));
        assert_eq!(xhci_diag_stage_label(0x02bd), Some("erdp-target-high"));
        assert_eq!(xhci_diag_stage_label(0x02be), Some("erdp-target-high-done"));
        assert_eq!(xhci_diag_stage_label(0x02bf), Some("erdp-defer-handoff"));
        assert_eq!(xhci_diag_stage_label(0x02c0), Some("erst-defer-size"));
        assert_eq!(xhci_diag_stage_label(0x02c1), Some("erst-defer-base"));
        assert_eq!(xhci_diag_stage_label(0x02c2), Some("erstsz-publish-begin"));
        assert_eq!(xhci_diag_stage_label(0x02c3), Some("erstsz-publish-write"));
        assert_eq!(
            xhci_diag_stage_label(0x02c4),
            Some("erstsz-publish-write-done")
        );
        assert_eq!(xhci_diag_stage_label(0x02c5), Some("erstba-publish-begin"));
        assert_eq!(xhci_diag_stage_label(0x02c6), Some("erstba-publish-write"));
        assert_eq!(
            xhci_diag_stage_label(0x02c7),
            Some("erstba-publish-write-done")
        );
        assert_eq!(xhci_diag_stage_label(0x02c8), Some("erstba-publish-high"));
        assert_eq!(
            xhci_diag_stage_label(0x02c9),
            Some("erstba-publish-high-done")
        );
        assert_eq!(xhci_diag_stage_label(0x02ca), Some("erstba-defer-handoff"));
        assert_eq!(xhci_diag_stage_label(0x02cb), Some("erstsz-post-run-begin"));
        assert_eq!(xhci_diag_stage_label(0x02cc), Some("erstsz-post-run-write"));
        assert_eq!(
            xhci_diag_stage_label(0x02cd),
            Some("erstsz-post-run-write-done")
        );
        assert_eq!(xhci_diag_stage_label(0x02ce), Some("erstba-post-run-begin"));
        assert_eq!(xhci_diag_stage_label(0x02cf), Some("erstba-post-run-write"));
        assert_eq!(
            xhci_diag_stage_label(0x02d0),
            Some("erstba-post-run-write-done")
        );
        assert_eq!(xhci_diag_stage_label(0x02d1), Some("erstba-post-run-high"));
        assert_eq!(
            xhci_diag_stage_label(0x02d2),
            Some("erstba-post-run-high-done")
        );
        assert_eq!(
            xhci_diag_stage_label(0x02d3),
            Some("erstba-post-run-handoff")
        );
        assert_eq!(xhci_diag_stage_label(0x02d4), Some("dcbaap-post-run-begin"));
        assert_eq!(xhci_diag_stage_label(0x02d5), Some("dcbaap-post-run-done"));
        assert_eq!(xhci_diag_stage_label(0x02d6), Some("crcr-post-run-begin"));
        assert_eq!(xhci_diag_stage_label(0x02d7), Some("crcr-post-run-done"));
        assert_eq!(xhci_diag_stage_label(0x02d8), Some("dnctrl-post-run-begin"));
        assert_eq!(xhci_diag_stage_label(0x02d9), Some("dnctrl-post-run-done"));
        assert_eq!(
            xhci_diag_stage_label(0x0314),
            Some("dnctrl-write-skip-preserve")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0315),
            Some("post-init-polling-irq-quiesce")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0316),
            Some("post-run-polling-irq-quiesce")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0317),
            Some("post-start-polling-irq-state")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0318),
            Some("post-start-polling-irq-settled")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0319),
            Some("post-start-polling-irq-timeout")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0320),
            Some("post-start-usbcmd-mask-state")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0321),
            Some("post-start-usbcmd-mask-write")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0322),
            Some("post-start-usbcmd-mask-write-done")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0323),
            Some("post-start-usbcmd-mask-skip")
        );
        assert_eq!(xhci_diag_stage_label(0x0324), Some("post-start-imod-write"));
        assert_eq!(
            xhci_diag_stage_label(0x0325),
            Some("post-start-imod-write-done")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0326),
            Some("post-start-erdp-write-low")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0327),
            Some("post-start-erdp-write-low-done")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0328),
            Some("post-start-erdp-write-high")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0329),
            Some("post-start-erdp-write-high-done")
        );
        assert_eq!(xhci_diag_stage_label(0x032a), Some("post-start-iman-write"));
        assert_eq!(
            xhci_diag_stage_label(0x032b),
            Some("post-start-iman-write-done")
        );
        assert_eq!(
            xhci_diag_stage_label(0x032c),
            Some("post-start-usbsts-clear-write")
        );
        assert_eq!(
            xhci_diag_stage_label(0x032d),
            Some("post-start-usbsts-clear-write-done")
        );
        assert_eq!(
            xhci_diag_stage_label(0x032e),
            Some("post-start-erdp-skip-preserve")
        );
        assert_eq!(
            xhci_diag_stage_label(0x032f),
            Some("post-start-iman-skip-preserve")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0330),
            Some("post-start-usbsts-clear-skip-preserve")
        );
        assert_eq!(
            xhci_diag_stage_label(0x02da),
            Some("erstsz-publish-skip-preserve")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0310),
            Some("erstba-publish-skip-preserve")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0311),
            Some("erdp-publish-skip-preserve")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0312),
            Some("dcbaap-publish-skip-preserve")
        );
        assert_eq!(
            xhci_diag_stage_label(0x0313),
            Some("crcr-publish-skip-preserve")
        );
        assert_eq!(xhci_diag_stage_label(0x0300), Some("cmd-submit"));
        assert_eq!(xhci_diag_stage_label(0x0301), Some("cmd-completion"));
        assert_eq!(xhci_diag_stage_label(0x0302), Some("cmd-fail"));
        assert_eq!(xhci_diag_stage_label(0x0303), Some("cmd-ring-enqueue"));
        assert_eq!(xhci_diag_stage_label(0x0304), Some("cmd-ccs-expected-ptr"));
        assert_eq!(xhci_diag_stage_label(0x0305), Some("cmd-ccs-mismatch"));
        assert_eq!(xhci_diag_stage_label(0x0306), Some("cmd-fail-state"));
        assert_eq!(xhci_diag_stage_label(0x0307), Some("cmd-timeout"));
        assert_eq!(xhci_diag_stage_label(0x0308), Some("cmd-wait-other-event"));
        assert_eq!(xhci_diag_stage_label(0x0309), Some("cmd-timeout-state"));
        assert_eq!(
            xhci_diag_stage_label(0x030a),
            Some("cmd-timeout-last-event")
        );
        assert_eq!(xhci_diag_stage_label(0x9999), None);
    }

    #[test]
    fn xhci_diag_exact_issue_labels_call_out_pre_run_usb_stalls() {
        assert_eq!(
            super::xhci_diag_stage_exact_issue_label(0x0200),
            Some("pre-reset-usbcmd-usbsts-read")
        );
        assert_eq!(
            super::xhci_diag_stage_exact_issue_label(0x0213),
            Some("live-usbsts-read-before-run")
        );
        assert_eq!(
            super::xhci_diag_stage_exact_issue_label(0x0215),
            Some("live-usbcmd-read-before-run")
        );
        assert_eq!(
            super::xhci_diag_stage_exact_issue_label(0x02eb),
            Some("usbcmd-run-barrier-wedged")
        );
        assert_eq!(
            super::xhci_diag_stage_exact_issue_label(0x02e9),
            Some("usbcmd-run-store-wedged")
        );
        assert_eq!(
            super::xhci_diag_stage_exact_issue_label(0x0248),
            Some("pre-run-dcbaap-low-store-wedged")
        );
        assert_eq!(
            super::xhci_diag_stage_exact_issue_label(0x0238),
            Some("pre-run-config-store-wedged")
        );
        assert_eq!(
            super::xhci_diag_stage_exact_issue_label(0x029e),
            Some("pre-run-dcbaap-low-store-wedged")
        );
        assert_eq!(
            super::xhci_diag_stage_exact_issue_label(0x0249),
            Some("pre-run-dcbaap-high-store-wedged")
        );
        assert_eq!(
            super::xhci_diag_stage_exact_issue_label(0x02a5),
            Some("post-run-dcbaap-low-store-wedged")
        );
        assert_eq!(
            super::xhci_diag_stage_exact_issue_label(0x02a7),
            Some("post-run-dcbaap-high-store-wedged")
        );
        assert_eq!(
            super::xhci_diag_stage_exact_issue_label(0x02f6),
            Some("pre-run-dcbaap-high-store-wedged")
        );
        assert_eq!(
            super::xhci_diag_stage_exact_issue_label(0x0254),
            Some("pre-run-crcr-low-store-wedged")
        );
        assert_eq!(
            super::xhci_diag_stage_exact_issue_label(0x0255),
            Some("pre-run-crcr-high-store-wedged")
        );
        assert_eq!(
            super::xhci_diag_stage_exact_issue_label(0x0277),
            Some("pre-run-erdp-low-store-wedged")
        );
        assert_eq!(
            super::xhci_diag_stage_exact_issue_label(0x0278),
            Some("pre-run-erdp-high-store-wedged")
        );
        assert_eq!(
            super::xhci_diag_stage_exact_issue_label(0x02c3),
            Some("pre-run-erstsz-store-wedged")
        );
        assert_eq!(
            super::xhci_diag_stage_exact_issue_label(0x02c6),
            Some("pre-run-erstba-low-store-wedged")
        );
        assert_eq!(
            super::xhci_diag_stage_exact_issue_label(0x02c8),
            Some("pre-run-erstba-high-store-wedged")
        );
        assert_eq!(
            super::xhci_diag_stage_exact_issue_label(0x0269),
            Some("pre-run-usbsts-clear-write-wedged")
        );
        assert_eq!(
            super::xhci_diag_stage_exact_issue_label(0x0267),
            Some("post-ready-imod-write-wedged")
        );
        assert_eq!(
            super::xhci_diag_stage_exact_issue_label(0x0268),
            Some("post-ready-iman-write-wedged")
        );
        assert_eq!(
            super::xhci_diag_stage_exact_issue_label(0x0256),
            Some("pre-run-dnctrl-write-wedged")
        );
        assert_eq!(
            super::xhci_diag_stage_exact_issue_label(0x02d8),
            Some("post-run-dnctrl-write-wedged")
        );
        assert_eq!(
            super::xhci_diag_stage_exact_issue_label(0x0319),
            Some("post-start-polling-irq-quiesce-timeout")
        );
        assert_eq!(
            super::xhci_diag_stage_exact_issue_label(0x0321),
            Some("post-start-usbcmd-mask-write-wedged")
        );
        assert_eq!(
            super::xhci_diag_stage_exact_issue_label(0x0324),
            Some("post-start-imod-write-wedged")
        );
        assert_eq!(
            super::xhci_diag_stage_exact_issue_label(0x0326),
            Some("post-start-erdp-low-store-wedged")
        );
        assert_eq!(
            super::xhci_diag_stage_exact_issue_label(0x0328),
            Some("post-start-erdp-high-store-wedged")
        );
        assert_eq!(
            super::xhci_diag_stage_exact_issue_label(0x032a),
            Some("post-start-iman-write-wedged")
        );
        assert_eq!(
            super::xhci_diag_stage_exact_issue_label(0x032c),
            Some("post-start-usbsts-clear-write-wedged")
        );
        assert_eq!(super::xhci_diag_stage_exact_issue_label(0x0214), None);
    }

    #[test]
    fn xhci_diag_stage_after_run_distinguishes_post_run_and_pre_run_edges() {
        assert!(super::xhci_diag_stage_after_run(0x02d4));
        assert!(super::xhci_diag_stage_after_run(0x02e9));
        assert!(super::xhci_diag_stage_after_run(0x0316));
        assert!(super::xhci_diag_stage_after_run(0x032c));
        assert!(!super::xhci_diag_stage_after_run(0x0248));
        assert!(!super::xhci_diag_stage_after_run(0x0213));
    }

    #[test]
    fn xhci_probe_failure_edge_label_prefers_trusted_selection_without_new_diag() {
        let before = XhciDiagSnapshot::empty();
        let after = XhciDiagSnapshot::empty();
        assert_eq!(
            super::xhci_probe_failure_edge_label(
                false,
                XhciFirmwareHandoff::ColdStartFromSnapshot,
                before,
                after,
            ),
            "trusted-path-selection"
        );
        assert_eq!(
            super::xhci_probe_failure_edge_label(
                true,
                XhciFirmwareHandoff::ColdStartFromSnapshot,
                before,
                after,
            ),
            "before-mailbox-reset-suppression"
        );
    }

    #[test]
    fn xhci_probe_failure_edge_label_separates_pre_run_and_post_run_failures() {
        let before = XhciDiagSnapshot::empty();
        let pre_run = XhciDiagSnapshot {
            line_count: 1,
            stage: 0x0248,
            a: 0,
            b: 0,
            c: 0,
        };
        let post_run = XhciDiagSnapshot {
            line_count: 1,
            stage: 0x02d4,
            a: 0,
            b: 0,
            c: 0,
        };
        assert_eq!(
            super::xhci_probe_failure_edge_label(
                false,
                XhciFirmwareHandoff::ColdStartFromSnapshot,
                before,
                pre_run,
            ),
            "first-live-ownership-write"
        );
        assert_eq!(
            super::xhci_probe_failure_edge_label(
                false,
                XhciFirmwareHandoff::ColdStartFromSnapshot,
                before,
                post_run,
            ),
            "after-run"
        );
    }

    #[test]
    fn xhci_diag_value_labels_name_pre_run_ring_snapshot_fields() {
        assert_eq!(
            xhci_diag_stage_value_labels(0x0117),
            Some(("handoff", "runtime_mask", "publish_mask"))
        );
        assert_eq!(
            xhci_diag_stage_value_labels(0x0200),
            Some(("usbcmd", "usbsts", "mmio"))
        );
        assert_eq!(
            xhci_diag_stage_value_labels(0x0204),
            Some(("mmio", "handoff", "seed_flags"))
        );
        assert_eq!(
            xhci_diag_stage_value_labels(0x0212),
            Some(("handoff", "seed_flags", "skip"))
        );
        assert_eq!(
            xhci_diag_stage_value_labels(0x0217),
            Some(("handoff", "seed_flags", "skip"))
        );
        assert_eq!(
            xhci_diag_stage_value_labels(0x0218),
            Some(("handoff", "seed_flags", "skip"))
        );
        assert_eq!(
            xhci_diag_stage_value_labels(0x02f0),
            Some(("dcbaa", "cmd_ring", "event_ring"))
        );
        assert_eq!(
            xhci_diag_stage_value_labels(0x02f1),
            Some(("erstba", "crcr", "erdp"))
        );
        assert_eq!(
            xhci_diag_stage_value_labels(0x02f2),
            Some(("staged_dcbaap", "current_crcr", "staged_erdp"))
        );
        assert_eq!(
            xhci_diag_stage_value_labels(0x02f3),
            Some(("staged_erstba", "staged_erstsz", "erstsz"))
        );
        assert_eq!(
            xhci_diag_stage_value_labels(0x02f4),
            Some(("publish_mask", "run_usbcmd", "run_mode"))
        );
        assert_eq!(
            xhci_diag_stage_value_labels(0x02f5),
            Some(("dcbaap_off", "crcr_off", "int_base"))
        );
        assert_eq!(
            xhci_diag_stage_value_labels(0x02f7),
            Some(("policy_mask", "handoff", "seed_flags"))
        );
        assert_eq!(
            xhci_diag_stage_value_labels(0x0332),
            Some(("iman", "masked_iman", "seed_flags"))
        );
        assert_eq!(
            xhci_diag_stage_value_labels(0x0316),
            Some(("erdp_ack", "iman_ip", "usbsts_clear"))
        );
        assert_eq!(
            xhci_diag_stage_value_labels(0x0317),
            Some(("attempt", "usbcmd", "usbsts_iman"))
        );
        assert_eq!(
            xhci_diag_stage_value_labels(0x0320),
            Some(("usbcmd", "masked_usbcmd", "masked_bits"))
        );
        assert_eq!(xhci_diag_stage_value_labels(0x02e9), None);
    }

    #[test]
    fn preferred_trusted_handoff_mode_uses_runtime_owned_rings_on_trusted_paths() {
        assert_eq!(
            super::xhci_preferred_trusted_handoff_mode(
                super::VL805_RUNTIME_RESET_STATE_UNATTEMPTED
            ),
            XhciFirmwareHandoff::ColdStartFromSnapshot
        );
        assert_eq!(
            super::xhci_preferred_trusted_handoff_mode(super::VL805_RUNTIME_RESET_STATE_NOTIFIED),
            XhciFirmwareHandoff::ColdStartFromSnapshot
        );
        assert_eq!(
            super::xhci_preferred_trusted_handoff_mode(
                super::VL805_RUNTIME_RESET_STATE_POSTED_FALLBACK
            ),
            XhciFirmwareHandoff::ColdStartFromSnapshot
        );
        assert_eq!(
            super::xhci_preferred_trusted_handoff_mode(
                super::VL805_RUNTIME_RESET_STATE_SOFT_CONTINUE
            ),
            XhciFirmwareHandoff::ColdStartFromSnapshot
        );
    }

    #[test]
    fn manual_preserve_state_strategies_keep_preserve_before_cold_start_fallbacks() {
        let (strategies, count) = xhci_runtime_init_strategies(
            XhciFirmwareHandoff::PreserveControllerState,
            super::VL805_RUNTIME_RESET_STATE_UNATTEMPTED,
            Some(LocalSeatXhciStopStateSnapshot {
                usbcmd: Some(0),
                usbsts: Some(1),
                iman0: Some(0),
            }),
        );
        assert_eq!(count, 3);
        assert_eq!(
            strategies[0],
            XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::PreserveControllerState, true)
        );
        assert_eq!(
            strategies[1],
            XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::ColdStartFromSnapshot, true)
        );
        assert_eq!(
            strategies[2],
            XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::ColdStartFromSnapshot, false)
        );
    }

    #[test]
    fn bootloader_owned_stop_state_strategies_skip_root_port_reads() {
        assert!(super::xhci_runtime_init_strategy_skips_root_port_reads(
            XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::PreserveControllerState, true),
        ));
        assert!(super::xhci_runtime_init_strategy_skips_root_port_reads(
            XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::ColdStartFromSnapshot, true),
        ));
        assert!(!super::xhci_runtime_init_strategy_skips_root_port_reads(
            XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::ColdStartFromSnapshot, false),
        ));
    }

    #[test]
    fn bootloader_owned_stop_state_strategy_skips_controller_entry() {
        assert!(super::xhci_runtime_init_strategy_skips_controller_entry(
            XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::ColdStartFromSnapshot, true),
        ));
        assert!(!super::xhci_runtime_init_strategy_skips_controller_entry(
            XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::None, true),
        ));
        assert!(!super::xhci_runtime_init_strategy_skips_controller_entry(
            XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::ColdStartFromSnapshot, false),
        ));
        assert!(!super::xhci_runtime_init_strategy_skips_controller_entry(
            XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::PreserveControllerState, true),
        ));
    }

    #[test]
    fn bootloader_owned_disabled_enumeration_is_not_no_connected_ports() {
        assert_eq!(
            UsbProbePathOutcome::EnumerationDisabledBootloaderOwned.as_str(),
            "enumeration-disabled-bootloader-owned"
        );
        assert!(
            UsbProbePathOutcome::NoConnectedPorts.tie_priority()
                > UsbProbePathOutcome::EnumerationDisabledBootloaderOwned.tie_priority()
        );
    }

    #[test]
    fn cold_start_trusted_strategies_without_mailbox_reset_stay_bootloader_owned() {
        let (strategies, count) = xhci_runtime_init_strategies(
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            super::VL805_RUNTIME_RESET_STATE_UNATTEMPTED,
            Some(LocalSeatXhciStopStateSnapshot {
                usbcmd: Some(0),
                usbsts: Some(1),
                iman0: Some(0),
            }),
        );
        assert_eq!(count, 1);
        assert_eq!(
            strategies[0],
            XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::ColdStartFromSnapshot, true)
        );
    }

    #[test]
    fn xhci_runtime_seed_snapshot_flag_bits_report_stop_state_seed() {
        let snapshot = Some(XhciRuntimeSeedSnapshot {
            usbcmd: Some(0),
            usbsts: Some(1),
            iman0: Some(0),
            dcbaap: None,
            crcr: None,
            erstba0: None,
            erdp0: None,
            erstsz0: None,
        });
        assert_eq!(super::xhci_runtime_seed_snapshot_flag_bits(snapshot), 0b011);
    }

    #[test]
    fn xhci_irq_sink_mode_requires_pre_controller_ready_sinks_for_trusted_fresh_init() {
        assert_eq!(
            xhci_irq_sink_mode(
                RPI4_XHCI_MMIO_HIGH_CANDIDATE,
                super::xhci_preferred_trusted_handoff_mode(
                    super::VL805_RUNTIME_RESET_STATE_UNATTEMPTED
                ),
                XhciIrqInstallPhase::PreControllerReady,
                false,
            ),
            XhciIrqSinkMode::Disabled
        );
        assert_eq!(
            xhci_irq_sink_mode(
                RPI4_XHCI_MMIO_HIGH_CANDIDATE,
                XhciFirmwareHandoff::ColdStartFromSnapshot,
                XhciIrqInstallPhase::PreControllerReady,
                true,
            ),
            XhciIrqSinkMode::TrustedPcieSinks
        );
        assert_eq!(
            xhci_irq_sink_mode(
                RPI4_XHCI_MMIO_HIGH_CANDIDATE,
                super::xhci_preferred_trusted_handoff_mode(
                    super::VL805_RUNTIME_RESET_STATE_UNATTEMPTED
                ),
                XhciIrqInstallPhase::ControllerReady,
                false,
            ),
            XhciIrqSinkMode::Disabled
        );
        assert!(!xhci_irq_sink_needed(
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            super::xhci_preferred_trusted_handoff_mode(
                super::VL805_RUNTIME_RESET_STATE_UNATTEMPTED
            ),
            XhciIrqInstallPhase::PreControllerReady,
            false,
        ));
        assert!(xhci_irq_sink_needed(
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            XhciFirmwareHandoff::ColdStartFromSnapshot,
            XhciIrqInstallPhase::PreControllerReady,
            true,
        ));
        assert!(xhci_irq_sink_needed(
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            XhciFirmwareHandoff::None,
            XhciIrqInstallPhase::PreControllerReady,
            true,
        ));
        assert!(!xhci_irq_sink_needed(
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            super::xhci_preferred_trusted_handoff_mode(
                super::VL805_RUNTIME_RESET_STATE_UNATTEMPTED
            ),
            XhciIrqInstallPhase::ControllerReady,
            false,
        ));
        assert!(!xhci_irq_sink_needed(
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            XhciFirmwareHandoff::None,
            XhciIrqInstallPhase::ControllerReady,
            false,
        ));
        assert!(!xhci_irq_sink_needed(
            RPI4_XHCI_MMIO_PRIMARY_CANDIDATE,
            XhciFirmwareHandoff::PreserveControllerState,
            XhciIrqInstallPhase::ControllerReady,
            false,
        ));
    }

    #[test]
    fn trusted_xhci_irq_sink_set_covers_bridge_and_child_intx_lines() {
        assert_eq!(
            TRUSTED_XHCI_PCIE_SINK_IRQS,
            [PI4_PCIE_BRIDGE_IRQ, PI4_VL805_XHCI_INTX_IRQ]
        );
        assert!(!TRUSTED_XHCI_PCIE_SINK_IRQS.contains(&PI4_GENERIC_VTIMER_IRQ));
    }

    #[test]
    fn trusted_pcie_irq_sink_failures_do_not_soft_ignore_missing_handlers() {
        assert!(!xhci_trusted_irq_soft_ignore_reason(
            PI4_PCIE_BRIDGE_IRQ,
            "irq-get-revoke-first-no-handler",
        ));
        assert!(!xhci_trusted_irq_soft_ignore_reason(
            PI4_VL805_XHCI_INTX_IRQ,
            "irq-get-revoke-first-owned",
        ));
        assert!(!xhci_trusted_irq_soft_ignore_reason(
            PI4_GENERIC_VTIMER_IRQ,
            "irq-handler-ambiguous",
        ));
    }

    #[test]
    fn trusted_xhci_irq_guard_must_cover_full_bounded_sink_set() {
        let partial = XhciIrqGuard {
            root_cnode: 0,
            bindings: [
                Some(XhciIrqBinding {
                    handler_slot: 0,
                    notification_slot: 0,
                    irq: PI4_PCIE_BRIDGE_IRQ,
                    owns_handler: false,
                    shadow: false,
                }),
                None,
            ],
        };
        assert!(!xhci_irq_guard_satisfies_phase(
            &partial,
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            XhciFirmwareHandoff::PreserveControllerState,
            XhciIrqInstallPhase::PreControllerReady,
            true,
        ));

        let full = XhciIrqGuard {
            root_cnode: 0,
            bindings: [
                Some(XhciIrqBinding {
                    handler_slot: 0,
                    notification_slot: 0,
                    irq: PI4_PCIE_BRIDGE_IRQ,
                    owns_handler: false,
                    shadow: false,
                }),
                Some(XhciIrqBinding {
                    handler_slot: 0,
                    notification_slot: 0,
                    irq: PI4_VL805_XHCI_INTX_IRQ,
                    owns_handler: false,
                    shadow: false,
                }),
            ],
        };
        assert!(xhci_irq_guard_satisfies_phase(
            &full,
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            XhciFirmwareHandoff::PreserveControllerState,
            XhciIrqInstallPhase::PreControllerReady,
            true,
        ));
        assert!(xhci_irq_guard_satisfies_phase(
            &partial,
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            XhciFirmwareHandoff::PreserveControllerState,
            XhciIrqInstallPhase::ControllerReady,
            false,
        ));
    }

    #[test]
    fn trusted_unseeded_cold_start_requires_primary_pcie_irq_contract() {
        assert!(xhci_runtime_init_strategy_requires_primary_pcie_irq(
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::ColdStartFromSnapshot, false),
        ));
        assert!(!xhci_runtime_init_strategy_requires_primary_pcie_irq(
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::ColdStartFromSnapshot, true),
        ));
        assert!(xhci_runtime_init_strategy_requires_primary_pcie_irq(
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::None, true),
        ));
        assert!(!xhci_runtime_init_strategy_requires_primary_pcie_irq(
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::PreserveControllerState, true),
        ));
        assert!(!xhci_runtime_init_strategy_requires_primary_pcie_irq(
            RPI4_XHCI_MMIO_PRIMARY_CANDIDATE,
            XhciRuntimeInitStrategy::new(XhciFirmwareHandoff::ColdStartFromSnapshot, false),
        ));
    }

    #[test]
    fn unique_existing_irq_handler_slot_requires_single_match() {
        fn scan(tags: &[crate::sel4::CapTag]) -> XhciIrqHandlerScanSummary {
            let mut first = None;
            let mut second = None;
            let mut count = 0usize;
            for (slot, tag) in tags.iter().copied().enumerate() {
                if matches!(tag, crate::sel4::CapTag::IrqHandler) {
                    count = count.saturating_add(1);
                    if first.is_none() {
                        first = Some(slot as sel4_sys::seL4_CPtr);
                    } else if second.is_none() {
                        second = Some(slot as sel4_sys::seL4_CPtr);
                    }
                }
            }
            XhciIrqHandlerScanSummary {
                first,
                second,
                count,
                scan_end: tags.len() as sel4_sys::seL4_CPtr,
            }
        }

        assert_eq!(
            scan(&[
                crate::sel4::CapTag::Null,
                crate::sel4::CapTag::IrqHandler,
                crate::sel4::CapTag::Frame,
            ])
            .unique_slot(),
            Ok(Some(1))
        );
        assert_eq!(
            scan(&[
                crate::sel4::CapTag::Null,
                crate::sel4::CapTag::Frame,
                crate::sel4::CapTag::Notification,
            ])
            .unique_slot(),
            Ok(None)
        );
        let summary = scan(&[
            crate::sel4::CapTag::IrqHandler,
            crate::sel4::CapTag::Frame,
            crate::sel4::CapTag::IrqHandler,
            crate::sel4::CapTag::Null,
        ]);
        assert_eq!(summary.first, Some(0));
        assert_eq!(summary.second, Some(2));
        assert_eq!(summary.count, 2);
        assert_eq!(summary.unique_slot(), Err(2));
    }

    #[test]
    fn xhci_irq_handler_acquisition_fails_closed_on_revoke_first() {
        assert_eq!(
            resolve_xhci_irq_handler_acquisition(sel4_sys::seL4_NoError, Ok(Some(0x42)),),
            Ok(())
        );
        assert_eq!(
            resolve_xhci_irq_handler_acquisition(sel4_sys::seL4_RevokeFirst, Ok(Some(0x42)),),
            Err("irq-get-revoke-first-owned")
        );
        assert_eq!(
            resolve_xhci_irq_handler_acquisition(sel4_sys::seL4_RevokeFirst, Ok(None)),
            Err("irq-get-revoke-first-no-handler")
        );
        assert_eq!(
            resolve_xhci_irq_handler_acquisition(sel4_sys::seL4_RevokeFirst, Err(2)),
            Err("irq-handler-ambiguous")
        );
        assert_eq!(
            resolve_xhci_irq_handler_acquisition(sel4_sys::seL4_DeleteFirst, Ok(Some(0x42))),
            Err("irq-get-handler")
        );
    }

    #[test]
    fn vl805_cfg_preseed_uses_map_only_in_safe_mode() {
        assert_eq!(vl805_cfg_preseed_mode(false), Vl805CfgPreseedMode::MapOnly);
        assert_eq!(
            vl805_cfg_preseed_mode(true),
            Vl805CfgPreseedMode::ReadMostly
        );
    }

    #[test]
    fn vl805_cfg_preseed_needed_keeps_safe_mode_ecam_window_pinned() {
        assert!(vl805_cfg_preseed_needed(false, false));
        assert!(vl805_cfg_preseed_needed(true, false));
        assert!(vl805_cfg_preseed_needed(false, true));
    }

    #[test]
    fn xhci_firmware_handoff_requires_memory_master_and_intx_disable() {
        assert!(!xhci_firmware_handoff_safe(None));
        assert!(!xhci_firmware_handoff_safe(Some(PCI_COMMAND_MEMORY_SPACE)));
        assert!(xhci_firmware_handoff_safe(Some(
            PCI_COMMAND_MEMORY_SPACE | PCI_COMMAND_BUS_MASTER | PCI_COMMAND_INTERRUPT_DISABLE
        )));
    }

    #[test]
    fn xhci_runtime_candidate_breadcrumb_helpers_classify_firmware_handoff() {
        assert_eq!(
            super::xhci_runtime_candidate_kind(
                RPI4_XHCI_MMIO_HIGH_CANDIDATE,
                Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
            ),
            "fw-high"
        );
        assert_eq!(
            super::xhci_runtime_candidate_kind(RPI4_XHCI_MMIO_PRIMARY_CANDIDATE, None),
            "legacy"
        );
        assert_eq!(
            super::xhci_runtime_candidate_skip_reason(
                RPI4_XHCI_MMIO_HIGH_CANDIDATE,
                false,
                None,
                Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
                None,
                true,
                false,
            ),
            "fw-handoff-unverified"
        );
        assert_eq!(
            super::xhci_runtime_candidate_skip_reason(
                RPI4_XHCI_MMIO_HIGH_CANDIDATE,
                false,
                None,
                Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
                None,
                false,
                false,
            ),
            "fw-handoff-unsafe"
        );
        assert_eq!(
            super::xhci_runtime_candidate_skip_reason(
                RPI4_XHCI_MMIO_PRIMARY_CANDIDATE,
                false,
                None,
                None,
                None,
                false,
                false,
            ),
            "legacy-runtime-disabled"
        );
    }

    #[test]
    fn xhci_high_candidate_requires_verified_runtime_source() {
        assert!(!xhci_runtime_mmio_candidate_allowed(
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            false,
            None,
            None,
            None,
            None,
            false,
        ));
        assert!(xhci_runtime_mmio_candidate_allowed(
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            false,
            None,
            Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
            None,
            None,
            false,
        ));
        assert!(!xhci_runtime_mmio_candidate_allowed(
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            true,
            None,
            None,
            None,
            None,
            false,
        ));
        assert!(!xhci_runtime_mmio_candidate_allowed(
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            false,
            None,
            None,
            Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
            None,
            false,
        ));
        assert!(xhci_runtime_mmio_candidate_allowed(
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            false,
            None,
            None,
            None,
            Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
            false,
        ));
        assert_eq!(
            xhci_runtime_candidate_skip_reason(
                RPI4_XHCI_MMIO_HIGH_CANDIDATE,
                true,
                None,
                Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
                None,
                true,
                false,
            ),
            "fw-handoff-unverified"
        );
        assert!(!xhci_runtime_mmio_candidate_allowed(
            RPI4_XHCI_MMIO_PRIMARY_CANDIDATE,
            false,
            None,
            None,
            None,
            None,
            false,
        ));
        assert!(!xhci_runtime_mmio_candidate_allowed(
            RPI4_XHCI_MMIO_PRIMARY_CANDIDATE,
            false,
            None,
            Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
            Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
            None,
            false,
        ));
    }

    #[test]
    fn xhci_alias_scan_advances_in_64k_windows() {
        assert_eq!(
            super::xhci_alias_scan_candidate(0x0000_0000_FE98_0000, 1),
            Some(0x0000_0000_FE99_0000)
        );
        assert_eq!(
            super::xhci_alias_scan_candidate(0x0000_0000_FE98_0000, XHCI_MMIO_ALIAS_SCAN_STEPS),
            Some(RPI4_XHCI_MMIO_PRIMARY_CANDIDATE)
        );
        assert_eq!(
            super::xhci_alias_scan_candidate(
                0x0000_0000_FE98_0000,
                XHCI_MMIO_ALIAS_SCAN_STEPS.saturating_add(1),
            ),
            None
        );
    }

    #[test]
    fn preferred_xhci_runtime_mmio_prefers_vl805_bar_hint() {
        assert_eq!(
            super::preferred_xhci_runtime_mmio(
                Some(RPI4_XHCI_MMIO_PRIMARY_CANDIDATE),
                None,
                Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
                None,
                false,
            ),
            Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE)
        );
        assert_eq!(
            super::preferred_xhci_runtime_mmio(
                None,
                None,
                Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
                None,
                false,
            ),
            Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE)
        );
    }

    #[test]
    fn preferred_xhci_runtime_mmio_keeps_high_handoff_preferred_over_legacy_pin() {
        assert_eq!(
            super::preferred_xhci_runtime_mmio(
                Some(RPI4_XHCI_MMIO_PRIMARY_CANDIDATE),
                Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
                None,
                None,
                false,
            ),
            Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE)
        );
    }

    #[test]
    fn preferred_xhci_runtime_mmio_filters_stale_legacy_aliases() {
        assert_eq!(
            super::preferred_xhci_runtime_mmio(
                Some(RPI4_XHCI_MMIO_PRIMARY_CANDIDATE),
                Some(0x0000_0000_7E9C_0000),
                None,
                Some(RPI4_XHCI_MMIO_PRIMARY_CANDIDATE),
                false,
            ),
            None
        );
        assert_eq!(
            super::preferred_xhci_runtime_mmio(
                Some(RPI4_XHCI_MMIO_PRIMARY_CANDIDATE),
                Some(0x0000_0000_7E9C_0000),
                None,
                Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
                false,
            ),
            Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE)
        );
    }

    #[test]
    fn preferred_xhci_runtime_mmio_does_not_prefer_legacy_mirror_for_trusted_high_handoff() {
        assert_eq!(
            super::preferred_xhci_runtime_mmio(
                Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
                Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
                None,
                None,
                true,
            ),
            Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE)
        );
    }

    #[test]
    fn xhci_runtime_blocks_legacy_candidates_when_high_bar_handoff_exists() {
        assert!(!super::xhci_runtime_mmio_candidate_allowed(
            RPI4_XHCI_MMIO_PRIMARY_CANDIDATE,
            false,
            Some((RPI4_XHCI_MMIO_PRIMARY_CANDIDATE, false)),
            Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
            Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
            None,
            true,
        ));
        assert_eq!(
            super::xhci_runtime_candidate_skip_reason(
                RPI4_XHCI_MMIO_PRIMARY_CANDIDATE,
                false,
                Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
                Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
                None,
                true,
                false,
            ),
            "legacy-runtime-disabled"
        );
    }

    #[test]
    fn xhci_preseed_static_fallbacks_remain_disabled() {
        assert!(!xhci_preseed_allows_static_legacy_fallbacks(None));
        assert!(!xhci_preseed_allows_static_legacy_fallbacks(Some(
            RPI4_XHCI_MMIO_HIGH_CANDIDATE
        )));
        assert!(!xhci_preseed_allows_static_legacy_fallbacks(Some(
            RPI4_XHCI_MMIO_PRIMARY_CANDIDATE
        )));
    }

    #[test]
    fn xhci_preseed_uses_pin_only_for_legacy_aliases_without_vl805_hint() {
        assert_eq!(
            xhci_preseed_pin_only_reason(
                RPI4_XHCI_MMIO_PRIMARY_CANDIDATE,
                None,
                None,
                None,
                false,
                false,
            ),
            Some("bcm2835-usb-not-xhci")
        );
        assert_eq!(
            xhci_preseed_pin_only_reason(
                RPI4_XHCI_MMIO_SECONDARY_CANDIDATE,
                None,
                None,
                None,
                false,
                false,
            ),
            Some("bcm2835-usb-not-xhci")
        );
        assert_eq!(
            xhci_preseed_pin_only_reason(
                RPI4_XHCI_MMIO_PRIMARY_CANDIDATE,
                None,
                Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
                None,
                false,
                false,
            ),
            Some("bcm2835-usb-not-xhci")
        );
        assert_eq!(
            xhci_preseed_pin_only_reason(
                RPI4_XHCI_MMIO_HIGH_CANDIDATE,
                None,
                None,
                None,
                false,
                false,
            ),
            None
        );
    }

    #[test]
    fn xhci_preseed_uses_pin_only_for_unverified_firmware_hint() {
        assert_eq!(
            xhci_preseed_pin_only_reason(
                0x0000_0000_FE9C_0000,
                Some(0x0000_0000_FE9C_0000),
                None,
                None,
                false,
                false,
            ),
            Some("firmware-hint-unverified")
        );
        assert_eq!(
            xhci_preseed_pin_only_reason(
                0x0000_0000_FE9C_0000,
                Some(0x0000_0000_FE9C_0000),
                Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
                None,
                false,
                false,
            ),
            None
        );
        assert_eq!(
            xhci_preseed_pin_only_reason(
                RPI4_XHCI_MMIO_HIGH_CANDIDATE,
                Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
                None,
                None,
                false,
                false,
            ),
            Some("firmware-hint-unverified")
        );
        let safe_cmd = Some(
            super::PCI_COMMAND_MEMORY_SPACE
                | super::PCI_COMMAND_BUS_MASTER
                | super::PCI_COMMAND_INTERRUPT_DISABLE,
        );
        assert_eq!(
            xhci_preseed_pin_only_reason(
                RPI4_XHCI_MMIO_HIGH_CANDIDATE,
                Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
                None,
                safe_cmd,
                false,
                false,
            ),
            Some("bootloader-handoff-unready")
        );
        assert_eq!(
            xhci_preseed_pin_only_reason(
                RPI4_XHCI_MMIO_HIGH_CANDIDATE,
                Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
                None,
                safe_cmd,
                true,
                false,
            ),
            Some("bootloader-handoff-irq-unquiesced")
        );
    }

    #[test]
    fn xhci_firmware_handoff_cold_start_trusts_safe_high_bar() {
        assert!(super::xhci_firmware_handoff_cold_start_trusted(
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
            Some(
                super::PCI_COMMAND_MEMORY_SPACE
                    | super::PCI_COMMAND_BUS_MASTER
                    | super::PCI_COMMAND_INTERRUPT_DISABLE,
            ),
            true,
            true,
        ));
        assert!(!super::xhci_firmware_handoff_cold_start_trusted(
            RPI4_XHCI_MMIO_PRIMARY_CANDIDATE,
            Some(RPI4_XHCI_MMIO_PRIMARY_CANDIDATE),
            Some(
                super::PCI_COMMAND_MEMORY_SPACE
                    | super::PCI_COMMAND_BUS_MASTER
                    | super::PCI_COMMAND_INTERRUPT_DISABLE,
            ),
            true,
            true,
        ));
        assert!(!super::xhci_firmware_handoff_cold_start_trusted(
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
            Some(super::PCI_COMMAND_MEMORY_SPACE | super::PCI_COMMAND_BUS_MASTER),
            true,
            true,
        ));
        assert!(!super::xhci_firmware_handoff_cold_start_trusted(
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
            Some(
                super::PCI_COMMAND_MEMORY_SPACE
                    | super::PCI_COMMAND_BUS_MASTER
                    | super::PCI_COMMAND_INTERRUPT_DISABLE,
            ),
            true,
            false,
        ));
    }

    #[test]
    fn xhci_firmware_handoff_cold_start_requires_ready_token() {
        let safe_cmd = Some(
            super::PCI_COMMAND_MEMORY_SPACE
                | super::PCI_COMMAND_BUS_MASTER
                | super::PCI_COMMAND_INTERRUPT_DISABLE,
        );
        assert!(!super::xhci_firmware_handoff_cold_start_trusted(
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
            safe_cmd,
            false,
            true,
        ));
    }

    #[test]
    fn xhci_runtime_vl805_mailbox_reset_requires_trusted_high_bar_handoff() {
        let safe_cmd = Some(
            super::PCI_COMMAND_MEMORY_SPACE
                | super::PCI_COMMAND_BUS_MASTER
                | super::PCI_COMMAND_INTERRUPT_DISABLE,
        );
        assert!(super::xhci_runtime_vl805_mailbox_reset_required(
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
            safe_cmd,
            true,
            true,
        ));
        assert!(!super::xhci_runtime_vl805_mailbox_reset_required(
            RPI4_XHCI_MMIO_PRIMARY_CANDIDATE,
            Some(RPI4_XHCI_MMIO_PRIMARY_CANDIDATE),
            safe_cmd,
            true,
            true,
        ));
        assert!(!super::xhci_runtime_vl805_mailbox_reset_required(
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
            safe_cmd,
            false,
            true,
        ));
        assert!(!super::xhci_runtime_vl805_mailbox_reset_required(
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
            safe_cmd,
            true,
            false,
        ));
    }

    #[test]
    fn xhci_bootloader_vl805_reset_authority_requires_trusted_stop_state() {
        let safe_cmd = Some(
            super::PCI_COMMAND_MEMORY_SPACE
                | super::PCI_COMMAND_BUS_MASTER
                | super::PCI_COMMAND_INTERRUPT_DISABLE,
        );
        let stop_state = Some(LocalSeatXhciStopStateSnapshot {
            usbcmd: Some(0),
            usbsts: Some(1),
            iman0: Some(0),
        });
        assert!(super::xhci_bootloader_vl805_reset_authorized(
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
            safe_cmd,
            true,
            true,
            true,
            stop_state,
        ));
        assert!(!super::xhci_bootloader_vl805_reset_authorized(
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
            safe_cmd,
            true,
            true,
            false,
            stop_state,
        ));
        assert!(!super::xhci_bootloader_vl805_reset_authorized(
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
            safe_cmd,
            true,
            true,
            true,
            None,
        ));
        assert!(!super::xhci_bootloader_vl805_reset_authorized(
            RPI4_XHCI_MMIO_PRIMARY_CANDIDATE,
            Some(RPI4_XHCI_MMIO_PRIMARY_CANDIDATE),
            safe_cmd,
            true,
            true,
            true,
            stop_state,
        ));
    }

    #[test]
    fn xhci_trusted_handoff_snapshot_requires_explicit_runtime_or_bootloader_authority() {
        let safe_cmd = Some(
            super::PCI_COMMAND_MEMORY_SPACE
                | super::PCI_COMMAND_BUS_MASTER
                | super::PCI_COMMAND_INTERRUPT_DISABLE,
        );
        assert!(!super::xhci_trusted_handoff_snapshot_allowed(
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
            safe_cmd,
            true,
            true,
            true,
        ));
        assert!(super::xhci_trusted_handoff_snapshot_allowed(
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
            safe_cmd,
            true,
            true,
            false,
        ));
        assert!(!super::xhci_trusted_handoff_snapshot_allowed(
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
            safe_cmd,
            false,
            true,
            true,
        ));
    }

    #[test]
    fn xhci_runtime_handoff_source_label_reflects_snapshot_path() {
        assert_eq!(
            super::xhci_runtime_handoff_source_label(true, true, false),
            "fw-handoff-runtime-reset-pending-snapshot"
        );
        assert_eq!(
            super::xhci_runtime_handoff_source_label(true, false, false),
            "fw-handoff-runtime-reset-pending-cold-init"
        );
        assert_eq!(
            super::xhci_runtime_handoff_source_label(false, true, false),
            "fw-handoff-direct-snapshot"
        );
        assert_eq!(
            super::xhci_runtime_handoff_source_label(true, true, true),
            "fw-handoff-bootloader-owned-snapshot"
        );
    }

    #[test]
    fn xhci_firmware_handoff_legacy_probe_stays_disabled() {
        let safe_cmd = Some(
            super::PCI_COMMAND_MEMORY_SPACE
                | super::PCI_COMMAND_BUS_MASTER
                | super::PCI_COMMAND_INTERRUPT_DISABLE,
        );
        assert!(!xhci_firmware_handoff_allows_legacy_probe(
            Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
            safe_cmd,
            true,
            true,
        ));
        assert!(!xhci_firmware_handoff_allows_legacy_probe(
            None, safe_cmd, false, false,
        ));
        assert!(!xhci_firmware_handoff_allows_legacy_probe(
            Some(RPI4_XHCI_MMIO_PRIMARY_CANDIDATE),
            safe_cmd,
            true,
            true,
        ));
        assert!(!xhci_firmware_handoff_allows_legacy_probe(
            Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
            safe_cmd,
            false,
            true,
        ));
    }

    #[test]
    fn xhci_high_bar_runtime_runs_polling_only() {
        assert!(super::xhci_polling_only_runtime(
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            super::xhci_preferred_trusted_handoff_mode(
                super::VL805_RUNTIME_RESET_STATE_UNATTEMPTED
            ),
        ));
        assert!(!super::xhci_polling_only_runtime(
            RPI4_XHCI_MMIO_PRIMARY_CANDIDATE,
            XhciFirmwareHandoff::PreserveControllerState,
        ));
        assert!(super::xhci_polling_only_runtime(
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            XhciFirmwareHandoff::None,
        ));
    }

    #[test]
    fn xhci_irq_sink_keeps_untrusted_paths_disabled() {
        assert!(super::xhci_irq_sink_needed(
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            super::xhci_preferred_trusted_handoff_mode(
                super::VL805_RUNTIME_RESET_STATE_UNATTEMPTED
            ),
            XhciIrqInstallPhase::ControllerReady,
            false,
        ));
        assert!(!super::xhci_irq_sink_needed(
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            XhciFirmwareHandoff::None,
            XhciIrqInstallPhase::ControllerReady,
            false,
        ));
        assert!(!super::xhci_irq_sink_needed(
            RPI4_XHCI_MMIO_PRIMARY_CANDIDATE,
            XhciFirmwareHandoff::PreserveControllerState,
            XhciIrqInstallPhase::ControllerReady,
            false,
        ));
    }

    #[test]
    fn xhci_firmware_handoff_hint_reason_classifies_contract_failures() {
        let safe_cmd = Some(
            super::PCI_COMMAND_MEMORY_SPACE
                | super::PCI_COMMAND_BUS_MASTER
                | super::PCI_COMMAND_INTERRUPT_DISABLE,
        );
        assert_eq!(
            xhci_firmware_handoff_hint_reason(None, safe_cmd, false, false),
            "hint-absent"
        );
        assert_eq!(
            xhci_firmware_handoff_hint_reason(
                Some(RPI4_XHCI_MMIO_PRIMARY_CANDIDATE),
                safe_cmd,
                true,
                true,
            ),
            "not-high-bar"
        );
        assert_eq!(
            xhci_firmware_handoff_hint_reason(
                Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
                Some(super::PCI_COMMAND_MEMORY_SPACE),
                true,
                true,
            ),
            "unsafe-pci-cmd"
        );
        assert_eq!(
            xhci_firmware_handoff_hint_reason(
                Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
                safe_cmd,
                false,
                true,
            ),
            "ready-token-absent"
        );
        assert_eq!(
            xhci_firmware_handoff_hint_reason(
                Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
                safe_cmd,
                true,
                false,
            ),
            "irq-quiesce-absent"
        );
        assert_eq!(
            xhci_firmware_handoff_hint_reason(
                Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
                safe_cmd,
                true,
                true,
            ),
            "bootloader-handoff-ready"
        );
    }

    #[test]
    fn xhci_runtime_alias_scan_requires_verified_source_for_non_high_candidates() {
        assert!(!xhci_runtime_allows_alias_scan(
            RPI4_XHCI_MMIO_PRIMARY_CANDIDATE,
            None,
            false,
        ));
        assert!(!xhci_runtime_allows_alias_scan(
            0x0000_0000_FE9C_0000,
            None,
            false,
        ));
        assert!(xhci_runtime_allows_alias_scan(
            RPI4_XHCI_MMIO_HIGH_CANDIDATE,
            None,
            false,
        ));
        assert!(!xhci_runtime_allows_alias_scan(
            RPI4_XHCI_MMIO_PRIMARY_CANDIDATE,
            Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
            false,
        ));
    }

    #[test]
    fn xhci_runtime_accessible_window_accepts_pinned_firmware_hint() {
        assert!(xhci_runtime_mmio_has_accessible_window(
            0x0000_0000_FE9C_0000,
            false,
            true,
            None,
            None,
            Some(0x0000_0000_FE9C_0000),
            None,
        ));
        assert!(!xhci_runtime_mmio_has_accessible_window(
            0x0000_0000_FE9C_0000,
            false,
            false,
            None,
            None,
            Some(0x0000_0000_FE9C_0000),
            None,
        ));
        assert!(xhci_runtime_mmio_has_accessible_window(
            RPI4_XHCI_MMIO_PRIMARY_CANDIDATE,
            false,
            false,
            None,
            Some(RPI4_XHCI_MMIO_PRIMARY_CANDIDATE),
            None,
            None,
        ));
        assert!(xhci_runtime_mmio_has_accessible_window(
            RPI4_XHCI_MMIO_PRIMARY_CANDIDATE,
            false,
            true,
            Some((RPI4_XHCI_MMIO_PRIMARY_CANDIDATE, false)),
            None,
            None,
            None,
        ));
    }

    #[test]
    fn xhci_runtime_rejects_pinned_legacy_fallback_with_high_firmware_breadcrumb() {
        assert!(!xhci_runtime_mmio_candidate_allowed(
            RPI4_XHCI_MMIO_PRIMARY_CANDIDATE,
            false,
            Some((RPI4_XHCI_MMIO_PRIMARY_CANDIDATE, false)),
            None,
            Some(RPI4_XHCI_MMIO_HIGH_CANDIDATE),
            None,
            false,
        ));
    }

    #[test]
    fn normalize_pi4_xhci_hint_translates_bcm2711_bus_aliases() {
        assert_eq!(
            translate_bcm2711_soc_reg_addr(0x0000_0000_7e9c_0000),
            0x0000_0000_fe9c_0000
        );
        assert_eq!(
            normalize_pi4_xhci_mmio_hint(Some(0x0000_0000_7e9c_0000)),
            Some(0x0000_0000_fe9c_0000)
        );
        assert_eq!(
            normalize_pi4_xhci_mmio_hint(Some(0x0000_0000_fe9c_0000)),
            Some(0x0000_0000_fe9c_0000)
        );
    }

    #[test]
    fn normalize_hub_tt_profile_forces_single_tt_port_one_and_clamps_ttt() {
        assert_eq!(normalize_hub_tt_profile(4, false, 1), (1, 1));
        assert_eq!(normalize_hub_tt_profile(0, false, 3), (1, 2));
    }

    #[test]
    fn normalize_hub_tt_profile_preserves_multi_tt_port_and_clamps_ttt() {
        assert_eq!(normalize_hub_tt_profile(4, true, 1), (4, 1));
        assert_eq!(normalize_hub_tt_profile(4, true, 3), (4, 2));
    }

    #[test]
    fn hid_report_descriptor_detects_keyboard_application_collection() {
        let keyboard_desc = [0x05, 0x01, 0x09, 0x06, 0xA1, 0x01, 0xC0];
        assert!(UsbKeyboard::hid_report_descriptor_is_keyboard(
            &keyboard_desc
        ));
    }

    #[test]
    fn hid_report_descriptor_rejects_mouse_application_collection() {
        let mouse_desc = [0x05, 0x01, 0x09, 0x02, 0xA1, 0x01, 0xC0];
        assert!(!UsbKeyboard::hid_report_descriptor_is_keyboard(&mouse_desc));
    }

    #[test]
    fn hub_class_control_wait_budget_matches_default_control_budget() {
        assert_eq!(HUB_CLASS_CONTROL_WAIT_SPINS, 20_000_000);
    }

    #[test]
    fn hub_retry_wait_spins_uses_fast_then_slow_tail() {
        assert_eq!(
            hub_retry_wait_spins(1, 4, 1),
            HUB_CLASS_CONTROL_WAIT_SPINS_FAST
        );
        assert_eq!(
            hub_retry_wait_spins(3, 4, 1),
            HUB_CLASS_CONTROL_WAIT_SPINS_FAST
        );
        assert_eq!(hub_retry_wait_spins(4, 4, 1), HUB_CLASS_CONTROL_WAIT_SPINS);
    }

    #[test]
    fn hub_retry_wait_spins_keeps_single_attempt_slow() {
        assert_eq!(hub_retry_wait_spins(1, 1, 1), HUB_CLASS_CONTROL_WAIT_SPINS);
    }

    #[test]
    fn hub_port_power_policy_defaults_to_deferred_scan() {
        assert!(!hub_should_eager_port_power(0));
        assert!(!hub_should_eager_port_power(1));
    }

    #[test]
    fn blind_prepare_powers_ports_for_switchable_hubs() {
        assert!(UsbKeyboard::hub_should_power_port_during_blind_prepare(0));
        assert!(UsbKeyboard::hub_should_power_port_during_blind_prepare(1));
        assert!(!UsbKeyboard::hub_should_power_port_during_blind_prepare(2));
    }

    #[test]
    fn power_kick_status_probe_is_deferred_for_individual_hubs() {
        assert!(UsbKeyboard::hub_should_probe_status_after_power_kick(0));
        assert!(!UsbKeyboard::hub_should_probe_status_after_power_kick(1));
    }

    #[test]
    fn hub_post_power_wait_uses_full_settle_for_individual_hubs() {
        assert_eq!(
            UsbKeyboard::hub_post_power_wait_ms(1, 50),
            HUB_POWER_SETTLE_MIN_MS
        );
        assert_eq!(UsbKeyboard::hub_post_power_wait_ms(1, 150), 300);
        assert_eq!(
            UsbKeyboard::hub_post_power_wait_ms(0, 50),
            HUB_PORT_STATUS_QUICK_RETRY_DELAY_MS.max(100)
        );
    }

    #[test]
    fn hub_port_index_candidates_probe_interface_high_byte_fallbacks() {
        let (indices, count) = UsbKeyboard::hub_port_index_candidates(0, 2);
        assert_eq!(count, (HUB_PORT_IFACE_FALLBACK_MAX as usize) + 1);
        assert_eq!(indices[0], 0x0002);
        assert_eq!(indices[1], 0x0102);
        assert_eq!(indices[2], 0x0202);
        assert_eq!(indices[3], 0x0302);
    }

    #[test]
    fn hub_port_index_candidates_keep_explicit_interface_without_duplicates() {
        let (indices, count) = UsbKeyboard::hub_port_index_candidates(2, 4);
        assert_eq!(indices[0], 0x0004);
        assert_eq!(indices[1], 0x0204);
        assert!(indices[..count].contains(&0x0104));
        assert!(indices[..count].contains(&0x0304));
    }

    #[test]
    fn mailbox_visible_dimension_prefers_non_zero_and_minimum() {
        assert_eq!(mailbox_visible_dimension(0, 0), None);
        assert_eq!(mailbox_visible_dimension(0, 1080), Some(1080));
        assert_eq!(mailbox_visible_dimension(1920, 0), Some(1920));
        assert_eq!(mailbox_visible_dimension(1920, 1080), Some(1080));
        assert_eq!(mailbox_visible_dimension(1080, 1920), Some(1080));
    }

    #[test]
    fn pi4_pcie_dma_window_keeps_low_phys_identity_mapped() {
        assert_eq!(pcie_dma_bus_addr(0x0400_3000), Some(0x0400_3000));
        assert_eq!(
            pcie_dma_bus_addr(RPI4_PCIE_DMA_LIMIT - PAGE_SIZE),
            Some(RPI4_PCIE_DMA_LIMIT - PAGE_SIZE)
        );
        assert_eq!(pcie_dma_bus_addr(RPI4_PCIE_DMA_LIMIT), None);
    }

    #[test]
    fn clamp_visible_width_respects_pitch_capacity() {
        assert_eq!(clamp_visible_width(1920, 1920 * 4), 1920);
        assert_eq!(clamp_visible_width(1920, 1280 * 4), 1280);
        assert_eq!(clamp_visible_width(640, 0), 0);
    }

    #[test]
    fn clamp_visible_height_respects_allocation_capacity() {
        assert_eq!(clamp_visible_height(1080, 4096, 4096 * 1080), 1080);
        assert_eq!(clamp_visible_height(1080, 4096, 4096 * 720), 720);
        assert_eq!(clamp_visible_height(720, 0, 4096 * 720), 0);
    }

    #[test]
    fn text_viewport_rounds_down_to_full_text_rows() {
        let rows = text_row_count(1080);
        assert_eq!(rows, 67);
        assert_eq!(text_viewport_height(1080, rows), rows * CHAR_HEIGHT);
    }

    #[test]
    fn text_viewport_keeps_single_row_for_small_framebuffers() {
        let rows = text_row_count(8);
        assert_eq!(rows, 1);
        assert_eq!(text_viewport_height(8, rows), 8);
    }

    #[test]
    fn text_backspace_target_rewinds_within_and_across_rows() {
        assert_eq!(text_backspace_target(0, 0, 80), (0, 0));
        assert_eq!(text_backspace_target(0, 5, 80), (0, 4));
        assert_eq!(text_backspace_target(2, 0, 80), (1, 79));
        assert_eq!(text_backspace_target(2, 0, 0), (0, 0));
    }

    #[test]
    fn append_wrapped_scrollback_rows_wraps_empty_and_long_lines() {
        let mut rows = Vec::new();
        append_wrapped_scrollback_rows(&mut rows, "", 4);
        append_wrapped_scrollback_rows(&mut rows, "abcdef", 4);
        assert_eq!(
            rows,
            Vec::from([String::new(), String::from("abcd"), String::from("ef")])
        );
    }

    #[test]
    fn wifi_progress_dots_cycle_through_three_states() {
        assert_eq!(wifi_progress_dot_count(0), 0);
        assert_eq!(wifi_progress_dot_count(1), 1);
        assert_eq!(wifi_progress_dot_count(2), 2);
        assert_eq!(wifi_progress_dot_count(3), 3);
        assert_eq!(wifi_progress_dot_count(4), 1);
    }

    #[test]
    fn wifi_progress_ticks_emit_only_on_slow_boundaries() {
        assert_eq!(wifi_progress_dots_for_ticks(0), 0);
        assert_eq!(
            wifi_progress_dots_for_ticks(WIFI_PROGRESS_EMIT_INTERVAL_TICKS - 1),
            0
        );
        assert_eq!(
            wifi_progress_dots_for_ticks(WIFI_PROGRESS_EMIT_INTERVAL_TICKS),
            1
        );
        assert_eq!(
            wifi_progress_dots_for_ticks(WIFI_PROGRESS_EMIT_INTERVAL_TICKS * 2),
            2
        );
        assert_eq!(
            wifi_progress_dots_for_ticks(WIFI_PROGRESS_EMIT_INTERVAL_TICKS * 3),
            3
        );
        assert_eq!(
            wifi_progress_dots_for_ticks(WIFI_PROGRESS_EMIT_INTERVAL_TICKS * 4),
            1
        );
    }

    #[test]
    fn keypad_enter_maps_to_newline() {
        assert_eq!(
            keyboard_scancode_to_char(scancode::KP_ENTER, false),
            Some('\n')
        );
        assert_eq!(
            keyboard_scancode_to_char(scancode::KP_ENTER, true),
            Some('\n')
        );
    }

    #[test]
    fn keyboard_display_scroll_delta_maps_arrow_keys_only() {
        assert_eq!(keyboard_display_scroll_delta_for_key(scancode::UP_ARROW), 1);
        assert_eq!(
            keyboard_display_scroll_delta_for_key(scancode::DOWN_ARROW),
            -1
        );
        assert_eq!(keyboard_display_scroll_delta_for_key(scancode::A), 0);
    }

    #[test]
    fn hid_keyboard_attach_rank_prefers_boot_keyboard() {
        assert_eq!(
            hid_keyboard_attach_rank(hid_subclass::BOOT, hid_protocol::KEYBOARD),
            Some(0)
        );
        assert_eq!(hid_keyboard_attach_source(0), "boot-keyboard");
        assert!(!hid_keyboard_candidate_requires_force_mode(0));
    }

    #[test]
    fn hid_keyboard_attach_rank_accepts_relaxed_keyboard_candidates() {
        assert_eq!(
            hid_keyboard_attach_rank(hid_subclass::NONE, hid_protocol::KEYBOARD),
            Some(1)
        );
        assert_eq!(
            hid_keyboard_attach_rank(hid_subclass::NONE, hid_protocol::NONE),
            Some(2)
        );
        assert_eq!(hid_keyboard_attach_source(1), "keyboard-protocol");
        assert_eq!(hid_keyboard_attach_source(2), "protocol-none-fallback");
        assert!(hid_keyboard_candidate_requires_force_mode(1));
        assert!(hid_keyboard_candidate_requires_force_mode(2));
    }

    #[test]
    fn hid_keyboard_attach_rank_rejects_non_keyboard_protocols() {
        assert_eq!(
            hid_keyboard_attach_rank(hid_subclass::BOOT, hid_protocol::MOUSE),
            None
        );
    }
}
