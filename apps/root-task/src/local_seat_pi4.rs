// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Raspberry Pi 4 local-seat backend (HDMI text mirror + USB keyboard ingress).
// Author: Lukas Bower

#![allow(unsafe_code)]

extern crate alloc;

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::cmp;
use core::hint::spin_loop;
use core::mem;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use font8x8::legacy::BASIC_LEGACY;
use spin::Mutex;
use usb_oxide::{
    class, completion, desc_type, find_hid_interfaces, hid_protocol, hid_subclass, hub_feature,
    hub_protocol, regs, request, scancode_to_ascii, set_xhci_diag_hook, ConfigDesc, DeviceDesc,
    Dma, HidDevice, HubDesc, SetupPacket, TtContext, UsbDevice, UsbError, XhciCtrl,
};

use crate::bootstrap::log as boot_log;
use crate::hal::{Hardware, KernelHal};

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
const TAG_NOTIFY_XHCI_RESET: u32 = 0x0003_0058;

const DEFAULT_FB_WIDTH: u32 = 1024;
const DEFAULT_FB_HEIGHT: u32 = 768;
const DEFAULT_FB_DEPTH: u32 = 32;
const DEFAULT_FB_ALIGNMENT: u32 = 16;
const PIXEL_ORDER_RGB: u32 = 1;

const CHAR_WIDTH: usize = 8;
const CHAR_HEIGHT: usize = 16;
const TAB_WIDTH: usize = 4;

const FG_COLOR: u32 = 0xFFFF_FFFF;
const BG_COLOR: u32 = 0xFF00_0000;
const RPI4_XHCI_MMIO_HIGH_CANDIDATE: usize = 0x0000_0006_0000_0000;
const RPI4_XHCI_MMIO_PRIMARY_CANDIDATE: usize = 0x0000_0000_FE98_0000;
const RPI4_XHCI_MMIO_SECONDARY_CANDIDATE: usize = 0x0000_0000_7E98_0000;

// Runtime xHCI probing fallbacks prefer the high UEFI aperture first, then
// legacy aliases.
const RPI4_XHCI_MMIO_FALLBACKS: [usize; 3] = [
    RPI4_XHCI_MMIO_HIGH_CANDIDATE,
    RPI4_XHCI_MMIO_PRIMARY_CANDIDATE,
    RPI4_XHCI_MMIO_SECONDARY_CANDIDATE,
];
// Boot-time xHCI pinning must only target controller BAR aliases.
const RPI4_XHCI_MMIO_PRESEED_CANDIDATES: [usize; 3] = [
    RPI4_XHCI_MMIO_HIGH_CANDIDATE,
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
const HUB_ENUM_MAX_DEPTH: usize = 2;
const HUB_MAX_DOWNSTREAM_PORTS: usize = 15;
const HUB_DESC_MAX_BYTES: usize = 12;
const HUB_PORT_STATUS_BYTES: usize = 4;
const HUB_PORT_STATUS_RETRY_LOOPS: usize = 64;
const HUB_PORT_STATUS_QUICK_RETRIES: usize = 4;
const HUB_SET_FEATURE_RETRIES: usize = 3;
const HUB_POST_CONFIG_SETTLE_MS: u64 = 250;
const HUB_POWER_SETTLE_MIN_MS: u64 = 200;
const HUB_RESET_SETTLE_MS: u64 = 50;
const HUB_PORT_STATUS_RETRY_DELAY_MS: u64 = 20;
const HUB_PORT_STATUS_QUICK_RETRY_DELAY_MS: u64 = 10;
const HUB_SET_FEATURE_RETRY_DELAY_MS: u64 = 10;
const HUB_CLASS_CONTROL_WAIT_SPINS: usize = 1_000_000;
const WAIT_MS_SPINS_PER_MS: usize = 50_000;
const WAIT_MS_MIN_SPINS: usize = 10_000;
const WAIT_MS_MAX_SPINS: usize = 25_000_000;
const USB_PROGRESS_TICK_MS: usize = 1_000;
const USB_PROGRESS_MAX_DOTS: usize = 64;
// Device untyped retype on seL4 is monotonic; retries can only consume more
// device window state without restoring earlier probe addresses.
const KEYBOARD_ATTACH_ATTEMPTS: usize = 1;
const KEYBOARD_RETRY_SPINS: usize = 200_000;
const VL805_PCI_DEV_ADDR: u32 = 0x0010_0000;
const VL805_NOTIFY_SETTLE_SPINS: usize = 50_000;
// On Pi4 + seL4 this mailbox reset notification can raise an early platform
// interrupt before IRQ handlers are installed, stalling boot at local-seat
// bring-up. Keep it disabled for now.
const VL805_RESET_NOTIFY_BEFORE_USB_PROBE: bool = false;
// Touching VL805 PCI config space during early bootstrap has correlated with
// fatal IRQ 27 entries on Pi4/seL4. Keep preseed writes disabled.
const VL805_CFG_PRESEED_TOUCH_ENABLED: bool = false;
// Runtime cfg-space touches can still trigger fatal IRQ 27 on some Pi4 UEFI
// paths. Keep this disabled until we have a proven non-faulting ECAM path.
const VL805_CFG_RUNTIME_TOUCH_ENABLED: bool = false;
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
const XHCI_MMIO_ALIAS_SCAN_PAGES: usize = XHCI_MMIO_PRESEED_BYTES_MAX / PAGE_SIZE;
const XHCI_HCI_VERSION_MIN: u16 = 0x0090;
const XHCI_HCI_VERSION_MAX: u16 = 0x0200;
// High-address DMA allocation has repeatedly stalled during Pi4 runtime xHCI
// bring-up. Force low DMA pool probing until high-path allocator faults are
// fully resolved.
const XHCI_FORCE_LOW_DMA_PROBE: bool = true;
// VL805 on Pi4 expects xHCI DMA pointers in the PCIe outbound DMA window
// address space, not raw CPU physical addresses.
const XHCI_PCIE_DMA_BUS_ALIAS_ENABLED: bool = true;
// Probe fallback for ambiguous Pi4/VL805 firmware mappings: try both PCIe bus
// aliased DMA addresses and raw physical addresses before giving up.
const XHCI_TRY_RAW_PHYS_DMA_FALLBACK: bool = true;
const RPI4_PCIE_DMA_BUS_OFFSET: usize = 0x4_0000_0000;
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

static VL805_RESET_NOTIFIED: AtomicBool = AtomicBool::new(false);
static VL805_RESET_BYPASSED_LOGGED: AtomicBool = AtomicBool::new(false);
static USB_DMA_RANGE_WARNED: AtomicBool = AtomicBool::new(false);
static VL805_CFG_SAFE_MODE_LOGGED: AtomicBool = AtomicBool::new(false);
static XHCI_PRESEED_ALREADY_PINNED_LOGGED: AtomicBool = AtomicBool::new(false);
static KEYBOARD_PRESEED_LOGGED: AtomicBool = AtomicBool::new(false);
static KEYBOARD_RUNTIME_INIT_LOGGED: AtomicBool = AtomicBool::new(false);
static XHCI_DIAG_HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);
static XHCI_MMIO_DIAG_LOGGED: AtomicBool = AtomicBool::new(false);
static XHCI_DMA_POLICY_LOGGED: AtomicBool = AtomicBool::new(false);
static VL805_CFG_VIRT: AtomicUsize = AtomicUsize::new(0);
static VL805_XHCI_MMIO_HINT: AtomicUsize = AtomicUsize::new(0);
static PINNED_XHCI_MMIO: Mutex<Option<PinnedMmioWindow>> = Mutex::new(None);
static PINNED_VL805_CFG: Mutex<Option<PinnedMmioWindow>> = Mutex::new(None);
static USB_PROGRESS_ACTIVE: AtomicBool = AtomicBool::new(false);
static USB_PROGRESS_DISPLAY_PTR: AtomicUsize = AtomicUsize::new(0);
static USB_PROGRESS_ELAPSED_MS: AtomicUsize = AtomicUsize::new(0);
static USB_PROGRESS_DOTS: AtomicUsize = AtomicUsize::new(0);

fn usb_progress_emit(dots: usize) {
    let ptr = USB_PROGRESS_DISPLAY_PTR.load(Ordering::Acquire);
    if ptr == 0 {
        return;
    }
    let mut line = heapless::String::<160>::new();
    let _ = line.push_str("[cohesix] starting USB subsystem ");
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

/// Concrete local-seat backend for Pi 4 (HDMI text + USB keyboard).
pub struct Pi4LocalSeat {
    display: HdmiTextSink,
    keyboard: Option<UsbKeyboard>,
    keyboard_init_attempted: bool,
    xhci_mmio_hint: Option<usize>,
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
            xhci_mmio_hint: hints.xhci_mmio_hint,
            hal_ptr: hal as *mut _ as usize,
        })
    }

    /// Preseed keyboard MMIO windows once UART bring-up has completed.
    pub fn preseed_keyboard_mmio(&mut self) {
        let Some(hal) = hal_from_ptr(self.hal_ptr) else {
            return;
        };
        let first_preseed = !KEYBOARD_PRESEED_LOGGED.swap(true, Ordering::AcqRel);
        if first_preseed {
            boot_log::force_uart_line("[local-seat] pi4 keyboard preseed begin");
        }
        // Keep bootstrap preseed side-effect free: pin mapping windows only.
        if VL805_CFG_PRESEED_TOUCH_ENABLED {
            prime_pinned_vl805_cfg_window(hal);
        } else if !VL805_CFG_SAFE_MODE_LOGGED.swap(true, Ordering::AcqRel) {
            boot_log::force_uart_line("[local-seat] vl805 pci cfg touch disabled (safe-mode)");
        }
        if first_preseed {
            boot_log::force_uart_line("[local-seat] pi4 xhci preseed begin");
        }
        prime_pinned_xhci_window(hal, self.xhci_mmio_hint);
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

    /// Poll USB keyboard and write canonical bytes into `out`.
    pub fn poll_keyboard_bytes(&mut self, out: &mut [u8]) -> usize {
        if self.keyboard.is_none() && !self.keyboard_init_attempted {
            self.keyboard_init_attempted = true;
            boot_log::force_uart_line("[local-seat] pi4 keyboard runtime init begin");
            usb_progress_begin(&mut self.display);
            self.preseed_keyboard_mmio();
            boot_log::force_uart_line("[local-seat] pi4 keyboard runtime init after preseed");
            let mut keyboard_error = None;
            if let Some(hal) = hal_from_ptr(self.hal_ptr) {
                for attempt in 1..=KEYBOARD_ATTACH_ATTEMPTS {
                    let mut line = heapless::String::<144>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut line,
                        format_args!("[local-seat] pi4 keyboard probe attempt={attempt}"),
                    );
                    boot_log::force_uart_line(line.as_str());
                    match UsbKeyboard::new(hal, self.xhci_mmio_hint) {
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
                            if attempt < KEYBOARD_ATTACH_ATTEMPTS {
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
            Some(keyboard) => keyboard.poll_bytes(out),
            None => 0,
        }
    }
}

struct Mailbox {
    regs: crate::sel4::DeviceFrame,
    regs_paddr: usize,
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
        return Err(error);
    };
    let span_bytes = coverage.limit.saturating_sub(coverage.base);
    let span_pages = cmp::max(1usize, div_ceil(span_bytes, PAGE_SIZE));
    let max_attempts = cmp::max(1usize, cmp::min(span_pages.saturating_add(1), attempt_cap));

    for attempt in 0..max_attempts {
        let frame = hal.map_device(paddr).map_err(|_| error)?;
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
    if size == 0 {
        return None;
    }
    let request_end = phys.checked_add(size)?;
    let pinned = PINNED_XHCI_MMIO.lock();
    let window = pinned.as_ref()?;
    let window_end = window.phys_start.checked_add(window.length)?;
    if phys < window.phys_start || request_end > window_end {
        return None;
    }
    let offset = phys.checked_sub(window.phys_start)?;
    window.virt_start.checked_add(offset)
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

fn xhci_diag_hook(stage: u16, a: u64, b: u64, c: u64) {
    let mut line = heapless::String::<176>::new();
    let _ = core::fmt::Write::write_fmt(
        &mut line,
        format_args!(
            "[local-seat] xhci.diag stage=0x{stage:04x} a=0x{a:016x} b=0x{b:016x} c=0x{c:016x}"
        ),
    );
    boot_log::force_uart_line(line.as_str());
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
    db_offset: u32,
    rts_offset: u32,
    max_slots: u8,
    max_ports: u8,
    max_scratchpad: u16,
    mmio_size: usize,
}

fn probe_xhci_capability_window(
    hal: &mut KernelHal<'_>,
    mmio_base: usize,
) -> Result<XhciCapProbe, &'static str> {
    let dma = SeatDma::new(hal, false, XHCI_PCIE_DMA_BUS_ALIAS_ENABLED);
    // SAFETY: Read-only capability probe over candidate xHCI MMIO.
    let init_mmio = unsafe { dma.map_mmio(mmio_base, XHCI_MMIO_INIT_BYTES) }.ok_or("map-init")?;
    // SAFETY: `init_mmio` points to a mapped MMIO page for volatile reads.
    let cap_length = unsafe { ptr::read_volatile((init_mmio + regs::CAPLENGTH) as *const u8) };
    // SAFETY: `init_mmio` points to a mapped MMIO page for volatile reads.
    let hci_version =
        unsafe { ptr::read_volatile((init_mmio + regs::CAPLENGTH + 2) as *const u16) };
    // SAFETY: `init_mmio` points to a mapped MMIO page for volatile reads.
    let hcs1 = unsafe { ptr::read_volatile((init_mmio + regs::HCSPARAMS1) as *const u32) };
    // SAFETY: `init_mmio` points to a mapped MMIO page for volatile reads.
    let hcs2 = unsafe { ptr::read_volatile((init_mmio + regs::HCSPARAMS2) as *const u32) };
    // SAFETY: `init_mmio` points to a mapped MMIO page for volatile reads.
    let db_offset = unsafe { ptr::read_volatile((init_mmio + regs::DBOFF) as *const u32) };
    // SAFETY: `init_mmio` points to a mapped MMIO page for volatile reads.
    let rts_offset = unsafe { ptr::read_volatile((init_mmio + regs::RTSOFF) as *const u32) };
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

fn probe_xhci_capability_with_alias_scan(
    hal: &mut KernelHal<'_>,
    mmio_base: usize,
) -> Result<(usize, XhciCapProbe), &'static str> {
    let probe = probe_xhci_capability_window(hal, mmio_base)?;
    if validate_xhci_capability_window(&probe).is_ok() {
        return Ok((mmio_base, probe));
    }

    // The Pi4 firmware/UEFI path can expose the VL805 xHCI block at an
    // offset inside legacy FE98 or high PCIe apertures. Probe nearby pages
    // to recover.
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
            "[local-seat] xhci cap scan base=0x{base:016x} pages={pages}",
            base = mmio_base,
            pages = XHCI_MMIO_ALIAS_SCAN_PAGES
        ),
    );
    boot_log::force_uart_line(scan_line.as_str());

    for page in 1..XHCI_MMIO_ALIAS_SCAN_PAGES {
        let Some(candidate) = mmio_base.checked_add(page.saturating_mul(PAGE_SIZE)) else {
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
                    "[local-seat] xhci cap relocated base=0x{base:016x} scan=0x{scan:016x} page={page}",
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
            "[local-seat] xhci cap scan exhausted base=0x{base:016x} pages={pages}",
            base = mmio_base,
            pages = XHCI_MMIO_ALIAS_SCAN_PAGES
        ),
    );
    boot_log::force_uart_line(exhausted.as_str());

    Err("cap-invalid")
}

fn prime_pinned_vl805_cfg_window(hal: &mut KernelHal<'_>) {
    if PINNED_VL805_CFG.lock().is_some() {
        return;
    }

    let mut prefix_frames = Vec::new();
    for &ecam_base in &VL805_ECAM_BASE_CANDIDATES {
        let Some(config_paddr) = ecam_base.checked_add(VL805_PCI_DEV_ADDR as usize) else {
            continue;
        };
        let config_page = config_paddr & !PAGE_MASK;
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
        remember_vl805_cfg_virt(config_virt);

        let vendor_device = pci_cfg_read_u32(config_virt, PCI_CFG_VENDOR_DEVICE);
        let class_revision = pci_cfg_read_u32(config_virt, PCI_CFG_CLASS_REVISION);
        let command_before =
            (pci_cfg_read_u32(config_virt, PCI_CFG_COMMAND_STATUS) & 0xffff) as u16;
        // Keep VL805 INTx masked during bring-up to avoid fatal IRQ 27 storms
        // while still enabling memory decode + DMA for xHCI rings.
        let command_required = command_before
            | PCI_COMMAND_MEMORY_SPACE
            | PCI_COMMAND_BUS_MASTER
            | PCI_COMMAND_INTERRUPT_DISABLE;
        if command_required != command_before {
            pci_cfg_write_u16(config_virt, PCI_CFG_COMMAND_STATUS, command_required);
        }
        let command_after = (pci_cfg_read_u32(config_virt, PCI_CFG_COMMAND_STATUS) & 0xffff) as u16;
        let bar0 = pci_cfg_read_u32(config_virt, PCI_CFG_BAR0);
        let bar1 = pci_cfg_read_u32(config_virt, PCI_CFG_BAR1);
        let bar_mmio = decode_pci_mmio_bar(bar0, bar1);
        core::mem::forget(frame);

        let mut pinned = PINNED_VL805_CFG.lock();
        *pinned = Some(PinnedMmioWindow {
            phys_start: config_page,
            length: PAGE_SIZE,
            virt_start: virt,
        });

        let mut line = heapless::String::<240>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[local-seat] vl805 pci cfg preseeded ecam=0x{ecam:016x} cfg=0x{cfg:016x} mode=read-mostly cfg_id=0x{cfg_id:08x} class=0x{class:06x} cmd=0x{before:04x}->0x{after:04x} bar0=0x{bar0:08x}",
                ecam = ecam_base,
                cfg = config_paddr,
                cfg_id = vendor_device,
                class = (class_revision >> 8) & 0x00ff_ffff,
                before = command_before,
                after = command_after,
            ),
        );
        boot_log::force_uart_line(line.as_str());
        let mut bar_line = heapless::String::<176>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut bar_line,
            format_args!("[local-seat] vl805 pci cfg bar bar0=0x{bar0:08x} bar1=0x{bar1:08x}"),
        );
        boot_log::force_uart_line(bar_line.as_str());
        if let Some(mmio) = bar_mmio {
            let mut hint = heapless::String::<176>::new();
            if xhci_mmio_candidate_valid(mmio) {
                remember_vl805_xhci_mmio_hint(mmio);
                let _ = core::fmt::Write::write_fmt(
                    &mut hint,
                    format_args!("[local-seat] vl805 pci cfg hint mmio=0x{mmio:016x}"),
                );
            } else {
                let _ = core::fmt::Write::write_fmt(
                    &mut hint,
                    format_args!(
                        "[local-seat] vl805 pci cfg hint rejected mmio=0x{mmio:016x} reason=invalid-candidate"
                    ),
                );
            }
            boot_log::force_uart_line(hint.as_str());
        } else {
            boot_log::force_uart_line(
                "[local-seat] vl805 pci cfg hint missing (BAR decode failed)",
            );
        }
        if (command_after & (PCI_COMMAND_MEMORY_SPACE | PCI_COMMAND_BUS_MASTER))
            != (PCI_COMMAND_MEMORY_SPACE | PCI_COMMAND_BUS_MASTER)
        {
            boot_log::force_uart_line(
                "[local-seat] vl805 pci cfg warning command bits missing after preseed",
            );
        }
        return;
    }

    boot_log::force_uart_line("[local-seat] vl805 pci cfg preseed unavailable");
}

fn prime_pinned_xhci_window(hal: &mut KernelHal<'_>, xhci_mmio_hint: Option<usize>) {
    if PINNED_XHCI_MMIO.lock().is_some() {
        if !XHCI_PRESEED_ALREADY_PINNED_LOGGED.swap(true, Ordering::AcqRel) {
            boot_log::force_uart_line("[local-seat] xhci preseed skipped (already pinned)");
        }
        return;
    }

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

    if let Some(hint) = xhci_mmio_hint {
        push_candidate(hint);
    }
    if let Some(hint) = current_vl805_xhci_mmio_hint() {
        push_candidate(hint);
    }
    for fallback in RPI4_XHCI_MMIO_PRESEED_CANDIDATES {
        push_candidate(fallback);
    }
    let mut plan = heapless::String::<192>::new();
    let _ = core::fmt::Write::write_fmt(
        &mut plan,
        format_args!(
            "[local-seat] xhci preseed candidates={} hint={}",
            candidate_count,
            if xhci_mmio_hint.is_some() {
                "yes"
            } else {
                "no"
            }
        ),
    );
    boot_log::force_uart_line(plan.as_str());

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
            if pin_xhci_mmio_window(hal, mmio, length).is_ok() {
                let mut line = heapless::String::<176>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] xhci mmio preseeded mmio=0x{mmio:016x} bytes=0x{bytes:05x}",
                        bytes = length
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

fn pin_xhci_mmio_window(
    hal: &mut KernelHal<'_>,
    phys_start: usize,
    length: usize,
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
    });
    Ok(())
}

impl Mailbox {
    fn new(hal: &mut KernelHal<'_>) -> Result<Self, Pi4SeatError> {
        let mut regs = None;
        let mut regs_paddr = 0usize;
        let mut prefix_maps = Vec::new();
        for &candidate in &MAILBOX_PAGE_PADDR_CANDIDATES {
            if let Ok(mapped) = Self::map_device_exact(hal, candidate, &mut prefix_maps) {
                regs = Some(mapped);
                regs_paddr = candidate;
                break;
            }
        }
        let regs = regs.ok_or(Pi4SeatError::MailboxMap)?;

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
            regs,
            regs_paddr,
            request,
            _prefix_maps: prefix_maps,
        })
    }

    fn notify_vl805_reset(&mut self) -> Result<(), Pi4SeatError> {
        let mut payload = [VL805_PCI_DEV_ADDR];
        self.call_tag(TAG_NOTIFY_XHCI_RESET, 4, &mut payload)?;
        for _ in 0..VL805_NOTIFY_SETTLE_SPINS {
            spin_loop();
        }
        Ok(())
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
        let base = self.regs.ptr().as_ptr() as usize;
        let Some(addr) = base.checked_add(offset) else {
            return 0;
        };
        // SAFETY: Register block was mapped as device memory by HAL.
        unsafe { ptr::read_volatile(addr as *const u32) }
    }

    fn write_reg(&self, offset: usize, value: u32) {
        let base = self.regs.ptr().as_ptr() as usize;
        let Some(addr) = base.checked_add(offset) else {
            return;
        };
        // SAFETY: Register block was mapped as device memory by HAL.
        unsafe {
            ptr::write_volatile(addr as *mut u32, value);
        }
    }
}

struct HdmiTextSink {
    width: usize,
    height: usize,
    pitch: usize,
    framebuffer_len: usize,
    cols: usize,
    rows: usize,
    row: usize,
    col: usize,
    framebuffer: *mut u8,
    backend: HdmiBackend,
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
                "[local-seat] fb mailbox bus=0x{bus:08x} phys=0x{phys:08x} end=0x{end:08x} size={size} pitch={pitch} {width}x{height}",
                bus = fb_bus,
                phys = fb_phys,
                end = fb_end,
                size = fb_size,
                pitch = pitch[0],
                width = virt[0],
                height = virt[1],
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
        Self::from_mapped_framebuffer(
            hal,
            fb_phys,
            fb_size,
            virt[0] as usize,
            virt[1] as usize,
            pitch[0] as usize,
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

        let mut sink = Self {
            width,
            height,
            pitch,
            framebuffer_len,
            cols: cmp::max(1, width / CHAR_WIDTH),
            rows: cmp::max(1, height / CHAR_HEIGHT),
            row: 0,
            col: 0,
            framebuffer,
            backend,
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
        for &byte in line.as_bytes() {
            self.put_byte(byte);
        }
        self.newline();
    }

    fn put_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => self.newline(),
            b'\r' => self.col = 0,
            b'\t' => {
                for _ in 0..TAB_WIDTH {
                    self.put_byte(b' ');
                }
            }
            _ => {
                if self.col >= self.cols {
                    self.newline();
                }
                self.draw_char(byte);
                self.col = self.col.saturating_add(1);
            }
        }
    }

    fn newline(&mut self) {
        self.col = 0;
        self.row = self.row.saturating_add(1);
        if self.row >= self.rows {
            self.clear_screen();
            self.row = 0;
        }
    }

    fn draw_char(&mut self, byte: u8) {
        let glyph = BASIC_LEGACY[usize::from(byte.min(0x7F))];
        let x0 = self.col.saturating_mul(CHAR_WIDTH);
        let y0 = self.row.saturating_mul(CHAR_HEIGHT);

        self.fill_rect(x0, y0, CHAR_WIDTH, CHAR_HEIGHT, BG_COLOR);

        for (gy, bits) in glyph.iter().enumerate() {
            for gx in 0..8 {
                if ((bits >> gx) & 1) == 0 {
                    continue;
                }
                let x = x0.saturating_add(gx);
                let y = y0.saturating_add(gy.saturating_mul(2));
                self.put_pixel(x, y, FG_COLOR);
                self.put_pixel(x, y.saturating_add(1), FG_COLOR);
            }
        }
    }

    fn clear_screen(&mut self) {
        self.fill_rect(0, 0, self.width, self.height, BG_COLOR);
    }

    fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: u32) {
        let x_end = cmp::min(self.width, x.saturating_add(w));
        let y_end = cmp::min(self.height, y.saturating_add(h));
        for yy in y..y_end {
            for xx in x..x_end {
                self.put_pixel(xx, yy, color);
            }
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
            ptr::write_volatile(addr, color);
        }
    }
}

impl Drop for HdmiTextSink {
    fn drop(&mut self) {
        let _ = self.mappings.len();
    }
}

fn notify_vl805_reset_once(hal: &mut KernelHal<'_>) {
    if VL805_RESET_NOTIFIED.load(Ordering::Acquire) {
        return;
    }

    let result = Mailbox::new(hal).and_then(|mut mailbox| mailbox.notify_vl805_reset());
    match result {
        Ok(()) => {
            VL805_RESET_NOTIFIED.store(true, Ordering::Release);
            boot_log::force_uart_line("[local-seat] vl805 reset notify=ok");
        }
        Err(err) => {
            let mut line = heapless::String::<160>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] vl805 reset notify=skipped detail={}",
                    err.as_str()
                ),
            );
            boot_log::force_uart_line(line.as_str());
        }
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
        // Keep VL805 INTx masked during bring-up to avoid fatal IRQ 27 storms
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
    last_keys: [u8; 6],
    poll_error_logged: bool,
    first_report_logged: bool,
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
    fn new(hal: &mut KernelHal<'_>, xhci_mmio_hint: Option<usize>) -> Result<Self, Pi4SeatError> {
        if !KEYBOARD_RUNTIME_INIT_LOGGED.swap(true, Ordering::AcqRel) {
            boot_log::force_uart_line("[local-seat] usb keyboard init path entered");
        }
        if !XHCI_DIAG_HOOK_INSTALLED.swap(true, Ordering::AcqRel) {
            set_xhci_diag_hook(Some(xhci_diag_hook));
            boot_log::force_uart_line("[local-seat] xhci diag hook installed");
        }
        if VL805_CFG_RUNTIME_TOUCH_ENABLED {
            if current_vl805_cfg_virt().is_some() {
                boot_log::force_uart_line("[local-seat] vl805 cfg present stage=usb-init-entry");
            } else {
                boot_log::force_uart_line("[local-seat] vl805 cfg missing stage=usb-init-entry");
            }
        } else if !VL805_CFG_SAFE_MODE_LOGGED.swap(true, Ordering::AcqRel) {
            boot_log::force_uart_line("[local-seat] vl805 pci cfg touch disabled (safe-mode)");
        }

        if VL805_RESET_NOTIFY_BEFORE_USB_PROBE {
            notify_vl805_reset_once(hal);
        } else if !VL805_RESET_BYPASSED_LOGGED.swap(true, Ordering::AcqRel) {
            boot_log::force_uart_line("[local-seat] vl805 reset notify=bypassed");
        }
        let (vl805_pci_mmio, pci_cfg_ready) = if VL805_CFG_RUNTIME_TOUCH_ENABLED {
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

        let mut candidates = [0usize; XHCI_MMIO_CANDIDATE_LIMIT];
        let mut candidate_count = 0usize;
        let mut consider_candidate = |mmio: usize| {
            if candidate_count >= candidates.len() {
                return;
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
                return;
            }
            if hal.device_coverage(mmio, crate::sel4::PAGE_BITS).is_none()
                && pinned_xhci_window_lookup(mmio, PAGE_SIZE).is_none()
            {
                if mmio == RPI4_XHCI_MMIO_PRIMARY_CANDIDATE || mmio == RPI4_XHCI_MMIO_HIGH_CANDIDATE
                {
                    let mut line = heapless::String::<208>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut line,
                        format_args!(
                            "[local-seat] xhci candidate forcing probe mmio=0x{mmio:016x} reason=no-device-coverage"
                        ),
                    );
                    boot_log::force_uart_line(line.as_str());
                } else {
                    let mut line = heapless::String::<192>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut line,
                        format_args!(
                            "[local-seat] xhci candidate skipped mmio=0x{mmio:016x} reason=no-device-coverage"
                        ),
                    );
                    boot_log::force_uart_line(line.as_str());
                    return;
                }
            }
            if candidates[..candidate_count].contains(&mmio) {
                return;
            }
            candidates[candidate_count] = mmio;
            candidate_count = candidate_count.saturating_add(1);
        };
        match vl805_pci_mmio {
            Some(vl805_mmio) => {
                consider_candidate(vl805_mmio);
                if let Some(hint) = xhci_mmio_hint {
                    if hint != vl805_mmio {
                        let mut line = heapless::String::<208>::new();
                        let _ = core::fmt::Write::write_fmt(
                            &mut line,
                            format_args!(
                                "[local-seat] xhci hint ignored hint=0x{hint:016x} verified=0x{verified:016x}",
                                verified = vl805_mmio
                            ),
                        );
                        boot_log::force_uart_line(line.as_str());
                    }
                }
            }
            None => {
                if let Some(hint) = xhci_mmio_hint {
                    consider_candidate(hint);
                }
                if let Some(hint) = current_vl805_xhci_mmio_hint() {
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
                    if XHCI_PCIE_DMA_BUS_ALIAS_ENABLED && XHCI_TRY_RAW_PHYS_DMA_FALLBACK {
                        "pcie-alias-then-phys"
                    } else if XHCI_PCIE_DMA_BUS_ALIAS_ENABLED {
                        "pcie-alias-only"
                    } else {
                        "phys-only"
                    }
                ),
            );
            boot_log::force_uart_line(line.as_str());
        }

        let mut saw_controller = false;
        let mut saw_keyboard_init_error = false;
        for &mmio_base in &candidates[..candidate_count] {
            let raw_probe = match probe_xhci_capability_window(hal, mmio_base) {
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
            };

            let mut cap_line = heapless::String::<320>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut cap_line,
                format_args!(
                    "[local-seat] xhci cap mmio=0x{mmio:016x} caplen=0x{caplen:02x} hciver=0x{hciver:04x} hcs1=0x{hcs1:08x} hcs2=0x{hcs2:08x} dboff=0x{dboff:08x} rtsoff=0x{rtsoff:08x} slots={} ports={} scratch={} span=0x{span:05x}",
                    raw_probe.max_slots,
                    raw_probe.max_ports,
                    raw_probe.max_scratchpad,
                    mmio = mmio_base,
                    caplen = raw_probe.cap_length,
                    hciver = raw_probe.hci_version,
                    hcs1 = raw_probe.hcs1,
                    hcs2 = raw_probe.hcs2,
                    dboff = raw_probe.db_offset,
                    rtsoff = raw_probe.rts_offset,
                    span = raw_probe.mmio_size,
                ),
            );
            boot_log::force_uart_line(cap_line.as_str());

            let (effective_mmio, _cap_probe) = match validate_xhci_capability_window(&raw_probe) {
                Ok(()) => (mmio_base, raw_probe),
                Err(reason) => match probe_xhci_capability_with_alias_scan(hal, mmio_base) {
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
                        (candidate, scanned_probe)
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
                },
            };

            // Keep probe order deterministic so field logs are directly
            // comparable across boots.
            let dma_probe_order: &[bool] = if XHCI_FORCE_LOW_DMA_PROBE {
                &[false]
            } else {
                &[false, true]
            };
            for &prefer_high in dma_probe_order {
                let dma_bus_modes: &[bool] =
                    if XHCI_PCIE_DMA_BUS_ALIAS_ENABLED && XHCI_TRY_RAW_PHYS_DMA_FALLBACK {
                        &[true, false]
                    } else if XHCI_PCIE_DMA_BUS_ALIAS_ENABLED {
                        &[true]
                    } else {
                        &[false]
                    };
                for (bus_mode_idx, &pcie_dma_bus_alias) in dma_bus_modes.iter().enumerate() {
                    let mut probe_line = heapless::String::<224>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut probe_line,
                        format_args!(
                            "[local-seat] xhci probe begin mmio=0x{mmio:016x} dma={} bus={}",
                            if prefer_high { "high" } else { "low" },
                            if pcie_dma_bus_alias {
                                "pcie-alias"
                            } else {
                                "phys"
                            },
                            mmio = effective_mmio
                        ),
                    );
                    boot_log::force_uart_line(probe_line.as_str());

                    let dma = SeatDma::new(hal, prefer_high, pcie_dma_bus_alias);
                    let ctrl = match XhciCtrl::new(effective_mmio, dma) {
                        Ok(ctrl) => {
                            saw_controller = true;
                            Arc::new(ctrl)
                        }
                        Err(err) => {
                            let mut line = heapless::String::<224>::new();
                            let _ = core::fmt::Write::write_fmt(
                                &mut line,
                                format_args!(
                                    "[local-seat] xhci probe failed mmio=0x{mmio:016x} dma={} bus={} detail={err:?}",
                                    if prefer_high { "high" } else { "low" },
                                    if pcie_dma_bus_alias {
                                        "pcie-alias"
                                    } else {
                                        "phys"
                                    },
                                    mmio = effective_mmio
                                ),
                            );
                            boot_log::force_uart_line(line.as_str());
                            if bus_mode_idx + 1 < dma_bus_modes.len() {
                                let next_bus = if dma_bus_modes[bus_mode_idx + 1] {
                                    "pcie-alias"
                                } else {
                                    "phys"
                                };
                                let mut fallback_line = heapless::String::<256>::new();
                                let _ = core::fmt::Write::write_fmt(
                                    &mut fallback_line,
                                    format_args!(
                                        "[local-seat] xhci probe fallback mmio=0x{mmio:016x} dma={} from_bus={} to_bus={} reason={err:?}",
                                        if prefer_high { "high" } else { "low" },
                                        if pcie_dma_bus_alias {
                                            "pcie-alias"
                                        } else {
                                            "phys"
                                        },
                                        next_bus,
                                        mmio = effective_mmio
                                    ),
                                );
                                boot_log::force_uart_line(fallback_line.as_str());
                            }
                            continue;
                        }
                    };

                    let max_ports = cmp::min(ctrl.max_ports() as usize, XHCI_MAX_PROBE_PORTS);
                    let mut connected_mask = 0u32;
                    let mut detect_passes_used = 1usize;
                    for pass in 0..XHCI_PORT_DETECT_PASSES {
                        detect_passes_used = pass.saturating_add(1);
                        connected_mask = 0;
                        for port in 0..max_ports {
                            if ctrl.port_connected(port as u8) {
                                connected_mask |= 1u32 << port;
                            }
                        }
                        if connected_mask != 0 || pass + 1 >= XHCI_PORT_DETECT_PASSES {
                            break;
                        }
                        for _ in 0..XHCI_PORT_DETECT_SETTLE_SPINS {
                            spin_loop();
                        }
                    }

                    let mut line = heapless::String::<224>::new();
                    let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] xhci online mmio=0x{mmio:016x} dma={} bus={} ports={} ctx={} connected_mask=0x{mask:04x} detect_passes={}",
                        if prefer_high { "high" } else { "low" },
                        if pcie_dma_bus_alias {
                            "pcie-alias"
                        } else {
                            "phys"
                        },
                        max_ports,
                        ctrl.context_size_bytes(),
                        detect_passes_used,
                        mmio = effective_mmio,
                        mask = connected_mask,
                    ),
                );
                    boot_log::force_uart_line(line.as_str());

                    for port in 0..max_ports {
                        if (connected_mask & (1u32 << port)) == 0 {
                            continue;
                        }

                        let mut device = match UsbDevice::new(ctrl.clone(), port as u8) {
                            Ok(device) => device,
                            Err(err) => {
                                let mut kind_line = heapless::String::<192>::new();
                                let _ = core::fmt::Write::write_fmt(
                                    &mut kind_line,
                                    format_args!(
                                        "[local-seat] usb root-enum classify port={} stage=address kind={} dma={} bus={}",
                                        port + 1,
                                        usb_address_error_kind(err),
                                        if prefer_high { "high" } else { "low" },
                                        if pcie_dma_bus_alias {
                                            "pcie-alias"
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
                                    if pcie_dma_bus_alias {
                                        "pcie-alias"
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
                        if let Err(err) = device.set_configuration(config.configuration) {
                            let mut line = heapless::String::<192>::new();
                            let _ = core::fmt::Write::write_fmt(
                            &mut line,
                            format_args!(
                                "[local-seat] usb root-enum failed port={} stage=set-config({}) detail={err:?}",
                                port + 1,
                                config.configuration
                            ),
                        );
                            boot_log::force_uart_line(line.as_str());
                            continue;
                        }

                        let device = Arc::new(device);
                        if let Some(hid) = Self::probe_device_for_keyboard(
                            device,
                            device_desc,
                            &config_blob,
                            HUB_ENUM_MAX_DEPTH,
                            &mut saw_keyboard_init_error,
                        ) {
                            hid.device().ctrl().host().seal_runtime();
                            return Ok(Self {
                                hid,
                                last_keys: [0; 6],
                                poll_error_logged: false,
                                first_report_logged: false,
                            });
                        }
                    }
                }
            }
        }

        if saw_keyboard_init_error {
            Err(Pi4SeatError::UsbKeyboardInit)
        } else if saw_controller {
            Err(Pi4SeatError::UsbKeyboardMissing)
        } else {
            Err(Pi4SeatError::XhciInit)
        }
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
        for (iface, ep_in) in interfaces {
            if iface.interface_subclass != hid_subclass::BOOT
                || iface.interface_protocol != hid_protocol::KEYBOARD
            {
                continue;
            }
            let hid = match HidDevice::from_interface(device.clone(), &iface, &ep_in) {
                Ok(hid) => hid,
                Err(_) => {
                    *saw_keyboard_init_error = true;
                    continue;
                }
            };
            if hid.queue_read().is_err() {
                *saw_keyboard_init_error = true;
                continue;
            }
            let mut line = heapless::String::<208>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] usb hid keyboard ready slot={} iface={} ep=0x{:02x}",
                    device.slot_id(),
                    iface.interface_number,
                    ep_in.endpoint_address
                ),
            );
            boot_log::force_uart_line(line.as_str());
            return Some(hid);
        }
        None
    }

    fn hub_interface_info(device_desc: DeviceDesc, config_blob: &[u8]) -> Option<HubInterfaceInfo> {
        if device_desc.device_class == class::HUB {
            let protocol = device_desc.device_protocol;
            return Some(HubInterfaceInfo {
                protocol,
                multi_tt: protocol == hub_protocol::HI_SPEED_MULTI_TT,
                interface_number: 0,
            });
        }

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

        let mut hub_line = heapless::String::<224>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut hub_line,
            format_args!(
                "[local-seat] hub desc slot={} iface={} ports={} pwr_mode={} pwr2good={}",
                device.slot_id(),
                hub_interface_number,
                max_ports,
                hub_desc.power_switching_mode(),
                hub_desc.pwr_on_2_pwr_good,
            ),
        );
        boot_log::force_uart_line(hub_line.as_str());

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
        let mut reset_feature_failed = 0usize;
        let mut ready_timeout = 0usize;
        let mut blind_probe_attempted = 0usize;
        let mut blind_probe_succeeded = 0usize;

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
                    Self::log_hub_port_terminal(
                        device.slot_id(),
                        downstream_port,
                        "disconnected-pre-reset",
                    );
                    continue;
                }
            } else {
                unavailable_pre_reset = unavailable_pre_reset.saturating_add(1);
                Self::log_hub_port_status_unavailable(
                    device.slot_id(),
                    downstream_port,
                    "pre-reset",
                );
                blind_probe_attempted = blind_probe_attempted.saturating_add(1);
                match Self::probe_hub_child_without_port_status(
                    &device,
                    downstream_port,
                    hub_multi_tt,
                    depth_remaining,
                    saw_keyboard_init_error,
                ) {
                    HubChildProbeResult::Keyboard(hid) => {
                        blind_probe_succeeded = blind_probe_succeeded.saturating_add(1);
                        return Some(hid);
                    }
                    HubChildProbeResult::ProbedNoKeyboard => {
                        continue;
                    }
                    HubChildProbeResult::Failed => {}
                }
                Self::log_hub_port_terminal(
                    device.slot_id(),
                    downstream_port,
                    "pre-status-unavailable",
                );
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
                depth_remaining,
                saw_keyboard_init_error,
                "status-path",
            ) {
                HubChildProbeResult::Keyboard(hid) => return Some(hid),
                HubChildProbeResult::ProbedNoKeyboard | HubChildProbeResult::Failed => continue,
            }
        }

        let mut summary = heapless::String::<256>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut summary,
            format_args!(
                "[local-seat] hub scan summary slot={} iface={} attempted_ports={} pre_status_unavailable={} disconnected_pre_reset={} reset_feature_failed={} ready_timeout={} blind_probe_attempted={} blind_probe_succeeded={}",
                device.slot_id(),
                hub_interface_number,
                attempted_ports,
                unavailable_pre_reset,
                disconnected_pre_reset,
                reset_feature_failed,
                ready_timeout,
                blind_probe_attempted,
                blind_probe_succeeded
            ),
        );
        boot_log::force_uart_line(summary.as_str());

        None
    }

    fn probe_hub_child_without_port_status(
        device: &Arc<UsbDevice<SeatDma>>,
        downstream_port: u8,
        hub_multi_tt: bool,
        depth_remaining: usize,
        saw_keyboard_init_error: &mut bool,
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
                    "[local-seat] hub blind-probe slot={} port={} route=0x{route:05x} speed={} source=pre-status-unavailable",
                    device.slot_id(),
                    downstream_port,
                    child_speed
                ),
            );
            boot_log::force_uart_line(line.as_str());

            match Self::probe_hub_child_with_route_and_speed(
                device,
                downstream_port,
                route,
                child_speed,
                hub_multi_tt,
                depth_remaining,
                saw_keyboard_init_error,
                "blind-pre-status",
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

    fn probe_hub_child_with_route_and_speed(
        device: &Arc<UsbDevice<SeatDma>>,
        downstream_port: u8,
        route: u32,
        child_speed: u8,
        hub_multi_tt: bool,
        depth_remaining: usize,
        saw_keyboard_init_error: &mut bool,
        source: &str,
    ) -> HubChildProbeResult {
        let tt_context = if (child_speed == usb_oxide::regs::SPEED_LOW
            || child_speed == usb_oxide::regs::SPEED_FULL)
            && device.speed() == usb_oxide::regs::SPEED_HIGH
        {
            Some(TtContext {
                hub_slot_id: device.slot_id(),
                downstream_port,
                multi_tt: hub_multi_tt,
            })
        } else {
            None
        };

        let mut child = match UsbDevice::new_routed(
            device.ctrl().clone(),
            route,
            device.root_hub_port(),
            child_speed,
            tt_context,
        ) {
            Ok(child) => child,
            Err(err) => {
                let mut line = heapless::String::<256>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] hub child failed slot={} port={} stage=address speed={} source={} detail={err:?}",
                        device.slot_id(),
                        downstream_port,
                        child_speed,
                        source
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
        if let Err(err) = child.set_configuration(config.configuration) {
            let mut line = heapless::String::<272>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!(
                    "[local-seat] hub child failed slot={} port={} stage=set-config({}) speed={} source={} detail={err:?}",
                    device.slot_id(),
                    downstream_port,
                    config.configuration,
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
        let setup = SetupPacket::new(
            0xA0,
            request::GET_DESCRIPTOR,
            (descriptor_type as u16) << 8,
            // Linux and USB hub class requests issue hub descriptor reads
            // against the hub device with wIndex=0.
            0,
            blob.len() as u16,
        );
        let transferred = match device.control_transfer(&setup, Some(&mut blob)) {
            Ok(transferred) => transferred,
            Err(err) => {
                let mut line = heapless::String::<288>::new();
                let _ = core::fmt::Write::write_fmt(
                    &mut line,
                    format_args!(
                        "[local-seat] hub desc read fail slot={} iface={} req=(bm=0xa0,b=0x{:02x},wValue=0x{:04x},wIndex=0x{:04x},wLen=0x{:04x}) detail={err:?}",
                        device.slot_id(),
                        hub_interface_number,
                        request::GET_DESCRIPTOR,
                        (descriptor_type as u16) << 8,
                        0,
                        blob.len() as u16
                    ),
                );
                boot_log::force_uart_line(line.as_str());
                return None;
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
    const fn hub_port_index(_interface_number: u8, port: u8) -> u16 {
        // Port-recipient hub requests encode the downstream port in wIndex.
        port as u16
    }

    fn hub_set_feature(
        device: &UsbDevice<SeatDma>,
        hub_interface_number: u8,
        feature: u16,
        port: u8,
    ) -> core::result::Result<(), UsbError> {
        let setup = SetupPacket::new(
            0x23,
            request::SET_FEATURE,
            feature,
            Self::hub_port_index(hub_interface_number, port),
            0,
        );
        device
            .control_transfer_with_wait_spins(&setup, None, HUB_CLASS_CONTROL_WAIT_SPINS)
            .map(|_| ())
    }

    #[inline]
    const fn hub_reset_feature(hub_protocol_code: u8) -> u16 {
        if hub_protocol_code == hub_protocol::SUPER_SPEED {
            hub_feature::BH_PORT_RESET
        } else {
            hub_feature::PORT_RESET
        }
    }

    fn hub_power_on_ports(
        device: &UsbDevice<SeatDma>,
        max_ports: usize,
        hub_interface_number: u8,
        power_mode: u8,
        pwr_on_2_pwr_good: u8,
    ) {
        let mut powered_ports = 0usize;
        // Only individual-switched hubs (mode 1) need per-port PORT_POWER
        // commands. Issuing PORT_POWER to ganged/no-switching hubs can stall
        // on some Pi4 topologies and break downstream status/reset probing.
        if power_mode == 1 {
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
                }
            }
        } else {
            let reason = if power_mode == 0 {
                "ganged-port-power"
            } else {
                "no-port-power-switching"
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
            match Self::hub_set_feature(device, hub_interface_number, feature, port) {
                Ok(()) => {
                    if attempt > 1 {
                        let mut line = heapless::String::<224>::new();
                        let _ = core::fmt::Write::write_fmt(
                            &mut line,
                            format_args!(
                                "[local-seat] hub ctl retry-ok slot={} iface={} port={} stage={} feature=0x{:04x} attempt={}/{}",
                                device.slot_id(),
                                hub_interface_number,
                                port,
                                stage,
                                feature,
                                attempt,
                                max_attempts
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

    fn hub_clear_feature(
        device: &UsbDevice<SeatDma>,
        hub_interface_number: u8,
        feature: u16,
        port: u8,
    ) -> bool {
        let setup = SetupPacket::new(
            0x23,
            request::CLEAR_FEATURE,
            feature,
            Self::hub_port_index(hub_interface_number, port),
            0,
        );
        device
            .control_transfer_with_wait_spins(&setup, None, HUB_CLASS_CONTROL_WAIT_SPINS)
            .is_ok()
    }

    fn hub_port_status_read(
        device: &UsbDevice<SeatDma>,
        hub_interface_number: u8,
        port: u8,
    ) -> Result<HubPortStatus, HubPortStatusReadError> {
        let mut bytes = [0u8; HUB_PORT_STATUS_BYTES];
        let setup = SetupPacket::new(
            0xA3,
            request::GET_STATUS,
            0,
            Self::hub_port_index(hub_interface_number, port),
            HUB_PORT_STATUS_BYTES as u16,
        );
        let transferred = match device.control_transfer_with_wait_spins(
            &setup,
            Some(&mut bytes),
            HUB_CLASS_CONTROL_WAIT_SPINS,
        ) {
            Ok(transferred) => transferred,
            Err(err) => return Err(HubPortStatusReadError::Control(err)),
        };
        if transferred < HUB_PORT_STATUS_BYTES {
            return Err(HubPortStatusReadError::ShortTransfer { transferred, bytes });
        }
        Ok(HubPortStatus {
            status: u16::from_le_bytes([bytes[0], bytes[1]]),
            change: u16::from_le_bytes([bytes[2], bytes[3]]),
        })
    }

    fn hub_port_status(
        device: &UsbDevice<SeatDma>,
        hub_interface_number: u8,
        port: u8,
    ) -> Option<HubPortStatus> {
        Self::hub_port_status_read(device, hub_interface_number, port).ok()
    }

    fn hub_port_status_with_retry(
        device: &UsbDevice<SeatDma>,
        hub_interface_number: u8,
        port: u8,
        stage: &str,
    ) -> Option<HubPortStatus> {
        for attempt in 1..=HUB_PORT_STATUS_QUICK_RETRIES {
            match Self::hub_port_status_read(device, hub_interface_number, port) {
                Ok(status) => {
                    let raw = status.raw_bytes();
                    let mut line = heapless::String::<256>::new();
                    let _ = core::fmt::Write::write_fmt(
                        &mut line,
                        format_args!(
                            "[local-seat] hub status read slot={} iface={} port={} stage={} attempt={}/{} status=0x{:04x} change=0x{:04x} raw={:02x}{:02x}{:02x}{:02x}",
                            device.slot_id(),
                            hub_interface_number,
                            port,
                            stage,
                            attempt,
                            HUB_PORT_STATUS_QUICK_RETRIES,
                            status.status,
                            status.change,
                            raw[0],
                            raw[1],
                            raw[2],
                            raw[3]
                        ),
                    );
                    boot_log::force_uart_line(line.as_str());
                    return Some(status);
                }
                Err(err) => {
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
        for attempt in 0..HUB_PORT_STATUS_RETRY_LOOPS {
            if let Some(status) = Self::hub_port_status(device, hub_interface_number, port) {
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
            wait_ms(HUB_PORT_STATUS_RETRY_DELAY_MS);
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
                        HUB_PORT_STATUS_RETRY_LOOPS,
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
                        HUB_PORT_STATUS_RETRY_LOOPS
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

    fn log_hub_status_read_error(
        slot_id: u8,
        hub_interface_number: u8,
        port: u8,
        stage: &str,
        attempt: usize,
        max_attempts: usize,
        err: HubPortStatusReadError,
    ) {
        let w_index = Self::hub_port_index(hub_interface_number, port);
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
            Err(_) => {
                if !self.poll_error_logged {
                    boot_log::force_uart_line(
                        "[local-seat] pi4 keyboard read queue failed detail=usb-queue-read",
                    );
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
            if let Some(ch) = scancode_to_ascii(key, shift) {
                if written >= out.len() {
                    break;
                }
                out[written] = ch as u8;
                written = written.saturating_add(1);
            }
        }

        self.last_keys = report.keys;
        written
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
    pcie_dma_bus_alias: bool,
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
    fn new(hal: &mut KernelHal<'_>, prefer_high: bool, pcie_dma_bus_alias: bool) -> Self {
        Self {
            state: Mutex::new(SeatDmaState {
                hal_ptr: hal as *mut _ as usize,
                prefer_high,
                pcie_dma_bus_alias,
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

        let mut frames = Vec::with_capacity(page_count);
        let mut expected_phys = 0usize;
        let mut expected_virt = 0usize;
        for idx in 0..page_count {
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
            let mut got_line = heapless::String::<224>::new();
            let _ = core::fmt::Write::write_fmt(
                &mut got_line,
                format_args!(
                    "[local-seat] xhci dma frame ready idx={} paddr=0x{phys:016x} vaddr=0x{virt:016x}",
                    idx
                ),
            );
            boot_log::force_uart_line(got_line.as_str());
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
                            if state.pcie_dma_bus_alias {
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
    if !XHCI_PCIE_DMA_BUS_ALIAS_ENABLED {
        return Some(phys);
    }
    phys.checked_add(RPI4_PCIE_DMA_BUS_OFFSET)
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
const fn div_ceil(value: usize, divisor: usize) -> usize {
    if value == 0 {
        0
    } else {
        1 + ((value - 1) / divisor)
    }
}

#[inline]
fn append_route_segment(route: u32, downstream_port: u8) -> Option<u32> {
    let mut depth = 0u32;
    let mut rem = route & 0x000f_ffff;
    while rem != 0 {
        depth = depth.saturating_add(1);
        rem >>= 4;
    }
    if depth >= 5 {
        return None;
    }
    let segment = cmp::min(downstream_port, 15) as u32;
    Some((route & 0x000f_ffff) | (segment << (depth * 4)))
}

#[inline]
fn read_config_desc(config_blob: &[u8]) -> Option<ConfigDesc> {
    if config_blob.len() < mem::size_of::<ConfigDesc>() {
        return None;
    }
    // SAFETY: The descriptor bytes may be unaligned in the returned USB blob.
    Some(unsafe { ptr::read_unaligned(config_blob.as_ptr().cast::<ConfigDesc>()) })
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
    use super::decode_pci_mmio_bar;

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
}
