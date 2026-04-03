// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide HAL-owned Pi 4 mailbox and SDIO host support for the CYW43455 Wi-Fi path.
// Author: Lukas Bower

#![allow(unsafe_code)]

use core::cmp;
use core::hint::spin_loop;
use core::mem;
use core::ptr;
use core::sync::atomic::{fence, Ordering};

use super::{
    HalError, Hardware, SdioBusWidth, SdioFunction, WifiDebugSnapshot, WifiFirmwareBundle,
    WifiPowerState, WifiResetState,
};
use crate::bootstrap::log as boot_log;
#[cfg(all(feature = "kernel", target_arch = "aarch64", target_os = "none"))]
use crate::local_seat_pi4::{wifi_progress_advance_loops, wifi_progress_tick};
use crate::rust_alloc::vec::Vec;
use crate::sel4::{page_get_address, DeviceFrame, PAGE_BITS};
use spin::Mutex;

#[cfg(not(all(feature = "kernel", target_arch = "aarch64", target_os = "none")))]
#[inline]
fn wifi_progress_advance_loops(_loops: usize) {}

#[cfg(not(all(feature = "kernel", target_arch = "aarch64", target_os = "none")))]
#[inline]
fn wifi_progress_tick() {}

include!(concat!(env!("OUT_DIR"), "/pi4_wifi_firmware.rs"));

const MAILBOX_PAGE_PADDR_CANDIDATES: [usize; 2] = [0xFE00_B000, 0x7E00_B000];
const GPIO_PAGE_PADDR_CANDIDATES: [usize; 2] = [0xFE20_0000, 0x7E20_0000];
const SDHCI_PAGE_PADDR_CANDIDATES: [usize; 2] = [0xFE30_0000, 0x7E30_0000];
const VC_BUS_ALIAS_BASES: [u32; 2] = [0xC000_0000, 0x4000_0000];
const VC_BUS_MASK: u32 = 0x3FFF_FFFF;
const PAGE_SIZE: usize = 1 << PAGE_BITS;

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
// The runtime VL805 reset-notify path can fall back to a posted property call
// if VideoCore does not acknowledge quickly. Keep the synchronous grace window
// deliberately shorter than the generic property path so runtime does not burn
// tens of millions of extra spins before the posted fallback can run.
const MAILBOX_WAIT_SPINS_NOTIFY_XHCI_RESET: usize = 5_000_000;
const MAILBOX_DRAIN_LIMIT: usize = 64;
const MAP_EXACT_ATTEMPT_CAP: usize = 2048;

const TAG_SET_POWER_STATE: u32 = 0x0002_8001;
const TAG_GET_CLOCK_RATE: u32 = 0x0003_0002;
const TAG_GET_MAX_CLOCK_RATE: u32 = 0x0003_0004;
const TAG_NOTIFY_XHCI_RESET: u32 = 0x0003_0058;
const TAG_GET_GPIO_STATE: u32 = 0x0003_0041;
const TAG_SET_GPIO_STATE: u32 = 0x0003_8041;
const TAG_GET_GPIO_CONFIG: u32 = 0x0003_0043;
const TAG_SET_GPIO_CONFIG: u32 = 0x0003_8043;

const POWER_STATE_REQ_ON: u32 = 1 << 0;
const POWER_STATE_REQ_WAIT: u32 = 1 << 1;
const POWER_DEVID_SDHCI: u32 = 0;
const CLOCK_ID_EMMC: u32 = 1;
const CLOCK_ID_EMMC2: u32 = 12;
const VL805_MAILBOX_RESET_DEV_ADDR: u32 = 0x0010_0000;

const EXPGPIO_BASE: u32 = 128;
const PI4_WIFI_GPIO: u32 = EXPGPIO_BASE + 1;
const GPIO_DIR_OUT: u32 = 1;
const PI4_WIFI_SDIO_PINS: [u32; 6] = [34, 35, 36, 37, 38, 39];
const PI4_WIFI_SDIO_PULLS: [u32; 6] = [0, 2, 2, 2, 2, 2];
const BCM2835_GPIO_FSEL_MASK: u32 = 0x7;
const BCM2711_GPIO_PULL_MASK: u32 = 0x3;
const BCM2711_GPIO_ALT3: u32 = 0x7;
const BCM2711_GPFSEL0: usize = 0x00;
const BCM2711_GPPUPPDN0: usize = 0xE4;

static PINNED_MAILBOX_REGS: Mutex<Option<MappedRegs>> = Mutex::new(None);
static PINNED_GPIO_REGS: Mutex<Option<MappedRegs>> = Mutex::new(None);
static PINNED_SDHCI_REGS: Mutex<Option<MappedRegs>> = Mutex::new(None);
static PINNED_MAILBOX_REQUEST: Mutex<Option<MappedRegs>> = Mutex::new(None);
static PINNED_MAILBOX_POSTED_REQUEST: Mutex<Option<MappedRegs>> = Mutex::new(None);
static MAILBOX_CALL_LOCK: Mutex<()> = Mutex::new(());
static MAILBOX_TRANSPORT_READY: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);
static MAILBOX_TRANSPORT_READY_LOGGED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

#[derive(Clone, Copy)]
struct MappedRegs {
    paddr: usize,
    vaddr: usize,
}

impl MappedRegs {
    fn from_frame(frame: &DeviceFrame) -> Self {
        Self {
            paddr: frame.paddr(),
            vaddr: frame.ptr().as_ptr() as usize,
        }
    }

    fn paddr(self) -> usize {
        self.paddr
    }

    fn vaddr(self) -> usize {
        self.vaddr
    }
}

fn cloned_pinned_regs(pinned: &Mutex<Option<MappedRegs>>) -> Option<MappedRegs> {
    pinned.lock().as_ref().copied()
}

pub(crate) fn pinned_mailbox_regs() -> Option<(usize, usize)> {
    cloned_pinned_regs(&PINNED_MAILBOX_REGS).map(|regs| (regs.paddr(), regs.vaddr()))
}

fn pinned_mailbox_request_page<H>(
    hal: &mut H,
    slot: &Mutex<Option<MappedRegs>>,
    reuse_action: &str,
    alloc_action: &str,
) -> Result<MappedRegs, HalError>
where
    H: Hardware<Error = HalError>,
{
    let mut shared_request = slot.lock();
    if let Some(frame) = shared_request.as_ref() {
        emit_breadcrumb(format_args!(
            "[pi4-wifi] mailbox request page paddr=0x{:08x} action={reuse_action}",
            frame.paddr()
        ));
        return Ok(*frame);
    }

    let frame = hal
        .alloc_dma_frame_low_attr(sel4_sys::seL4_ARM_Page_Uncached)
        .map_err(|_| HalError::Unsupported("mailbox-dma"))?;
    let mapped = MappedRegs {
        paddr: frame.paddr(),
        vaddr: frame.ptr().as_ptr() as usize,
    };
    emit_breadcrumb(format_args!(
        "[pi4-wifi] mailbox request page paddr=0x{:08x} action={alloc_action}",
        mapped.paddr()
    ));
    core::mem::forget(frame);
    *shared_request = Some(mapped);
    Ok(mapped)
}

struct GpioBank {
    regs: MappedRegs,
}

impl GpioBank {
    fn new<H>(hal: &mut H) -> Result<Self, HalError>
    where
        H: Hardware<Error = HalError>,
    {
        let regs = if let Some(regs) = cloned_pinned_regs(&PINNED_GPIO_REGS) {
            regs
        } else {
            let mut prefix_maps = Vec::new();
            let regs = map_exact(hal, &GPIO_PAGE_PADDR_CANDIDATES, &mut prefix_maps)?;
            let regs = MappedRegs::from_frame(&regs);
            let mut slot = PINNED_GPIO_REGS.lock();
            if slot.is_none() {
                *slot = Some(regs);
            }
            regs
        };
        Ok(Self { regs })
    }

    fn configure_wifi_sdio_pins(&self) {
        emit_breadcrumb(format_args!("[pi4-wifi] gpio sdio mux begin"));
        for &pin in &PI4_WIFI_SDIO_PINS {
            self.set_function(pin, BCM2711_GPIO_ALT3);
        }
        for (&pin, &pull) in PI4_WIFI_SDIO_PINS.iter().zip(PI4_WIFI_SDIO_PULLS.iter()) {
            self.set_pull(pin, pull);
        }
        let fsel3 = self.read32(bcm2711_gpfsel_offset(PI4_WIFI_SDIO_PINS[0]));
        let pud2 = self.read32(bcm2711_puppdn_offset(PI4_WIFI_SDIO_PINS[0]));
        emit_breadcrumb(format_args!(
            "[pi4-wifi] gpio sdio mux ready fsel3=0x{fsel3:08x} pud2=0x{pud2:08x}"
        ));
    }

    fn set_function(&self, gpio: u32, function: u32) {
        let offset = bcm2711_gpfsel_offset(gpio);
        let value = update_bcm2711_gpio_function(self.read32(offset), gpio, function);
        self.write32(offset, value);
    }

    fn set_pull(&self, gpio: u32, pull: u32) {
        let offset = bcm2711_puppdn_offset(gpio);
        let value = update_bcm2711_gpio_pull(self.read32(offset), gpio, pull);
        self.write32(offset, value);
    }

    fn read32(&self, offset: usize) -> u32 {
        let base = self.regs.vaddr();
        // SAFETY: `regs` is a mapped BCM2711 GPIO MMIO page owned by the HAL, and
        // all accesses use aligned 32-bit register offsets within that page.
        unsafe { ptr::read_volatile((base + offset) as *const u32) }
    }

    fn write32(&self, offset: usize, value: u32) {
        let base = self.regs.vaddr();
        // SAFETY: `regs` is a mapped BCM2711 GPIO MMIO page owned by the HAL, and
        // all accesses use aligned 32-bit register offsets within that page.
        unsafe { ptr::write_volatile((base + offset) as *mut u32, value) };
        for _ in 0..SDHCI_WRITE_DELAY_LOOPS {
            spin_loop();
        }
    }
}

fn bcm2711_gpfsel_offset(gpio: u32) -> usize {
    BCM2711_GPFSEL0 + ((gpio as usize) / 10) * 4
}

fn bcm2711_gpfsel_shift(gpio: u32) -> u32 {
    (gpio % 10) * 3
}

fn bcm2711_puppdn_offset(gpio: u32) -> usize {
    BCM2711_GPPUPPDN0 + ((gpio as usize) / 16) * 4
}

fn bcm2711_puppdn_shift(gpio: u32) -> u32 {
    (gpio % 16) * 2
}

fn update_bcm2711_gpio_function(word: u32, gpio: u32, function: u32) -> u32 {
    let shift = bcm2711_gpfsel_shift(gpio);
    let mask = BCM2835_GPIO_FSEL_MASK << shift;
    (word & !mask) | ((function & BCM2835_GPIO_FSEL_MASK) << shift)
}

fn update_bcm2711_gpio_pull(word: u32, gpio: u32, pull: u32) -> u32 {
    let shift = bcm2711_puppdn_shift(gpio);
    let mask = BCM2711_GPIO_PULL_MASK << shift;
    (word & !mask) | ((pull & BCM2711_GPIO_PULL_MASK) << shift)
}

fn emit_breadcrumb(args: core::fmt::Arguments<'_>) {
    let mut line = heapless::String::<224>::new();
    let _ = core::fmt::Write::write_fmt(&mut line, args);
    boot_log::force_uart_line(line.as_str());
}

#[inline]
fn merge_u8_word(word: u32, offset: usize, value: u8) -> u32 {
    let shift = ((offset & 0x3) * 8) as u32;
    let mask = !(0xFFu32 << shift);
    (word & mask) | (u32::from(value) << shift)
}

#[inline]
fn merge_u16_word(word: u32, offset: usize, value: u16) -> u32 {
    let shift = ((offset & 0x2) * 8) as u32;
    let mask = !(0xFFFFu32 << shift);
    (word & mask) | (u32::from(value) << shift)
}

#[inline]
const fn backplane_small_access_addr(addr: u32) -> u32 {
    addr
}

#[inline]
const fn backplane_byte_function_addr(addr: u32) -> u32 {
    addr & BACKPLANE_ADDRESS_MASK
}

#[inline]
const fn backplane_transfer_function_addr(addr: u32) -> u32 {
    (addr & BACKPLANE_ADDRESS_MASK) | BACKPLANE_32BIT_FLAG
}

#[inline]
const fn backplane_word_aligned_addr(addr: u32) -> u32 {
    addr & !0x3
}

#[inline]
const fn backplane_word_increment_addr() -> bool {
    false
}

#[inline]
const fn backplane_word_function_addr(addr: u32) -> u32 {
    backplane_transfer_function_addr(addr)
}

#[inline]
const fn core_ctrl_function_addr(addr: u32) -> u32 {
    backplane_word_function_addr(backplane_word_aligned_addr(addr))
}

#[inline]
const fn core_ctrl_current_window_addr(addr: u32) -> u32 {
    addr & BACKPLANE_ADDRESS_MASK
}

#[inline]
const fn core_ctrl_trace_function_addr(addr: u32) -> u32 {
    core_ctrl_function_addr(addr)
}

#[inline]
const fn backplane_word_byte_shift(addr: u32) -> u32 {
    (addr & 0x3) * 8
}

#[inline]
const fn backplane_window_base(addr: u32) -> u32 {
    addr & BACKPLANE_WINDOW_MASK
}

#[inline]
fn backplane_window_reprogram_needed(programmed_window: Option<u32>, addr: u32) -> bool {
    match programmed_window {
        Some(window) => window != backplane_window_base(addr),
        None => true,
    }
}

#[inline]
const fn backplane_window_register_bytes(addr: u32) -> (u8, u8, u8) {
    let window = backplane_window_base(addr);
    (
        ((window >> 8) & 0xFF) as u8,
        ((window >> 16) & 0xFF) as u8,
        ((window >> 24) & 0xFF) as u8,
    )
}

#[inline]
fn backplane_window_program_sequence(
    window_low: u8,
    window_mid: u8,
    window_high: u8,
) -> [(&'static str, u32, u8); 3] {
    [
        ("high", SBSDIO_FUNC1_SBADDRHIGH, window_high),
        ("mid", SBSDIO_FUNC1_SBADDRMID, window_mid),
        ("low", SBSDIO_FUNC1_SBADDRLOW, window_low),
    ]
}

#[inline]
const fn core_ctrl_access_mode_label() -> &'static str {
    "cmd53-windowed-read32-cmd53-byte-current-window fallback=cmd53-byte-rewindow"
}

#[inline]
const fn core_ctrl_reset_assert_access_mode_label() -> &'static str {
    "cmd53-word-windowed fallback=cmd52-byte-current-window-rewindow"
}

#[inline]
const fn core_ctrl_reset_clear_access_mode_label() -> &'static str {
    "cmd53-word-windowed fallback=cmd52-byte-current-window"
}

#[inline]
const fn core_ctrl_reset_clear_retry_access_mode_label() -> &'static str {
    "cmd52-byte-current-window retry=preserved-cache"
}

#[inline]
const fn core_ctrl_postreset_access_mode_label(_base: u32, _offset: u32) -> &'static str {
    "cmd53-byte-current-window fallback=cmd53-byte-rewindow"
}

#[inline]
const fn core_ctrl_postreset_read_uses_cmd52_current_window(base: u32, _offset: u32) -> bool {
    base == CYW43_ARMCR4_CORE_BASE
}

#[inline]
const fn core_ctrl_postreset_read_access_mode_label(base: u32, offset: u32) -> &'static str {
    if core_ctrl_postreset_read_uses_cmd52_current_window(base, offset) {
        "cmd52-byte-current-window retry=cmd52-byte-rewindow"
    } else {
        "cmd53-windowed-read32-cmd53-byte-current-window fallback=cmd53-byte-rewindow"
    }
}

#[inline]
const fn core_ctrl_in_reset_access_mode_label(base: u32, offset: u32) -> &'static str {
    if core_ctrl_in_reset_write_uses_word_path(base, offset) {
        "cmd53-word-windowed-in-reset fallback=cmd52-current-window-rewindow"
    } else {
        "cmd52-byte-current-window fallback=cmd52-byte-rewindow"
    }
}

fn log_core_ctrl_access(op: &'static str, base: u32, offset: u32, value: Option<u8>) {
    let addr = base.saturating_add(offset);
    let trace_bus = core_ctrl_trace_function_addr(addr);
    let bus = if op == "write8" {
        core_ctrl_current_window_addr(addr)
    } else {
        trace_bus
    };
    let shift = backplane_word_byte_shift(addr);
    match value {
        Some(value) => emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware core-ctrl access op={op} mode={} base=0x{base:08x} off=0x{offset:03x} addr=0x{addr:08x} window=0x{window:08x} bus=0x{bus:05x} trace_bus=0x{trace_bus:05x} shift={shift} inc={} value=0x{value:02x}",
            core_ctrl_access_mode_label(),
            backplane_word_increment_addr() as u8,
            window = addr & BACKPLANE_WINDOW_MASK,
        )),
        None => emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware core-ctrl access op={op} mode={} base=0x{base:08x} off=0x{offset:03x} addr=0x{addr:08x} window=0x{window:08x} bus=0x{bus:05x} trace_bus=0x{trace_bus:05x} shift={shift} inc={}",
            core_ctrl_access_mode_label(),
            backplane_word_increment_addr() as u8,
            window = addr & BACKPLANE_WINDOW_MASK,
        )),
    }
}

fn log_core_ctrl_reset_write(base: u32, offset: u32, value: u8, mode: &'static str) {
    let addr = base.saturating_add(offset);
    emit_breadcrumb(format_args!(
        "[pi4-wifi] firmware core-ctrl reset-write mode={} base=0x{base:08x} off=0x{offset:03x} addr=0x{addr:08x} window=0x{window:08x} bus=0x{bus:05x} trace_bus=0x{trace_bus:05x} shift={shift} inc={inc} value=0x{value:02x}",
        mode,
        bus = core_ctrl_current_window_addr(addr),
        trace_bus = core_ctrl_trace_function_addr(addr),
        shift = backplane_word_byte_shift(addr),
        inc = backplane_word_increment_addr() as u8,
        window = addr & BACKPLANE_WINDOW_MASK,
    ));
}

fn log_core_ctrl_in_reset_write(base: u32, offset: u32, value: u8) {
    let addr = base.saturating_add(offset);
    emit_breadcrumb(format_args!(
        "[pi4-wifi] firmware core-ctrl in-reset-write mode={} base=0x{base:08x} off=0x{offset:03x} addr=0x{addr:08x} bus=0x{bus:05x} trace_bus=0x{trace_bus:05x} value=0x{value:02x}",
        core_ctrl_in_reset_access_mode_label(base, offset),
        bus = core_ctrl_trace_function_addr(addr),
        trace_bus = core_ctrl_trace_function_addr(addr),
    ));
}

fn log_core_ctrl_postreset_write(base: u32, offset: u32, value: u8) {
    let addr = base.saturating_add(offset);
    emit_breadcrumb(format_args!(
        "[pi4-wifi] firmware core-ctrl postreset-write mode={} base=0x{base:08x} off=0x{offset:03x} addr=0x{addr:08x} window=0x{window:08x} bus=0x{bus:05x} trace_bus=0x{trace_bus:05x} shift={shift} inc={inc} value=0x{value:02x}",
        core_ctrl_postreset_access_mode_label(base, offset),
        bus = core_ctrl_current_window_addr(addr),
        trace_bus = core_ctrl_trace_function_addr(addr),
        shift = backplane_word_byte_shift(addr),
        inc = backplane_word_increment_addr() as u8,
        window = addr & BACKPLANE_WINDOW_MASK,
    ));
}

fn log_core_ctrl_postreset_read(base: u32, offset: u32) {
    let addr = base.saturating_add(offset);
    emit_breadcrumb(format_args!(
        "[pi4-wifi] firmware core-ctrl access op=read8 mode={} base=0x{base:08x} off=0x{offset:03x} addr=0x{addr:08x} window=0x{window:08x} bus=0x{bus:05x} trace_bus=0x{trace_bus:05x} shift={shift} inc={inc}",
        core_ctrl_postreset_read_access_mode_label(base, offset),
        bus = core_ctrl_trace_function_addr(addr),
        trace_bus = core_ctrl_trace_function_addr(addr),
        shift = backplane_word_byte_shift(addr),
        inc = backplane_word_increment_addr() as u8,
        window = addr & BACKPLANE_WINDOW_MASK,
    ));
}

#[inline]
const fn core_ctrl_in_reset_write_uses_word_path(base: u32, offset: u32) -> bool {
    base == CYW43_ARMCR4_CORE_BASE && offset == AI_IOCTRL_OFFSET
}

#[inline]
const fn core_reset_needs_postreset_window_reprime(base: u32) -> bool {
    base == CYW43_SOCRAM_CORE_BASE
}

#[inline]
const fn core_ctrl_can_skip_redundant_in_reset_write(
    base: u32,
    asserted_reset: bool,
    prior_value: u8,
    next_value: u8,
) -> bool {
    asserted_reset && base == CYW43_SOCRAM_CORE_BASE && prior_value == next_value
}

#[inline]
const fn core_ctrl_can_defer_in_reset_readback(
    base: u32,
    asserted_reset: bool,
    skipped_write: bool,
) -> bool {
    base == CYW43_SOCRAM_CORE_BASE && asserted_reset && skipped_write
}

#[inline]
const fn core_reset_prepare_hold_value(reset: u8) -> u8 {
    reset | AI_CORE_PRERESET_IOCTRL
}

#[inline]
const fn core_reset_can_skip_pre_clear_in_reset_write(base: u32, skipped_disable: bool) -> bool {
    skipped_disable && base == CYW43_SOCRAM_CORE_BASE
}

#[inline]
const fn backplane_window_differs_by_mid_byte_only(
    current_window_base: u32,
    target_window_addr: u32,
) -> bool {
    let (current_low, current_mid, current_high) =
        backplane_window_register_bytes(current_window_base);
    let (target_low, target_mid, target_high) = backplane_window_register_bytes(target_window_addr);
    current_low == target_low && current_mid != target_mid && current_high == target_high
}

#[inline]
const fn core_ctrl_can_defer_clear_reset_readback(base: u32, attempt: usize) -> bool {
    attempt == 0 && (base == CYW43_SOCRAM_CORE_BASE || base == CYW43_ARMCR4_CORE_BASE)
}

#[inline]
const fn core_reset_can_skip_disable(base: u32, prereset: u8, reset: u8, postreset: u8) -> bool {
    (base == CYW43_SOCRAM_CORE_BASE && prereset == 0 && reset == 0 && postreset == 0)
        || (base == CYW43_ARMCR4_CORE_BASE
            && prereset == ARMCR4_BCMA_IOCTL_CPUHALT
            && reset == 0
            && postreset == 0)
}

#[inline]
const fn core_reset_needs_clear_reset_prewrite_settle(_base: u32, _attempt: usize) -> bool {
    false
}

#[inline]
const fn core_reset_can_retry_clear_reset_write(base: u32, attempt: usize) -> bool {
    base == CYW43_SOCRAM_CORE_BASE && attempt == 0
}

#[inline]
const fn core_reset_needs_clear_reset_ht_assist(base: u32) -> bool {
    base == CYW43_SOCRAM_CORE_BASE
}

#[inline]
const fn core_disable_uses_upstream_socram_disable(base: u32, prereset: u8, reset: u8) -> bool {
    base == CYW43_SOCRAM_CORE_BASE && prereset == 0 && reset == 0
}

#[inline]
const fn clear_reset_keepalive_chunk_loops(remaining_loops: usize) -> usize {
    if remaining_loops > CYW43_CORE_CONTROL_SETTLE_LOOPS {
        CYW43_CORE_CONTROL_SETTLE_LOOPS
    } else {
        remaining_loops
    }
}

#[inline]
const fn ht_clock_assist_shadow_is_complete(
    last_wakeupctrl: Option<u8>,
    last_sleepcsr: Option<u8>,
    last_cardcap: Option<u8>,
) -> bool {
    last_wakeupctrl.is_some() && last_sleepcsr.is_some() && last_cardcap.is_some()
}

#[inline]
fn is_sdhci_command_error(err: &HalError) -> bool {
    matches!(err, HalError::Unsupported("sdhci-command-error"))
}

#[inline]
fn is_sdhci_int_timeout(err: &HalError) -> bool {
    matches!(err, HalError::Unsupported("sdhci-int-timeout"))
}

#[inline]
fn is_sdhci_fragile_read_error(err: &HalError) -> bool {
    is_sdhci_command_error(err) || is_sdhci_int_timeout(err)
}

#[inline]
fn is_sdhci_io_path_error(err: &HalError) -> bool {
    matches!(
        err,
        HalError::Unsupported("sdhci-command-error")
            | HalError::Unsupported("sdhci-transfer-command")
            | HalError::Unsupported("sdhci-transfer-data")
            | HalError::Unsupported("sdhci-transfer-finish")
    )
}

#[inline]
fn is_sdio_cmd52_access_error(err: &HalError) -> bool {
    matches!(
        err,
        HalError::Unsupported("sdio-cmd52-read") | HalError::Unsupported("sdio-cmd52-write")
    )
}

#[inline]
fn is_armcr4_postreset_fragile_read_error(err: &HalError) -> bool {
    is_sdhci_fragile_read_error(err) || matches!(err, HalError::Unsupported("sdio-cmd52-read"))
}

#[inline]
const fn io_direct_cmd53_byte_fallback_allowed(function: SdioFunction) -> bool {
    matches!(function, SdioFunction::Function1)
}

#[inline]
fn chipcommon_config_can_assume_write_commit(addr: u32, err: &HalError) -> bool {
    let _ = addr;
    let _ = err;
    false
}

#[inline]
fn chipcommon_config_can_assume_window_commit(err: &HalError) -> bool {
    let _ = err;
    false
}

#[inline]
fn firmware_backplane_write_can_retry(err: &HalError, attempt: usize) -> bool {
    attempt == 0 && is_sdhci_io_path_error(err)
}

#[inline]
fn firmware_window_write_can_retry(err: &HalError, attempt: usize) -> bool {
    attempt == 0 && chipcommon_config_can_assume_window_commit(err)
}

#[inline]
const fn firmware_transfer_uses_byte_mode(byte_mode_fallback: bool) -> bool {
    byte_mode_fallback
}

#[inline]
const fn firmware_upload_prefers_byte_mode(current_clock_hz: u32, ht_ready: bool) -> bool {
    !ht_ready && current_clock_hz <= CYW43_STARTUP_CLOCK_HZ
}

#[inline]
const fn control_plane_clock_target_hz(current_clock_hz: u32, preferred_data_clock_hz: u32) -> u32 {
    let floor = if preferred_data_clock_hz >= CYW43_CONTROL_PLANE_CLOCK_HZ {
        preferred_data_clock_hz
    } else {
        CYW43_CONTROL_PLANE_CLOCK_HZ
    };
    if current_clock_hz >= floor {
        current_clock_hz
    } else {
        floor
    }
}

#[inline]
fn firmware_phase_can_retry(err: &HalError, attempt: usize) -> bool {
    attempt == 0
        && (is_sdhci_io_path_error(err)
            || is_sdio_cmd52_access_error(err)
            || matches!(err, HalError::Unsupported("sdhci-int-timeout")))
}

#[inline]
const fn setup_firmware_channel_uses_experimental_order(
    allow_function2_ready_bypass: bool,
) -> bool {
    let _ = allow_function2_ready_bypass;
    // Function-2-first sequencing on the no-HT path has proven to hide partial
    // mailbox/interrupt-mask programming without ever producing a real ready
    // indication. Keep the firmware-channel setup order stable even when the
    // bring-up path is still willing to bypass IORX.
    false
}

#[inline]
const fn firmware_channel_write_restore_clock_hz(
    experimental_no_ht_transport: bool,
    current_clock_hz: u32,
) -> Option<u32> {
    let _ = experimental_no_ht_transport;
    if current_clock_hz > CYW43_STARTUP_CLOCK_HZ {
        Some(current_clock_hz)
    } else {
        None
    }
}

#[inline]
fn setup_firmware_channel_can_assume_write_committed(
    allow_function2_ready_bypass: bool,
    attempt: usize,
    err: &HalError,
) -> bool {
    let _ = allow_function2_ready_bypass;
    let _ = attempt;
    let _ = err;
    false
}

#[inline]
const fn wait_for_firmware_ready_uses_experimental_mailbox_read(
    allow_function2_ready_bypass: bool,
) -> bool {
    allow_function2_ready_bypass
}

#[inline]
const fn wait_for_firmware_ready_restore_clock_hz(
    allow_function2_ready_bypass: bool,
    current_clock_hz: u32,
) -> Option<u32> {
    if allow_function2_ready_bypass && current_clock_hz > CYW43_STARTUP_CLOCK_HZ {
        Some(current_clock_hz)
    } else {
        None
    }
}

#[inline]
fn wait_for_firmware_ready_can_assume_mailbox_ready(
    allow_function2_ready_bypass: bool,
    attempt: usize,
    err: &HalError,
) -> bool {
    allow_function2_ready_bypass
        && attempt > 0
        && (is_sdhci_fragile_read_error(err)
            || is_sdio_cmd52_access_error(err)
            || matches!(err, HalError::Unsupported("sdhci-int-timeout")))
}

#[inline]
fn experimental_control_plane_write_can_assume_committed(
    experimental_no_ht_transport: bool,
    first_control_plane_write_pending: bool,
    promoted_probe_pending: bool,
    err: &HalError,
) -> bool {
    let _ = experimental_no_ht_transport;
    let _ = first_control_plane_write_pending;
    let _ = promoted_probe_pending;
    let _ = err;
    false
}

#[inline]
fn experimental_control_plane_write_can_retry_on_startup_link(
    experimental_no_ht_transport: bool,
    first_control_plane_write_pending: bool,
    current_clock_hz: u32,
    err: &HalError,
) -> bool {
    experimental_no_ht_transport
        && first_control_plane_write_pending
        && current_clock_hz > CYW43_STARTUP_CLOCK_HZ
        && is_sdhci_io_path_error(err)
}

#[inline]
fn experimental_control_plane_write_can_promote_after_post_write_rearm_timeout(
    experimental_no_ht_transport: bool,
    first_control_plane_write_pending: bool,
    err: &HalError,
) -> bool {
    experimental_no_ht_transport
        && first_control_plane_write_pending
        && matches!(
            err,
            HalError::Unsupported("sdio-function2-ready-timeout")
                | HalError::Unsupported("cyw43-control-plane-startup-link-reply-timeout")
        )
}

#[inline]
const fn experimental_control_plane_write_needs_post_write_rearm(
    experimental_no_ht_transport: bool,
    first_control_plane_write_pending: bool,
) -> bool {
    experimental_no_ht_transport && first_control_plane_write_pending
}

#[inline]
const fn control_plane_startup_link_timeout_needs_promoted_probe(
    experimental_no_ht_transport: bool,
    reply_rearm_mode: u8,
) -> bool {
    experimental_no_ht_transport && reply_rearm_mode == control_plane_reply_rearm_startup_link()
}

#[inline]
const fn experimental_control_plane_reply_rearm_limit() -> usize {
    2
}

#[inline]
const fn control_plane_reply_rearm_none() -> u8 {
    0
}

#[inline]
const fn control_plane_reply_rearm_startup_link() -> u8 {
    1
}

#[inline]
const fn control_plane_reply_rearm_promoted_link() -> u8 {
    2
}

#[inline]
const fn control_plane_reply_rearm_pending(reply_rearm_mode: u8) -> bool {
    matches!(
        reply_rearm_mode,
        mode if mode == control_plane_reply_rearm_startup_link()
            || mode == control_plane_reply_rearm_promoted_link()
    )
}

#[inline]
const fn control_plane_reply_rearm_uses_promoted_link(reply_rearm_mode: u8) -> bool {
    reply_rearm_mode == control_plane_reply_rearm_promoted_link()
}

#[inline]
const fn control_plane_reply_rearm_mode_name(reply_rearm_mode: u8) -> &'static str {
    match reply_rearm_mode {
        mode if mode == control_plane_reply_rearm_startup_link() => "startup-link",
        mode if mode == control_plane_reply_rearm_promoted_link() => "promoted-link",
        _ => "none",
    }
}

#[inline]
const fn control_plane_zero_frame_needs_reply_rearm(
    reply_rearm_mode: u8,
    function2_ready: bool,
    reply_rearm_attempts: usize,
) -> bool {
    control_plane_reply_rearm_pending(reply_rearm_mode)
        && !function2_ready
        && reply_rearm_attempts < experimental_control_plane_reply_rearm_limit()
}

#[inline]
const fn control_plane_reply_rearm_attempts_after_rearm(
    function2_ready: bool,
    next_attempt: u8,
) -> u8 {
    if function2_ready {
        0
    } else {
        next_attempt
    }
}

#[inline]
const fn control_plane_promoted_probe_stalled_after_rearm(
    reply_rearm_mode: u8,
    speculative_promoted_probe: bool,
    function2_ready_after: bool,
    next_attempt: u8,
) -> bool {
    control_plane_reply_rearm_uses_promoted_link(reply_rearm_mode)
        && speculative_promoted_probe
        && !function2_ready_after
        && (next_attempt as usize) >= experimental_control_plane_reply_rearm_limit()
}

#[inline]
const fn control_plane_startup_link_probe_stalled_after_rearm(
    reply_rearm_mode: u8,
    function2_ready_after: bool,
    next_attempt: usize,
) -> bool {
    reply_rearm_mode == control_plane_reply_rearm_startup_link()
        && !function2_ready_after
        && next_attempt >= experimental_control_plane_reply_rearm_limit()
}

#[inline]
const fn control_plane_snapshot_uses_live_sdio_core_reads(
    experimental_no_ht_transport: bool,
) -> bool {
    !experimental_no_ht_transport
}

#[inline]
const fn cyw43_transport_mode_name(experimental_no_ht_transport: bool) -> &'static str {
    if experimental_no_ht_transport {
        "bounded-no-ht"
    } else {
        "strict"
    }
}

#[inline]
const fn experimental_no_ht_f2_fifo_chunk_limit() -> usize {
    // The no-HT recovery path still reaches firmware-ready, but 512-byte
    // function-2 bursts keep tripping the first control-plane exchanges on Pi
    // 4. Stay on 64-byte chunks until the transport proves it can survive a
    // matched control reply.
    SDIO_FUNCTION_ENABLE_F1.block_size as usize
}

#[inline]
const fn experimental_function2_fifo_chunk_limit(
    function: SdioFunction,
    increment_addr: bool,
    experimental_no_ht_transport: bool,
) -> usize {
    if experimental_no_ht_transport
        && matches!(function, SdioFunction::Function2)
        && !increment_addr
    {
        experimental_no_ht_f2_fifo_chunk_limit()
    } else {
        SDIO_MAX_BYTE_MODE
    }
}

#[inline]
fn core_reset_can_assume_clear_reset_retry_commit(
    base: u32,
    offset: u32,
    attempt: usize,
    err: &HalError,
) -> bool {
    let _ = base;
    let _ = offset;
    let _ = attempt;
    let _ = err;
    false
}

#[inline]
fn core_reset_can_assume_postreset_clock_en_commit(base: u32, offset: u32, err: &HalError) -> bool {
    let _ = base;
    let _ = offset;
    let _ = err;
    false
}

#[inline]
fn core_reset_can_defer_postreset_clock_en_readback(
    base: u32,
    offset: u32,
    err: &HalError,
) -> bool {
    base == CYW43_ARMCR4_CORE_BASE
        && offset == AI_IOCTRL_OFFSET
        && is_armcr4_postreset_fragile_read_error(err)
}

#[inline]
fn core_reset_can_defer_postreset_reset_readback(base: u32, offset: u32, err: &HalError) -> bool {
    base == CYW43_ARMCR4_CORE_BASE
        && offset == AI_RESETCTRL_OFFSET
        && is_armcr4_postreset_fragile_read_error(err)
}

#[inline]
const fn core_reset_postreset_clock_en_read_reason(base: u32) -> &'static str {
    if base == CYW43_ARMCR4_CORE_BASE {
        "armcr4-fragile-postreset-read"
    } else {
        "socram-fragile-postreset-read"
    }
}

#[inline]
const fn core_reset_postreset_reset_read_reason(base: u32) -> &'static str {
    if base == CYW43_ARMCR4_CORE_BASE {
        "armcr4-fragile-postreset-reset-read"
    } else {
        "socram-fragile-postreset-reset-read"
    }
}

#[inline]
fn core_wait_can_retry_after_read_error(base: u32, err: &HalError) -> bool {
    base == CYW43_ARMCR4_CORE_BASE && is_armcr4_postreset_fragile_read_error(err)
}

#[inline]
fn core_wait_should_raise_control_plane_clock(
    base: u32,
    current_clock_hz: u32,
    err: &HalError,
) -> bool {
    base == CYW43_ARMCR4_CORE_BASE
        && current_clock_hz < CYW43_CONTROL_PLANE_CLOCK_HZ
        && is_armcr4_postreset_fragile_read_error(err)
}

#[inline]
fn core_wait_can_defer_after_read_error(
    base: u32,
    attempt: usize,
    current_clock_hz: u32,
    err: &HalError,
) -> bool {
    base == CYW43_ARMCR4_CORE_BASE
        && attempt >= 2
        && current_clock_hz >= CYW43_CONTROL_PLANE_CLOCK_HZ
        && is_armcr4_postreset_fragile_read_error(err)
}

#[inline]
const fn core_reset_can_skip_postreset_verify(base: u32) -> bool {
    base == CYW43_SOCRAM_CORE_BASE
}

#[inline]
const fn chipcommon_config_retry_source_window(addr: u32) -> Option<u32> {
    if addr == CYW43_SOCRAM_CORE_BASE + 0x10 {
        Some(backplane_window_base(
            CYW43_SOCRAM_CORE_BASE + AI_RESETCTRL_OFFSET,
        ))
    } else {
        None
    }
}

#[inline]
const fn chipcommon_config_source_window(
    programmed_window: Option<u32>,
    shadow_window_addr: Option<u32>,
    addr: u32,
) -> Option<u32> {
    match programmed_window {
        Some(window) => Some(window),
        None => match shadow_window_addr {
            Some(window_addr) => Some(backplane_window_base(window_addr)),
            None => chipcommon_config_retry_source_window(addr),
        },
    }
}

#[inline]
fn chipcommon_config_can_use_mid_only_window_switch(
    programmed_window: Option<u32>,
    shadow_window_addr: Option<u32>,
    addr: u32,
) -> bool {
    let Some(source_window) =
        chipcommon_config_source_window(programmed_window, shadow_window_addr, addr)
    else {
        return false;
    };
    backplane_window_differs_by_mid_byte_only(source_window, addr)
}

const fn chipcommon_config_is_phase_addr(addr: u32) -> bool {
    addr == CYW43_SOCRAM_CORE_BASE + 0x10 || addr == CYW43_SOCRAM_CORE_BASE + 0x44
}

#[inline]
const fn core_reset_clear_preserves_window_cache() -> bool {
    true
}

#[inline]
const fn core_reset_clear_allows_immediate_rewindow_fallback() -> bool {
    false
}

#[inline]
const fn restore_programmed_backplane_window(last_window_addr: Option<u32>) -> Option<u32> {
    match last_window_addr {
        Some(window_addr) => Some(backplane_window_base(window_addr)),
        None => None,
    }
}

fn log_sdio_cmd53_shape(
    prefix: &'static str,
    cmd: u16,
    arg: u32,
    len: usize,
    plan: SdioTransferPlan,
) {
    let write = ((arg >> 31) & 1) != 0;
    let function = ((arg >> 28) & 0x7) as u8;
    let block_mode = ((arg >> 27) & 1) != 0;
    let increment_addr = ((arg >> 26) & 1) != 0;
    let addr = (arg >> 9) & 0x1_FFFF;
    let count = arg & 0x1FF;
    emit_breadcrumb(format_args!(
        "[pi4-wifi] sdhci xfer meta stage={prefix} cmd={cmd} op={} fn={} addr=0x{addr:05x} inc={} blk={} count={} len={} blksz={} blkcnt={} flagged={} trn=0x{:04x}",
        if write { "write" } else { "read" },
        function,
        increment_addr as u8,
        block_mode as u8,
        count,
        len,
        plan.block_size,
        plan.block_count,
        ((addr & BACKPLANE_32BIT_FLAG) != 0) as u8,
        plan.transfer_mode,
    ));
}

#[inline]
fn should_log_sdio_transfer_chunk(
    function: SdioFunction,
    increment_addr: bool,
    chunk_len: usize,
    offset: usize,
) -> bool {
    if function != SdioFunction::Function1 {
        return false;
    }
    if !increment_addr {
        return chunk_len <= 8;
    }
    offset / SDIO_MAX_BYTE_MODE < SDIO_TRANSFER_TRACE_INCREMENT_CHUNKS
}

#[inline]
const fn should_log_firmware_upload_progress(
    offset: usize,
    chunk_len: usize,
    total_len: usize,
) -> bool {
    if total_len <= chunk_len {
        return true;
    }
    if offset == 0 || offset + chunk_len == total_len {
        return true;
    }
    offset / CYW43_FIRMWARE_PROGRESS_INTERVAL
        != (offset + chunk_len) / CYW43_FIRMWARE_PROGRESS_INTERVAL
}

#[inline]
fn sdio_transfer_addr(addr: u32, offset: usize, increment_addr: bool) -> Result<u32, HalError> {
    if !increment_addr {
        return Ok(addr);
    }
    let delta = u32::try_from(offset).map_err(|_| HalError::Unsupported("sdio-addr-overflow"))?;
    addr.checked_add(delta)
        .ok_or(HalError::Unsupported("sdio-addr-overflow"))
}

fn log_sdio_transfer_chunk(
    function: SdioFunction,
    addr: u32,
    chunk_addr: u32,
    offset: usize,
    chunk_len: usize,
    increment_addr: bool,
    write: bool,
    plan: SdioTransferPlan,
) {
    emit_breadcrumb(format_args!(
        "[pi4-wifi] sdio xfer chunk fn={} op={} base=0x{:05x} chunk=0x{:05x} off={} len={} inc={} blk={} blksz={} blkcnt={} count={} flagged={}",
        function.number(),
        if write { "write" } else { "read" },
        addr & 0x1_FFFF,
        chunk_addr & 0x1_FFFF,
        offset,
        chunk_len,
        increment_addr as u8,
        plan.block_mode as u8,
        plan.block_size,
        plan.block_count,
        plan.cmd53_count,
        ((chunk_addr & BACKPLANE_32BIT_FLAG) != 0) as u8,
    ));
}

fn wifi_power_state_name(state: WifiPowerState) -> &'static str {
    match state {
        WifiPowerState::Off => "off",
        WifiPowerState::On => "on",
    }
}

fn wifi_reset_state_name(state: WifiResetState) -> &'static str {
    match state {
        WifiResetState::Asserted => "asserted",
        WifiResetState::Deasserted => "deasserted",
    }
}

#[inline]
const fn wifi_gpio_line_enabled(power_state: WifiPowerState) -> bool {
    matches!(power_state, WifiPowerState::On)
}

#[inline]
const fn wifi_gpio_transition_target(
    was_enabled: bool,
    power_state: WifiPowerState,
) -> Option<bool> {
    let enabled = wifi_gpio_line_enabled(power_state);
    if was_enabled == enabled {
        None
    } else {
        Some(enabled)
    }
}

fn sdio_bus_width_name(width: SdioBusWidth) -> &'static str {
    match width {
        SdioBusWidth::OneBit => "1bit",
        SdioBusWidth::FourBit => "4bit",
    }
}

#[inline]
fn yn(flag: bool) -> &'static str {
    if flag {
        "y"
    } else {
        "n"
    }
}

fn spin_settle(loops: usize) {
    for _ in 0..loops {
        spin_loop();
    }
    wifi_progress_advance_loops(loops);
}

fn bounded_spin_settle(stage: &'static str, loops: usize) {
    emit_breadcrumb(format_args!(
        "[pi4-wifi] settle stage={stage} loops={loops}"
    ));
    spin_settle(loops);
}

#[inline]
const fn sdhci_power_ready(power: u8, present: u32) -> bool {
    (power & (SDHCI_POWER_330 | SDHCI_POWER_ON)) == (SDHCI_POWER_330 | SDHCI_POWER_ON)
        && (present & (SDHCI_CARD_PRESENT | SDHCI_CARD_STATE_STABLE))
            == (SDHCI_CARD_PRESENT | SDHCI_CARD_STATE_STABLE)
        && (present & (SDHCI_CMD_INHIBIT | SDHCI_DATA_INHIBIT | SDHCI_DAT_ACTIVE)) == 0
}

#[inline]
const fn ht_clock_request_value() -> u8 {
    SBSDIO_HT_AVAIL_REQ
}

#[inline]
const fn required_ht_clock_request_value(last_chipclkcsr: Option<u8>) -> u8 {
    transport_phase_chipclk_value(last_chipclkcsr) | SBSDIO_HT_AVAIL_REQ | SBSDIO_ALP_AVAIL_REQ
}

#[inline]
const fn required_ht_clock_retry_request_value(last_chipclkcsr: Option<u8>) -> u8 {
    required_ht_clock_request_value(last_chipclkcsr) | SBSDIO_FORCE_HW_CLKREQ_OFF
}

#[inline]
const fn required_ht_clock_wait_loops(stronger_retry_request: bool) -> usize {
    if stronger_retry_request {
        CYW43_CORE_CONTROL_SETTLE_LOOPS
    } else {
        SDIO_INIT_WAIT_LOOPS
    }
}

#[inline]
const fn required_ht_clock_bounded_no_ht_shortcut_loops() -> usize {
    CYW43_HT_CLOCK_SOFT_WAIT_LOOPS
}

#[inline]
const fn ht_clock_progress_chunk_loops(remaining_loops: usize) -> usize {
    if remaining_loops > CYW43_HT_CLOCK_SOFT_WAIT_LOOPS {
        CYW43_HT_CLOCK_SOFT_WAIT_LOOPS
    } else {
        remaining_loops
    }
}

#[inline]
const fn ht_clock_alp_prime_request_value(last_chipclkcsr: Option<u8>) -> u8 {
    transport_phase_chipclk_value(last_chipclkcsr) | SBSDIO_ALP_AVAIL_REQ
}

#[inline]
const fn transport_phase_chipclk_value(last_chipclkcsr: Option<u8>) -> u8 {
    let preserved = match last_chipclkcsr {
        Some(value) => {
            let mut preserved = value & SBSDIO_CHIPCLKCSR_WRITABLE_MASK;
            if (value & SBSDIO_HT_AVAIL) != 0 {
                preserved |= SBSDIO_HT_AVAIL_REQ;
            }
            preserved
        }
        None => 0,
    };
    preserved | SBSDIO_FORCE_HT
}

#[inline]
const fn firmware_bulk_clock_candidates(restore_clock_hz: u32, ht_ready: bool) -> [u32; 4] {
    if ht_ready {
        [
            CYW43_FIRMWARE_BULK_CLOCK_HZ,
            CYW43_FIRMWARE_BULK_CLOCK_HZ / 2,
            CYW43_FIRMWARE_BULK_CLOCK_HZ / 4,
            restore_clock_hz,
        ]
    } else {
        [
            if restore_clock_hz >= 400_000 {
                restore_clock_hz
            } else {
                400_000
            },
            0,
            0,
            0,
        ]
    }
}

#[inline]
const fn ht_clock_timeout_can_continue(required: bool, last_chipclk: u8) -> bool {
    required && (last_chipclk & SBSDIO_ALP_AVAIL) != 0 && (last_chipclk & SBSDIO_HT_AVAIL_REQ) != 0
}

#[inline]
const fn ht_clock_timeout_can_enter_bounded_no_ht_transport(
    last_chipclk: Option<u8>,
    last_wakeupctrl: Option<u8>,
    last_sleepcsr: Option<u8>,
    last_cardcap: Option<u8>,
) -> bool {
    match last_chipclk {
        Some(chipclk) => {
            ht_clock_timeout_can_continue(true, chipclk)
                && (chipclk & SBSDIO_FORCE_HT) != 0
                && ht_clock_assist_shadow_is_complete(last_wakeupctrl, last_sleepcsr, last_cardcap)
        }
        None => false,
    }
}

#[inline]
const fn ht_clock_retry_can_cutover_to_bounded_no_ht_early(
    stronger_retry_request: bool,
    completed_loops: usize,
    last_chipclk: Option<u8>,
    last_wakeupctrl: Option<u8>,
    last_sleepcsr: Option<u8>,
    last_cardcap: Option<u8>,
) -> bool {
    stronger_retry_request
        && completed_loops >= required_ht_clock_bounded_no_ht_shortcut_loops()
        && ht_clock_timeout_can_enter_bounded_no_ht_transport(
            last_chipclk,
            last_wakeupctrl,
            last_sleepcsr,
            last_cardcap,
        )
}

#[inline]
fn log_ht_clock_status(
    stage: &'static str,
    phase: &'static str,
    chipclk: u8,
    wake_ctrl: Option<u8>,
    sleep_csr: Option<u8>,
    cardcap: Option<u8>,
) {
    emit_breadcrumb(format_args!(
        "[pi4-wifi] firmware stage={stage} status={phase} csr=0x{chipclk:02x} ht_req={ht_req} alp_req={alp_req} force_ht={force_ht} clkreq_off={clkreq_off} alp={alp} ht={ht} wake=0x{wake:02x}/{wake_set} sleep=0x{sleep:02x}/{sleep_set} cardcap=0x{cardcap:02x}/{cardcap_set}",
        ht_req = yn((chipclk & SBSDIO_HT_AVAIL_REQ) != 0),
        alp_req = yn((chipclk & SBSDIO_ALP_AVAIL_REQ) != 0),
        force_ht = yn((chipclk & SBSDIO_FORCE_HT) != 0),
        clkreq_off = yn((chipclk & SBSDIO_FORCE_HW_CLKREQ_OFF) != 0),
        alp = yn((chipclk & SBSDIO_ALP_AVAIL) != 0),
        ht = yn((chipclk & SBSDIO_HT_AVAIL) != 0),
        wake = wake_ctrl.unwrap_or(0),
        wake_set = yn(wake_ctrl.is_some()),
        sleep = sleep_csr.unwrap_or(0),
        sleep_set = yn(sleep_csr.is_some()),
        cardcap = cardcap.unwrap_or(0),
        cardcap_set = yn(cardcap.is_some()),
    ));
}

#[inline]
fn next_distinct_firmware_bulk_clock_candidate(
    candidates: &[u32; 4],
    attempt_index: usize,
) -> Option<u32> {
    for &candidate in &candidates[attempt_index + 1..] {
        if candidate != 0 && !candidates[..=attempt_index].contains(&candidate) {
            return Some(candidate);
        }
    }
    None
}

#[inline]
const fn should_prime_ht_clock_assist_before_reset(last_chipclkcsr: Option<u8>) -> bool {
    match last_chipclkcsr {
        Some(value) => (value & (SBSDIO_HT_AVAIL_REQ | SBSDIO_FORCE_HT | SBSDIO_HT_AVAIL)) == 0,
        None => true,
    }
}

#[inline]
const fn ai_core_is_up(ioctrl: u8, resetctrl: u8) -> bool {
    (ioctrl & AI_CORE_PRERESET_IOCTRL) == AI_CORE_POSTRESET_IOCTRL
        && (resetctrl & AI_RESETCTRL_BIT_RESET) == 0
}

#[inline]
const fn ai_core_control_bits(ioctrl: u8) -> u8 {
    ioctrl & AI_CORE_PRERESET_IOCTRL
}

#[inline]
const fn ai_core_extra_bits(ioctrl: u8) -> u8 {
    ioctrl & !AI_CORE_PRERESET_IOCTRL
}

#[inline]
const fn ai_core_clock_enabled(ioctrl: u8) -> bool {
    (ioctrl & AI_IOCTRL_BIT_CLOCK_EN) != 0
}

#[inline]
const fn ai_core_force_gated(ioctrl: u8) -> bool {
    (ioctrl & AI_IOCTRL_BIT_FGC) != 0
}

#[inline]
const fn ai_core_state_reason(ioctrl: u8, resetctrl: u8) -> &'static str {
    let control_bits = ai_core_control_bits(ioctrl);
    let extra_bits = ai_core_extra_bits(ioctrl);
    if (resetctrl & AI_RESETCTRL_BIT_RESET) != 0 {
        if ioctrl == 0 {
            "reset-held-clock-off"
        } else if control_bits == AI_CORE_PRERESET_IOCTRL {
            if extra_bits == 0 {
                "reset-held-fgc-clock"
            } else {
                "reset-held-fgc-clock-extras"
            }
        } else if control_bits == AI_CORE_POSTRESET_IOCTRL {
            if extra_bits == 0 {
                "reset-held-clock-en"
            } else {
                "reset-held-clock-en-extras"
            }
        } else {
            "reset-held-unexpected-io"
        }
    } else if ai_core_is_up(ioctrl, resetctrl) {
        if extra_bits == 0 {
            "core-up"
        } else {
            "clock-en-extras"
        }
    } else if ioctrl == 0 {
        "clock-off"
    } else if control_bits == AI_CORE_PRERESET_IOCTRL {
        if extra_bits == 0 {
            "fgc-still-set"
        } else {
            "fgc-still-set-extras"
        }
    } else if ai_core_clock_enabled(ioctrl) && !ai_core_force_gated(ioctrl) {
        "clock-en-unexpected-io"
    } else {
        "unexpected-state"
    }
}

fn mailbox_tag_name(tag: u32) -> &'static str {
    match tag {
        TAG_SET_POWER_STATE => "set-power-state",
        TAG_GET_CLOCK_RATE => "get-clock-rate",
        TAG_GET_MAX_CLOCK_RATE => "get-max-clock-rate",
        TAG_NOTIFY_XHCI_RESET => "notify-xhci-reset",
        TAG_GET_GPIO_STATE => "get-gpio-state",
        TAG_SET_GPIO_STATE => "set-gpio-state",
        TAG_GET_GPIO_CONFIG => "get-gpio-config",
        TAG_SET_GPIO_CONFIG => "set-gpio-config",
        _ => "unknown",
    }
}

#[inline]
const fn mailbox_request_page_actions() -> (&'static str, &'static str) {
    ("reuse-shared", "alloc-shared")
}

#[inline]
const fn mailbox_posted_alias(tag: u32) -> Option<u32> {
    match tag {
        TAG_NOTIFY_XHCI_RESET => Some(VC_BUS_ALIAS_BASES[0]),
        _ => None,
    }
}

#[inline]
const fn mailbox_recv_wait_spins(tag: u32) -> usize {
    match tag {
        TAG_NOTIFY_XHCI_RESET => MAILBOX_WAIT_SPINS_NOTIFY_XHCI_RESET,
        _ => MAILBOX_WAIT_SPINS,
    }
}

#[inline]
const fn mailbox_ack_alias_count(tag: u32) -> usize {
    match tag {
        // Runtime VL805 reset-notify should not burn a second acknowledged
        // request through the alternate alias. If the primary alias does not
        // complete cleanly, the caller falls straight into the dedicated
        // posted fallback path instead of mutating the shared request page
        // again and waiting through another reply window.
        TAG_NOTIFY_XHCI_RESET => 1,
        _ => VC_BUS_ALIAS_BASES.len(),
    }
}

#[inline]
const fn mailbox_reply_token_addr(token: u32) -> u32 {
    (token & VC_BUS_MASK) & !0xF
}

#[inline]
const fn mailbox_reply_matches_request_page(expected: u32, actual: u32) -> bool {
    mailbox_reply_token_addr(expected) == mailbox_reply_token_addr(actual)
}

/// Result of the Pi 4 VL805 mailbox reset-notify handoff.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Vl805ResetNotifyResult {
    /// VideoCore acknowledged the property call synchronously.
    Acked,
    /// VideoCore never replied, so root-task used the dedicated posted fallback.
    PostedFallback,
}

fn sdhci_status_reason(status: u32) -> &'static str {
    if (status & SDHCI_INT_TIMEOUT) != 0 {
        "timeout"
    } else if (status & SDHCI_INT_CRC) != 0 {
        "crc"
    } else if (status & SDHCI_INT_END_BIT) != 0 {
        "end-bit"
    } else if (status & SDHCI_INT_INDEX) != 0 {
        "index"
    } else if (status & SDHCI_INT_DATA_TIMEOUT) != 0 {
        "data-timeout"
    } else if (status & SDHCI_INT_DATA_CRC) != 0 {
        "data-crc"
    } else if (status & SDHCI_INT_DATA_END_BIT) != 0 {
        "data-end-bit"
    } else if (status & SDHCI_INT_ERROR) != 0 {
        "error"
    } else if (status & SDHCI_INT_RESPONSE) != 0 {
        "complete"
    } else {
        "unknown"
    }
}

fn is_mailbox_protocol_error(err: &HalError) -> bool {
    matches!(err, HalError::Unsupported("mailbox-protocol"))
}

fn mailbox_protocol_reason(
    expected_tag: u32,
    status: u32,
    reply_tag: u32,
    value_status: u32,
) -> &'static str {
    if status != MAILBOX_RESPONSE_SUCCESS {
        "status"
    } else if reply_tag != expected_tag {
        "reply-tag"
    } else if (value_status & MAILBOX_VALUE_RESPONSE) == 0 {
        "value-response"
    } else {
        "unknown"
    }
}

fn preseed_register_block<H>(
    hal: &mut H,
    candidates: &[usize],
    pinned: &Mutex<Option<MappedRegs>>,
) -> bool
where
    H: Hardware<Error = HalError>,
{
    if pinned.lock().is_some() {
        return true;
    }

    let mut prefix_maps = Vec::new();
    let Ok(regs) = map_exact(hal, candidates, &mut prefix_maps) else {
        return false;
    };

    let regs = MappedRegs::from_frame(&regs);
    let mut slot = pinned.lock();
    if slot.is_none() {
        *slot = Some(regs);
    }
    true
}

pub fn preseed_mailbox_mmio<H>(hal: &mut H) -> bool
where
    H: Hardware<Error = HalError>,
{
    let mailbox = preseed_register_block(hal, &MAILBOX_PAGE_PADDR_CANDIDATES, &PINNED_MAILBOX_REGS);
    boot_log::force_uart_line(if mailbox {
        "[pi4-wifi] mmio preseeded mailbox=yes"
    } else {
        "[pi4-wifi] mmio preseeded mailbox=no"
    });
    mailbox
}

pub fn preseed_gpio_mmio<H>(hal: &mut H) -> bool
where
    H: Hardware<Error = HalError>,
{
    let gpio = preseed_register_block(hal, &GPIO_PAGE_PADDR_CANDIDATES, &PINNED_GPIO_REGS);
    boot_log::force_uart_line(if gpio {
        "[pi4-wifi] mmio preseeded gpio=yes"
    } else {
        "[pi4-wifi] mmio preseeded gpio=no"
    });
    gpio
}

pub fn preseed_sdhci_mmio<H>(hal: &mut H) -> bool
where
    H: Hardware<Error = HalError>,
{
    let sdhci = preseed_register_block(hal, &SDHCI_PAGE_PADDR_CANDIDATES, &PINNED_SDHCI_REGS);
    boot_log::force_uart_line(if sdhci {
        "[pi4-wifi] mmio preseeded sdhci=yes"
    } else {
        "[pi4-wifi] mmio preseeded sdhci=no"
    });
    sdhci
}

pub fn preseed_mmio<H>(hal: &mut H)
where
    H: Hardware<Error = HalError>,
{
    let mailbox = preseed_mailbox_mmio(hal);
    let gpio = preseed_gpio_mmio(hal);
    let sdhci = preseed_sdhci_mmio(hal);
    match (mailbox, gpio, sdhci) {
        (true, true, true) => boot_log::force_uart_line(
            "[pi4-wifi] mmio preseed summary mailbox=yes gpio=yes sdhci=yes",
        ),
        (true, true, false) => boot_log::force_uart_line(
            "[pi4-wifi] mmio preseed summary mailbox=yes gpio=yes sdhci=no",
        ),
        (true, false, true) => boot_log::force_uart_line(
            "[pi4-wifi] mmio preseed summary mailbox=yes gpio=no sdhci=yes",
        ),
        (true, false, false) => boot_log::force_uart_line(
            "[pi4-wifi] mmio preseed summary mailbox=yes gpio=no sdhci=no",
        ),
        (false, true, true) => boot_log::force_uart_line(
            "[pi4-wifi] mmio preseed summary mailbox=no gpio=yes sdhci=yes",
        ),
        (false, true, false) => boot_log::force_uart_line(
            "[pi4-wifi] mmio preseed summary mailbox=no gpio=yes sdhci=no",
        ),
        (false, false, true) => boot_log::force_uart_line(
            "[pi4-wifi] mmio preseed summary mailbox=no gpio=no sdhci=yes",
        ),
        (false, false, false) => {
            boot_log::force_uart_line("[pi4-wifi] mmio preseed summary mailbox=no gpio=no sdhci=no")
        }
    }
}

const SDHCI_BLOCK_SIZE: usize = 0x04;
const SDHCI_BLOCK_COUNT: usize = 0x06;
const SDHCI_ARGUMENT: usize = 0x08;
const SDHCI_TRANSFER_MODE: usize = 0x0C;
const SDHCI_COMMAND: usize = 0x0E;
const SDHCI_RESPONSE: usize = 0x10;
const SDHCI_BUFFER: usize = 0x20;
const SDHCI_PRESENT_STATE: usize = 0x24;
const SDHCI_HOST_CONTROL: usize = 0x28;
const SDHCI_POWER_CONTROL: usize = 0x29;
const SDHCI_CLOCK_CONTROL: usize = 0x2C;
const SDHCI_TIMEOUT_CONTROL: usize = 0x2E;
const SDHCI_SOFTWARE_RESET: usize = 0x2F;
const SDHCI_INT_STATUS: usize = 0x30;
const SDHCI_INT_ENABLE: usize = 0x34;
const SDHCI_SIGNAL_ENABLE: usize = 0x38;
const SDHCI_CAPABILITIES: usize = 0x40;
const SDHCI_HOST_VERSION: usize = 0xFE;

const SDHCI_TRNS_BLK_CNT_EN: u16 = 1 << 1;
const SDHCI_TRNS_READ: u16 = 1 << 4;
const SDHCI_TRNS_MULTI: u16 = 1 << 5;

const SDHCI_CMD_RESP_NONE: u16 = 0x00;
const SDHCI_CMD_RESP_LONG: u16 = 0x01;
const SDHCI_CMD_RESP_SHORT: u16 = 0x02;
const SDHCI_CMD_RESP_SHORT_BUSY: u16 = 0x03;
const SDHCI_CMD_CRC: u16 = 0x08;
const SDHCI_CMD_INDEX: u16 = 0x10;
const SDHCI_CMD_DATA: u16 = 0x20;

const SDHCI_CMD_INHIBIT: u32 = 1 << 0;
const SDHCI_DATA_INHIBIT: u32 = 1 << 1;
const SDHCI_DAT_ACTIVE: u32 = 1 << 2;
const SDHCI_SPACE_AVAILABLE: u32 = 1 << 10;
const SDHCI_DATA_AVAILABLE: u32 = 1 << 11;
const SDHCI_CARD_PRESENT: u32 = 1 << 16;
const SDHCI_CARD_STATE_STABLE: u32 = 1 << 17;
const SDHCI_CARD_DETECT_PIN_LEVEL: u32 = 1 << 18;
const SDHCI_WRITE_PROTECT: u32 = 1 << 19;
const SDHCI_DATA_LVL_MASK: u32 = 0x00F0_0000;

const SDHCI_CTRL_4BITBUS: u8 = 1 << 1;

const SDHCI_POWER_ON: u8 = 0x01;
const SDHCI_POWER_330: u8 = 0x0E;

const SDHCI_CLOCK_INT_STABLE: u16 = 1 << 1;
const SDHCI_CLOCK_CARD_EN: u16 = 1 << 2;
const SDHCI_CLOCK_INT_EN: u16 = 1 << 0;
const SDHCI_DIVIDER_SHIFT: u16 = 8;
const SDHCI_DIVIDER_HI_SHIFT: u16 = 6;
const SDHCI_DIV_MASK: u16 = 0xFF;
const SDHCI_DIV_HI_MASK: u16 = 0x300;
const SDHCI_SPEC_VER_MASK: u16 = 0x00FF;
const SDHCI_SPEC_300: u16 = 2;

const SDHCI_RESET_ALL: u8 = 0x01;
const SDHCI_RESET_CMD: u8 = 0x02;
const SDHCI_RESET_DATA: u8 = 0x04;

const SDHCI_INT_RESPONSE: u32 = 1 << 0;
const SDHCI_INT_DATA_END: u32 = 1 << 1;
const SDHCI_INT_SPACE_AVAIL: u32 = 1 << 4;
const SDHCI_INT_DATA_AVAIL: u32 = 1 << 5;
const SDHCI_INT_CARD_INT: u32 = 1 << 8;
const SDHCI_INT_ERROR: u32 = 1 << 15;
const SDHCI_INT_TIMEOUT: u32 = 1 << 16;
const SDHCI_INT_CRC: u32 = 1 << 17;
const SDHCI_INT_END_BIT: u32 = 1 << 18;
const SDHCI_INT_INDEX: u32 = 1 << 19;
const SDHCI_INT_DATA_TIMEOUT: u32 = 1 << 20;
const SDHCI_INT_DATA_CRC: u32 = 1 << 21;
const SDHCI_INT_DATA_END_BIT: u32 = 1 << 22;
const SDHCI_INT_ALL_MASK: u32 = u32::MAX;
const SDHCI_INT_CMD_MASK: u32 =
    SDHCI_INT_RESPONSE | SDHCI_INT_TIMEOUT | SDHCI_INT_CRC | SDHCI_INT_END_BIT | SDHCI_INT_INDEX;
const SDHCI_INT_DATA_MASK: u32 = SDHCI_INT_DATA_END
    | SDHCI_INT_SPACE_AVAIL
    | SDHCI_INT_DATA_AVAIL
    | SDHCI_INT_DATA_TIMEOUT
    | SDHCI_INT_DATA_CRC
    | SDHCI_INT_DATA_END_BIT;

const SDIO_CMD5: u16 = 5;
const SDIO_CMD3: u16 = 3;
const SDIO_CMD7: u16 = 7;
const SDIO_CMD52: u16 = 52;
const SDIO_CMD53: u16 = 53;

const SDIO_R4_READY: u32 = 1 << 31;
const SDIO_OCR_3V2_3V4: u32 = 0x00FF_8000;

const SDIO_CCCR_IOEX: u32 = 0x02;
const SDIO_CCCR_IORX: u32 = 0x03;
const SDIO_CCCR_IENX: u32 = 0x04;
const SDIO_CCCR_IF: u32 = 0x07;
const SDIO_CCCR_BRCM_CARDCAP: u32 = 0xF0;
const SDIO_CCCR_BRCM_CARDCAP_CMD_NODEC: u8 = 0x08;
const SDIO_BUS_WIDTH_1BIT: u8 = 0x00;
const SDIO_BUS_WIDTH_4BIT: u8 = 0x02;
const SDIO_CCCR_FBR_BASE: u32 = 0x100;
const SDIO_FBR_BLKSIZE: u32 = 0x10;
const SDIO_FUNC_ENABLE_1: u8 = 0x02;
const SDIO_FUNC_ENABLE_2: u8 = 0x04;
const SDIO_FUNC_READY_1: u8 = 0x02;
const SDIO_FUNC_READY_2: u8 = 0x04;
const SDIO_FUNCTION_READY_POLLS: usize = 64;
const SDIO_FUNCTION_READY_POLLS_FUNCTION2: usize = 16;
const SDIO_FUNCTION_READY_POLLS_FUNCTION2_REPLY_PROBE: usize = 64;
const SDIO_FUNCTION_READY_POLLS_FUNCTION2_EXTENDED: usize = 256;
const SDIO_FUNCTION_READY_SETTLE_LOOPS: usize = 200_000;
const SDIO_FUNCTION_READY_SETTLE_LOOPS_FUNCTION2: usize = 50_000;
const SDIO_FUNCTION_READY_SETTLE_LOOPS_FUNCTION2_REPLY_PROBE: usize =
    SDIO_FUNCTION_READY_SETTLE_LOOPS_FUNCTION2;
const SDIO_FUNCTION_READY_SETTLE_LOOPS_FUNCTION2_EXTENDED: usize = 200_000;
const SDIO_FUNCTION2_READY_RETRY_LIMIT: usize = 1;
const SDIO_CCCR_IEN_FUNC0: u8 = 1 << 0;
const SDIO_CCCR_IEN_FUNC1: u8 = 1 << 1;
const SDIO_CCCR_IEN_FUNC2: u8 = 1 << 2;

const SBSDIO_WATERMARK: u32 = 0x10008;
const SBSDIO_DEVICE_CTL: u32 = 0x10009;
const SBSDIO_DEVCTL_F2WM_ENAB: u8 = 0x10;
const SBSDIO_FUNC1_SBADDRLOW: u32 = 0x1000A;
const SBSDIO_FUNC1_SBADDRMID: u32 = 0x1000B;
const SBSDIO_FUNC1_SBADDRHIGH: u32 = 0x1000C;
const SBSDIO_FUNC1_CHIPCLKCSR: u32 = 0x1000E;
const SBSDIO_FUNC1_SDIOPULLUP: u32 = 0x1000F;
const SBSDIO_FUNC1_RFRAMEBCLO: u32 = 0x1001B;
const SBSDIO_FUNC1_RFRAMEBCHI: u32 = 0x1001C;
const SBSDIO_FUNC1_MESBUSYCTRL: u32 = 0x1001D;
const SBSDIO_FUNC1_WAKEUPCTRL: u32 = 0x1001E;
const SBSDIO_FUNC1_SLEEPCSR: u32 = 0x1001F;

const SBSDIO_ALP_AVAIL_REQ: u8 = 0x08;
const SBSDIO_HT_AVAIL_REQ: u8 = 0x10;
const SBSDIO_FORCE_HT: u8 = 0x02;
const SBSDIO_FORCE_HW_CLKREQ_OFF: u8 = 0x20;
const SBSDIO_ALP_AVAIL: u8 = 0x40;
const SBSDIO_HT_AVAIL: u8 = 0x80;
const SBSDIO_CHIPCLKCSR_WRITABLE_MASK: u8 =
    SBSDIO_ALP_AVAIL_REQ | SBSDIO_HT_AVAIL_REQ | SBSDIO_FORCE_HT | SBSDIO_FORCE_HW_CLKREQ_OFF;
const SBSDIO_WAKE_TILL_HT_AVAIL: u8 = 0x02;
const SBSDIO_FUNC1_SLEEPCSR_KSO_EN: u8 = 1;
const SBSDIO_FUNC1_SLEEPCSR_KSO_MASK: u8 = 0x01;

const SDPCMD_REG_HOSTINTMASK: u32 = 0x24;
const SDPCMD_REG_TOHOSTMAILBOXDATA: u32 = 0x4C;
const SDPCMD_REG_TOSBMAILBOXDATA: u32 = 0x48;
const SDIO_INT_STATUS: u32 = 0x20;

const I_HMB_SW_MASK: u32 = 0x0000_00F0;
const I_HMB_FC_CHANGE: u32 = 1 << 5;
const I_HMB_FRAME_IND: u32 = 1 << 6;
const I_HMB_HOST_INT: u32 = 1 << 7;
const I_CHIPACTIVE: u32 = 1 << 29;
const HOSTINTMASK: u32 = I_HMB_SW_MASK | I_CHIPACTIVE;
const HMB_DATA_DEVREADY: u32 = 0x0002;
const HMB_DATA_FWREADY: u32 = 0x0008;
const HMB_DATA_VERSION_MASK: u32 = 0x00FF_0000;
const HMB_DATA_VERSION_SHIFT: u32 = 16;
const SDPCM_PROT_VERSION: u32 = 4;
const CY_43455_F2_WATERMARK: u8 = 0x60;
const CY_43455_MESBUSYCTRL: u8 = 0xD0;

const BACKPLANE_ADDRESS_MASK: u32 = 0x7FFF;
const BACKPLANE_WINDOW_MASK: u32 = 0xFFFF_8000;
const BACKPLANE_32BIT_FLAG: u32 = 0x8000;

const AI_IOCTRL_OFFSET: u32 = 0x408;
const AI_IOCTRL_BIT_FGC: u8 = 0x02;
const AI_IOCTRL_BIT_CLOCK_EN: u8 = 0x01;
const AI_RESETCTRL_OFFSET: u32 = 0x800;
const AI_RESETCTRL_BIT_RESET: u8 = 0x01;
const ARMCR4_BCMA_IOCTL_CPUHALT: u8 = 0x20;
const AI_CORE_PRERESET_IOCTRL: u8 = AI_IOCTRL_BIT_FGC | AI_IOCTRL_BIT_CLOCK_EN;
const AI_CORE_POSTRESET_IOCTRL: u8 = AI_IOCTRL_BIT_CLOCK_EN;
const ARMCR4_CAP: u32 = 0x0004;
const ARMCR4_BANKIDX: u32 = 0x0040;
const ARMCR4_BANKINFO: u32 = 0x0044;
const ARMCR4_BSZ_MASK: u32 = 0x7F;
const ARMCR4_BLK_1K_MASK: u32 = 0x200;
const ARMCR4_TCBANB_MASK: u32 = 0x0F;
const ARMCR4_TCBANB_SHIFT: u32 = 0;
const ARMCR4_TCBBNB_MASK: u32 = 0xF0;
const ARMCR4_TCBBNB_SHIFT: u32 = 4;
const ARMCR4_BSZ_MULT: u32 = 8192;

const CYW43_CHIPCOMMON_BASE: u32 = 0x1800_0000;
const CYW43_SDIO_CORE_BASE: u32 = 0x1800_2000;
const CYW43_ARMCR4_CORE_BASE: u32 = 0x1810_3000;
const CYW43_SOCRAM_CORE_BASE: u32 = 0x1810_4000;
const CYW43_RAM_BASE_4345: u32 = 0x0019_8000;

#[inline]
const fn sdio_core_reg_addr(offset: u32) -> u32 {
    CYW43_SDIO_CORE_BASE + offset
}

#[inline]
const fn sdio_core_transfer_function_addr(offset: u32) -> u32 {
    backplane_transfer_function_addr(sdio_core_reg_addr(offset))
}

#[inline]
const fn sdio_core_transfer_increment_addr() -> bool {
    true
}

const SDIO_INIT_WAIT_LOOPS: usize = 50_000;
const SDIO_HOST_RESET_LOOPS: usize = 50_000;
const SDIO_CLOCK_STABLE_LOOPS: usize = 50_000;
const SDIO_CMD_WAIT_LOOPS: usize = 200_000;
const SDIO_DATA_WAIT_LOOPS: usize = 200_000;
const SDIO_CARD_INIT_ATTEMPTS: usize = 2;
const SDHCI_POWER_OFF_SETTLE_LOOPS: usize = 500_000;
const SDHCI_POWER_READY_LOOPS: usize = 500_000;
const SDHCI_POWER_SETTLE_LOOPS: usize = 20_000_000;
const SDIO_CARD_INIT_RETRY_SETTLE_LOOPS: usize = 500_000;
const WIFI_POWER_SETTLE_LOOPS: usize = 500_000;
const WIFI_POWER_DROP_SETTLE_LOOPS: usize = 20_000_000;
const CYW43_CORE_CONTROL_SETTLE_LOOPS: usize = 500_000;
const CYW43_SOCRAM_CLEAR_RESET_PREWRITE_SETTLE_LOOPS: usize = 20_000_000;
const CYW43_SOCRAM_CLEAR_RESET_KEEPALIVE_CHUNK_LOOPS: usize = CYW43_CORE_CONTROL_SETTLE_LOOPS;
const CYW43_CORE_RESET_RETRY_LIMIT: usize = 50;
const SDHCI_WRITE_DELAY_LOOPS: usize = 256;
const SDHCI_WRITE_GAP_SPIN_LOOPS: usize = SDHCI_WRITE_DELAY_LOOPS * 32;
const CYW43_READY_LOOPS: usize = 1_000;
const CYW43_TRANSFER_CHUNK: usize = 256;
const CYW43_FIRMWARE_TRANSFER_CHUNK: usize = CYW43_TRANSFER_CHUNK;
const CYW43_FIRMWARE_PROGRESS_INTERVAL: usize = 16 * 1024;
const CYW43_FIRMWARE_BULK_CLOCK_HZ: u32 = 12_500_000;
const CYW43_STARTUP_CLOCK_HZ: u32 = 400_000;
const CYW43_CONTROL_PLANE_CLOCK_HZ: u32 = 12_500_000;
const SDIO_MAX_BYTE_MODE: usize = 511;
const SDIO_TRANSFER_TRACE_INCREMENT_CHUNKS: usize = 2;
const CYW43_HT_CLOCK_INITIAL_WAIT_LOOPS: usize = 2_048;
const CYW43_HT_CLOCK_SOFT_WAIT_LOOPS: usize = 8_192;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResponseType {
    None,
    Ocr,
    Short,
    ShortBusy,
    Long,
}

#[derive(Clone, Copy)]
struct CardInfo {
    rca: u16,
    ocr: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SdioTransferPlan {
    block_size: u16,
    block_count: u16,
    cmd53_count: u16,
    block_mode: bool,
    transfer_mode: u16,
}

#[inline]
const fn sdhci_present_buffer_ready_mask(write: bool) -> u32 {
    if write {
        SDHCI_SPACE_AVAILABLE
    } else {
        SDHCI_DATA_AVAILABLE
    }
}

#[inline]
const fn sdhci_interrupt_buffer_ready_mask(write: bool) -> u32 {
    if write {
        SDHCI_INT_SPACE_AVAIL
    } else {
        SDHCI_INT_DATA_AVAIL
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct SdioFunctionEnableStep {
    function: SdioFunction,
    enable_bit: u8,
    ready_bit: u8,
    block_size: u16,
    timeout_error: &'static str,
}

const SDIO_FUNCTION_ENABLE_F1: SdioFunctionEnableStep = SdioFunctionEnableStep {
    function: SdioFunction::Function1,
    enable_bit: SDIO_FUNC_ENABLE_1,
    ready_bit: SDIO_FUNC_READY_1,
    block_size: 64,
    timeout_error: "sdio-function1-ready-timeout",
};

const SDIO_FUNCTION_ENABLE_F2: SdioFunctionEnableStep = SdioFunctionEnableStep {
    function: SdioFunction::Function2,
    enable_bit: SDIO_FUNC_ENABLE_2,
    ready_bit: SDIO_FUNC_READY_2,
    // Linux brcmfmac keeps BCM43455 on a 512-byte F2 block size.
    block_size: 512,
    timeout_error: "sdio-function2-ready-timeout",
};

const SDIO_FUNCTION_ENABLE_SEQUENCE: [SdioFunctionEnableStep; 2] =
    [SDIO_FUNCTION_ENABLE_F1, SDIO_FUNCTION_ENABLE_F2];

#[inline]
const fn sdio_function_ready_transport_stage(step: SdioFunctionEnableStep) -> Option<&'static str> {
    match step.function {
        SdioFunction::Function2 => Some("sdio-function2-ready"),
        _ => None,
    }
}

#[inline]
const fn sdio_function_ready_retry_limit(step: SdioFunctionEnableStep) -> usize {
    match step.function {
        SdioFunction::Function2 => SDIO_FUNCTION2_READY_RETRY_LIMIT,
        _ => 0,
    }
}

#[inline]
const fn sdio_function_ready_polls(step: SdioFunctionEnableStep) -> usize {
    match step.function {
        SdioFunction::Function2 => SDIO_FUNCTION_READY_POLLS_FUNCTION2,
        _ => SDIO_FUNCTION_READY_POLLS,
    }
}

#[inline]
const fn sdio_function_ready_settle_loops(step: SdioFunctionEnableStep) -> usize {
    match step.function {
        SdioFunction::Function2 => SDIO_FUNCTION_READY_SETTLE_LOOPS_FUNCTION2,
        _ => SDIO_FUNCTION_READY_SETTLE_LOOPS,
    }
}

#[inline]
const fn sdio_function_ready_extended_polls(step: SdioFunctionEnableStep) -> Option<usize> {
    match step.function {
        SdioFunction::Function2 => Some(SDIO_FUNCTION_READY_POLLS_FUNCTION2_EXTENDED),
        _ => None,
    }
}

#[inline]
const fn sdio_function_ready_extended_settle_loops(step: SdioFunctionEnableStep) -> Option<usize> {
    match step.function {
        SdioFunction::Function2 => Some(SDIO_FUNCTION_READY_SETTLE_LOOPS_FUNCTION2_EXTENDED),
        _ => None,
    }
}

#[inline]
const fn sdio_function_ready_uses_short_probe_only_budget(
    step: SdioFunctionEnableStep,
    budget: SdioFunctionReadyBudget,
) -> bool {
    matches!(step.function, SdioFunction::Function2)
        && matches!(budget, SdioFunctionReadyBudget::ExperimentalBypass)
}

#[inline]
const fn sdio_function_ready_uses_control_plane_reply_probe_budget(
    step: SdioFunctionEnableStep,
    budget: SdioFunctionReadyBudget,
) -> bool {
    matches!(step.function, SdioFunction::Function2)
        && matches!(budget, SdioFunctionReadyBudget::ControlPlaneReplyProbe)
}

#[inline]
const fn sdio_function_ready_retry_limit_for(
    step: SdioFunctionEnableStep,
    budget: SdioFunctionReadyBudget,
) -> usize {
    if sdio_function_ready_uses_short_probe_only_budget(step, budget)
        || sdio_function_ready_uses_control_plane_reply_probe_budget(step, budget)
    {
        0
    } else {
        sdio_function_ready_retry_limit(step)
    }
}

#[inline]
const fn sdio_function_ready_extended_polls_for(
    step: SdioFunctionEnableStep,
    budget: SdioFunctionReadyBudget,
) -> Option<usize> {
    if sdio_function_ready_uses_short_probe_only_budget(step, budget) {
        None
    } else if sdio_function_ready_uses_control_plane_reply_probe_budget(step, budget) {
        Some(SDIO_FUNCTION_READY_POLLS_FUNCTION2_REPLY_PROBE)
    } else {
        sdio_function_ready_extended_polls(step)
    }
}

#[inline]
const fn sdio_function_ready_extended_settle_loops_for(
    step: SdioFunctionEnableStep,
    budget: SdioFunctionReadyBudget,
) -> Option<usize> {
    if sdio_function_ready_uses_short_probe_only_budget(step, budget) {
        None
    } else if sdio_function_ready_uses_control_plane_reply_probe_budget(step, budget) {
        Some(SDIO_FUNCTION_READY_SETTLE_LOOPS_FUNCTION2_REPLY_PROBE)
    } else {
        sdio_function_ready_extended_settle_loops(step)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SdioFunctionReadyBudget {
    Strict,
    ControlPlaneReplyProbe,
    ExperimentalBypass,
}

#[inline]
const fn sdio_function_ready_budget_name(budget: SdioFunctionReadyBudget) -> &'static str {
    match budget {
        SdioFunctionReadyBudget::Strict => "strict",
        SdioFunctionReadyBudget::ControlPlaneReplyProbe => "reply-probe",
        SdioFunctionReadyBudget::ExperimentalBypass => "experimental-bypass",
    }
}

#[inline]
const fn sdio_function_ready_timeout_can_continue_experimentally(
    step: SdioFunctionEnableStep,
    desired: u8,
    ready: u8,
    budget: SdioFunctionReadyBudget,
) -> bool {
    matches!(budget, SdioFunctionReadyBudget::ExperimentalBypass)
        && matches!(step.function, SdioFunction::Function2)
        && (desired & step.enable_bit) == step.enable_bit
        && (ready & SDIO_FUNC_READY_1) == SDIO_FUNC_READY_1
        && (ready & step.ready_bit) == 0
}

#[inline]
const fn control_plane_promote_rearm_mode_name(speculative_ready_probe: bool) -> &'static str {
    if speculative_ready_probe {
        "speculative-empty-poll"
    } else {
        "strict"
    }
}

#[inline]
const fn control_plane_promote_rearm_budget(
    speculative_ready_probe: bool,
) -> SdioFunctionReadyBudget {
    if speculative_ready_probe {
        SdioFunctionReadyBudget::ExperimentalBypass
    } else {
        SdioFunctionReadyBudget::Strict
    }
}

#[inline]
const fn sdio_function_ready_budget_for_bypass(
    allow_ready_timeout_bypass: bool,
) -> SdioFunctionReadyBudget {
    if allow_ready_timeout_bypass {
        SdioFunctionReadyBudget::ExperimentalBypass
    } else {
        SdioFunctionReadyBudget::Strict
    }
}

pub struct Pi4WifiState {
    mailbox: Mailbox,
    host: SdioHost,
    power_state: WifiPowerState,
    reset_state: WifiResetState,
}

impl Pi4WifiState {
    pub fn new<H>(hal: &mut H) -> Result<Self, HalError>
    where
        H: Hardware<Error = HalError>,
    {
        log::info!("[pi4-wifi] hal init: begin");
        let mailbox = Mailbox::new(hal).map_err(|err| {
            log::warn!("[pi4-wifi] hal init: mailbox failed: {err}");
            err
        })?;
        let gpio = GpioBank::new(hal).map_err(|err| {
            log::warn!("[pi4-wifi] hal init: gpio failed: {err}");
            err
        })?;
        let host = SdioHost::new(hal, &mailbox).map_err(|err| {
            log::warn!("[pi4-wifi] hal init: sdhci failed: {err}");
            err
        })?;
        gpio.configure_wifi_sdio_pins();
        log::info!(
            "[pi4-wifi] hal init: mailbox=0x{:08x} sdhci=0x{:08x} base_clock={}Hz",
            mailbox.regs.paddr(),
            host.regs_paddr,
            host.base_clock_hz,
        );
        Ok(Self {
            mailbox,
            host,
            power_state: WifiPowerState::Off,
            reset_state: WifiResetState::Asserted,
        })
    }

    #[must_use]
    pub fn firmware_bundle(&self) -> WifiFirmwareBundle<'static> {
        WifiFirmwareBundle::new(
            PI4_WIFI_FIRMWARE,
            PI4_WIFI_NVRAM,
            Some(PI4_WIFI_CLM_BLOB),
            PI4_WIFI_BOARD_TYPE,
        )
    }

    pub fn set_power(&mut self, state: WifiPowerState) -> Result<(), HalError> {
        emit_breadcrumb(format_args!(
            "[pi4-wifi] power state={}",
            wifi_power_state_name(state)
        ));
        let was_enabled = wifi_gpio_line_enabled(self.power_state);
        self.power_state = state;
        self.apply_wifi_line(was_enabled)
    }

    pub fn set_reset(&mut self, state: WifiResetState) -> Result<(), HalError> {
        emit_breadcrumb(format_args!(
            "[pi4-wifi] reset state={}",
            wifi_reset_state_name(state)
        ));
        self.reset_state = state;
        if matches!(state, WifiResetState::Deasserted) {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] reset state=deasserted action=logical-only settle=skipped"
            ));
        }
        Ok(())
    }

    pub fn reset_host(&mut self) -> Result<(), HalError> {
        emit_breadcrumb(format_args!("[pi4-wifi] host reset begin"));
        self.host.reset_controller()
    }

    pub fn set_clock_hz(&mut self, target_hz: u32) -> Result<u32, HalError> {
        emit_breadcrumb(format_args!(
            "[pi4-wifi] host clock request={}Hz",
            target_hz
        ));
        self.host.set_clock_hz(target_hz)
    }

    #[must_use]
    pub const fn recommended_data_clock_hz(&self) -> u32 {
        self.host.preferred_data_clock_hz
    }

    pub fn debug_dump_state(&mut self, stage: &'static str) -> Result<WifiDebugSnapshot, HalError> {
        self.host.log_transport_shadow(stage);
        self.debug_snapshot()
    }

    pub fn debug_probe_ht_clock(&mut self) -> Result<bool, HalError> {
        self.host.wait_for_ht_clock_with_stage(
            "debug-probe-ht",
            "debug-probe-ht assist",
            false,
            false,
        )
    }

    pub fn set_bus_width(&mut self, width: SdioBusWidth) -> Result<(), HalError> {
        emit_breadcrumb(format_args!(
            "[pi4-wifi] host bus-width={}",
            sdio_bus_width_name(width)
        ));
        self.host.set_bus_width(width)
    }

    pub fn io_direct_read(&mut self, function: SdioFunction, addr: u32) -> Result<u8, HalError> {
        self.host.ensure_card_ready()?;
        self.host.io_direct_read(function, addr)
    }

    pub fn io_direct_write(
        &mut self,
        function: SdioFunction,
        addr: u32,
        value: u8,
    ) -> Result<(), HalError> {
        self.host.ensure_card_ready()?;
        self.host.io_direct_write(function, addr, value)
    }

    pub fn io_extended(
        &mut self,
        function: SdioFunction,
        addr: u32,
        increment_addr: bool,
        write: bool,
        buffer: &mut [u8],
    ) -> Result<(), HalError> {
        self.host.ensure_card_ready()?;
        self.host
            .io_extended(function, addr, increment_addr, write, buffer)
    }

    pub fn init_cyw43_transport(&mut self) -> Result<(), HalError> {
        self.host.init_cyw43_transport()
    }

    pub fn load_cyw43_firmware(&mut self) -> Result<(), HalError> {
        self.host.load_firmware(self.firmware_bundle())
    }

    pub fn read_cyw43_frame(&mut self, out: &mut [u8]) -> Result<usize, HalError> {
        self.host.read_frame(out)
    }

    pub fn write_cyw43_frame(&mut self, frame: &mut [u8]) -> Result<(), HalError> {
        self.host.write_frame(frame)
    }

    #[must_use]
    pub fn cyw43_control_plane_chunk_limit(&self) -> usize {
        self.host.control_plane_chunk_limit()
    }

    #[must_use]
    pub const fn cyw43_control_plane_probe_pending(&self) -> bool {
        self.host.experimental_control_plane_write_probe_pending
    }

    #[must_use]
    pub const fn cyw43_experimental_no_ht_transport(&self) -> bool {
        self.host.experimental_no_ht_transport
    }

    pub fn finish_cyw43_experimental_transport_probe(&mut self) {
        self.host.finish_experimental_transport_probe();
    }

    pub fn rearm_cyw43_control_plane_promoted_link(
        &mut self,
        speculative_ready_probe: bool,
    ) -> Result<(), HalError> {
        self.host
            .rearm_firmware_channel_after_transport_promotion(speculative_ready_probe)
    }

    pub fn rearm_cyw43_control_plane_slow_link(&mut self) -> Result<(), HalError> {
        self.host.rearm_firmware_channel_on_startup_link()
    }

    pub fn log_cyw43_control_plane_snapshot(&mut self, stage: &'static str) {
        self.host.log_control_plane_finish_snapshot(stage);
    }

    fn debug_snapshot(&mut self) -> Result<WifiDebugSnapshot, HalError> {
        let (card_ready, card_rca, card_ocr) = match self.host.card {
            Some(card) => (true, card.rca, card.ocr),
            None => (false, 0, 0),
        };
        let (io_enable, io_ready) = if card_ready {
            (
                Some(
                    self.host
                        .io_direct_read(SdioFunction::Function0, SDIO_CCCR_IOEX)?,
                ),
                Some(
                    self.host
                        .io_direct_read(SdioFunction::Function0, SDIO_CCCR_IORX)?,
                ),
            )
        } else {
            (None, None)
        };
        Ok(WifiDebugSnapshot {
            power_state: self.power_state,
            reset_state: self.reset_state,
            current_clock_hz: self.host.current_clock_hz,
            preferred_data_clock_hz: self.host.preferred_data_clock_hz,
            bus_width: self.host.desired_bus_width,
            card_ready,
            card_rca,
            card_ocr,
            io_enable,
            io_ready,
            chipclkcsr: self.host.last_chipclkcsr,
            wakeupctrl: self.host.last_wakeupctrl,
            sleepcsr: self.host.last_sleepcsr,
            cardcap: self.host.last_cardcap,
            programmed_backplane_window: self.host.programmed_backplane_window,
            shadow_backplane_window: self.host.last_backplane_window,
            shadow_backplane_fn_addr: self.host.last_backplane_function_addr,
        })
    }

    fn apply_wifi_line(&mut self, was_enabled: bool) -> Result<(), HalError> {
        let enabled = wifi_gpio_line_enabled(self.power_state);
        emit_breadcrumb(format_args!(
            "[pi4-wifi] gpio wl-on={} power={} reset={}",
            enabled as u8,
            wifi_power_state_name(self.power_state),
            wifi_reset_state_name(self.reset_state),
        ));
        let Some(target_enabled) = wifi_gpio_transition_target(was_enabled, self.power_state)
        else {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] gpio wl-on unchanged={} action=skip-mailbox",
                enabled as u8
            ));
            return Ok(());
        };
        emit_breadcrumb(format_args!(
            "[pi4-wifi] gpio wl-on transition {}->{}",
            was_enabled as u8, target_enabled as u8
        ));
        self.mailbox
            .configure_gpio_output(PI4_WIFI_GPIO, target_enabled as u32)?;
        if !was_enabled && target_enabled {
            bounded_spin_settle("wifi-power-on", WIFI_POWER_SETTLE_LOOPS);
        } else if was_enabled && !target_enabled {
            bounded_spin_settle("wifi-power-off", WIFI_POWER_DROP_SETTLE_LOOPS);
            self.host.mark_power_cycled();
        }
        Ok(())
    }
}

struct Mailbox {
    regs: MappedRegs,
    request: MappedRegs,
}

impl Mailbox {
    fn new_with_request_slot<H>(
        hal: &mut H,
        request_slot: &Mutex<Option<MappedRegs>>,
        reuse_action: &str,
        alloc_action: &str,
    ) -> Result<Self, HalError>
    where
        H: Hardware<Error = HalError>,
    {
        let regs = if let Some(regs) = cloned_pinned_regs(&PINNED_MAILBOX_REGS) {
            regs
        } else {
            let mut prefix_maps = Vec::new();
            let regs = map_exact(hal, &MAILBOX_PAGE_PADDR_CANDIDATES, &mut prefix_maps)?;
            MappedRegs::from_frame(&regs)
        };
        // Keep one uncached mailbox request page alive for acknowledged property
        // calls so Wi-Fi bring-up reuses a stable VC mailbox buffer address
        // instead of racing stale replies from short-lived per-call pages.
        let request = pinned_mailbox_request_page(hal, request_slot, reuse_action, alloc_action)?;
        Ok(Self { regs, request })
    }

    fn new<H>(hal: &mut H) -> Result<Self, HalError>
    where
        H: Hardware<Error = HalError>,
    {
        let (reuse_action, alloc_action) = mailbox_request_page_actions();
        Self::new_with_request_slot(hal, &PINNED_MAILBOX_REQUEST, reuse_action, alloc_action)
    }

    fn new_xhci_reset<H>(hal: &mut H) -> Result<Self, HalError>
    where
        H: Hardware<Error = HalError>,
    {
        // Reset-notify is serialized through the global mailbox lock and already
        // has a dedicated posted fallback path, so reuse the long-lived
        // acknowledged request page instead of allocating another uncached page
        // late in boot.
        Self::new(hal)
    }

    fn power_on_module(&mut self, module: u32) -> Result<(), HalError> {
        emit_breadcrumb(format_args!(
            "[pi4-wifi] mailbox power-on module=0x{module:08x}"
        ));
        let mut payload = [module, POWER_STATE_REQ_ON | POWER_STATE_REQ_WAIT];
        self.call_tag(TAG_SET_POWER_STATE, 8, &mut payload)?;
        Ok(())
    }

    fn get_clock_rate(&mut self, clock_id: u32) -> Result<u32, HalError> {
        emit_breadcrumb(format_args!("[pi4-wifi] mailbox clock-query id={clock_id}"));
        let mut payload = [clock_id, 0];
        self.call_tag(TAG_GET_CLOCK_RATE, 4, &mut payload)?;
        if payload[1] != 0 {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] mailbox clock-query id={clock_id} rate={}Hz",
                payload[1]
            ));
            return Ok(payload[1]);
        }

        self.call_tag(TAG_GET_MAX_CLOCK_RATE, 4, &mut payload)?;
        if payload[1] == 0 {
            return Err(HalError::Unsupported("mailbox-clock-rate"));
        }
        emit_breadcrumb(format_args!(
            "[pi4-wifi] mailbox clock-query fallback id={clock_id} rate={}Hz",
            payload[1]
        ));
        Ok(payload[1])
    }

    fn configure_gpio_output(&mut self, gpio: u32, state: u32) -> Result<(), HalError> {
        emit_breadcrumb(format_args!(
            "[pi4-wifi] mailbox gpio begin gpio={gpio} state={state}"
        ));
        let current_state = match self.gpio_state(gpio) {
            Ok(current) => Some(current),
            Err(err) => {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] mailbox gpio-state unavailable gpio={gpio} err={err}"
                ));
                None
            }
        };
        if current_state == Some(state) && state == 0 {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] mailbox gpio already-low gpio={gpio} skip-write=yes"
            ));
            return Ok(());
        }
        let polarity = self.gpio_polarity(gpio)?;
        let mut config = [gpio, GPIO_DIR_OUT, polarity, 0, 0, state];
        match self.call_tag(TAG_SET_GPIO_CONFIG, 24, &mut config) {
            Ok(()) => {}
            Err(err) if is_mailbox_protocol_error(&err) => {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] mailbox gpio-config unsupported gpio={gpio} polarity={polarity} fallback=state-only"
                ));
            }
            Err(err) => return Err(err),
        }

        let mut level = [gpio, state];
        match self.call_tag(TAG_SET_GPIO_STATE, 8, &mut level) {
            Ok(()) => {}
            Err(err) if is_mailbox_protocol_error(&err) => {
                if let Ok(confirm) = self.gpio_state(gpio) {
                    emit_breadcrumb(format_args!(
                        "[pi4-wifi] mailbox gpio-state confirm gpio={gpio} value={confirm}"
                    ));
                    if confirm == state {
                        emit_breadcrumb(format_args!(
                            "[pi4-wifi] mailbox gpio-state matched gpio={gpio} treating-as-success"
                        ));
                        return Ok(());
                    }
                }
                return Err(err);
            }
            Err(err) => return Err(err),
        }
        emit_breadcrumb(format_args!(
            "[pi4-wifi] mailbox gpio complete gpio={gpio} state={state}"
        ));
        Ok(())
    }

    fn gpio_state(&mut self, gpio: u32) -> Result<u32, HalError> {
        emit_breadcrumb(format_args!("[pi4-wifi] mailbox gpio-state gpio={gpio}"));
        let mut payload = [gpio, 0];
        self.call_tag(TAG_GET_GPIO_STATE, 4, &mut payload)?;
        emit_breadcrumb(format_args!(
            "[pi4-wifi] mailbox gpio-state gpio={gpio} value={}",
            payload[1]
        ));
        Ok(payload[1])
    }

    fn gpio_polarity(&mut self, gpio: u32) -> Result<u32, HalError> {
        emit_breadcrumb(format_args!("[pi4-wifi] mailbox gpio-polarity gpio={gpio}"));
        let mut config = [gpio, 0, 0, 0, 0];
        self.call_tag(TAG_GET_GPIO_CONFIG, 4, &mut config)?;
        emit_breadcrumb(format_args!(
            "[pi4-wifi] mailbox gpio-polarity gpio={gpio} polarity={}",
            config[2]
        ));
        Ok(config[2])
    }

    fn call_tag(
        &mut self,
        tag: u32,
        request_len_bytes: u32,
        payload: &mut [u32],
    ) -> Result<(), HalError> {
        let _mailbox_call_lock = MAILBOX_CALL_LOCK.lock();
        let original_payload = payload.to_vec();
        let words = {
            unsafe {
                core::slice::from_raw_parts_mut(self.request.vaddr() as *mut u32, PAGE_SIZE / 4)
            }
        };

        let mut last_err = HalError::Unsupported("mailbox-protocol");
        let alias_count = mailbox_ack_alias_count(tag);
        for (alias_index, &alias_base) in VC_BUS_ALIAS_BASES[..alias_count].iter().enumerate() {
            self.encode_request(words, tag, request_len_bytes, &original_payload)?;
            let request_bus = phys_to_bus(self.request.paddr(), alias_base)
                .ok_or(HalError::Unsupported("mailbox-bus-alias"))?;
            match self.send(request_bus, mailbox_recv_wait_spins(tag)) {
                Ok(()) => {
                    if words[1] != MAILBOX_RESPONSE_SUCCESS
                        || words[2] != tag
                        || (words[4] & MAILBOX_VALUE_RESPONSE) == 0
                    {
                        self.log_protocol_reply(tag, alias_base, words);
                        last_err = HalError::Unsupported("mailbox-protocol");
                        continue;
                    }
                    MAILBOX_TRANSPORT_READY.store(true, Ordering::Release);
                    if !MAILBOX_TRANSPORT_READY_LOGGED.swap(true, Ordering::AcqRel) {
                        emit_breadcrumb(format_args!(
                            "[pi4-wifi] mailbox transport ready tag={}",
                            mailbox_tag_name(tag)
                        ));
                    }
                    if alias_index > 0 {
                        emit_breadcrumb(format_args!(
                            "[pi4-wifi] mailbox alias fallback tag={} alias=0x{alias_base:08x}",
                            mailbox_tag_name(tag)
                        ));
                    }
                    payload.copy_from_slice(&words[5..5 + payload.len()]);
                    return Ok(());
                }
                Err(err @ HalError::Unsupported("mailbox-timeout"))
                | Err(err @ HalError::Unsupported("mailbox-protocol")) => {
                    last_err = err;
                    if alias_index + 1 == alias_count {
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
    ) -> Result<(), HalError> {
        let total_words = 6usize
            .checked_add(payload.len())
            .ok_or(HalError::Unsupported("mailbox-request-overflow"))?;
        if total_words > words.len() {
            return Err(HalError::Unsupported("mailbox-request-oversize"));
        }

        words.fill(0);
        words[0] = u32::try_from(total_words.saturating_mul(mem::size_of::<u32>()))
            .map_err(|_| HalError::Unsupported("mailbox-request-size"))?;
        words[1] = 0;
        words[2] = tag;
        words[3] = u32::try_from(payload.len().saturating_mul(mem::size_of::<u32>()))
            .map_err(|_| HalError::Unsupported("mailbox-request-len"))?;
        words[4] = request_len_bytes;
        words[5..5 + payload.len()].copy_from_slice(payload);
        words[5 + payload.len()] = 0;

        fence(Ordering::SeqCst);
        Ok(())
    }

    fn post_tag(&self, tag: u32, request_len_bytes: u32, payload: &[u32]) -> Result<(), HalError> {
        let _mailbox_call_lock = MAILBOX_CALL_LOCK.lock();
        let request = unsafe {
            // SAFETY: `request` is a permanently pinned uncached DMA page dedicated
            // to fire-and-forget property traffic and is not reused for
            // acknowledged mailbox requests.
            core::slice::from_raw_parts_mut(
                self.request.vaddr() as *mut u32,
                PAGE_SIZE / core::mem::size_of::<u32>(),
            )
        };
        self.encode_request(request, tag, request_len_bytes, payload)?;
        let alias_base =
            mailbox_posted_alias(tag).ok_or(HalError::Unsupported("mailbox-posted-tag"))?;
        let request_bus = phys_to_bus(self.request.paddr(), alias_base)
            .ok_or(HalError::Unsupported("mailbox-bus-alias"))?;
        self.send_posted(request_bus)?;
        MAILBOX_TRANSPORT_READY.store(true, Ordering::Release);
        if !MAILBOX_TRANSPORT_READY_LOGGED.swap(true, Ordering::AcqRel) {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] mailbox transport ready tag={}",
                mailbox_tag_name(tag)
            ));
        }
        emit_breadcrumb(format_args!(
            "[pi4-wifi] mailbox posted tag={} alias=0x{alias_base:08x}",
            mailbox_tag_name(tag)
        ));
        Ok(())
    }

    fn prepare_send(&self) -> Result<(), HalError> {
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
                return Err(HalError::Unsupported("mailbox-timeout"));
            }
            spin_loop();
        }
        Ok(())
    }

    fn send_posted(&self, data: u32) -> Result<(), HalError> {
        self.prepare_send()?;
        self.write_reg(
            MAILBOX_WRITE_OFFSET,
            (data & !0xF) | (MAILBOX_CHANNEL_PROPERTY & 0xF),
        );
        fence(Ordering::SeqCst);
        Ok(())
    }

    fn send(&self, data: u32, recv_wait_spins: usize) -> Result<(), HalError> {
        let expected = data & !0xF;
        self.send_posted(data)?;

        let mut wait = 0usize;
        loop {
            while self.read_reg(MAILBOX_STATUS0_OFFSET) & MAILBOX_EMPTY != 0 {
                wait = wait.saturating_add(1);
                if wait >= recv_wait_spins {
                    self.log_timeout("recv");
                    return Err(HalError::Unsupported("mailbox-timeout"));
                }
                spin_loop();
            }

            let value = self.read_reg(MAILBOX_READ_OFFSET);
            if (value & 0xF) == MAILBOX_CHANNEL_PROPERTY {
                let actual = value & !0xF;
                if !mailbox_reply_matches_request_page(expected, actual) {
                    emit_breadcrumb(format_args!(
                        "[pi4-wifi] mailbox stray reply expected=0x{expected:08x} actual=0x{value:08x} action=ignored",
                    ));
                    continue;
                }
                if actual != expected {
                    emit_breadcrumb(format_args!(
                        "[pi4-wifi] mailbox alias reply accepted expected=0x{expected:08x} actual=0x{value:08x}",
                    ));
                }
                return Ok(());
            }
        }
    }

    fn log_protocol_reply(&self, tag: u32, alias_base: u32, words: &[u32]) {
        let status = words.get(1).copied().unwrap_or_default();
        let reply_tag = words.get(2).copied().unwrap_or_default();
        let value_len = words.get(3).copied().unwrap_or_default();
        let value_status = words.get(4).copied().unwrap_or_default();
        let value0 = words.get(5).copied().unwrap_or_default();
        let value1 = words.get(6).copied().unwrap_or_default();
        let reason = mailbox_protocol_reason(tag, status, reply_tag, value_status);
        emit_breadcrumb(format_args!(
            "[pi4-wifi] mailbox protocol fail tag={} alias=0x{alias_base:08x} reason={reason}",
            mailbox_tag_name(tag),
        ));
        emit_breadcrumb(format_args!(
            "[pi4-wifi] mailbox protocol data st=0x{status:08x} tag=0x{reply_tag:08x} len=0x{value_len:08x} val=0x{value_status:08x} v0=0x{value0:08x} v1=0x{value1:08x}",
        ));
    }

    fn log_timeout(&self, phase: &str) {
        let status0 = self.read_reg(MAILBOX_STATUS0_OFFSET);
        let status1 = self.read_reg(MAILBOX_STATUS1_OFFSET);
        emit_breadcrumb(format_args!(
            "[pi4-wifi] mailbox timeout phase={phase} regs=0x{regs:08x} status0=0x{status0:08x} status1=0x{status1:08x}",
            regs = self.regs.paddr()
        ));
    }

    fn read_reg(&self, offset: usize) -> u32 {
        let base = self.regs.vaddr();
        unsafe { ptr::read_volatile((base + offset) as *const u32) }
    }

    fn write_reg(&self, offset: usize, value: u32) {
        let base = self.regs.vaddr();
        unsafe { ptr::write_volatile((base + offset) as *mut u32, value) };
    }
}

pub fn notify_vl805_reset<H>(hal: &mut H) -> Result<Vl805ResetNotifyResult, HalError>
where
    H: Hardware<Error = HalError>,
{
    emit_breadcrumb(format_args!(
        "[pi4-wifi] mailbox xhci-reset-notify device=0x{VL805_MAILBOX_RESET_DEV_ADDR:08x}"
    ));
    let mut mailbox = Mailbox::new_xhci_reset(hal)?;
    let mut payload = [VL805_MAILBOX_RESET_DEV_ADDR];
    match mailbox.call_tag(TAG_NOTIFY_XHCI_RESET, 4, &mut payload) {
        Ok(()) => {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] mailbox xhci-reset-notify complete"
            ));
            Ok(Vl805ResetNotifyResult::Acked)
        }
        Err(
            err @ (HalError::Unsupported("mailbox-timeout")
            | HalError::Unsupported("mailbox-protocol")),
        ) => {
            let fallback_reason = match err {
                HalError::Unsupported("mailbox-timeout") => "timeout",
                HalError::Unsupported("mailbox-protocol") => "protocol",
                _ => unreachable!(),
            };
            emit_breadcrumb(format_args!(
                "[pi4-wifi] mailbox xhci-reset-notify {fallback_reason} action=posted-fallback"
            ));
            let request = pinned_mailbox_request_page(
                hal,
                &PINNED_MAILBOX_POSTED_REQUEST,
                "reuse-posted",
                "alloc-posted",
            )?;
            let posted_mailbox = Mailbox {
                regs: mailbox.regs,
                request,
            };
            let fallback_payload = [VL805_MAILBOX_RESET_DEV_ADDR];
            posted_mailbox.post_tag(TAG_NOTIFY_XHCI_RESET, 4, &fallback_payload)?;
            emit_breadcrumb(format_args!(
                "[pi4-wifi] mailbox xhci-reset-notify posted-fallback"
            ));
            Ok(Vl805ResetNotifyResult::PostedFallback)
        }
        Err(err) => Err(err),
    }
}

struct SdioHost {
    regs: MappedRegs,
    regs_paddr: usize,
    base_clock_hz: u32,
    current_clock_hz: u32,
    preferred_data_clock_hz: u32,
    desired_bus_width: SdioBusWidth,
    card: Option<CardInfo>,
    programmed_backplane_window: Option<u32>,
    last_backplane_window: Option<u32>,
    last_backplane_function_addr: Option<u32>,
    last_backplane_window_low: u8,
    last_backplane_window_mid: u8,
    last_backplane_window_high: u8,
    last_chipclkcsr: Option<u8>,
    last_wakeupctrl: Option<u8>,
    last_sleepcsr: Option<u8>,
    last_cardcap: Option<u8>,
    experimental_no_ht_transport: bool,
    experimental_control_plane_write_probe_pending: bool,
    experimental_control_plane_reply_rearm_mode: u8,
    experimental_control_plane_reply_rearm_attempts: u8,
    experimental_control_plane_promoted_probe_pending: bool,
    block_size_count_shadow: u32,
    transfer_mode_shadow: u32,
}

impl SdioHost {
    fn new<H>(hal: &mut H, mailbox: &Mailbox) -> Result<Self, HalError>
    where
        H: Hardware<Error = HalError>,
    {
        let regs = if let Some(regs) = cloned_pinned_regs(&PINNED_SDHCI_REGS) {
            regs
        } else {
            let mut prefix_maps = Vec::new();
            let regs = map_exact(hal, &SDHCI_PAGE_PADDR_CANDIDATES, &mut prefix_maps)?;
            MappedRegs::from_frame(&regs)
        };
        let regs_paddr = regs.paddr();
        let mut mailbox = MailboxRef(mailbox);
        let base_clock_hz = match mailbox.query_clock_hz() {
            Ok(rate) => {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] sdhci base-clock source=mailbox rate={}Hz",
                    rate
                ));
                rate
            }
            Err(err) => {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] sdhci base-clock source=fallback rate=100000000Hz err={err}"
                ));
                100_000_000
            }
        };
        emit_breadcrumb(format_args!(
            "[pi4-wifi] sdhci access mode=bcm2835-shadow gap=spin delay_loops={}",
            SDHCI_WRITE_GAP_SPIN_LOOPS
        ));
        Ok(Self {
            regs,
            regs_paddr,
            base_clock_hz,
            current_clock_hz: 0,
            preferred_data_clock_hz: CYW43_FIRMWARE_BULK_CLOCK_HZ,
            desired_bus_width: SdioBusWidth::OneBit,
            card: None,
            programmed_backplane_window: None,
            last_backplane_window: None,
            last_backplane_function_addr: None,
            last_backplane_window_low: 0,
            last_backplane_window_mid: 0,
            last_backplane_window_high: 0,
            last_chipclkcsr: None,
            last_wakeupctrl: None,
            last_sleepcsr: None,
            last_cardcap: None,
            experimental_no_ht_transport: false,
            experimental_control_plane_write_probe_pending: false,
            experimental_control_plane_reply_rearm_mode: control_plane_reply_rearm_none(),
            experimental_control_plane_reply_rearm_attempts: 0,
            experimental_control_plane_promoted_probe_pending: false,
            block_size_count_shadow: 0,
            transfer_mode_shadow: 0,
        })
    }

    fn mark_power_cycled(&mut self) {
        self.card = None;
        self.current_clock_hz = 0;
        self.preferred_data_clock_hz = CYW43_FIRMWARE_BULK_CLOCK_HZ;
        self.experimental_no_ht_transport = false;
        self.experimental_control_plane_write_probe_pending = false;
        self.experimental_control_plane_reply_rearm_mode = control_plane_reply_rearm_none();
        self.experimental_control_plane_reply_rearm_attempts = 0;
        self.experimental_control_plane_promoted_probe_pending = false;
        self.block_size_count_shadow = 0;
        self.transfer_mode_shadow = 0;
        self.clear_backplane_window_cache();
    }

    fn clear_backplane_window_cache(&mut self) {
        self.programmed_backplane_window = None;
        self.last_backplane_window = None;
        self.last_backplane_function_addr = None;
        self.last_backplane_window_low = 0;
        self.last_backplane_window_mid = 0;
        self.last_backplane_window_high = 0;
    }

    fn invalidate_programmed_backplane_window(&mut self, stage: &'static str) {
        self.programmed_backplane_window = None;
        let shadow_window = self.last_backplane_window.unwrap_or(0);
        let function = self.last_backplane_function_addr.unwrap_or(0);
        emit_breadcrumb(format_args!(
            "[pi4-wifi] backplane cache stage={stage} action=invalidate-programmed-window shadow_window=0x{shadow_window:08x} fn=0x{function:05x}"
        ));
        self.log_transport_shadow(stage);
    }

    fn remember_backplane_window(
        &mut self,
        window_addr: u32,
        function_addr: u32,
        low: u8,
        mid: u8,
        high: u8,
    ) {
        self.last_backplane_window = Some(window_addr);
        self.last_backplane_function_addr = Some(function_addr);
        self.last_backplane_window_low = low;
        self.last_backplane_window_mid = mid;
        self.last_backplane_window_high = high;
    }

    fn remember_chipclkcsr(&mut self, value: u8) {
        self.last_chipclkcsr = Some(value);
    }

    fn remember_wakeupctrl(&mut self, value: u8) {
        self.last_wakeupctrl = Some(value);
    }

    fn remember_sleepcsr(&mut self, value: u8) {
        self.last_sleepcsr = Some(value);
    }

    fn remember_cardcap(&mut self, value: u8) {
        self.last_cardcap = Some(value);
    }

    fn log_transport_shadow(&self, stage: &'static str) {
        emit_breadcrumb(format_args!(
            "[pi4-wifi] sdio shadow {stage} regs=0x{regs:08x} window=0x{window:08x} fn=0x{function:05x} bytes={low:02x}:{mid:02x}:{high:02x} cached={cached}",
            regs = self.regs_paddr,
            window = self.last_backplane_window.unwrap_or(0),
            function = self.last_backplane_function_addr.unwrap_or(0),
            low = self.last_backplane_window_low,
            mid = self.last_backplane_window_mid,
            high = self.last_backplane_window_high,
            cached = yn(self.last_backplane_window.is_some()),
        ));
        emit_breadcrumb(format_args!(
            "[pi4-wifi] sdio shadow {stage} chipclk=0x{chipclk:02x}/{} wake=0x{wake:02x}/{} sleep=0x{sleep:02x}/{} cardcap=0x{cardcap:02x}/{} hz={} width={}",
            yn(self.last_chipclkcsr.is_some()),
            yn(self.last_wakeupctrl.is_some()),
            yn(self.last_sleepcsr.is_some()),
            yn(self.last_cardcap.is_some()),
            self.current_clock_hz,
            sdio_bus_width_name(self.desired_bus_width),
            chipclk = self.last_chipclkcsr.unwrap_or(0),
            wake = self.last_wakeupctrl.unwrap_or(0),
            sleep = self.last_sleepcsr.unwrap_or(0),
            cardcap = self.last_cardcap.unwrap_or(0),
        ));
    }

    fn control_plane_chunk_limit(&self) -> usize {
        experimental_function2_fifo_chunk_limit(
            SdioFunction::Function2,
            false,
            self.experimental_no_ht_transport,
        )
    }

    fn finish_experimental_transport_probe(&mut self) {
        self.experimental_no_ht_transport = false;
        self.experimental_control_plane_write_probe_pending = false;
        self.experimental_control_plane_promoted_probe_pending = false;
    }

    fn enter_bounded_no_ht_transport(&mut self, stage: &'static str) {
        self.experimental_no_ht_transport = true;
        self.experimental_control_plane_write_probe_pending = true;
        self.experimental_control_plane_reply_rearm_mode = control_plane_reply_rearm_none();
        self.experimental_control_plane_reply_rearm_attempts = 0;
        self.experimental_control_plane_promoted_probe_pending = false;
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware stage={stage} action=transport-mode mode={} current_clock={}Hz width={} chunk_limit={} probe_pending={}",
            cyw43_transport_mode_name(self.experimental_no_ht_transport),
            self.current_clock_hz,
            sdio_bus_width_name(self.desired_bus_width),
            self.control_plane_chunk_limit(),
            yn(self.experimental_control_plane_write_probe_pending),
        ));
        self.log_transport_shadow("bounded-no-ht-transport");
    }

    fn rearm_firmware_channel_on_startup_link(&mut self) -> Result<(), HalError> {
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware stage=slow-link-channel-rearm action=begin current={}Hz width={}",
            self.current_clock_hz,
            sdio_bus_width_name(self.desired_bus_width),
        ));
        self.refresh_transport_phase_for("slow-link-channel-rearm")?;
        let mut attempt = 0usize;
        loop {
            match self
                .rearm_firmware_channel_once(SdioFunctionReadyBudget::ExperimentalBypass, false)
            {
                Ok(()) => {
                    self.experimental_control_plane_promoted_probe_pending = false;
                    emit_breadcrumb(format_args!(
                        "[pi4-wifi] firmware stage=slow-link-channel-rearm action=ready"
                    ));
                    self.log_control_plane_finish_snapshot("slow-link-channel-rearm-ready");
                    return Ok(());
                }
                Err(err) if firmware_phase_can_retry(&err, attempt) => {
                    emit_breadcrumb(format_args!(
                        "[pi4-wifi] firmware stage=slow-link-channel-rearm attempt={} err={err} action=recover-retry",
                        attempt + 1
                    ));
                    self.recover_command_path_and_refresh_transport("slow-link-channel-rearm")?;
                    attempt += 1;
                }
                Err(err) => return Err(err),
            }
        }
    }

    fn rearm_firmware_channel_after_first_control_write(&mut self) -> Result<(), HalError> {
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware stage=control-plane-post-write-rearm action=begin current={}Hz width={}",
            self.current_clock_hz,
            sdio_bus_width_name(self.desired_bus_width),
        ));
        self.refresh_transport_phase_for("control-plane-post-write-rearm")?;
        let mut attempt = 0usize;
        loop {
            match self
                .rearm_firmware_channel_once(SdioFunctionReadyBudget::ControlPlaneReplyProbe, true)
            {
                Ok(()) => {
                    self.experimental_control_plane_reply_rearm_mode =
                        control_plane_reply_rearm_startup_link();
                    self.experimental_control_plane_reply_rearm_attempts = 0;
                    self.experimental_control_plane_promoted_probe_pending = false;
                    emit_breadcrumb(format_args!(
                        "[pi4-wifi] firmware stage=control-plane-post-write-rearm action=ready"
                    ));
                    self.log_control_plane_finish_snapshot("control-plane-post-write-rearm-ready");
                    return Ok(());
                }
                Err(err) if firmware_phase_can_retry(&err, attempt) => {
                    emit_breadcrumb(format_args!(
                        "[pi4-wifi] firmware stage=control-plane-post-write-rearm attempt={} err={err} action=recover-retry",
                        attempt + 1
                    ));
                    self.recover_command_path_and_refresh_transport(
                        "control-plane-post-write-rearm",
                    )?;
                    attempt += 1;
                }
                Err(err) => return Err(err),
            }
        }
    }

    fn promote_control_plane_after_post_write_rearm_timeout(
        &mut self,
        err: &HalError,
    ) -> Result<(), HalError> {
        let target_clock_hz =
            control_plane_clock_target_hz(self.current_clock_hz, self.preferred_data_clock_hz);
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware stage=control-plane-post-write-rearm action=promote-after-timeout err={err} current={}Hz width={} target_clock={}Hz target_bus_width=4 mode=speculative-first-reply",
            self.current_clock_hz,
            sdio_bus_width_name(self.desired_bus_width),
            target_clock_hz,
        ));
        self.set_bus_width(SdioBusWidth::FourBit)?;
        self.ensure_control_plane_clock("control-plane-post-write-promote-clock")?;
        self.rearm_firmware_channel_after_transport_promotion(true)?;
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware stage=control-plane-post-write-rearm action=promote-ready current={}Hz width={} mode=speculative-first-reply",
            self.current_clock_hz,
            sdio_bus_width_name(self.desired_bus_width),
        ));
        Ok(())
    }

    fn rearm_firmware_channel_after_transport_promotion(
        &mut self,
        speculative_ready_probe: bool,
    ) -> Result<(), HalError> {
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware stage=control-plane-promote-rearm action=begin current={}Hz width={} mode={}",
            self.current_clock_hz,
            sdio_bus_width_name(self.desired_bus_width),
            control_plane_promote_rearm_mode_name(speculative_ready_probe),
        ));
        self.refresh_transport_phase_for("control-plane-promote-rearm")?;
        let mut attempt = 0usize;
        loop {
            match self.rearm_firmware_channel_once(
                control_plane_promote_rearm_budget(speculative_ready_probe),
                speculative_ready_probe,
            ) {
                Ok(()) => {
                    self.experimental_control_plane_reply_rearm_mode =
                        control_plane_reply_rearm_promoted_link();
                    self.experimental_control_plane_reply_rearm_attempts = 0;
                    self.experimental_control_plane_promoted_probe_pending =
                        speculative_ready_probe;
                    emit_breadcrumb(format_args!(
                        "[pi4-wifi] firmware stage=control-plane-promote-rearm action=ready mode={}",
                        control_plane_promote_rearm_mode_name(speculative_ready_probe),
                    ));
                    self.log_control_plane_finish_snapshot("control-plane-promote-rearm-ready");
                    return Ok(());
                }
                Err(err) if firmware_phase_can_retry(&err, attempt) => {
                    emit_breadcrumb(format_args!(
                        "[pi4-wifi] firmware stage=control-plane-promote-rearm attempt={} err={err} action=recover-retry mode={}",
                        attempt + 1,
                        control_plane_promote_rearm_mode_name(speculative_ready_probe),
                    ));
                    self.recover_command_path_and_refresh_transport("control-plane-promote-rearm")?;
                    attempt += 1;
                }
                Err(err) => return Err(err),
            }
        }
    }

    fn maybe_rearm_control_plane_reply_on_zero_frame(&mut self) -> Result<(), HalError> {
        let ready = self.io_direct_read(SdioFunction::Function0, SDIO_CCCR_IORX)?;
        let function2_ready = (ready & SDIO_FUNC_READY_2) == SDIO_FUNC_READY_2;
        let attempt = usize::from(self.experimental_control_plane_reply_rearm_attempts);
        let reply_rearm_mode = self.experimental_control_plane_reply_rearm_mode;
        let promoted_rearm = control_plane_reply_rearm_uses_promoted_link(reply_rearm_mode);
        let speculative_promoted_probe = self.experimental_control_plane_promoted_probe_pending;

        if function2_ready {
            self.experimental_control_plane_reply_rearm_mode = control_plane_reply_rearm_none();
            self.experimental_control_plane_reply_rearm_attempts = 0;
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware stage=control-plane-reply action=function2-ready iorx=0x{ready:02x}"
            ));
            return Ok(());
        }

        if control_plane_startup_link_probe_stalled_after_rearm(
            reply_rearm_mode,
            function2_ready,
            attempt,
        ) {
            let err = HalError::Unsupported("cyw43-control-plane-startup-link-reply-timeout");
            if control_plane_startup_link_timeout_needs_promoted_probe(
                self.experimental_no_ht_transport,
                reply_rearm_mode,
            ) {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware stage=control-plane-reply action=promote-startup-link-timeout mode={} attempt={} iorx=0x{ready:02x} current_clock={}Hz width={} chunk_limit={} no_ht={}",
                    control_plane_reply_rearm_mode_name(reply_rearm_mode),
                    attempt,
                    self.current_clock_hz,
                    sdio_bus_width_name(self.desired_bus_width),
                    self.control_plane_chunk_limit(),
                    self.experimental_no_ht_transport,
                ));
                self.promote_control_plane_after_post_write_rearm_timeout(&err)?;
                return Ok(());
            }
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware stage=control-plane-reply action=startup-link-rearm-stalled mode={} attempt={} iorx=0x{ready:02x} current_clock={}Hz width={} chunk_limit={} no_ht={}",
                control_plane_reply_rearm_mode_name(reply_rearm_mode),
                attempt,
                self.current_clock_hz,
                sdio_bus_width_name(self.desired_bus_width),
                self.control_plane_chunk_limit(),
                self.experimental_no_ht_transport,
            ));
            self.log_control_plane_finish_snapshot("control-plane-startup-link-rearm-stalled");
            self.experimental_control_plane_reply_rearm_mode = control_plane_reply_rearm_none();
            self.experimental_control_plane_reply_rearm_attempts = 0;
            return Err(err);
        }

        if !control_plane_zero_frame_needs_reply_rearm(reply_rearm_mode, function2_ready, attempt) {
            return Ok(());
        }

        let next_attempt = self
            .experimental_control_plane_reply_rearm_attempts
            .saturating_add(1);
        self.experimental_control_plane_reply_rearm_attempts = next_attempt;
        let action = if promoted_rearm {
            "promoted-rearm"
        } else {
            "zero-frame-rearm"
        };
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware stage=control-plane-reply action={action} mode={} attempt={} iorx=0x{ready:02x} current_clock={}Hz width={} chunk_limit={} no_ht={}",
            control_plane_reply_rearm_mode_name(reply_rearm_mode),
            next_attempt
            ,
            self.current_clock_hz,
            sdio_bus_width_name(self.desired_bus_width),
            self.control_plane_chunk_limit(),
            self.experimental_no_ht_transport,
        ));
        if promoted_rearm {
            self.rearm_firmware_channel_after_transport_promotion(speculative_promoted_probe)?;
        } else {
            self.rearm_firmware_channel_after_first_control_write()?;
        }
        let ready_after = self.io_direct_read(SdioFunction::Function0, SDIO_CCCR_IORX)?;
        let function2_ready_after = (ready_after & SDIO_FUNC_READY_2) == SDIO_FUNC_READY_2;
        self.experimental_control_plane_reply_rearm_attempts =
            control_plane_reply_rearm_attempts_after_rearm(function2_ready_after, next_attempt);
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware stage=control-plane-reply action={action}-ready mode={} attempt={} iorx=0x{ready_after:02x} current_clock={}Hz width={} chunk_limit={} no_ht={}",
            control_plane_reply_rearm_mode_name(reply_rearm_mode),
            next_attempt,
            self.current_clock_hz,
            sdio_bus_width_name(self.desired_bus_width),
            self.control_plane_chunk_limit(),
            self.experimental_no_ht_transport,
        ));
        if function2_ready_after {
            self.experimental_control_plane_reply_rearm_mode = control_plane_reply_rearm_none();
            return Ok(());
        }
        if control_plane_startup_link_probe_stalled_after_rearm(
            reply_rearm_mode,
            function2_ready_after,
            usize::from(next_attempt),
        ) {
            let err = HalError::Unsupported("cyw43-control-plane-startup-link-reply-timeout");
            if control_plane_startup_link_timeout_needs_promoted_probe(
                self.experimental_no_ht_transport,
                reply_rearm_mode,
            ) {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware stage=control-plane-reply action=promote-startup-link-timeout mode={} attempt={} iorx=0x{ready_after:02x} current_clock={}Hz width={} chunk_limit={} no_ht={}",
                    control_plane_reply_rearm_mode_name(reply_rearm_mode),
                    next_attempt,
                    self.current_clock_hz,
                    sdio_bus_width_name(self.desired_bus_width),
                    self.control_plane_chunk_limit(),
                    self.experimental_no_ht_transport,
                ));
                self.promote_control_plane_after_post_write_rearm_timeout(&err)?;
                return Ok(());
            }
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware stage=control-plane-reply action=startup-link-rearm-stalled mode={} attempt={} iorx=0x{ready_after:02x} current_clock={}Hz width={} chunk_limit={} no_ht={}",
                control_plane_reply_rearm_mode_name(reply_rearm_mode),
                next_attempt,
                self.current_clock_hz,
                sdio_bus_width_name(self.desired_bus_width),
                self.control_plane_chunk_limit(),
                self.experimental_no_ht_transport,
            ));
            self.log_control_plane_finish_snapshot("control-plane-startup-link-rearm-stalled");
            self.experimental_control_plane_reply_rearm_mode = control_plane_reply_rearm_none();
            self.experimental_control_plane_reply_rearm_attempts = 0;
            return Err(err);
        }
        if control_plane_promoted_probe_stalled_after_rearm(
            reply_rearm_mode,
            speculative_promoted_probe,
            function2_ready_after,
            next_attempt,
        ) {
            self.experimental_control_plane_reply_rearm_mode = control_plane_reply_rearm_none();
            self.experimental_control_plane_reply_rearm_attempts = 0;
            self.experimental_control_plane_promoted_probe_pending = false;
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware stage=control-plane-reply action=promoted-rearm-stalled mode={} attempt={} iorx=0x{ready_after:02x} current_clock={}Hz width={} chunk_limit={} no_ht={}",
                control_plane_reply_rearm_mode_name(reply_rearm_mode),
                next_attempt,
                self.current_clock_hz,
                sdio_bus_width_name(self.desired_bus_width),
                self.control_plane_chunk_limit(),
                self.experimental_no_ht_transport,
            ));
            return Err(HalError::Unsupported(
                "cyw43-control-plane-promoted-rearm-timeout",
            ));
        }
        Ok(())
    }

    fn rearm_firmware_channel_once(
        &mut self,
        function2_ready_budget: SdioFunctionReadyBudget,
        allow_setup_write_bypass: bool,
    ) -> Result<(), HalError> {
        self.enable_function2(function2_ready_budget)?;
        let restore_clock_hz = firmware_channel_write_restore_clock_hz(
            self.experimental_no_ht_transport,
            self.current_clock_hz,
        );
        if let Some(clock_hz) = restore_clock_hz {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware stage=firmware-channel-write-clock action=lower current={}Hz target={}Hz width={} budget={} no_ht={}",
                self.current_clock_hz,
                CYW43_STARTUP_CLOCK_HZ,
                sdio_bus_width_name(self.desired_bus_width),
                sdio_function_ready_budget_name(function2_ready_budget),
                self.experimental_no_ht_transport,
            ));
            self.set_clock_hz(CYW43_STARTUP_CLOCK_HZ)?;
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware stage=firmware-channel-write-clock action=lower-ready previous={}Hz current={}Hz width={} budget={} no_ht={}",
                clock_hz,
                self.current_clock_hz,
                sdio_bus_width_name(self.desired_bus_width),
                sdio_function_ready_budget_name(function2_ready_budget),
                self.experimental_no_ht_transport,
            ));
        }
        let write_result = (|| {
            self.write_sdio_core_u32_for_firmware_channel(
                "slow-link-channel-rearm-mailbox-version",
                SDPCMD_REG_TOSBMAILBOXDATA,
                SDPCM_PROT_VERSION << HMB_DATA_VERSION_SHIFT,
                allow_setup_write_bypass,
            )?;
            self.write_sdio_core_u32_for_firmware_channel(
                "slow-link-channel-rearm-hostintmask",
                SDPCMD_REG_HOSTINTMASK,
                HOSTINTMASK,
                allow_setup_write_bypass,
            )?;
            self.io_direct_write(
                SdioFunction::Function1,
                SBSDIO_WATERMARK,
                CY_43455_F2_WATERMARK,
            )?;
            let devctl = self.io_direct_read(SdioFunction::Function1, SBSDIO_DEVICE_CTL)?;
            self.io_direct_write(
                SdioFunction::Function1,
                SBSDIO_DEVICE_CTL,
                devctl | SBSDIO_DEVCTL_F2WM_ENAB,
            )?;
            self.io_direct_write(
                SdioFunction::Function1,
                SBSDIO_FUNC1_MESBUSYCTRL,
                CY_43455_MESBUSYCTRL,
            )?;
            Ok(())
        })();
        if let Some(clock_hz) = restore_clock_hz {
            match self.set_clock_hz(clock_hz) {
                Ok(restored_clock_hz) => {
                    emit_breadcrumb(format_args!(
                        "[pi4-wifi] firmware stage=firmware-channel-write-clock action=restore restored={}Hz width={} budget={} no_ht={}",
                        restored_clock_hz,
                        sdio_bus_width_name(self.desired_bus_width),
                        sdio_function_ready_budget_name(function2_ready_budget),
                        self.experimental_no_ht_transport,
                    ));
                }
                Err(err) => {
                    emit_breadcrumb(format_args!(
                        "[pi4-wifi] firmware stage=firmware-channel-write-clock action=restore-fail target={}Hz current={}Hz width={} budget={} no_ht={} err={err}",
                        clock_hz,
                        self.current_clock_hz,
                        sdio_bus_width_name(self.desired_bus_width),
                        sdio_function_ready_budget_name(function2_ready_budget),
                        self.experimental_no_ht_transport,
                    ));
                    return Err(err);
                }
            }
        }
        write_result
    }

    fn log_control_plane_finish_snapshot(&mut self, stage: &'static str) {
        self.log_transport_shadow(stage);

        let ioex = self
            .io_direct_read(SdioFunction::Function0, SDIO_CCCR_IOEX)
            .ok();
        let iorx = self
            .io_direct_read(SdioFunction::Function0, SDIO_CCCR_IORX)
            .ok();
        let ienx = self
            .io_direct_read(SdioFunction::Function0, SDIO_CCCR_IENX)
            .ok();
        emit_breadcrumb(format_args!(
            "[pi4-wifi] control-plane snapshot {stage} ioex=0x{ioex:02x}/{} iorx=0x{iorx:02x}/{} ienx=0x{ienx:02x}/{}",
            yn(ioex.is_some()),
            yn(iorx.is_some()),
            yn(ienx.is_some()),
            ioex = ioex.unwrap_or(0),
            iorx = iorx.unwrap_or(0),
            ienx = ienx.unwrap_or(0),
        ));

        let use_live_sdio_core_reads =
            control_plane_snapshot_uses_live_sdio_core_reads(self.experimental_no_ht_transport);
        if !use_live_sdio_core_reads {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] control-plane snapshot {stage} action=skip-unstable-sdio-core-readbacks current_clock={}Hz width={} no_ht=1 reply_mode={} reply_attempts={} chunk_limit={}",
                self.current_clock_hz,
                sdio_bus_width_name(self.desired_bus_width),
                control_plane_reply_rearm_mode_name(self.experimental_control_plane_reply_rearm_mode),
                self.experimental_control_plane_reply_rearm_attempts,
                self.control_plane_chunk_limit(),
            ));
        }
        let hostintmask = if use_live_sdio_core_reads {
            self.read_control_plane_snapshot_u32(stage, "hostintmask", SDPCMD_REG_HOSTINTMASK)
        } else {
            None
        };
        let tohost_mailbox = if use_live_sdio_core_reads {
            self.read_control_plane_snapshot_u32(
                stage,
                "tohost-mailbox",
                SDPCMD_REG_TOHOSTMAILBOXDATA,
            )
        } else {
            None
        };
        let int_status = if use_live_sdio_core_reads {
            self.read_control_plane_snapshot_u32(stage, "int-status", SDIO_INT_STATUS)
        } else {
            None
        };
        let rframe_lo = self
            .io_direct_read(SdioFunction::Function1, SBSDIO_FUNC1_RFRAMEBCLO)
            .ok();
        let rframe_hi = self
            .io_direct_read(SdioFunction::Function1, SBSDIO_FUNC1_RFRAMEBCHI)
            .ok();
        let watermark = self
            .io_direct_read(SdioFunction::Function1, SBSDIO_WATERMARK)
            .ok();
        let devctl = self
            .io_direct_read(SdioFunction::Function1, SBSDIO_DEVICE_CTL)
            .ok();
        let mesbusy = self
            .io_direct_read(SdioFunction::Function1, SBSDIO_FUNC1_MESBUSYCTRL)
            .ok();
        emit_breadcrumb(format_args!(
            "[pi4-wifi] control-plane snapshot {stage} hostintmask=0x{hostintmask:08x}/{} tohost=0x{tohost:08x}/{} int_status=0x{int_status:08x}/{} rframe=0x{rframe_hi:02x}{rframe_lo:02x}/{} watermark=0x{watermark:02x}/{} devctl=0x{devctl:02x}/{} mesbusy=0x{mesbusy:02x}/{}",
            yn(hostintmask.is_some()),
            yn(tohost_mailbox.is_some()),
            yn(int_status.is_some()),
            yn(rframe_lo.is_some() && rframe_hi.is_some()),
            yn(watermark.is_some()),
            yn(devctl.is_some()),
            yn(mesbusy.is_some()),
            hostintmask = hostintmask.unwrap_or(0),
            tohost = tohost_mailbox.unwrap_or(0),
            int_status = int_status.unwrap_or(0),
            rframe_hi = rframe_hi.unwrap_or(0),
            rframe_lo = rframe_lo.unwrap_or(0),
            watermark = watermark.unwrap_or(0),
            devctl = devctl.unwrap_or(0),
            mesbusy = mesbusy.unwrap_or(0),
        ));
    }

    fn read_control_plane_snapshot_u32(
        &mut self,
        stage: &'static str,
        name: &'static str,
        offset: u32,
    ) -> Option<u32> {
        self.read_sdio_core_u32_with_f1_fallback(stage, name, offset)
            .ok()
    }

    fn reset_controller(&mut self) -> Result<(), HalError> {
        self.write16(SDHCI_CLOCK_CONTROL, 0);
        self.write8(SDHCI_POWER_CONTROL, 0);
        bounded_spin_settle("sdhci-power-off", SDHCI_POWER_OFF_SETTLE_LOOPS);
        self.software_reset(SDHCI_RESET_ALL)?;
        self.write8(SDHCI_POWER_CONTROL, SDHCI_POWER_330 | SDHCI_POWER_ON);
        self.settle_power_on_ready();
        self.write8(SDHCI_TIMEOUT_CONTROL, 0x0E);
        self.write32(SDHCI_INT_STATUS, SDHCI_INT_ALL_MASK);
        self.write32(SDHCI_INT_ENABLE, SDHCI_INT_ALL_MASK);
        self.write32(SDHCI_SIGNAL_ENABLE, 0);
        if let Err(err) = self.set_clock_hz(400_000) {
            emit_breadcrumb(format_args!("[pi4-wifi] host reset clock-retry err={err}"));
            self.software_reset(SDHCI_RESET_CMD | SDHCI_RESET_DATA).ok();
            self.write32(SDHCI_INT_STATUS, SDHCI_INT_ALL_MASK);
            self.set_clock_hz(400_000)?;
        }
        self.apply_host_bus_width(self.desired_bus_width);
        self.card = None;
        self.clear_backplane_window_cache();
        self.log_host_state("after-reset");
        Ok(())
    }

    fn settle_power_on_ready(&mut self) {
        emit_breadcrumb(format_args!(
            "[pi4-wifi] settle stage=sdhci-power-on mode=poll limit={}",
            SDHCI_POWER_READY_LOOPS
        ));
        for loops in 0..SDHCI_POWER_READY_LOOPS {
            let power = self.read8(SDHCI_POWER_CONTROL);
            let present = self.read32(SDHCI_PRESENT_STATE);
            if sdhci_power_ready(power, present) {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] settle stage=sdhci-power-on ready=poll loops={}",
                    loops + 1
                ));
                return;
            }
            spin_loop();
        }
        emit_breadcrumb(format_args!(
            "[pi4-wifi] settle stage=sdhci-power-on action=fallback-spin loops={}",
            SDHCI_POWER_SETTLE_LOOPS
        ));
        self.log_host_state("sdhci-power-on-poll-timeout");
        spin_settle(SDHCI_POWER_SETTLE_LOOPS);
    }

    fn set_clock_hz(&mut self, target_hz: u32) -> Result<u32, HalError> {
        let target_hz = target_hz.max(1);
        self.wait_inhibit_clear(true)?;
        self.write16(SDHCI_CLOCK_CONTROL, 0);
        if target_hz == 0 {
            self.current_clock_hz = 0;
            return Ok(0);
        }

        let version = self.read16(SDHCI_HOST_VERSION) & SDHCI_SPEC_VER_MASK;
        let divider = self.compute_divider(target_hz, version);
        let encoded_divider = if version >= SDHCI_SPEC_300 {
            divider >> 1
        } else {
            divider >> 1
        };
        let mut clock = SDHCI_CLOCK_INT_EN;
        clock |= (encoded_divider & SDHCI_DIV_MASK) << SDHCI_DIVIDER_SHIFT;
        clock |= ((encoded_divider & SDHCI_DIV_HI_MASK) >> 8) << SDHCI_DIVIDER_HI_SHIFT;
        self.write16(SDHCI_CLOCK_CONTROL, clock);
        self.wait_for_int_clock_stable()?;
        self.write16(SDHCI_CLOCK_CONTROL, clock | SDHCI_CLOCK_CARD_EN);

        self.current_clock_hz = if divider == 0 {
            self.base_clock_hz
        } else {
            self.base_clock_hz / u32::from(divider)
        };
        Ok(self.current_clock_hz)
    }

    fn set_bus_width(&mut self, width: SdioBusWidth) -> Result<(), HalError> {
        self.desired_bus_width = width;
        self.apply_host_bus_width(width);
        if self.card.is_some() {
            let value = match width {
                SdioBusWidth::OneBit => SDIO_BUS_WIDTH_1BIT,
                SdioBusWidth::FourBit => SDIO_BUS_WIDTH_4BIT,
            };
            self.io_direct_write(SdioFunction::Function0, SDIO_CCCR_IF, value)?;
        }
        Ok(())
    }

    fn ensure_card_ready(&mut self) -> Result<(), HalError> {
        if self.card.is_some() {
            return Ok(());
        }

        let mut last_err = HalError::Unsupported("sdio-card-init");
        for attempt in 1..=SDIO_CARD_INIT_ATTEMPTS {
            emit_breadcrumb(format_args!("[pi4-wifi] sdio card-init attempt={attempt}"));
            match self.try_card_init() {
                Ok(()) => return Ok(()),
                Err(err) => {
                    last_err = err;
                    if attempt == SDIO_CARD_INIT_ATTEMPTS {
                        break;
                    }
                    emit_breadcrumb(format_args!(
                        "[pi4-wifi] sdio card-init retry attempt={} err={}",
                        attempt + 1,
                        last_err
                    ));
                    bounded_spin_settle("sdio-card-retry", SDIO_CARD_INIT_RETRY_SETTLE_LOOPS);
                }
            }
        }

        Err(last_err)
    }

    fn try_card_init(&mut self) -> Result<(), HalError> {
        emit_breadcrumb(format_args!("[pi4-wifi] sdio card-init begin"));
        self.reset_controller()?;

        emit_breadcrumb(format_args!("[pi4-wifi] sdio card-init phase=cmd0"));
        self.send_command(0, 0, ResponseType::None)?;

        emit_breadcrumb(format_args!("[pi4-wifi] sdio card-init phase=cmd5-probe"));
        self.log_host_state("before-cmd5-probe");
        let mut ocr = 0u32;
        for _ in 0..SDIO_INIT_WAIT_LOOPS {
            ocr = self.send_command(SDIO_CMD5, 0, ResponseType::Ocr)?[0];
            if (ocr & SDIO_OCR_3V2_3V4) != 0 {
                break;
            }
            spin_loop();
        }
        if (ocr & SDIO_OCR_3V2_3V4) == 0 {
            return Err(HalError::Unsupported("sdio-ocr-timeout"));
        }
        emit_breadcrumb(format_args!("[pi4-wifi] sdio card-ocr raw=0x{ocr:08x}"));

        let desired_ocr = ocr & SDIO_OCR_3V2_3V4;
        emit_breadcrumb(format_args!(
            "[pi4-wifi] sdio card-init phase=cmd5-ready ocr=0x{desired_ocr:08x}"
        ));
        self.log_host_state("before-cmd5-ready");
        for _ in 0..SDIO_INIT_WAIT_LOOPS {
            ocr = self.send_command(SDIO_CMD5, desired_ocr, ResponseType::Ocr)?[0];
            if (ocr & SDIO_R4_READY) != 0 {
                break;
            }
            spin_loop();
        }
        if (ocr & SDIO_R4_READY) == 0 {
            return Err(HalError::Unsupported("sdio-card-not-ready"));
        }

        emit_breadcrumb(format_args!("[pi4-wifi] sdio card-init phase=cmd3"));
        let rca = (self.send_command(SDIO_CMD3, 0, ResponseType::Short)?[0] >> 16) as u16;
        if rca == 0 {
            return Err(HalError::Unsupported("sdio-missing-rca"));
        }
        emit_breadcrumb(format_args!("[pi4-wifi] sdio card-init phase=cmd7"));
        self.send_command(SDIO_CMD7, u32::from(rca) << 16, ResponseType::ShortBusy)?;
        self.card = Some(CardInfo { rca, ocr });
        self.apply_host_bus_width(self.desired_bus_width);

        if matches!(self.desired_bus_width, SdioBusWidth::FourBit) {
            self.io_direct_write(SdioFunction::Function0, SDIO_CCCR_IF, SDIO_BUS_WIDTH_4BIT)?;
        }

        emit_breadcrumb(format_args!(
            "[pi4-wifi] sdio card-init ready rca=0x{rca:04x} ocr=0x{ocr:08x} width={}",
            sdio_bus_width_name(self.desired_bus_width)
        ));
        Ok(())
    }

    fn io_direct_read(&mut self, function: SdioFunction, addr: u32) -> Result<u8, HalError> {
        self.io_direct_read_with_cmd53_fallback(function, addr, true)
    }

    fn io_direct_read_no_cmd53_fallback(
        &mut self,
        function: SdioFunction,
        addr: u32,
    ) -> Result<u8, HalError> {
        self.io_direct_read_with_cmd53_fallback(function, addr, false)
    }

    fn io_direct_read_with_cmd53_fallback(
        &mut self,
        function: SdioFunction,
        addr: u32,
        allow_cmd53_fallback: bool,
    ) -> Result<u8, HalError> {
        let arg = cmd52_argument(function, addr, false, 0);
        let resp = self.send_command(SDIO_CMD52, arg, ResponseType::Short)?[0];
        let status = r5_status(resp);
        if status != 0 {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] sdio cmd52 fail op=read fn={} addr=0x{addr:05x} resp=0x{resp:08x} r5=0x{status:04x}",
                function.number()
            ));
            if allow_cmd53_fallback && io_direct_cmd53_byte_fallback_allowed(function) {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] sdio cmd52 fallback op=read fn={} addr=0x{addr:05x} to=cmd53-byte",
                    function.number()
                ));
                match self.io_direct_fallback_read(function, addr) {
                    Ok(value) => return Ok(value),
                    Err(fallback_err) => {
                        emit_breadcrumb(format_args!(
                            "[pi4-wifi] sdio cmd52 fallback op=read fn={} addr=0x{addr:05x} err={fallback_err}",
                            function.number()
                        ));
                        return Err(fallback_err);
                    }
                }
            }
            return Err(HalError::Unsupported("sdio-cmd52-read"));
        }
        Ok((resp & 0xFF) as u8)
    }

    fn io_direct_write(
        &mut self,
        function: SdioFunction,
        addr: u32,
        value: u8,
    ) -> Result<(), HalError> {
        let arg = cmd52_argument(function, addr, true, value);
        let resp = self.send_command(SDIO_CMD52, arg, ResponseType::Short)?[0];
        let status = r5_status(resp);
        if status != 0 {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] sdio cmd52 fail op=write fn={} addr=0x{addr:05x} val=0x{value:02x} resp=0x{resp:08x} r5=0x{status:04x}",
                function.number()
            ));
            if io_direct_cmd53_byte_fallback_allowed(function) {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] sdio cmd52 fallback op=write fn={} addr=0x{addr:05x} to=cmd53-byte",
                    function.number()
                ));
                match self.io_direct_fallback_write(function, addr, value) {
                    Ok(()) => return Ok(()),
                    Err(fallback_err) => {
                        emit_breadcrumb(format_args!(
                            "[pi4-wifi] sdio cmd52 fallback op=write fn={} addr=0x{addr:05x} err={fallback_err}",
                            function.number()
                        ));
                        return Err(fallback_err);
                    }
                }
            }
            return Err(HalError::Unsupported("sdio-cmd52-write"));
        }
        Ok(())
    }

    fn io_direct_fallback_read(
        &mut self,
        function: SdioFunction,
        addr: u32,
    ) -> Result<u8, HalError> {
        let mut byte = [0u8; 1];
        self.recover_command_path("sdio-cmd52-read-cmd53-byte");
        self.io_extended_byte_mode(function, addr, false, false, &mut byte)?;
        Ok(byte[0])
    }

    fn io_direct_fallback_write(
        &mut self,
        function: SdioFunction,
        addr: u32,
        value: u8,
    ) -> Result<(), HalError> {
        let mut byte = [value];
        self.recover_command_path("sdio-cmd52-write-cmd53-byte");
        self.io_extended_byte_mode(function, addr, false, true, &mut byte)
    }

    fn io_extended(
        &mut self,
        function: SdioFunction,
        addr: u32,
        increment_addr: bool,
        write: bool,
        buffer: &mut [u8],
    ) -> Result<(), HalError> {
        self.io_extended_with_mode(
            function,
            addr,
            increment_addr,
            write,
            buffer,
            false,
            true,
            false,
        )
    }

    fn io_extended_quiet(
        &mut self,
        function: SdioFunction,
        addr: u32,
        increment_addr: bool,
        write: bool,
        buffer: &mut [u8],
    ) -> Result<(), HalError> {
        self.io_extended_with_mode(
            function,
            addr,
            increment_addr,
            write,
            buffer,
            false,
            false,
            true,
        )
    }

    fn io_extended_byte_mode(
        &mut self,
        function: SdioFunction,
        addr: u32,
        increment_addr: bool,
        write: bool,
        buffer: &mut [u8],
    ) -> Result<(), HalError> {
        self.io_extended_with_mode(
            function,
            addr,
            increment_addr,
            write,
            buffer,
            true,
            true,
            false,
        )
    }

    fn io_extended_byte_mode_quiet(
        &mut self,
        function: SdioFunction,
        addr: u32,
        increment_addr: bool,
        write: bool,
        buffer: &mut [u8],
    ) -> Result<(), HalError> {
        self.io_extended_with_mode(
            function,
            addr,
            increment_addr,
            write,
            buffer,
            true,
            false,
            true,
        )
    }

    fn io_extended_with_mode(
        &mut self,
        function: SdioFunction,
        addr: u32,
        increment_addr: bool,
        write: bool,
        buffer: &mut [u8],
        byte_mode_only: bool,
        trace_chunks: bool,
        quiet_settle: bool,
    ) -> Result<(), HalError> {
        if buffer.is_empty() {
            return Ok(());
        }

        let chunk_limit = experimental_function2_fifo_chunk_limit(
            function,
            increment_addr,
            self.experimental_no_ht_transport,
        );
        if chunk_limit < SDIO_MAX_BYTE_MODE && buffer.len() > chunk_limit {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] sdio xfer chunk-limit fn={} op={} addr=0x{addr:05x} len={} limit={} reason=experimental-no-ht-f2-fifo",
                function.number(),
                if write { "write" } else { "read" },
                buffer.len(),
                chunk_limit,
            ));
        }

        let mut offset = 0usize;
        while offset < buffer.len() {
            let chunk_len = cmp::min(buffer.len() - offset, chunk_limit);
            let chunk = &mut buffer[offset..offset + chunk_len];
            let plan = if byte_mode_only {
                sdio_byte_mode_transfer_plan(chunk_len, write)?
            } else {
                sdio_transfer_plan(function, chunk_len, write)?
            };
            let chunk_addr = sdio_transfer_addr(addr, offset, increment_addr)?;
            if trace_chunks
                && should_log_sdio_transfer_chunk(function, increment_addr, chunk_len, offset)
            {
                log_sdio_transfer_chunk(
                    function,
                    addr,
                    chunk_addr,
                    offset,
                    chunk_len,
                    increment_addr,
                    write,
                    plan,
                );
            }
            let arg = (u32::from(write) << 31)
                | (u32::from(function.number()) << 28)
                | (u32::from(plan.block_mode) << 27)
                | (u32::from(increment_addr) << 26)
                | ((chunk_addr & 0x1_FFFF) << 9)
                | u32::from(plan.cmd53_count);
            self.transfer_command(SDIO_CMD53, arg, chunk, write, plan, quiet_settle)?;
            offset += chunk_len;
        }
        Ok(())
    }

    fn read_function_enable_state(&mut self, stage: &'static str) -> Result<(u8, u8), HalError> {
        let ioex = self.io_direct_read(SdioFunction::Function0, SDIO_CCCR_IOEX)?;
        let ready = self.io_direct_read(SdioFunction::Function0, SDIO_CCCR_IORX)?;
        emit_breadcrumb(format_args!(
            "[pi4-wifi] sdio enable-functions state stage={stage} ioex=0x{ioex:02x} ready=0x{ready:02x}"
        ));
        Ok((ioex, ready))
    }

    fn log_function_ready_timeout_state(
        &self,
        step: SdioFunctionEnableStep,
        desired: u8,
        ready: u8,
        attempt: usize,
    ) {
        emit_breadcrumb(format_args!(
            "[pi4-wifi] sdio function-ready fn={} timeout-state attempt={} desired=0x{desired:02x} ready=0x{ready:02x} current_clock={}Hz preferred_data_clock={}Hz width={} chipclk=0x{:02x} wake=0x{:02x} sleep=0x{:02x} cardcap=0x{:02x}",
            step.function.number(),
            attempt + 1,
            self.current_clock_hz,
            self.preferred_data_clock_hz,
            sdio_bus_width_name(self.desired_bus_width),
            self.last_chipclkcsr.unwrap_or(0),
            self.last_wakeupctrl.unwrap_or(0),
            self.last_sleepcsr.unwrap_or(0),
            self.last_cardcap.unwrap_or(0),
        ));
    }

    fn enable_function1(&mut self) -> Result<(), HalError> {
        emit_breadcrumb(format_args!("[pi4-wifi] sdio enable-function1 begin"));
        self.read_function_enable_state("before-f1")?;
        self.enable_function_step(SDIO_FUNCTION_ENABLE_F1, SdioFunctionReadyBudget::Strict)?;
        let (ioex, ready) = self.read_function_enable_state("after-f1")?;
        emit_breadcrumb(format_args!(
            "[pi4-wifi] sdio enable-function1 ready ioex=0x{ioex:02x} ready=0x{ready:02x}"
        ));
        Ok(())
    }

    fn enable_function2(
        &mut self,
        function_ready_budget: SdioFunctionReadyBudget,
    ) -> Result<(), HalError> {
        emit_breadcrumb(format_args!("[pi4-wifi] sdio enable-function2 begin"));
        self.read_function_enable_state("before-f2")?;
        self.enable_function_step(SDIO_FUNCTION_ENABLE_F2, function_ready_budget)?;
        let ien = SDIO_CCCR_IEN_FUNC0 | SDIO_CCCR_IEN_FUNC1 | SDIO_CCCR_IEN_FUNC2;
        self.io_direct_write(SdioFunction::Function0, SDIO_CCCR_IENX, ien)?;
        let (ioex, ready) = self.read_function_enable_state("after-f2")?;
        emit_breadcrumb(format_args!(
            "[pi4-wifi] sdio enable-function2 ready ioex=0x{ioex:02x} ready=0x{ready:02x} ien=0x{ien:02x}"
        ));
        Ok(())
    }

    fn enable_function_step(
        &mut self,
        step: SdioFunctionEnableStep,
        function_ready_budget: SdioFunctionReadyBudget,
    ) -> Result<(), HalError> {
        let function_number = step.function.number();
        let ioex_before = self.io_direct_read(SdioFunction::Function0, SDIO_CCCR_IOEX)?;
        let ready_before = self.io_direct_read(SdioFunction::Function0, SDIO_CCCR_IORX)?;
        let desired = ioex_before | step.enable_bit;
        emit_breadcrumb(format_args!(
            "[pi4-wifi] sdio function-enable fn={} ioex=0x{ioex_before:02x} ready=0x{ready_before:02x} desired=0x{desired:02x}",
            function_number
        ));
        if desired != ioex_before {
            self.io_direct_write(SdioFunction::Function0, SDIO_CCCR_IOEX, desired)?;
        }
        let ioex_after = self.io_direct_read(SdioFunction::Function0, SDIO_CCCR_IOEX)?;
        emit_breadcrumb(format_args!(
            "[pi4-wifi] sdio function-enable fn={} ioex-after=0x{ioex_after:02x}",
            function_number
        ));
        if (ioex_after & step.enable_bit) != step.enable_bit {
            return Err(HalError::Unsupported("sdio-function-enable-latch"));
        }

        let ready_stage = sdio_function_ready_transport_stage(step);
        let mut last_ready = ready_before;
        let use_short_probe_only_budget =
            sdio_function_ready_uses_short_probe_only_budget(step, function_ready_budget);
        // Function 2 uses one of three readiness policies: strict Linux-style
        // dwell/retry, a short speculative probe that fails fast, or the
        // existing no-HT experimental bypass that continues without IORX.
        for attempt in 0..=sdio_function_ready_retry_limit_for(step, function_ready_budget) {
            if let Some(stage) = ready_stage {
                if attempt == 0 {
                    emit_breadcrumb(format_args!(
                        "[pi4-wifi] sdio function-ready fn={} action=phase-ht-assist stage=pre-poll",
                        function_number
                    ));
                } else {
                    emit_breadcrumb(format_args!(
                        "[pi4-wifi] sdio function-ready fn={} action=recover-retry attempt={}",
                        function_number, attempt
                    ));
                    self.recover_command_path_and_refresh_transport(stage)?;
                    emit_breadcrumb(format_args!(
                        "[pi4-wifi] sdio function-enable fn={} action=reassert-enable desired=0x{desired:02x}",
                        function_number
                    ));
                    self.io_direct_write(SdioFunction::Function0, SDIO_CCCR_IOEX, desired)?;
                }
                self.refresh_transport_phase_for(stage)?;
            }

            let mut logged_ready = u8::MAX;
            let mut ready_polls = sdio_function_ready_polls(step);
            let mut settle_loops = sdio_function_ready_settle_loops(step);
            let mut used_extended_budget = false;
            loop {
                for poll in 0..ready_polls {
                    let ready = self.io_direct_read(SdioFunction::Function0, SDIO_CCCR_IORX)?;
                    last_ready = ready;
                    if poll == 0 || poll + 1 == ready_polls || ready != logged_ready {
                        emit_breadcrumb(format_args!(
                            "[pi4-wifi] sdio function-ready fn={} poll={}/{} ready=0x{ready:02x} need=0x{need:02x}",
                            function_number,
                            poll + 1,
                            ready_polls,
                            need = step.ready_bit
                        ));
                        logged_ready = ready;
                    }
                    if (ready & step.ready_bit) == step.ready_bit {
                        self.set_function_block_size(step.function, step.block_size)?;
                        emit_breadcrumb(format_args!(
                            "[pi4-wifi] sdio function-ready fn={} block={} ready=0x{ready:02x}",
                            function_number, step.block_size
                        ));
                        return Ok(());
                    }
                    if poll + 1 != ready_polls {
                        for _ in 0..settle_loops {
                            spin_loop();
                        }
                        wifi_progress_advance_loops(settle_loops);
                    }
                }
                self.log_function_ready_timeout_state(step, desired, last_ready, attempt);
                if !used_extended_budget {
                    if let (Some(extended_polls), Some(extended_settle_loops)) = (
                        sdio_function_ready_extended_polls_for(step, function_ready_budget),
                        sdio_function_ready_extended_settle_loops_for(step, function_ready_budget),
                    ) {
                        emit_breadcrumb(format_args!(
                            "[pi4-wifi] sdio function-ready fn={} action=linux-f2-extended-wait polls={} settle_loops={}",
                            function_number, extended_polls, extended_settle_loops
                        ));
                        ready_polls = extended_polls;
                        settle_loops = extended_settle_loops;
                        used_extended_budget = true;
                        continue;
                    }
                    if use_short_probe_only_budget {
                        emit_breadcrumb(format_args!(
                            "[pi4-wifi] sdio function-ready fn={} action=skip-linux-f2-extended-wait reason={}",
                            function_number,
                            sdio_function_ready_budget_name(function_ready_budget),
                        ));
                    }
                }
                break;
            }
        }
        if sdio_function_ready_timeout_can_continue_experimentally(
            step,
            desired,
            last_ready,
            function_ready_budget,
        ) {
            if let Some(stage) = ready_stage {
                self.refresh_transport_phase_for(stage)?;
            }
            emit_breadcrumb(format_args!(
                "[pi4-wifi] sdio function-ready fn={} action=experimental-continue-without-ready desired=0x{desired:02x} ready=0x{last_ready:02x}",
                function_number
            ));
            self.set_function_block_size(step.function, step.block_size)?;
            emit_breadcrumb(format_args!(
                "[pi4-wifi] sdio function-ready fn={} block={} ready=0x{last_ready:02x} assumed=yes",
                function_number, step.block_size
            ));
            return Ok(());
        }
        Err(HalError::Unsupported(step.timeout_error))
    }

    fn bring_up_backplane(&mut self) -> Result<(), HalError> {
        emit_breadcrumb(format_args!("[pi4-wifi] cyw43 backplane begin"));
        emit_breadcrumb(format_args!("[pi4-wifi] cyw43 backplane stage=alp-request"));
        self.io_direct_write(
            SdioFunction::Function1,
            SBSDIO_FUNC1_CHIPCLKCSR,
            SBSDIO_ALP_AVAIL_REQ,
        )?;
        self.remember_chipclkcsr(SBSDIO_ALP_AVAIL_REQ);
        let mut alp_ready = false;
        for _ in 0..SDIO_INIT_WAIT_LOOPS {
            let chipclk = self.io_direct_read(SdioFunction::Function1, SBSDIO_FUNC1_CHIPCLKCSR)?;
            self.remember_chipclkcsr(chipclk);
            if (chipclk & SBSDIO_ALP_AVAIL) != 0 {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] cyw43 backplane stage=alp-ready csr=0x{chipclk:02x}"
                ));
                alp_ready = true;
                break;
            }
            spin_loop();
        }
        if !alp_ready {
            return Err(HalError::Unsupported("cyw43-alp-clock-timeout"));
        }
        emit_breadcrumb(format_args!("[pi4-wifi] cyw43 backplane stage=alp-clear"));
        self.io_direct_write(SdioFunction::Function1, SBSDIO_FUNC1_CHIPCLKCSR, 0)?;
        self.remember_chipclkcsr(0);
        emit_breadcrumb(format_args!(
            "[pi4-wifi] cyw43 backplane stage=misc-config deferred reason=minimal-sdio-bringup"
        ));
        emit_breadcrumb(format_args!("[pi4-wifi] cyw43 backplane ready"));
        Ok(())
    }

    fn setup_firmware_channel(
        &mut self,
        allow_function2_ready_bypass: bool,
    ) -> Result<(), HalError> {
        emit_breadcrumb(format_args!("[pi4-wifi] firmware channel begin"));
        self.refresh_transport_phase_for("setup-firmware-channel")?;
        let mut attempt = 0usize;
        loop {
            match self.setup_firmware_channel_once(allow_function2_ready_bypass) {
                Ok(()) => {
                    emit_breadcrumb(format_args!("[pi4-wifi] firmware channel ready"));
                    self.log_control_plane_finish_snapshot("setup-firmware-channel-ready");
                    return Ok(());
                }
                Err(err) if firmware_phase_can_retry(&err, attempt) => {
                    emit_breadcrumb(format_args!(
                        "[pi4-wifi] firmware stage=setup-firmware-channel attempt={} err={err} action=recover-retry",
                        attempt + 1
                    ));
                    self.recover_command_path_and_refresh_transport("setup-firmware-channel")?;
                    attempt += 1;
                }
                Err(err) => return Err(err),
            }
        }
    }

    fn setup_firmware_channel_once(
        &mut self,
        allow_function2_ready_bypass: bool,
    ) -> Result<(), HalError> {
        let restore_clock_hz = firmware_channel_write_restore_clock_hz(
            self.experimental_no_ht_transport,
            self.current_clock_hz,
        );
        if let Some(clock_hz) = restore_clock_hz {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware stage=firmware-channel-write-clock action=lower context=setup current={}Hz target={}Hz width={} no_ht={}",
                self.current_clock_hz,
                CYW43_STARTUP_CLOCK_HZ,
                sdio_bus_width_name(self.desired_bus_width),
                self.experimental_no_ht_transport,
            ));
            self.set_clock_hz(CYW43_STARTUP_CLOCK_HZ)?;
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware stage=firmware-channel-write-clock action=lower-ready context=setup previous={}Hz current={}Hz width={} no_ht={}",
                clock_hz,
                self.current_clock_hz,
                sdio_bus_width_name(self.desired_bus_width),
                self.experimental_no_ht_transport,
            ));
        }
        let write_result = (|| {
            let experimental_order =
                setup_firmware_channel_uses_experimental_order(allow_function2_ready_bypass);
            if experimental_order {
                emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware stage=setup-firmware-channel action=experimental-function2-first"
            ));
                self.enable_function2(sdio_function_ready_budget_for_bypass(
                    allow_function2_ready_bypass,
                ))?;
            }
            self.write_sdio_core_u32_for_firmware_channel(
                "setup-firmware-channel-mailbox-version",
                SDPCMD_REG_TOSBMAILBOXDATA,
                SDPCM_PROT_VERSION << HMB_DATA_VERSION_SHIFT,
                allow_function2_ready_bypass,
            )?;
            if !experimental_order {
                self.enable_function2(sdio_function_ready_budget_for_bypass(
                    allow_function2_ready_bypass,
                ))?;
            }
            self.write_sdio_core_u32_for_firmware_channel(
                "setup-firmware-channel-hostintmask",
                SDPCMD_REG_HOSTINTMASK,
                HOSTINTMASK,
                allow_function2_ready_bypass,
            )?;
            self.io_direct_write(
                SdioFunction::Function1,
                SBSDIO_WATERMARK,
                CY_43455_F2_WATERMARK,
            )?;
            let devctl = self.io_direct_read(SdioFunction::Function1, SBSDIO_DEVICE_CTL)?;
            self.io_direct_write(
                SdioFunction::Function1,
                SBSDIO_DEVICE_CTL,
                devctl | SBSDIO_DEVCTL_F2WM_ENAB,
            )?;
            self.io_direct_write(
                SdioFunction::Function1,
                SBSDIO_FUNC1_MESBUSYCTRL,
                CY_43455_MESBUSYCTRL,
            )?;
            Ok(())
        })();
        if let Some(clock_hz) = restore_clock_hz {
            match self.set_clock_hz(clock_hz) {
                Ok(restored_clock_hz) => {
                    emit_breadcrumb(format_args!(
                        "[pi4-wifi] firmware stage=firmware-channel-write-clock action=restore context=setup restored={}Hz width={} no_ht={}",
                        restored_clock_hz,
                        sdio_bus_width_name(self.desired_bus_width),
                        self.experimental_no_ht_transport,
                    ));
                }
                Err(err) => {
                    emit_breadcrumb(format_args!(
                        "[pi4-wifi] firmware stage=firmware-channel-write-clock action=restore-fail context=setup target={}Hz current={}Hz width={} no_ht={} err={err}",
                        clock_hz,
                        self.current_clock_hz,
                        sdio_bus_width_name(self.desired_bus_width),
                        self.experimental_no_ht_transport,
                    ));
                    return Err(err);
                }
            }
        }
        write_result
    }

    fn wait_for_firmware_ready(
        &mut self,
        allow_function2_ready_bypass: bool,
    ) -> Result<(), HalError> {
        let restore_clock_hz = wait_for_firmware_ready_restore_clock_hz(
            allow_function2_ready_bypass,
            self.current_clock_hz,
        );
        if let Some(clock_hz) = restore_clock_hz {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware stage=firmware-ready-clock action=lower current={}Hz target={}Hz width={} no_ht={}",
                self.current_clock_hz,
                CYW43_STARTUP_CLOCK_HZ,
                sdio_bus_width_name(self.desired_bus_width),
                self.experimental_no_ht_transport,
            ));
            self.set_clock_hz(CYW43_STARTUP_CLOCK_HZ)?;
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware stage=firmware-ready-clock action=lower-ready previous={}Hz current={}Hz width={} no_ht={}",
                clock_hz,
                self.current_clock_hz,
                sdio_bus_width_name(self.desired_bus_width),
                self.experimental_no_ht_transport,
            ));
        }
        let ready_result = (|| {
            self.refresh_transport_phase_for("wait-firmware-ready")?;
            let mut recovery_attempt = 0usize;
            for _ in 0..CYW43_READY_LOOPS {
                let value = match self.read_sdio_core_u32_for_firmware_ready(
                    "wait-firmware-ready",
                    SDPCMD_REG_TOHOSTMAILBOXDATA,
                    allow_function2_ready_bypass,
                    recovery_attempt,
                ) {
                    Ok(value) => value,
                    Err(err) if firmware_phase_can_retry(&err, recovery_attempt) => {
                        emit_breadcrumb(format_args!(
                            "[pi4-wifi] firmware stage=wait-firmware-ready attempt={} err={err} action=recover-retry",
                            recovery_attempt + 1
                        ));
                        self.recover_command_path_and_refresh_transport("wait-firmware-ready")?;
                        recovery_attempt += 1;
                        continue;
                    }
                    Err(err) => return Err(err),
                };
                if value & (HMB_DATA_DEVREADY | HMB_DATA_FWREADY) != 0 {
                    let version = (value & HMB_DATA_VERSION_MASK) >> HMB_DATA_VERSION_SHIFT;
                    if version != 0 && version != SDPCM_PROT_VERSION {
                        return Err(HalError::Unsupported("cyw43-protocol-version"));
                    }
                    emit_breadcrumb(format_args!(
                        "[pi4-wifi] firmware ready mailbox=0x{value:08x} version={version}"
                    ));
                    return Ok(());
                }
                spin_loop();
            }
            Err(HalError::Unsupported("cyw43-firmware-ready-timeout"))
        })();
        if let Some(clock_hz) = restore_clock_hz {
            match self.set_clock_hz(clock_hz) {
                Ok(restored_clock_hz) => {
                    emit_breadcrumb(format_args!(
                        "[pi4-wifi] firmware stage=firmware-ready-clock action=restore restored={}Hz width={} no_ht={}",
                        restored_clock_hz,
                        sdio_bus_width_name(self.desired_bus_width),
                        self.experimental_no_ht_transport,
                    ));
                }
                Err(err) => {
                    emit_breadcrumb(format_args!(
                        "[pi4-wifi] firmware stage=firmware-ready-clock action=restore-fail target={}Hz current={}Hz width={} no_ht={} err={err}",
                        clock_hz,
                        self.current_clock_hz,
                        sdio_bus_width_name(self.desired_bus_width),
                        self.experimental_no_ht_transport,
                    ));
                    return Err(err);
                }
            }
        }
        ready_result
    }

    fn backplane_read8(&mut self, addr: u32) -> Result<u8, HalError> {
        self.with_backplane_small_window(addr, |this, function_addr| {
            this.io_direct_read(SdioFunction::Function1, function_addr)
        })
    }

    fn backplane_write8(&mut self, addr: u32, value: u8) -> Result<(), HalError> {
        self.with_backplane_small_window(addr, |this, function_addr| {
            this.io_direct_write(SdioFunction::Function1, function_addr, value)
        })
    }

    fn backplane_read16(&mut self, addr: u32) -> Result<u16, HalError> {
        let mut bytes = [0u8; 2];
        self.backplane_read_small(backplane_small_access_addr(addr), &mut bytes)?;
        Ok(u16::from_le_bytes(bytes))
    }

    fn backplane_write16(&mut self, addr: u32, value: u16) -> Result<(), HalError> {
        self.backplane_write_small(backplane_small_access_addr(addr), &value.to_le_bytes())
    }

    fn backplane_read32(&mut self, addr: u32) -> Result<u32, HalError> {
        let mut bytes = [0u8; 4];
        self.backplane_read_small(backplane_small_access_addr(addr), &mut bytes)?;
        Ok(u32::from_le_bytes(bytes))
    }

    fn backplane_write32(&mut self, addr: u32, value: u32) -> Result<(), HalError> {
        self.backplane_write_small(backplane_small_access_addr(addr), &value.to_le_bytes())
    }

    fn backplane_word_read32(&mut self, addr: u32) -> Result<u32, HalError> {
        let mut bytes = [0u8; 4];
        self.with_backplane_window_addr(
            addr,
            backplane_word_function_addr(backplane_word_aligned_addr(addr)),
            |this, bus_addr| {
                this.io_extended(
                    SdioFunction::Function1,
                    bus_addr,
                    backplane_word_increment_addr(),
                    false,
                    &mut bytes,
                )?;
                Ok(())
            },
        )?;
        Ok(u32::from_le_bytes(bytes))
    }

    fn backplane_word_write32(&mut self, addr: u32, value: u32) -> Result<(), HalError> {
        let mut bytes = value.to_le_bytes();
        self.with_backplane_window_addr(
            addr,
            backplane_word_function_addr(backplane_word_aligned_addr(addr)),
            |this, bus_addr| {
                this.io_extended(
                    SdioFunction::Function1,
                    bus_addr,
                    backplane_word_increment_addr(),
                    true,
                    &mut bytes,
                )
            },
        )
    }

    fn backplane_read(&mut self, addr: u32, len: usize) -> Result<[u8; 4], HalError> {
        let mut result = [0u8; 4];
        let read_len = len.min(result.len());
        self.with_backplane_transfer_window(addr, |this, function_addr| {
            this.io_extended(
                SdioFunction::Function1,
                function_addr,
                true,
                false,
                &mut result[..read_len],
            )
        })?;
        Ok(result)
    }

    fn backplane_read_into(&mut self, addr: u32, out: &mut [u8]) -> Result<(), HalError> {
        let mut offset = 0usize;
        while offset < out.len() {
            let window_offset = (addr as usize + offset) & BACKPLANE_ADDRESS_MASK as usize;
            let window_remaining =
                (BACKPLANE_ADDRESS_MASK as usize + 1).saturating_sub(window_offset);
            let chunk_len = cmp::min(
                out.len() - offset,
                cmp::min(CYW43_TRANSFER_CHUNK, window_remaining),
            );
            let chunk_addr = addr
                .checked_add(
                    u32::try_from(offset)
                        .map_err(|_| HalError::Unsupported("backplane-read-overflow"))?,
                )
                .ok_or(HalError::Unsupported("backplane-read-overflow"))?;
            self.with_backplane_transfer_window(chunk_addr, |this, function_addr| {
                this.io_extended(
                    SdioFunction::Function1,
                    function_addr,
                    true,
                    false,
                    &mut out[offset..offset + chunk_len],
                )
            })?;
            offset += chunk_len;
        }
        Ok(())
    }

    fn backplane_write(&mut self, addr: u32, data: &[u8]) -> Result<(), HalError> {
        let mut offset = 0usize;
        while offset < data.len() {
            let window_offset = (addr as usize + offset) & BACKPLANE_ADDRESS_MASK as usize;
            let window_remaining =
                (BACKPLANE_ADDRESS_MASK as usize + 1).saturating_sub(window_offset);
            let chunk_len = cmp::min(
                data.len() - offset,
                cmp::min(CYW43_TRANSFER_CHUNK, window_remaining),
            );
            let chunk_addr = addr
                .checked_add(
                    u32::try_from(offset)
                        .map_err(|_| HalError::Unsupported("backplane-write-overflow"))?,
                )
                .ok_or(HalError::Unsupported("backplane-write-overflow"))?;
            let mut staging = [0u8; CYW43_TRANSFER_CHUNK];
            staging[..chunk_len].copy_from_slice(&data[offset..offset + chunk_len]);
            self.with_backplane_transfer_window(chunk_addr, |this, function_addr| {
                this.io_extended(
                    SdioFunction::Function1,
                    function_addr,
                    true,
                    true,
                    &mut staging[..chunk_len],
                )
            })?;
            offset += chunk_len;
        }
        Ok(())
    }

    fn backplane_write_firmware_retrying_window(
        &mut self,
        retry_stage: &'static str,
        window_stage: &'static str,
        window_assumed_stage: &'static str,
        prefer_byte_mode: bool,
        addr: u32,
        data: &[u8],
    ) -> Result<(), HalError> {
        let mut byte_mode_fallback = prefer_byte_mode;
        let mut retry_count = 0usize;
        let mut first_retry_offset = None;
        let mut offset = 0usize;
        while offset < data.len() {
            let chunk_addr = addr
                .checked_add(
                    u32::try_from(offset)
                        .map_err(|_| HalError::Unsupported("backplane-write-overflow"))?,
                )
                .ok_or(HalError::Unsupported("backplane-write-overflow"))?;
            let mut attempt = 0usize;
            loop {
                let window_offset = (addr as usize + offset) & BACKPLANE_ADDRESS_MASK as usize;
                let window_remaining =
                    (BACKPLANE_ADDRESS_MASK as usize + 1).saturating_sub(window_offset);
                let chunk_len = cmp::min(
                    data.len() - offset,
                    cmp::min(CYW43_FIRMWARE_TRANSFER_CHUNK, window_remaining),
                );
                if attempt > 0 {
                    self.prepare_firmware_upload_window(
                        window_stage,
                        window_assumed_stage,
                        chunk_addr,
                    )?;
                }
                if should_log_firmware_upload_progress(offset, chunk_len, data.len()) {
                    emit_breadcrumb(format_args!(
                        "[pi4-wifi] firmware stage={window_stage} progress={}/{} addr=0x{chunk_addr:08x}",
                        offset + chunk_len,
                        data.len(),
                    ));
                    wifi_progress_tick();
                }
                let mut staging = [0u8; CYW43_FIRMWARE_TRANSFER_CHUNK];
                staging[..chunk_len].copy_from_slice(&data[offset..offset + chunk_len]);
                match self.with_backplane_transfer_window_quiet(
                    chunk_addr,
                    |this, function_addr| {
                        if firmware_transfer_uses_byte_mode(byte_mode_fallback) {
                            this.io_extended_byte_mode_quiet(
                                SdioFunction::Function1,
                                function_addr,
                                true,
                                true,
                                &mut staging[..chunk_len],
                            )
                        } else {
                            this.io_extended_quiet(
                                SdioFunction::Function1,
                                function_addr,
                                true,
                                true,
                                &mut staging[..chunk_len],
                            )
                        }
                    },
                ) {
                    Ok(()) => {
                        offset += chunk_len;
                        break;
                    }
                    Err(err) if firmware_backplane_write_can_retry(&err, attempt) => {
                        retry_count = retry_count.saturating_add(1);
                        first_retry_offset.get_or_insert(offset);
                        if !byte_mode_fallback {
                            byte_mode_fallback = true;
                            emit_breadcrumb(format_args!(
                                "[pi4-wifi] firmware stage={retry_stage} addr=0x{chunk_addr:08x} off={offset} len={chunk_len} err={err} action=switch-byte-mode next_len={chunk_len}"
                            ));
                        } else {
                            emit_breadcrumb(format_args!(
                                "[pi4-wifi] firmware stage={retry_stage} addr=0x{chunk_addr:08x} off={offset} len={chunk_len} err={err} action=recover-retry mode=byte"
                            ));
                        }
                        self.recover_command_path_and_refresh_transport(retry_stage)?;
                        attempt += 1;
                    }
                    Err(err) => return Err(err),
                }
            }
        }
        if retry_count != 0 {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware stage={retry_stage} summary retries={retry_count} first_retry_off={} mode=byte-fallback",
                first_retry_offset.unwrap_or(0)
            ));
        }
        Ok(())
    }

    fn backplane_read_small(&mut self, addr: u32, out: &mut [u8]) -> Result<(), HalError> {
        self.with_backplane_small_window(addr, |this, function_addr| {
            for (index, slot) in out.iter_mut().enumerate() {
                let byte_addr = function_addr
                    .checked_add(index as u32)
                    .ok_or(HalError::Unsupported("backplane-read-overflow"))?;
                *slot = this.io_direct_read(SdioFunction::Function1, byte_addr)?;
            }
            Ok(())
        })
    }

    fn backplane_write_small(&mut self, addr: u32, data: &[u8]) -> Result<(), HalError> {
        self.with_backplane_small_window(addr, |this, function_addr| {
            for (index, value) in data.iter().copied().enumerate() {
                let byte_addr = function_addr
                    .checked_add(index as u32)
                    .ok_or(HalError::Unsupported("backplane-write-overflow"))?;
                this.io_direct_write(SdioFunction::Function1, byte_addr, value)?;
            }
            Ok(())
        })
    }

    fn configure_chipcommon(&mut self) -> Result<(), HalError> {
        self.refresh_transport_phase_for("chipcommon-config")?;
        // Upstream CYW43 writes these remap registers in SOCSRAM immediately
        // after SOCSRAM reset, before firmware upload begins.
        let writes = [
            (CYW43_SOCRAM_CORE_BASE + 0x10, 3u32),
            (CYW43_SOCRAM_CORE_BASE + 0x44, 0u32),
        ];
        let prior_programmed_window = self.programmed_backplane_window;
        let prior_shadow_window = self.last_backplane_window;
        if chipcommon_config_can_use_mid_only_window_switch(
            prior_programmed_window,
            prior_shadow_window,
            writes[0].0,
        ) {
            let current_window = chipcommon_config_source_window(
                prior_programmed_window,
                prior_shadow_window,
                writes[0].0,
            )
            .unwrap_or(0);
            match self
                .chipcommon_config_enter_window_direct_phase(current_window, writes[0].0)
                .and_then(|()| {
                    for (addr, value) in writes {
                        self.chipcommon_config_write32_direct_phase(addr, value)?;
                    }
                    Ok(())
                }) {
                Ok(()) => return Ok(()),
                Err(err) => {
                    emit_breadcrumb(format_args!(
                        "[pi4-wifi] firmware stage=chipcommon-config-direct-fallback addr=0x{addr:08x} current_window=0x{current_window:08x} err={err}",
                        addr = writes[0].0,
                    ));
                    self.recover_command_path_and_refresh_transport(
                        "chipcommon-config-direct-fallback",
                    )?;
                }
            }
        }
        for (addr, value) in writes {
            if chipcommon_config_is_phase_addr(addr) {
                if self.programmed_backplane_window != Some(backplane_window_base(addr)) {
                    let current_window = chipcommon_config_source_window(
                        prior_programmed_window,
                        prior_shadow_window,
                        addr,
                    )
                    .unwrap_or(0);
                    self.recover_command_path_and_refresh_transport("chipcommon-config-retry")?;
                    self.chipcommon_config_enter_window_direct_phase(current_window, addr)?;
                }
                self.chipcommon_config_write32_direct_phase(addr, value)?;
            } else if chipcommon_config_can_use_mid_only_window_switch(
                prior_programmed_window,
                prior_shadow_window,
                addr,
            ) {
                let current_window = chipcommon_config_source_window(
                    prior_programmed_window,
                    prior_shadow_window,
                    addr,
                )
                .unwrap_or(0);
                self.recover_command_path_and_refresh_transport("chipcommon-config-retry")?;
                self.chipcommon_config_enter_window_direct_phase(current_window, addr)?;
                self.chipcommon_config_write32_direct_phase(addr, value)?;
            } else {
                self.backplane_write32(addr, value)?;
            }
        }
        Ok(())
    }

    fn chipcommon_config_enter_window_direct_phase(
        &mut self,
        current_window: u32,
        addr: u32,
    ) -> Result<(), HalError> {
        let target_window = backplane_window_base(addr);
        let function_addr = addr & BACKPLANE_ADDRESS_MASK;
        let (target_low, target_mid, target_high) = backplane_window_register_bytes(addr);
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware stage=chipcommon-config-retry addr=0x{addr:08x} current_window=0x{current_window:08x} target_window=0x{target_window:08x} reason=mid-byte-only-window-switch-direct"
        ));
        emit_breadcrumb(format_args!(
            "[pi4-wifi] backplane window retarget current=0x{current_window:08x} target=0x{target_window:08x} low=0x{target_low:02x} mid=0x{target_mid:02x} high=0x{target_high:02x} path=cmd52-mid-only"
        ));
        if let Err(err) =
            self.io_direct_write(SdioFunction::Function1, SBSDIO_FUNC1_SBADDRMID, target_mid)
        {
            if !chipcommon_config_can_assume_window_commit(&err) {
                return Err(err);
            }
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware stage=chipcommon-config-window-assumed-committed addr=0x{addr:08x} current_window=0x{current_window:08x} target_window=0x{target_window:08x} err={err} reason=chipcommon-mid-window-timeout"
            ));
            self.remember_backplane_window(
                addr,
                function_addr,
                target_low,
                target_mid,
                target_high,
            );
            self.programmed_backplane_window = Some(target_window);
            self.recover_command_path_preserve_window_and_refresh_transport(
                "chipcommon-config-window-assumed-committed",
            )?;
            return Ok(());
        }
        self.programmed_backplane_window = Some(target_window);
        self.remember_backplane_window(addr, function_addr, target_low, target_mid, target_high);
        Ok(())
    }

    fn chipcommon_config_write32_direct_phase(
        &mut self,
        addr: u32,
        value: u32,
    ) -> Result<(), HalError> {
        let bytes = value.to_le_bytes();
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware stage=chipcommon-config-write addr=0x{addr:08x} value=0x{value:08x} path=cmd53-byte-windowed"
        ));
        match self.backplane_write(addr, &bytes) {
            Ok(()) => Ok(()),
            Err(err) => {
                if !chipcommon_config_can_assume_write_commit(addr, &err) {
                    return Err(err);
                }
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware stage=chipcommon-config-write-retry addr=0x{addr:08x} value=0x{value:08x} err={err} reason=current-window-payload-timeout path=cmd53-byte-windowed"
                ));
                self.restore_window_cache_from_shadow_and_refresh_transport(
                    "chipcommon-config-write-retry",
                )?;
                let retry_bytes = value.to_le_bytes();
                match self.backplane_write(addr, &retry_bytes) {
                    Ok(()) => Ok(()),
                    Err(retry_err) => {
                        if !chipcommon_config_can_assume_write_commit(addr, &retry_err) {
                            return Err(retry_err);
                        }
                        emit_breadcrumb(format_args!(
                            "[pi4-wifi] firmware stage=chipcommon-config-write-fixed addr=0x{addr:08x} value=0x{value:08x} err={retry_err} reason=current-window-payload-timeout path=cmd53-fixed-window"
                        ));
                        self.recover_command_path_preserve_window_and_refresh_transport(
                            "chipcommon-config-write-fixed",
                        )?;
                        let mut fixed_bytes = value.to_le_bytes();
                        let fixed_addr = addr & BACKPLANE_ADDRESS_MASK;
                        match self.io_extended(
                            SdioFunction::Function1,
                            fixed_addr,
                            false,
                            true,
                            &mut fixed_bytes,
                        ) {
                            Ok(()) => Ok(()),
                            Err(fixed_err) => {
                                if !chipcommon_config_can_assume_write_commit(addr, &fixed_err) {
                                    return Err(fixed_err);
                                }
                                emit_breadcrumb(format_args!(
                                    "[pi4-wifi] firmware stage=chipcommon-config-write-assumed-committed addr=0x{addr:08x} value=0x{value:08x} err={fixed_err} reason=chipcommon-phase-write-timeout path=cmd53-fixed-window"
                                ));
                                self.recover_command_path_preserve_window_and_refresh_transport(
                                    "chipcommon-config-write-assumed-committed",
                                )?;
                                Ok(())
                            }
                        }
                    }
                }
            }
        }
    }

    fn with_backplane_window_addr<T>(
        &mut self,
        window_addr: u32,
        function_addr: u32,
        f: impl FnOnce(&mut Self, u32) -> Result<T, HalError>,
    ) -> Result<T, HalError> {
        let trace_window = (function_addr & BACKPLANE_32BIT_FLAG) != 0;
        self.with_backplane_window_addr_trace(window_addr, function_addr, trace_window, f)
    }

    fn with_backplane_window_addr_quiet<T>(
        &mut self,
        window_addr: u32,
        function_addr: u32,
        f: impl FnOnce(&mut Self, u32) -> Result<T, HalError>,
    ) -> Result<T, HalError> {
        self.with_backplane_window_addr_trace(window_addr, function_addr, false, f)
    }

    fn with_backplane_window_addr_trace<T>(
        &mut self,
        window_addr: u32,
        function_addr: u32,
        trace_window: bool,
        f: impl FnOnce(&mut Self, u32) -> Result<T, HalError>,
    ) -> Result<T, HalError> {
        let window_base = backplane_window_base(window_addr);
        let (window_low, window_mid, window_high) = backplane_window_register_bytes(window_addr);
        if trace_window {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] backplane window program window=0x{window_addr:08x} fn_addr=0x{function_addr:05x} low=0x{window_low:02x} mid=0x{window_mid:02x} high=0x{window_high:02x}"
            ));
        }
        self.remember_backplane_window(
            window_addr,
            function_addr,
            window_low,
            window_mid,
            window_high,
        );
        if backplane_window_reprogram_needed(self.programmed_backplane_window, window_addr) {
            for (_, register, value) in
                backplane_window_program_sequence(window_low, window_mid, window_high)
            {
                self.io_direct_write(SdioFunction::Function1, register, value)?;
            }
            self.programmed_backplane_window = Some(window_base);
        } else if trace_window {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] backplane window reuse window=0x{window_base:08x} fn_addr=0x{function_addr:05x}"
            ));
        }
        if trace_window {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] backplane window ready window=0x{window_addr:08x} fn_addr=0x{function_addr:05x}"
            ));
        }
        f(self, function_addr)
    }

    fn with_backplane_small_window<T>(
        &mut self,
        addr: u32,
        f: impl FnOnce(&mut Self, u32) -> Result<T, HalError>,
    ) -> Result<T, HalError> {
        self.with_backplane_window_addr(addr, backplane_byte_function_addr(addr), f)
    }

    fn with_backplane_transfer_window<T>(
        &mut self,
        addr: u32,
        f: impl FnOnce(&mut Self, u32) -> Result<T, HalError>,
    ) -> Result<T, HalError> {
        self.with_backplane_window_addr(addr, backplane_transfer_function_addr(addr), f)
    }

    fn with_backplane_transfer_window_quiet<T>(
        &mut self,
        addr: u32,
        f: impl FnOnce(&mut Self, u32) -> Result<T, HalError>,
    ) -> Result<T, HalError> {
        self.with_backplane_window_addr_quiet(addr, backplane_transfer_function_addr(addr), f)
    }

    fn prepare_firmware_upload_window(
        &mut self,
        stage: &'static str,
        assumed_stage: &'static str,
        addr: u32,
    ) -> Result<(), HalError> {
        self.refresh_transport_phase_for(stage)?;
        let window_base = backplane_window_base(addr);
        let function_addr = backplane_transfer_function_addr(addr);
        let (window_low, window_mid, window_high) = backplane_window_register_bytes(addr);
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware stage={stage} addr=0x{addr:08x} target_window=0x{window_base:08x} low=0x{window_low:02x} mid=0x{window_mid:02x} high=0x{window_high:02x}"
        ));
        self.remember_backplane_window(addr, function_addr, window_low, window_mid, window_high);
        if !backplane_window_reprogram_needed(self.programmed_backplane_window, addr) {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware stage={stage} target_window=0x{window_base:08x} action=reuse"
            ));
            return Ok(());
        }
        for (register_name, register, value) in
            backplane_window_program_sequence(window_low, window_mid, window_high)
        {
            let mut attempt = 0usize;
            loop {
                match self.io_direct_write(SdioFunction::Function1, register, value) {
                    Ok(()) => break,
                    Err(err) if firmware_window_write_can_retry(&err, attempt) => {
                        emit_breadcrumb(format_args!(
                            "[pi4-wifi] firmware stage={stage} addr=0x{addr:08x} reg={register_name} value=0x{value:02x} err={err} action=recover-retry attempt={}",
                            attempt + 1
                        ));
                        self.recover_command_path_and_refresh_transport(stage)?;
                        attempt += 1;
                    }
                    Err(err) => {
                        if !chipcommon_config_can_assume_window_commit(&err) {
                            return Err(err);
                        }
                        emit_breadcrumb(format_args!(
                            "[pi4-wifi] firmware stage={assumed_stage} addr=0x{addr:08x} reg={register_name} value=0x{value:02x} err={err} reason=firmware-window-timeout action=assume-committed attempt={}",
                            attempt + 1
                        ));
                        self.programmed_backplane_window = Some(window_base);
                        self.recover_command_path_preserve_window_and_refresh_transport(
                            assumed_stage,
                        )?;
                        break;
                    }
                }
            }
        }
        self.programmed_backplane_window = Some(window_base);
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware stage={stage} target_window=0x{window_base:08x} action=ready"
        ));
        Ok(())
    }

    fn read_f1_u32(&mut self, addr: u32) -> Result<u32, HalError> {
        let mut bytes = [0u8; 4];
        for (index, slot) in bytes.iter_mut().enumerate() {
            let byte_addr = addr
                .checked_add(index as u32)
                .ok_or(HalError::Unsupported("f1-read32-overflow"))?;
            *slot = self.io_direct_read(SdioFunction::Function1, byte_addr)?;
        }
        Ok(u32::from_le_bytes(bytes))
    }

    fn read_f1_u32_incrementing_cmd53(&mut self, addr: u32) -> Result<u32, HalError> {
        let mut bytes = [0u8; 4];
        self.io_extended_byte_mode(SdioFunction::Function1, addr, true, false, &mut bytes)?;
        Ok(u32::from_le_bytes(bytes))
    }

    fn transfer_sdio_core_u32(
        &mut self,
        offset: u32,
        write: bool,
        bytes: &mut [u8; 4],
    ) -> Result<(), HalError> {
        let addr = sdio_core_reg_addr(offset);
        self.with_backplane_window_addr(
            addr,
            sdio_core_transfer_function_addr(offset),
            |this, function_addr| {
                this.io_extended(
                    SdioFunction::Function1,
                    function_addr,
                    sdio_core_transfer_increment_addr(),
                    write,
                    bytes,
                )
            },
        )
    }

    fn read_sdio_core_u32(&mut self, offset: u32) -> Result<u32, HalError> {
        let mut bytes = [0u8; 4];
        self.transfer_sdio_core_u32(offset, false, &mut bytes)?;
        Ok(u32::from_le_bytes(bytes))
    }

    fn read_sdio_core_u32_with_f1_fallback(
        &mut self,
        stage: &'static str,
        name: &'static str,
        offset: u32,
    ) -> Result<u32, HalError> {
        match self.read_sdio_core_u32(offset) {
            Ok(value) => Ok(value),
            Err(err) => {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] control-plane snapshot {stage} reg={name} action=sdio-core-transfer-fallback err={err}"
                ));
                match self.read_f1_u32_incrementing_cmd53(offset) {
                    Ok(value) => Ok(value),
                    Err(fallback_err) => {
                        emit_breadcrumb(format_args!(
                            "[pi4-wifi] control-plane snapshot {stage} reg={name} action=f1-byte-fallback err={fallback_err}"
                        ));
                        self.read_f1_u32(offset)
                    }
                }
            }
        }
    }

    fn read_sdio_core_u32_for_firmware_ready(
        &mut self,
        stage: &'static str,
        offset: u32,
        allow_function2_ready_bypass: bool,
        attempt: usize,
    ) -> Result<u32, HalError> {
        match self.read_sdio_core_u32(offset) {
            Ok(value) => return Ok(value),
            Err(err) => {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware stage={stage} action=sdio-core-transfer-fallback addr=0x{offset:05x} backplane=0x{backplane:08x} err={err}",
                    backplane = sdio_core_reg_addr(offset),
                ));
            }
        }

        if !wait_for_firmware_ready_uses_experimental_mailbox_read(allow_function2_ready_bypass) {
            return self.read_f1_u32(offset);
        }

        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware stage={stage} action=experimental-cmd53-incrementing-read addr=0x{offset:05x}"
        ));
        match self.read_f1_u32_incrementing_cmd53(offset) {
            Ok(value) => {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware stage={stage} action=experimental-cmd53-incrementing-read-ok addr=0x{offset:05x} value=0x{value:08x}"
                ));
                Ok(value)
            }
            Err(err)
                if wait_for_firmware_ready_can_assume_mailbox_ready(
                    allow_function2_ready_bypass,
                    attempt,
                    &err,
                ) =>
            {
                let assumed_ready = HMB_DATA_DEVREADY
                    | HMB_DATA_FWREADY
                    | (SDPCM_PROT_VERSION << HMB_DATA_VERSION_SHIFT);
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware stage={stage} action=experimental-assume-ready addr=0x{offset:05x} value=0x{assumed_ready:08x} err={err}"
                ));
                self.recover_command_path_and_refresh_transport(stage)?;
                Ok(assumed_ready)
            }
            Err(err) => Err(err),
        }
    }

    fn write_f1_u32(&mut self, addr: u32, value: u32) -> Result<(), HalError> {
        for (index, value) in value.to_le_bytes().into_iter().enumerate() {
            let byte_addr = addr
                .checked_add(index as u32)
                .ok_or(HalError::Unsupported("f1-write32-overflow"))?;
            self.io_direct_write(SdioFunction::Function1, byte_addr, value)?;
        }
        Ok(())
    }

    fn write_f1_u32_incrementing_cmd53(&mut self, addr: u32, value: u32) -> Result<(), HalError> {
        let mut bytes = value.to_le_bytes();
        self.io_extended_byte_mode(SdioFunction::Function1, addr, true, true, &mut bytes)
    }

    fn write_sdio_core_u32(&mut self, offset: u32, value: u32) -> Result<(), HalError> {
        let mut bytes = value.to_le_bytes();
        self.transfer_sdio_core_u32(offset, true, &mut bytes)
    }

    fn write_sdio_core_u32_for_firmware_channel(
        &mut self,
        stage: &'static str,
        offset: u32,
        value: u32,
        allow_function2_ready_bypass: bool,
    ) -> Result<(), HalError> {
        match self.write_sdio_core_u32(offset, value) {
            Ok(()) => return Ok(()),
            Err(err) => {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware stage={stage} action=sdio-core-transfer-fallback addr=0x{offset:05x} backplane=0x{backplane:08x} value=0x{value:08x} err={err}",
                    backplane = sdio_core_reg_addr(offset),
                ));
            }
        }

        if !setup_firmware_channel_uses_experimental_order(allow_function2_ready_bypass) {
            return self.write_f1_u32(offset, value);
        }

        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware stage={stage} action=experimental-cmd53-incrementing addr=0x{offset:05x} value=0x{value:08x}"
        ));
        match self.write_f1_u32_incrementing_cmd53(offset, value) {
            Ok(()) => {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware stage={stage} action=experimental-cmd53-incrementing-ok addr=0x{offset:05x} value=0x{value:08x}"
                ));
                Ok(())
            }
            Err(err) => {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware stage={stage} action=experimental-direct-byte-fallback addr=0x{offset:05x} value=0x{value:08x} err={err}"
                ));
                match self.write_f1_u32(offset, value) {
                    Ok(()) => {
                        emit_breadcrumb(format_args!(
                            "[pi4-wifi] firmware stage={stage} action=experimental-direct-byte-fallback-ok addr=0x{offset:05x} value=0x{value:08x}"
                        ));
                        Ok(())
                    }
                    Err(err)
                        if setup_firmware_channel_can_assume_write_committed(
                            allow_function2_ready_bypass,
                            0,
                            &err,
                        ) =>
                    {
                        emit_breadcrumb(format_args!(
                            "[pi4-wifi] firmware stage={stage} action=experimental-assume-committed addr=0x{offset:05x} value=0x{value:08x} err={err}"
                        ));
                        self.recover_command_path_and_refresh_transport(stage)?;
                        Ok(())
                    }
                    Err(err) => Err(err),
                }
            }
        }
    }

    fn next_frame_len(&mut self) -> Result<usize, HalError> {
        let lo =
            usize::from(self.io_direct_read(SdioFunction::Function1, SBSDIO_FUNC1_RFRAMEBCLO)?);
        let hi =
            usize::from(self.io_direct_read(SdioFunction::Function1, SBSDIO_FUNC1_RFRAMEBCHI)?);
        let frame_len = (hi << 8) | lo;
        if frame_len != 0 {
            self.experimental_control_plane_reply_rearm_mode = control_plane_reply_rearm_none();
            return Ok(frame_len);
        }
        if control_plane_reply_rearm_pending(self.experimental_control_plane_reply_rearm_mode) {
            self.maybe_rearm_control_plane_reply_on_zero_frame()?;
            let lo =
                usize::from(self.io_direct_read(SdioFunction::Function1, SBSDIO_FUNC1_RFRAMEBCLO)?);
            let hi =
                usize::from(self.io_direct_read(SdioFunction::Function1, SBSDIO_FUNC1_RFRAMEBCHI)?);
            let frame_len = (hi << 8) | lo;
            if frame_len != 0 {
                self.experimental_control_plane_reply_rearm_mode = control_plane_reply_rearm_none();
            }
            return Ok(frame_len);
        }
        Ok(0)
    }

    fn read_frame(&mut self, out: &mut [u8]) -> Result<usize, HalError> {
        let frame_len = self.next_frame_len()?;
        if frame_len == 0 {
            return Ok(0);
        }
        if frame_len > out.len() {
            return Err(HalError::Unsupported("cyw43-frame-oversize"));
        }
        self.io_extended(
            SdioFunction::Function2,
            0,
            false,
            false,
            &mut out[..frame_len],
        )?;
        Ok(frame_len)
    }

    fn write_frame(&mut self, frame: &mut [u8]) -> Result<(), HalError> {
        let first_control_plane_write_pending = self.experimental_control_plane_write_probe_pending;
        let promoted_probe_pending = self.experimental_control_plane_promoted_probe_pending;
        let frame_len = frame.len();
        self.experimental_control_plane_write_probe_pending = false;
        let finish_successful_write = |this: &mut Self| -> Result<(), HalError> {
            if experimental_control_plane_write_needs_post_write_rearm(
                this.experimental_no_ht_transport,
                first_control_plane_write_pending,
            ) {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware stage=control-plane-write action=experimental-post-write-rearm len={} chunk_limit={}",
                    frame_len,
                    this.control_plane_chunk_limit(),
                ));
                match this.rearm_firmware_channel_after_first_control_write() {
                    Ok(()) => {}
                    Err(err)
                        if experimental_control_plane_write_can_promote_after_post_write_rearm_timeout(
                            this.experimental_no_ht_transport,
                            first_control_plane_write_pending,
                            &err,
                        ) =>
                    {
                        this.promote_control_plane_after_post_write_rearm_timeout(&err)?;
                    }
                    Err(err) => return Err(err),
                }
            }
            Ok(())
        };
        match self.io_extended(SdioFunction::Function2, 0, false, true, frame) {
            Ok(()) => finish_successful_write(self),
            Err(err)
                if experimental_control_plane_write_can_retry_on_startup_link(
                    self.experimental_no_ht_transport,
                    first_control_plane_write_pending,
                    self.current_clock_hz,
                    &err,
                ) =>
            {
                let previous_clock_hz = self.current_clock_hz;
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware stage=control-plane-write action=fallback-startup-link len={} chunk_limit={} current={}Hz target={}Hz err={err}",
                    frame_len,
                    self.control_plane_chunk_limit(),
                    previous_clock_hz,
                    CYW43_STARTUP_CLOCK_HZ,
                ));
                self.set_clock_hz(CYW43_STARTUP_CLOCK_HZ)?;
                self.refresh_transport_phase_for("control-plane-write-startup-retry")?;
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware stage=control-plane-write action=fallback-startup-link-ready previous={}Hz current={}Hz len={} chunk_limit={}",
                    previous_clock_hz,
                    self.current_clock_hz,
                    frame_len,
                    self.control_plane_chunk_limit(),
                ));
                match self.io_extended(SdioFunction::Function2, 0, false, true, frame) {
                    Ok(()) => {
                        emit_breadcrumb(format_args!(
                            "[pi4-wifi] firmware stage=control-plane-write action=fallback-startup-link-ok len={} chunk_limit={}",
                            frame_len,
                            self.control_plane_chunk_limit(),
                        ));
                        finish_successful_write(self)
                    }
                    Err(retry_err) => {
                        self.log_control_plane_finish_snapshot(
                            "control-plane-write-startup-retry-failed",
                        );
                        Err(retry_err)
                    }
                }
            }
            Err(err)
                if experimental_control_plane_write_can_assume_committed(
                    self.experimental_no_ht_transport,
                    first_control_plane_write_pending,
                    promoted_probe_pending,
                    &err,
                ) =>
            {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware stage=control-plane-write action=experimental-assume-control-write len={} chunk_limit={} err={err}",
                    frame.len(),
                    self.control_plane_chunk_limit(),
                ));
                self.recover_command_path_and_refresh_transport("control-plane-write")?;
                self.log_control_plane_finish_snapshot("control-plane-write-assumed");
                Ok(())
            }
            Err(err) => {
                if self.experimental_no_ht_transport && first_control_plane_write_pending {
                    self.log_control_plane_finish_snapshot("control-plane-write-failed");
                }
                Err(err)
            }
        }
    }

    fn chip_id(&mut self) -> Result<u32, HalError> {
        emit_breadcrumb(format_args!("[pi4-wifi] cyw43 transport stage=chip-id"));
        Ok(self.backplane_read32(CYW43_CHIPCOMMON_BASE)? & 0xFFFF)
    }

    fn ram_size(&mut self) -> Result<u32, HalError> {
        let cap = self.backplane_read32(CYW43_ARMCR4_CORE_BASE + ARMCR4_CAP)?;
        let nab = (cap & ARMCR4_TCBANB_MASK) >> ARMCR4_TCBANB_SHIFT;
        let nbb = (cap & ARMCR4_TCBBNB_MASK) >> ARMCR4_TCBBNB_SHIFT;
        let total_banks = nab + nbb;
        let mut size = 0u32;
        for index in 0..total_banks {
            self.backplane_write32(CYW43_ARMCR4_CORE_BASE + ARMCR4_BANKIDX, index)?;
            let info = self.backplane_read32(CYW43_ARMCR4_CORE_BASE + ARMCR4_BANKINFO)?;
            let mut block_size = ARMCR4_BSZ_MULT;
            if (info & ARMCR4_BLK_1K_MASK) != 0 {
                block_size >>= 3;
            }
            size = size.saturating_add(((info & ARMCR4_BSZ_MASK) + 1).saturating_mul(block_size));
        }
        Ok(size)
    }

    fn firmware_ram_base(&self) -> u32 {
        CYW43_RAM_BASE_4345
    }

    fn init_cyw43_transport(&mut self) -> Result<(), HalError> {
        emit_breadcrumb(format_args!("[pi4-wifi] cyw43 transport init begin"));
        self.ensure_card_ready()?;
        self.enable_function1()?;
        self.bring_up_backplane()?;
        let chip_id = self.chip_id()?;
        emit_breadcrumb(format_args!(
            "[pi4-wifi] cyw43 transport stage=chip-id-ready value=0x{chip_id:08x}"
        ));
        emit_breadcrumb(format_args!(
            "[pi4-wifi] cyw43 transport ready chip=0x{chip_id:08x}"
        ));
        Ok(())
    }

    fn load_firmware(&mut self, bundle: WifiFirmwareBundle<'static>) -> Result<(), HalError> {
        let ram_base = self.firmware_ram_base();
        let ram_size = self.ram_size()?;
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware load begin ram_base=0x{ram_base:08x} ram_size=0x{ram_size:08x}"
        ));
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware core-ctrl mode={} split-window",
            core_ctrl_access_mode_label()
        ));
        if should_prime_ht_clock_assist_before_reset(self.last_chipclkcsr) {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware stage=pre-reset-ht-assist"
            ));
            self.enable_ht_clock_assist_for("pre-reset-ht-assist")?;
            self.log_transport_shadow("pre-reset-ht-assist");
        } else {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware stage=pre-reset-ht-assist skipped csr=0x{:02x}",
                self.last_chipclkcsr.unwrap_or(0)
            ));
        }
        emit_breadcrumb(format_args!("[pi4-wifi] firmware stage=armcr4-disable"));
        self.core_disable(
            CYW43_ARMCR4_CORE_BASE,
            ARMCR4_BCMA_IOCTL_CPUHALT,
            ARMCR4_BCMA_IOCTL_CPUHALT,
        )?;
        emit_breadcrumb(format_args!("[pi4-wifi] firmware stage=socram-disable"));
        self.core_disable(CYW43_SOCRAM_CORE_BASE, 0, 0)?;
        emit_breadcrumb(format_args!("[pi4-wifi] firmware stage=socram-reset"));
        self.core_reset(CYW43_SOCRAM_CORE_BASE, 0, 0, 0)?;
        emit_breadcrumb(format_args!("[pi4-wifi] firmware stage=chipcommon-config"));
        self.configure_chipcommon()?;
        let nvram = normalize_nvram(bundle.nvram);
        let nvram_offset = ram_base
            .checked_add(ram_size)
            .and_then(|value| value.checked_sub(4))
            .and_then(|value| value.checked_sub(u32::try_from(nvram.len()).ok()?))
            .ok_or(HalError::Unsupported("cyw43-nvram-range"))?;
        let nvram_words =
            u32::try_from(nvram.len() / 4).map_err(|_| HalError::Unsupported("cyw43-nvram-len"))?;
        let nvram_magic = (!nvram_words << 16) | nvram_words;
        let nvram_tail = ram_base
            .checked_add(ram_size)
            .and_then(|value| value.checked_sub(4))
            .ok_or(HalError::Unsupported("cyw43-nvram-tail"))?;
        let prewrite_ht_ready = self.wait_for_ht_clock_with_stage(
            "pre-write-ht-clock",
            "pre-write-ht-clock assist",
            false,
            false,
        )?;
        let clock_candidates =
            firmware_bulk_clock_candidates(self.current_clock_hz, prewrite_ht_ready);
        let prefer_byte_mode =
            firmware_upload_prefers_byte_mode(self.current_clock_hz, prewrite_ht_ready);
        let mut selected_bulk_clock_hz = CYW43_FIRMWARE_BULK_CLOCK_HZ;
        let mut upload_complete = false;
        for (attempt_index, &clock_hz) in clock_candidates.iter().enumerate() {
            if clock_hz == 0 || clock_candidates[..attempt_index].contains(&clock_hz) {
                continue;
            }
            match self.with_firmware_bulk_clock("write-firmware-bulk", clock_hz, |this| {
                this.write_firmware_payloads(
                    bundle.firmware,
                    &nvram,
                    ram_base,
                    nvram_offset,
                    nvram_tail,
                    nvram_magic,
                    prefer_byte_mode,
                )
            }) {
                Ok(()) => {
                    selected_bulk_clock_hz = clock_hz;
                    upload_complete = true;
                    break;
                }
                Err(err) if is_sdhci_io_path_error(&err) => {
                    emit_breadcrumb(format_args!(
                        "[pi4-wifi] firmware stage=write-firmware-bulk clock=attempt-fail request={}Hz err={err}",
                        clock_hz
                    ));
                    if let Some(next_clock_hz) = next_distinct_firmware_bulk_clock_candidate(
                        &clock_candidates,
                        attempt_index,
                    ) {
                        emit_breadcrumb(format_args!(
                            "[pi4-wifi] firmware stage=write-firmware-bulk clock=fallback-next request={}Hz from={}Hz",
                            next_clock_hz, clock_hz
                        ));
                        self.recover_command_path_and_refresh_transport(
                            "write-firmware-bulk-fallback",
                        )?;
                        continue;
                    }
                    return Err(err);
                }
                Err(err) => return Err(err),
            }
        }
        if !upload_complete {
            return Err(HalError::Unsupported("cyw43-firmware-clock-exhausted"));
        }
        self.preferred_data_clock_hz = selected_bulk_clock_hz;
        self.recover_command_path_and_refresh_transport("armcr4-reset-prep")?;
        emit_breadcrumb(format_args!("[pi4-wifi] firmware stage=armcr4-reset"));
        self.core_reset(CYW43_ARMCR4_CORE_BASE, ARMCR4_BCMA_IOCTL_CPUHALT, 0, 0)?;
        self.ensure_control_plane_clock("armcr4-core-up-clock")?;
        self.recover_command_path_and_refresh_transport("armcr4-core-up-clock")?;
        emit_breadcrumb(format_args!("[pi4-wifi] firmware stage=armcr4-core-up"));
        self.wait_for_core_up(CYW43_ARMCR4_CORE_BASE, "armcr4-core-up")?;
        self.ensure_control_plane_clock("wait-ht-clock-prep")?;
        let strict_ht_ready =
            self.require_ht_clock_ready("wait-ht-clock", "wait-ht-clock assist")?;
        self.ensure_control_plane_clock("setup-firmware-channel-clock")?;
        if strict_ht_ready {
            self.experimental_no_ht_transport = false;
            self.experimental_control_plane_write_probe_pending = false;
        } else {
            self.enter_bounded_no_ht_transport("wait-ht-clock");
        }
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware stage=setup-firmware-channel mode={} chunk_limit={}",
            cyw43_transport_mode_name(self.experimental_no_ht_transport),
            self.control_plane_chunk_limit(),
        ));
        let allow_function2_ready_bypass = self.experimental_no_ht_transport;
        self.setup_firmware_channel(allow_function2_ready_bypass)?;
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware stage=wait-firmware-ready mode={} chunk_limit={}",
            cyw43_transport_mode_name(self.experimental_no_ht_transport),
            self.control_plane_chunk_limit(),
        ));
        self.wait_for_firmware_ready(allow_function2_ready_bypass)?;
        if allow_function2_ready_bypass {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware stage=post-firmware-ready-function2-recheck action=repoll"
            ));
            self.enable_function2(SdioFunctionReadyBudget::ExperimentalBypass)?;
        }
        emit_breadcrumb(format_args!("[pi4-wifi] firmware load ready"));
        Ok(())
    }

    fn wait_for_ht_clock_with_stage(
        &mut self,
        stage: &'static str,
        assist_stage: &'static str,
        required: bool,
        stronger_retry_request: bool,
    ) -> Result<bool, HalError> {
        let soft_wait_loops = if required {
            required_ht_clock_wait_loops(stronger_retry_request)
        } else {
            CYW43_HT_CLOCK_SOFT_WAIT_LOOPS
        };
        let request = if required {
            if stronger_retry_request {
                required_ht_clock_retry_request_value(self.last_chipclkcsr)
            } else {
                required_ht_clock_request_value(self.last_chipclkcsr)
            }
        } else {
            ht_clock_request_value()
        };
        if required && stronger_retry_request {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware stage={stage} action=retry-stronger-request request=0x{request:02x} wait_loops={soft_wait_loops}"
            ));
        }
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware stage={stage} request=0x{request:02x}"
        ));
        self.io_direct_write(SdioFunction::Function1, SBSDIO_FUNC1_CHIPCLKCSR, request)?;
        self.remember_chipclkcsr(request);
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware stage={stage} request-issued"
        ));
        let request_chipclk =
            self.io_direct_read(SdioFunction::Function1, SBSDIO_FUNC1_CHIPCLKCSR)?;
        self.remember_chipclkcsr(request_chipclk);
        log_ht_clock_status(
            stage,
            "request-readback",
            request_chipclk,
            self.last_wakeupctrl,
            self.last_sleepcsr,
            self.last_cardcap,
        );
        let mut last_chipclk = 0u8;
        for _ in 0..CYW43_HT_CLOCK_INITIAL_WAIT_LOOPS {
            let chipclk = self.io_direct_read(SdioFunction::Function1, SBSDIO_FUNC1_CHIPCLKCSR)?;
            self.remember_chipclkcsr(chipclk);
            last_chipclk = chipclk;
            if (chipclk & SBSDIO_HT_AVAIL) != 0 {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware stage={stage} ready csr=0x{chipclk:02x}"
                ));
                return Ok(true);
            }
            spin_loop();
        }
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware stage={stage} retry=assist csr=0x{last_chipclk:02x}"
        ));
        self.enable_ht_clock_assist_for(assist_stage)?;
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware stage={stage} retry=alp-prime csr=0x{last_chipclk:02x}"
        ));
        // Keep FORCE_HT / HT request bits asserted while priming ALP. Dropping
        // back to a raw ALP request sheds the stronger clock request state on
        // the exact path that later times out at HT readiness.
        let alp_request = ht_clock_alp_prime_request_value(Some(last_chipclk));
        self.io_direct_write(
            SdioFunction::Function1,
            SBSDIO_FUNC1_CHIPCLKCSR,
            alp_request,
        )?;
        self.remember_chipclkcsr(alp_request);
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware stage={stage} alp-request=0x{alp_request:02x}"
        ));
        for _ in 0..soft_wait_loops {
            let chipclk = self.io_direct_read(SdioFunction::Function1, SBSDIO_FUNC1_CHIPCLKCSR)?;
            self.remember_chipclkcsr(chipclk);
            last_chipclk = chipclk;
            if (chipclk & SBSDIO_ALP_AVAIL) != 0 {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware stage={stage} alp-ready csr=0x{chipclk:02x}"
                ));
                break;
            }
            spin_loop();
        }
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware stage={stage} ht-rerequest request=0x{request:02x}"
        ));
        self.io_direct_write(SdioFunction::Function1, SBSDIO_FUNC1_CHIPCLKCSR, request)?;
        self.remember_chipclkcsr(request);
        let rerequest_chipclk =
            self.io_direct_read(SdioFunction::Function1, SBSDIO_FUNC1_CHIPCLKCSR)?;
        self.remember_chipclkcsr(rerequest_chipclk);
        last_chipclk = rerequest_chipclk;
        log_ht_clock_status(
            stage,
            "ht-rerequest-readback",
            rerequest_chipclk,
            self.last_wakeupctrl,
            self.last_sleepcsr,
            self.last_cardcap,
        );
        if required && stronger_retry_request {
            let mut remaining_loops = soft_wait_loops;
            let mut completed_loops = 0usize;
            let mut refresh_index = 0usize;
            while remaining_loops > 0 {
                let chunk_loops = ht_clock_progress_chunk_loops(remaining_loops);
                for _ in 0..chunk_loops {
                    let chipclk =
                        self.io_direct_read(SdioFunction::Function1, SBSDIO_FUNC1_CHIPCLKCSR)?;
                    self.remember_chipclkcsr(chipclk);
                    last_chipclk = chipclk;
                    if (chipclk & SBSDIO_HT_AVAIL) != 0 {
                        emit_breadcrumb(format_args!(
                            "[pi4-wifi] firmware stage={stage} ready csr=0x{chipclk:02x}"
                        ));
                        return Ok(true);
                    }
                    spin_loop();
                }
                completed_loops = completed_loops.saturating_add(chunk_loops);
                remaining_loops -= chunk_loops;
                if ht_clock_retry_can_cutover_to_bounded_no_ht_early(
                    stronger_retry_request,
                    completed_loops,
                    self.last_chipclkcsr,
                    self.last_wakeupctrl,
                    self.last_sleepcsr,
                    self.last_cardcap,
                ) {
                    let shortcut_loops = required_ht_clock_bounded_no_ht_shortcut_loops();
                    emit_breadcrumb(format_args!(
                        "[pi4-wifi] firmware stage={stage} action=bounded-no-ht-early-exit polls={completed_loops}/{soft_wait_loops} shortcut={shortcut_loops} csr=0x{last_chipclk:02x}"
                    ));
                    log_ht_clock_status(
                        stage,
                        "bounded-no-ht-early-exit",
                        last_chipclk,
                        self.last_wakeupctrl,
                        self.last_sleepcsr,
                        self.last_cardcap,
                    );
                    self.log_transport_shadow("wait-ht-clock-bounded-no-ht-early-exit");
                    return Ok(false);
                }
                if remaining_loops == 0 {
                    break;
                }
                refresh_index = refresh_index.saturating_add(1);
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware stage={stage} progress=wait-ht-clock polls={completed_loops}/{soft_wait_loops} csr=0x{last_chipclk:02x} refresh_index={refresh_index}"
                ));
                self.refresh_ht_clock_assist_from_shadow_for(stage)?;
                wifi_progress_tick();
            }
        } else {
            for _ in 0..soft_wait_loops {
                let chipclk =
                    self.io_direct_read(SdioFunction::Function1, SBSDIO_FUNC1_CHIPCLKCSR)?;
                self.remember_chipclkcsr(chipclk);
                last_chipclk = chipclk;
                if (chipclk & SBSDIO_HT_AVAIL) != 0 {
                    emit_breadcrumb(format_args!(
                        "[pi4-wifi] firmware stage={stage} ready csr=0x{chipclk:02x}"
                    ));
                    return Ok(true);
                }
                spin_loop();
            }
        }
        if required {
            if ht_clock_timeout_can_continue(required, last_chipclk) {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware stage={stage} timeout-soft csr=0x{last_chipclk:02x} action=continue-with-force-ht-refresh"
                ));
                log_ht_clock_status(
                    stage,
                    "timeout-soft",
                    last_chipclk,
                    self.last_wakeupctrl,
                    self.last_sleepcsr,
                    self.last_cardcap,
                );
                self.remember_chipclkcsr(last_chipclk);
                self.log_transport_shadow("wait-ht-clock-timeout-soft");
                Ok(false)
            } else {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware stage={stage} timeout csr=0x{last_chipclk:02x}"
                ));
                log_ht_clock_status(
                    stage,
                    "timeout-hard",
                    last_chipclk,
                    self.last_wakeupctrl,
                    self.last_sleepcsr,
                    self.last_cardcap,
                );
                self.log_transport_shadow("wait-ht-clock-timeout");
                Err(HalError::Unsupported("cyw43-ht-clock-timeout"))
            }
        } else {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware stage={stage} timeout-soft csr=0x{last_chipclk:02x} action=continue-with-fallback-clocks"
            ));
            log_ht_clock_status(
                stage,
                "timeout-soft",
                last_chipclk,
                self.last_wakeupctrl,
                self.last_sleepcsr,
                self.last_cardcap,
            );
            self.log_transport_shadow("wait-ht-clock-timeout-soft");
            Ok(false)
        }
    }

    fn require_ht_clock_ready(
        &mut self,
        stage: &'static str,
        assist_stage: &'static str,
    ) -> Result<bool, HalError> {
        for attempt in 0..=1 {
            match self.wait_for_ht_clock_with_stage(stage, assist_stage, true, attempt > 0)? {
                true => return Ok(true),
                false if attempt == 0 => {
                    emit_breadcrumb(format_args!(
                        "[pi4-wifi] firmware stage={stage} action=recover-retry reason=ht-not-ready"
                    ));
                    self.recover_command_path_and_refresh_transport(stage)?;
                }
                false => {
                    if ht_clock_timeout_can_enter_bounded_no_ht_transport(
                        self.last_chipclkcsr,
                        self.last_wakeupctrl,
                        self.last_sleepcsr,
                        self.last_cardcap,
                    ) {
                        emit_breadcrumb(format_args!(
                            "[pi4-wifi] firmware stage={stage} action=continue mode={} reason=ht-not-ready csr=0x{:02x}",
                            cyw43_transport_mode_name(true),
                            self.last_chipclkcsr.unwrap_or(0),
                        ));
                        self.log_transport_shadow("wait-ht-clock-bounded-no-ht");
                        return Ok(false);
                    }
                    emit_breadcrumb(format_args!(
                        "[pi4-wifi] firmware stage={stage} action=fail reason=ht-not-ready"
                    ));
                    return Err(HalError::Unsupported("cyw43-ht-clock-timeout"));
                }
            }
        }
        Err(HalError::Unsupported("cyw43-ht-clock-timeout"))
    }

    fn ensure_control_plane_clock(&mut self, stage: &'static str) -> Result<(), HalError> {
        let target_clock_hz =
            control_plane_clock_target_hz(self.current_clock_hz, self.preferred_data_clock_hz);
        if target_clock_hz <= self.current_clock_hz {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware stage={stage} action=skip current={}Hz preferred={}Hz",
                self.current_clock_hz, self.preferred_data_clock_hz
            ));
            self.preferred_data_clock_hz = self.preferred_data_clock_hz.max(self.current_clock_hz);
            return Ok(());
        }

        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware stage={stage} action=raise request={}Hz from={}Hz",
            target_clock_hz, self.current_clock_hz
        ));
        let effective_hz = self.set_clock_hz(target_clock_hz)?;
        self.preferred_data_clock_hz = self.preferred_data_clock_hz.max(effective_hz);
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware stage={stage} action=raise-ready effective={}Hz",
            effective_hz
        ));
        Ok(())
    }

    fn write_firmware_payloads(
        &mut self,
        firmware: &[u8],
        nvram: &[u8],
        ram_base: u32,
        nvram_offset: u32,
        nvram_tail: u32,
        nvram_magic: u32,
        prefer_byte_mode: bool,
    ) -> Result<(), HalError> {
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware stage=write-firmware bytes={}",
            firmware.len()
        ));
        if prefer_byte_mode {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware stage=write-firmware mode=byte-start reason=startup-clock-no-ht current_clock={}Hz",
                self.current_clock_hz
            ));
        }
        self.prepare_firmware_upload_window(
            "write-firmware-window",
            "write-firmware-window-assumed-committed",
            ram_base,
        )?;
        self.backplane_write_firmware_retrying_window(
            "write-firmware-bulk-retry",
            "write-firmware-window",
            "write-firmware-window-assumed-committed",
            prefer_byte_mode,
            ram_base,
            firmware,
        )?;
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware stage=write-nvram bytes={} offset=0x{nvram_offset:08x}",
            nvram.len()
        ));
        self.prepare_firmware_upload_window(
            "write-nvram-window",
            "write-nvram-window-assumed-committed",
            nvram_offset,
        )?;
        self.backplane_write_firmware_retrying_window(
            "write-nvram-bulk-retry",
            "write-nvram-window",
            "write-nvram-window-assumed-committed",
            prefer_byte_mode,
            nvram_offset,
            nvram,
        )?;
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware stage=write-nvram-tail magic=0x{nvram_magic:08x}"
        ));
        self.prepare_firmware_upload_window(
            "write-nvram-tail-window",
            "write-nvram-tail-window-assumed-committed",
            nvram_tail,
        )?;
        let nvram_magic_bytes = nvram_magic.to_le_bytes();
        self.backplane_write_firmware_retrying_window(
            "write-nvram-tail-bulk-retry",
            "write-nvram-tail-window",
            "write-nvram-tail-window-assumed-committed",
            prefer_byte_mode,
            nvram_tail,
            &nvram_magic_bytes,
        )?;
        Ok(())
    }

    fn wait_for_core_up(&mut self, base: u32, stage: &'static str) -> Result<(), HalError> {
        let mut last_ioctrl = 0u8;
        let mut last_resetctrl = 0u8;
        let mut attempts = 0usize;
        for attempt in 0..CYW43_CORE_RESET_RETRY_LIMIT {
            attempts = attempt.saturating_add(1);
            last_ioctrl = match self.core_ctrl_postreset_read8_logged(base, AI_IOCTRL_OFFSET, stage)
            {
                Ok(value) => value,
                Err(err) => {
                    if core_wait_should_raise_control_plane_clock(base, self.current_clock_hz, &err)
                    {
                        emit_breadcrumb(format_args!(
                            "[pi4-wifi] firmware stage={stage} readback-retry io-err={err} attempt={attempts} action=raise-control-plane-clock from={}Hz",
                            self.current_clock_hz
                        ));
                        self.ensure_control_plane_clock("armcr4-core-up-readback-prep")?;
                        self.recover_command_path_and_refresh_transport(stage)?;
                        spin_settle(CYW43_CORE_CONTROL_SETTLE_LOOPS);
                        continue;
                    }
                    if core_wait_can_defer_after_read_error(
                        base,
                        attempt,
                        self.current_clock_hz,
                        &err,
                    ) {
                        last_ioctrl = AI_CORE_POSTRESET_IOCTRL;
                        last_resetctrl = 0;
                        emit_breadcrumb(format_args!(
                            "[pi4-wifi] firmware stage={stage} readback-deferred io=0x{last_ioctrl:02x} reset=0x{last_resetctrl:02x} attempt={attempts} err={err} reason=armcr4-fragile-postreset-read-assumed-core-up"
                        ));
                        self.recover_command_path_and_refresh_transport(stage)?;
                        return Ok(());
                    }
                    if !core_wait_can_retry_after_read_error(base, &err) {
                        return Err(err);
                    }
                    emit_breadcrumb(format_args!(
                        "[pi4-wifi] firmware stage={stage} readback-retry io-err={err} attempt={attempts} reason=armcr4-fragile-postreset-read"
                    ));
                    self.recover_command_path_and_refresh_transport(stage)?;
                    spin_settle(CYW43_CORE_CONTROL_SETTLE_LOOPS);
                    continue;
                }
            };
            last_resetctrl = match self.core_ctrl_postreset_read8_logged(
                base,
                AI_RESETCTRL_OFFSET,
                stage,
            ) {
                Ok(value) => value,
                Err(err) => {
                    if core_wait_should_raise_control_plane_clock(base, self.current_clock_hz, &err)
                    {
                        emit_breadcrumb(format_args!(
                            "[pi4-wifi] firmware stage={stage} readback-retry reset-err={err} attempt={attempts} action=raise-control-plane-clock from={}Hz",
                            self.current_clock_hz
                        ));
                        self.ensure_control_plane_clock("armcr4-core-up-readback-prep")?;
                        self.recover_command_path_and_refresh_transport(stage)?;
                        spin_settle(CYW43_CORE_CONTROL_SETTLE_LOOPS);
                        continue;
                    }
                    if core_wait_can_defer_after_read_error(
                        base,
                        attempt,
                        self.current_clock_hz,
                        &err,
                    ) {
                        if last_ioctrl == 0 {
                            last_ioctrl = AI_CORE_POSTRESET_IOCTRL;
                        }
                        last_resetctrl = 0;
                        emit_breadcrumb(format_args!(
                            "[pi4-wifi] firmware stage={stage} readback-deferred io=0x{last_ioctrl:02x} reset=0x{last_resetctrl:02x} attempt={attempts} err={err} reason=armcr4-fragile-postreset-read-assumed-core-up"
                        ));
                        self.recover_command_path_and_refresh_transport(stage)?;
                        return Ok(());
                    }
                    if !core_wait_can_retry_after_read_error(base, &err) {
                        return Err(err);
                    }
                    emit_breadcrumb(format_args!(
                        "[pi4-wifi] firmware stage={stage} readback-retry reset-err={err} attempt={attempts} reason=armcr4-fragile-postreset-read"
                    ));
                    self.recover_command_path_and_refresh_transport(stage)?;
                    spin_settle(CYW43_CORE_CONTROL_SETTLE_LOOPS);
                    continue;
                }
            };
            if ai_core_is_up(last_ioctrl, last_resetctrl) {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware stage={stage} ready io=0x{last_ioctrl:02x} reset=0x{last_resetctrl:02x} attempts={attempts} reason={}",
                    ai_core_state_reason(last_ioctrl, last_resetctrl),
                ));
                return Ok(());
            }
            spin_settle(CYW43_CORE_CONTROL_SETTLE_LOOPS);
        }
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware stage={stage} timeout io=0x{last_ioctrl:02x} reset=0x{last_resetctrl:02x} attempts={attempts} reason={}",
            ai_core_state_reason(last_ioctrl, last_resetctrl),
        ));
        Err(HalError::Unsupported("cyw43-core-up-timeout"))
    }

    fn core_ctrl_windowed_read8(&mut self, base: u32, offset: u32) -> Result<u8, HalError> {
        let addr = base.saturating_add(offset);
        let word = self.backplane_word_read32(addr)?;
        Ok(((word >> backplane_word_byte_shift(addr)) & 0xFF) as u8)
    }

    fn core_ctrl_windowed_write8(
        &mut self,
        base: u32,
        offset: u32,
        value: u8,
    ) -> Result<(), HalError> {
        let addr = base.saturating_add(offset);
        self.with_backplane_window_addr(addr, core_ctrl_function_addr(addr), |this, bus_addr| {
            let _ = bus_addr;
            this.io_direct_write(
                SdioFunction::Function1,
                core_ctrl_current_window_addr(addr),
                value,
            )
        })
    }

    fn core_ctrl_postreset_current_window_transfer8(
        &mut self,
        base: u32,
        offset: u32,
        write: bool,
        byte: &mut [u8; 1],
    ) -> Result<(), HalError> {
        let addr = base.saturating_add(offset);
        self.with_backplane_window_addr(addr, core_ctrl_trace_function_addr(addr), |this, _| {
            this.io_extended_byte_mode(
                SdioFunction::Function1,
                core_ctrl_current_window_addr(addr),
                true,
                write,
                byte,
            )
        })
    }

    fn core_ctrl_postreset_current_window_read8(
        &mut self,
        base: u32,
        offset: u32,
    ) -> Result<u8, HalError> {
        let mut byte = [0u8; 1];
        self.core_ctrl_postreset_current_window_transfer8(base, offset, false, &mut byte)?;
        Ok(byte[0])
    }

    fn core_ctrl_postreset_current_window_write8(
        &mut self,
        base: u32,
        offset: u32,
        value: u8,
    ) -> Result<(), HalError> {
        let mut byte = [value; 1];
        self.core_ctrl_postreset_current_window_transfer8(base, offset, true, &mut byte)
    }

    fn core_ctrl_postreset_current_window_cmd52_read8(
        &mut self,
        base: u32,
        offset: u32,
    ) -> Result<u8, HalError> {
        let addr = base.saturating_add(offset);
        self.with_backplane_window_addr(
            addr,
            core_ctrl_current_window_addr(addr),
            |this, bus_addr| {
                this.io_direct_read_no_cmd53_fallback(SdioFunction::Function1, bus_addr)
            },
        )
    }

    fn core_ctrl_current_window_transfer8(
        &mut self,
        base: u32,
        offset: u32,
        write: bool,
        byte: &mut [u8; 1],
    ) -> Result<(), HalError> {
        let addr = base.saturating_add(offset);
        self.with_backplane_window_addr(addr, core_ctrl_trace_function_addr(addr), |this, _| {
            this.io_extended_byte_mode(
                SdioFunction::Function1,
                core_ctrl_current_window_addr(addr),
                true,
                write,
                byte,
            )
        })
    }

    fn core_ctrl_current_window_read8(&mut self, base: u32, offset: u32) -> Result<u8, HalError> {
        let mut byte = [0u8; 1];
        self.core_ctrl_current_window_transfer8(base, offset, false, &mut byte)?;
        Ok(byte[0])
    }

    fn core_ctrl_fallback_write8_cmd52_rewindow(
        &mut self,
        base: u32,
        offset: u32,
        value: u8,
    ) -> Result<(), HalError> {
        self.backplane_write8(base.saturating_add(offset), value)
    }

    fn core_ctrl_in_reset_write32(
        &mut self,
        base: u32,
        offset: u32,
        value: u8,
    ) -> Result<(), HalError> {
        self.backplane_word_write32(base.saturating_add(offset), u32::from(value))
    }

    fn core_ctrl_read8(&mut self, base: u32, offset: u32) -> Result<u8, HalError> {
        match self.core_ctrl_windowed_read8(base, offset) {
            Ok(value) => Ok(value),
            Err(err) => {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware core-ctrl fallback op=read8 base=0x{base:08x} off=0x{offset:03x} from=cmd53-word-windowed to=cmd53-byte-current-window err={err}"
                ));
                self.recover_command_path("core-ctrl-cmd53-current-window");
                match self.core_ctrl_current_window_read8(base, offset) {
                    Ok(value) => Ok(value),
                    Err(current_window_err) => {
                        emit_breadcrumb(format_args!(
                            "[pi4-wifi] firmware core-ctrl fallback op=read8 base=0x{base:08x} off=0x{offset:03x} to=cmd53-byte-rewindow err={current_window_err}"
                        ));
                        self.recover_command_path("core-ctrl-cmd53-rewindow");
                        match self.core_ctrl_current_window_read8(base, offset) {
                            Ok(value) => Ok(value),
                            Err(fallback_err) => {
                                emit_breadcrumb(format_args!(
                                    "[pi4-wifi] firmware core-ctrl fallback op=read8 base=0x{base:08x} off=0x{offset:03x} to=cmd53-byte-rewindow err={fallback_err}"
                                ));
                                Err(fallback_err)
                            }
                        }
                    }
                }
            }
        }
    }

    fn core_ctrl_postreset_read8(&mut self, base: u32, offset: u32) -> Result<u8, HalError> {
        if core_ctrl_postreset_read_uses_cmd52_current_window(base, offset) {
            match self.core_ctrl_postreset_current_window_cmd52_read8(base, offset) {
                Ok(value) => return Ok(value),
                Err(err) => {
                    emit_breadcrumb(format_args!(
                        "[pi4-wifi] firmware core-ctrl fallback op=read8-postreset base=0x{base:08x} off=0x{offset:03x} from=cmd52-byte-current-window to=cmd52-byte-rewindow err={err}"
                    ));
                    self.recover_command_path("core-ctrl-postreset-cmd52-rewindow");
                    match self.core_ctrl_postreset_current_window_cmd52_read8(base, offset) {
                        Ok(value) => return Ok(value),
                        Err(fallback_err) => {
                            emit_breadcrumb(format_args!(
                                "[pi4-wifi] firmware core-ctrl fallback op=read8-postreset base=0x{base:08x} off=0x{offset:03x} to=cmd52-byte-rewindow err={fallback_err}"
                            ));
                            return Err(fallback_err);
                        }
                    }
                }
            }
        }
        match self.core_ctrl_windowed_read8(base, offset) {
            Ok(value) => Ok(value),
            Err(err) => {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware core-ctrl fallback op=read8-postreset base=0x{base:08x} off=0x{offset:03x} from=cmd53-windowed-read32 to=cmd53-byte-current-window err={err}"
                ));
                self.recover_command_path("core-ctrl-postreset-cmd53-current-window");
                match self.core_ctrl_postreset_current_window_read8(base, offset) {
                    Ok(value) => Ok(value),
                    Err(current_window_err) => {
                        emit_breadcrumb(format_args!(
                            "[pi4-wifi] firmware core-ctrl fallback op=read8-postreset base=0x{base:08x} off=0x{offset:03x} to=cmd53-byte-rewindow err={current_window_err}"
                        ));
                        self.recover_command_path("core-ctrl-postreset-cmd53-rewindow");
                        match self.core_ctrl_postreset_current_window_read8(base, offset) {
                            Ok(value) => Ok(value),
                            Err(fallback_err) => {
                                emit_breadcrumb(format_args!(
                                    "[pi4-wifi] firmware core-ctrl fallback op=read8-postreset base=0x{base:08x} off=0x{offset:03x} to=cmd53-byte-rewindow err={fallback_err}"
                                ));
                                Err(fallback_err)
                            }
                        }
                    }
                }
            }
        }
    }

    fn core_ctrl_write8(&mut self, base: u32, offset: u32, value: u8) -> Result<(), HalError> {
        match self.core_ctrl_windowed_write8(base, offset, value) {
            Ok(()) => Ok(()),
            Err(err) => {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware core-ctrl fallback op=write8 base=0x{base:08x} off=0x{offset:03x} from=cmd52-byte-current-window to=cmd52-byte-rewindow err={err}"
                ));
                self.recover_command_path("core-ctrl-cmd52-rewindow");
                match self.core_ctrl_fallback_write8_cmd52_rewindow(base, offset, value) {
                    Ok(()) => Ok(()),
                    Err(fallback_err) => {
                        emit_breadcrumb(format_args!(
                            "[pi4-wifi] firmware core-ctrl fallback op=write8 base=0x{base:08x} off=0x{offset:03x} to=cmd52-byte-rewindow err={fallback_err}"
                        ));
                        Err(fallback_err)
                    }
                }
            }
        }
    }

    fn core_ctrl_word_primary_write8(
        &mut self,
        base: u32,
        offset: u32,
        value: u8,
        op: &'static str,
        current_window_stage: &'static str,
        rewindow_stage: &'static str,
    ) -> Result<(), HalError> {
        match self.backplane_word_write32(base.saturating_add(offset), u32::from(value)) {
            Ok(()) => Ok(()),
            Err(err) => {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware core-ctrl fallback op={op} base=0x{base:08x} off=0x{offset:03x} from=cmd53-word-windowed to=cmd52-byte-current-window err={err}"
                ));
                self.recover_command_path(current_window_stage);
                match self.core_ctrl_windowed_write8(base, offset, value) {
                    Ok(()) => Ok(()),
                    Err(current_window_err) => {
                        emit_breadcrumb(format_args!(
                            "[pi4-wifi] firmware core-ctrl fallback op={op} base=0x{base:08x} off=0x{offset:03x} to=cmd52-byte-rewindow err={current_window_err}"
                        ));
                        self.recover_command_path(rewindow_stage);
                        match self.core_ctrl_fallback_write8_cmd52_rewindow(base, offset, value) {
                            Ok(()) => Ok(()),
                            Err(fallback_err) => {
                                emit_breadcrumb(format_args!(
                                    "[pi4-wifi] firmware core-ctrl fallback op={op} base=0x{base:08x} off=0x{offset:03x} to=cmd52-byte-rewindow err={fallback_err}"
                                ));
                                Err(fallback_err)
                            }
                        }
                    }
                }
            }
        }
    }

    fn core_ctrl_reset_assert_write8(
        &mut self,
        base: u32,
        offset: u32,
        value: u8,
    ) -> Result<(), HalError> {
        self.core_ctrl_word_primary_write8(
            base,
            offset,
            value,
            "write8-reset-assert",
            "core-ctrl-reset-assert-cmd52-current-window",
            "core-ctrl-reset-assert-cmd52-rewindow",
        )
    }

    fn core_ctrl_reset_clear_write8(
        &mut self,
        base: u32,
        offset: u32,
        value: u8,
    ) -> Result<(), HalError> {
        match self.backplane_word_write32(base.saturating_add(offset), u32::from(value)) {
            Ok(()) => Ok(()),
            Err(err) => {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware core-ctrl fallback op=write8-reset-clear base=0x{base:08x} off=0x{offset:03x} from=cmd53-word-windowed to=cmd52-byte-current-window err={err}"
                ));
                if core_reset_clear_preserves_window_cache() {
                    self.recover_command_path_preserve_window(
                        "core-ctrl-reset-clear-cmd52-current-window",
                    );
                } else {
                    self.recover_command_path("core-ctrl-reset-clear-cmd52-current-window");
                }
                match self.core_ctrl_windowed_write8(base, offset, value) {
                    Ok(()) => {
                        emit_breadcrumb(format_args!(
                            "[pi4-wifi] firmware core-ctrl reset-clear stage=cmd52-current-window-ok base=0x{base:08x} off=0x{offset:03x} cache=preserved value=0x{value:02x}"
                        ));
                        Ok(())
                    }
                    Err(current_window_err) => {
                        emit_breadcrumb(format_args!(
                            "[pi4-wifi] firmware core-ctrl fallback op=write8-reset-clear base=0x{base:08x} off=0x{offset:03x} to=cmd52-byte-current-window err={current_window_err}"
                        ));
                        if !core_reset_clear_allows_immediate_rewindow_fallback() {
                            return Err(current_window_err);
                        }
                        self.recover_command_path("core-ctrl-reset-clear-cmd52-rewindow");
                        match self.core_ctrl_fallback_write8_cmd52_rewindow(base, offset, value) {
                            Ok(()) => Ok(()),
                            Err(fallback_err) => {
                                emit_breadcrumb(format_args!(
                                    "[pi4-wifi] firmware core-ctrl fallback op=write8-reset-clear base=0x{base:08x} off=0x{offset:03x} to=cmd52-byte-rewindow err={fallback_err}"
                                ));
                                Err(fallback_err)
                            }
                        }
                    }
                }
            }
        }
    }

    fn core_ctrl_reset_clear_retry_current_window_write8(
        &mut self,
        base: u32,
        offset: u32,
        value: u8,
    ) -> Result<(), HalError> {
        match self.core_ctrl_windowed_write8(base, offset, value) {
            Ok(()) => {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware core-ctrl reset-clear stage=cmd52-current-window-ok base=0x{base:08x} off=0x{offset:03x} cache=preserved value=0x{value:02x}"
                ));
                Ok(())
            }
            Err(err) => {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware core-ctrl fallback op=write8-reset-clear-retry base=0x{base:08x} off=0x{offset:03x} to=cmd52-byte-current-window err={err}"
                ));
                Err(err)
            }
        }
    }

    fn core_ctrl_postreset_write8(
        &mut self,
        base: u32,
        offset: u32,
        value: u8,
    ) -> Result<(), HalError> {
        match self.core_ctrl_postreset_current_window_write8(base, offset, value) {
            Ok(()) => Ok(()),
            Err(err) => {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware core-ctrl fallback op=write8-postreset base=0x{base:08x} off=0x{offset:03x} from=cmd53-byte-current-window to=cmd53-byte-rewindow err={err}"
                ));
                self.recover_command_path("core-ctrl-postreset-cmd53-rewindow");
                match self.core_ctrl_postreset_current_window_write8(base, offset, value) {
                    Ok(()) => Ok(()),
                    Err(fallback_err) => {
                        emit_breadcrumb(format_args!(
                            "[pi4-wifi] firmware core-ctrl fallback op=write8-postreset base=0x{base:08x} off=0x{offset:03x} to=cmd53-byte-rewindow err={fallback_err}"
                        ));
                        Err(fallback_err)
                    }
                }
            }
        }
    }

    fn core_ctrl_write8_in_reset(
        &mut self,
        base: u32,
        offset: u32,
        value: u8,
    ) -> Result<(), HalError> {
        if core_ctrl_in_reset_write_uses_word_path(base, offset) {
            match self.core_ctrl_in_reset_write32(base, offset, value) {
                Ok(()) => return Ok(()),
                Err(err) => {
                    emit_breadcrumb(format_args!(
                        "[pi4-wifi] firmware core-ctrl fallback op=write8-in-reset base=0x{base:08x} off=0x{offset:03x} from=cmd53-word-windowed to=cmd52-byte-current-window err={err}"
                    ));
                    self.recover_command_path("core-ctrl-in-reset-cmd52-current-window");
                    match self.core_ctrl_windowed_write8(base, offset, value) {
                        Ok(()) => return Ok(()),
                        Err(current_window_err) => {
                            emit_breadcrumb(format_args!(
                                "[pi4-wifi] firmware core-ctrl fallback op=write8-in-reset base=0x{base:08x} off=0x{offset:03x} to=cmd52-byte-rewindow err={current_window_err}"
                            ));
                            self.recover_command_path("core-ctrl-in-reset-cmd52-rewindow");
                            match self.core_ctrl_fallback_write8_cmd52_rewindow(base, offset, value)
                            {
                                Ok(()) => return Ok(()),
                                Err(fallback_err) => {
                                    emit_breadcrumb(format_args!(
                                        "[pi4-wifi] firmware core-ctrl fallback op=write8-in-reset base=0x{base:08x} off=0x{offset:03x} to=cmd52-byte-rewindow err={fallback_err}"
                                    ));
                                    return Err(fallback_err);
                                }
                            }
                        }
                    }
                }
            }
        }
        match self.core_ctrl_windowed_write8(base, offset, value) {
            Ok(()) => Ok(()),
            Err(err) => {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware core-ctrl fallback op=write8-in-reset base=0x{base:08x} off=0x{offset:03x} from=cmd52-byte-current-window to=cmd52-byte-rewindow err={err}"
                ));
                self.recover_command_path("core-ctrl-in-reset-cmd52-rewindow");
                match self.core_ctrl_fallback_write8_cmd52_rewindow(base, offset, value) {
                    Ok(()) => Ok(()),
                    Err(fallback_err) => {
                        emit_breadcrumb(format_args!(
                            "[pi4-wifi] firmware core-ctrl fallback op=write8-in-reset base=0x{base:08x} off=0x{offset:03x} to=cmd52-byte-rewindow err={fallback_err}"
                        ));
                        Err(fallback_err)
                    }
                }
            }
        }
    }

    fn core_ctrl_read8_logged(
        &mut self,
        base: u32,
        offset: u32,
        stage: &'static str,
    ) -> Result<u8, HalError> {
        log_core_ctrl_access("read8", base, offset, None);
        match self.core_ctrl_read8(base, offset) {
            Ok(value) => Ok(value),
            Err(err) => {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware core-ctrl access stage={stage} op=read8 err={err} base=0x{base:08x} off=0x{offset:03x}"
                ));
                self.log_transport_shadow(stage);
                return Err(err);
            }
        }
    }

    fn core_ctrl_postreset_read8_logged(
        &mut self,
        base: u32,
        offset: u32,
        stage: &'static str,
    ) -> Result<u8, HalError> {
        log_core_ctrl_postreset_read(base, offset);
        match self.core_ctrl_postreset_read8(base, offset) {
            Ok(value) => Ok(value),
            Err(err) => {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware core-ctrl access stage={stage} op=read8 err={err} base=0x{base:08x} off=0x{offset:03x}"
                ));
                self.log_transport_shadow(stage);
                Err(err)
            }
        }
    }

    fn core_ctrl_write8_logged(
        &mut self,
        base: u32,
        offset: u32,
        value: u8,
        stage: &'static str,
    ) -> Result<(), HalError> {
        log_core_ctrl_access("write8", base, offset, Some(value));
        if let Err(err) = self.core_ctrl_write8(base, offset, value) {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware core-ctrl access stage={stage} op=write8 err={err} base=0x{base:08x} off=0x{offset:03x}"
            ));
            self.log_transport_shadow(stage);
            return Err(err);
        }
        Ok(())
    }

    fn core_ctrl_reset_assert_write8_logged(
        &mut self,
        base: u32,
        offset: u32,
        value: u8,
        stage: &'static str,
    ) -> Result<(), HalError> {
        log_core_ctrl_reset_write(
            base,
            offset,
            value,
            core_ctrl_reset_assert_access_mode_label(),
        );
        if let Err(err) = self.core_ctrl_reset_assert_write8(base, offset, value) {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware core-ctrl access stage={stage} op=write8 err={err} base=0x{base:08x} off=0x{offset:03x}"
            ));
            self.log_transport_shadow(stage);
            return Err(err);
        }
        Ok(())
    }

    fn core_ctrl_reset_clear_write8_logged(
        &mut self,
        base: u32,
        offset: u32,
        value: u8,
        stage: &'static str,
    ) -> Result<(), HalError> {
        log_core_ctrl_reset_write(
            base,
            offset,
            value,
            core_ctrl_reset_clear_access_mode_label(),
        );
        if let Err(err) = self.core_ctrl_reset_clear_write8(base, offset, value) {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware core-ctrl access stage={stage} op=write8 err={err} base=0x{base:08x} off=0x{offset:03x}"
            ));
            self.log_transport_shadow(stage);
            return Err(err);
        }
        Ok(())
    }

    fn core_ctrl_reset_clear_retry_current_window_write8_logged(
        &mut self,
        base: u32,
        offset: u32,
        value: u8,
        stage: &'static str,
    ) -> Result<(), HalError> {
        log_core_ctrl_reset_write(
            base,
            offset,
            value,
            core_ctrl_reset_clear_retry_access_mode_label(),
        );
        if let Err(err) =
            self.core_ctrl_reset_clear_retry_current_window_write8(base, offset, value)
        {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware core-ctrl access stage={stage} op=write8 err={err} base=0x{base:08x} off=0x{offset:03x}"
            ));
            self.log_transport_shadow(stage);
            return Err(err);
        }
        Ok(())
    }

    fn core_ctrl_postreset_write8_logged(
        &mut self,
        base: u32,
        offset: u32,
        value: u8,
        stage: &'static str,
    ) -> Result<(), HalError> {
        log_core_ctrl_postreset_write(base, offset, value);
        if let Err(err) = self.core_ctrl_postreset_write8(base, offset, value) {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware core-ctrl access stage={stage} op=write8 err={err} base=0x{base:08x} off=0x{offset:03x}"
            ));
            self.log_transport_shadow(stage);
            return Err(err);
        }
        Ok(())
    }

    fn core_ctrl_write8_in_reset_logged(
        &mut self,
        base: u32,
        offset: u32,
        value: u8,
        stage: &'static str,
    ) -> Result<(), HalError> {
        log_core_ctrl_in_reset_write(base, offset, value);
        if let Err(err) = self.core_ctrl_write8_in_reset(base, offset, value) {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware core-ctrl access stage={stage} op=write8 err={err} base=0x{base:08x} off=0x{offset:03x}"
            ));
            self.log_transport_shadow(stage);
            return Err(err);
        }
        Ok(())
    }

    fn enable_ht_clock_assist_for(&mut self, stage: &'static str) -> Result<(), HalError> {
        let mut wake_ctrl =
            self.io_direct_read(SdioFunction::Function1, SBSDIO_FUNC1_WAKEUPCTRL)?;
        wake_ctrl |= SBSDIO_WAKE_TILL_HT_AVAIL;
        self.io_direct_write(SdioFunction::Function1, SBSDIO_FUNC1_WAKEUPCTRL, wake_ctrl)?;
        self.remember_wakeupctrl(wake_ctrl);

        let mut cardcap = self.io_direct_read(SdioFunction::Function0, SDIO_CCCR_BRCM_CARDCAP)?;
        cardcap |= SDIO_CCCR_BRCM_CARDCAP_CMD_NODEC;
        self.io_direct_write(SdioFunction::Function0, SDIO_CCCR_BRCM_CARDCAP, cardcap)?;
        self.remember_cardcap(cardcap);

        self.io_direct_write(
            SdioFunction::Function1,
            SBSDIO_FUNC1_CHIPCLKCSR,
            SBSDIO_FORCE_HT,
        )?;
        self.remember_chipclkcsr(SBSDIO_FORCE_HT);

        let mut sleep_csr = self.io_direct_read(SdioFunction::Function1, SBSDIO_FUNC1_SLEEPCSR)?;
        sleep_csr |= SBSDIO_FUNC1_SLEEPCSR_KSO_EN;
        self.io_direct_write(SdioFunction::Function1, SBSDIO_FUNC1_SLEEPCSR, sleep_csr)?;
        self.remember_sleepcsr(sleep_csr);

        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware stage={stage} wake=0x{wake_ctrl:02x} cardcap=0x{cardcap:02x} sleep=0x{sleep_csr:02x}"
        ));
        Ok(())
    }

    fn refresh_ht_clock_assist_from_shadow_for(
        &mut self,
        stage: &'static str,
    ) -> Result<(), HalError> {
        if !ht_clock_assist_shadow_is_complete(
            self.last_wakeupctrl,
            self.last_sleepcsr,
            self.last_cardcap,
        ) {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware stage={stage} refresh=full reason=shadow-miss"
            ));
            return self.enable_ht_clock_assist_for(stage);
        }

        let wake_ctrl = self.last_wakeupctrl.unwrap_or(0) | SBSDIO_WAKE_TILL_HT_AVAIL;
        let cardcap = self.last_cardcap.unwrap_or(0) | SDIO_CCCR_BRCM_CARDCAP_CMD_NODEC;
        let sleep_csr = self.last_sleepcsr.unwrap_or(0) | SBSDIO_FUNC1_SLEEPCSR_KSO_EN;
        let chipclk = transport_phase_chipclk_value(self.last_chipclkcsr);

        self.io_direct_write(SdioFunction::Function1, SBSDIO_FUNC1_WAKEUPCTRL, wake_ctrl)?;
        self.remember_wakeupctrl(wake_ctrl);
        self.io_direct_write(SdioFunction::Function0, SDIO_CCCR_BRCM_CARDCAP, cardcap)?;
        self.remember_cardcap(cardcap);
        self.io_direct_write(SdioFunction::Function1, SBSDIO_FUNC1_CHIPCLKCSR, chipclk)?;
        self.remember_chipclkcsr(chipclk);
        self.io_direct_write(SdioFunction::Function1, SBSDIO_FUNC1_SLEEPCSR, sleep_csr)?;
        self.remember_sleepcsr(sleep_csr);

        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware stage={stage} refresh=shadow wake=0x{wake_ctrl:02x} cardcap=0x{cardcap:02x} sleep=0x{sleep_csr:02x} chipclk=0x{chipclk:02x}"
        ));
        Ok(())
    }

    fn refresh_transport_phase_for(&mut self, stage: &'static str) -> Result<(), HalError> {
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware stage={stage} action=phase-ht-assist"
        ));
        self.refresh_ht_clock_assist_from_shadow_for(stage)
    }

    fn settle_with_ht_clock_assist(
        &mut self,
        stage: &'static str,
        total_loops: usize,
    ) -> Result<(), HalError> {
        if total_loops == 0 {
            return Ok(());
        }

        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware stage={stage} loops={total_loops} chunk_loops={chunk_loops} mode=ht-keepalive",
            chunk_loops = CYW43_SOCRAM_CLEAR_RESET_KEEPALIVE_CHUNK_LOOPS,
        ));
        self.refresh_ht_clock_assist_from_shadow_for(stage)?;

        let mut remaining_loops = total_loops;
        let mut refresh_index = 0usize;
        while remaining_loops > 0 {
            let chunk_loops = clear_reset_keepalive_chunk_loops(remaining_loops);
            spin_settle(chunk_loops);
            remaining_loops -= chunk_loops;
            if remaining_loops == 0 {
                break;
            }

            refresh_index = refresh_index.saturating_add(1);
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware stage={stage} refresh-index={refresh_index} remaining_loops={remaining_loops}"
            ));
            self.refresh_ht_clock_assist_from_shadow_for(stage)?;
        }
        Ok(())
    }

    fn core_disable(&mut self, base: u32, prereset: u8, reset: u8) -> Result<(), HalError> {
        let ioctrl = self.core_ctrl_read8(base, AI_IOCTRL_OFFSET)?;
        let resetctrl = self.core_ctrl_read8(base, AI_RESETCTRL_OFFSET)?;
        let upstream_socram_disable =
            core_disable_uses_upstream_socram_disable(base, prereset, reset);
        let prereset_stage = if upstream_socram_disable {
            "prereset-zero-ioctrl"
        } else {
            "prereset-fgc-clock"
        };
        let prereset_ioctrl = if upstream_socram_disable {
            0
        } else {
            prereset | AI_CORE_PRERESET_IOCTRL
        };
        let mut asserted_reset = false;
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware core-disable base=0x{base:08x} stage=entry io=0x{ioctrl:02x} reset=0x{resetctrl:02x} prereset=0x{prereset:02x} hold=0x{reset:02x} reason={}",
            ai_core_state_reason(ioctrl, resetctrl),
        ));
        if (resetctrl & AI_RESETCTRL_BIT_RESET) == 0 {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware core-disable base=0x{base:08x} stage={prereset_stage} value=0x{prereset_ioctrl:02x}"
            ));
            self.core_ctrl_write8_logged(base, AI_IOCTRL_OFFSET, prereset_ioctrl, prereset_stage)?;
            bounded_spin_settle(
                "cyw43-core-prereset-fgc-clock",
                CYW43_CORE_CONTROL_SETTLE_LOOPS,
            );
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware core-disable base=0x{base:08x} stage=assert-reset value=0x{value:02x}",
                value = AI_RESETCTRL_BIT_RESET,
            ));
            self.core_ctrl_reset_assert_write8_logged(
                base,
                AI_RESETCTRL_OFFSET,
                AI_RESETCTRL_BIT_RESET,
                "assert-reset",
            )?;
            asserted_reset = true;
            if upstream_socram_disable {
                bounded_spin_settle("cyw43-core-reset-assert", CYW43_CORE_CONTROL_SETTLE_LOOPS);
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware core-disable base=0x{base:08x} stage=assert-reset-settled detail=upstream-socram-disable-deferred"
                ));
                self.log_transport_shadow("core-disable-assert-reset");
                return Ok(());
            } else {
                bounded_spin_settle("cyw43-core-reset-assert", CYW43_CORE_CONTROL_SETTLE_LOOPS);
            }
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware core-disable base=0x{base:08x} stage=assert-reset-settled detail=readback-deferred"
            ));
            self.log_transport_shadow("core-disable-assert-reset");
        } else {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware core-disable base=0x{base:08x} stage=already-reset io=0x{ioctrl:02x} reset=0x{resetctrl:02x} prereset=0x{prereset:02x} hold=0x{reset:02x} reason={}",
                ai_core_state_reason(ioctrl, resetctrl),
            ));
        }
        let reset_hold_ioctrl = core_reset_prepare_hold_value(reset);
        let skipped_in_reset_write = core_ctrl_can_skip_redundant_in_reset_write(
            base,
            asserted_reset,
            prereset_ioctrl,
            reset_hold_ioctrl,
        );
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware core-disable base=0x{base:08x} stage=in-reset-configure value=0x{reset_hold_ioctrl:02x}"
        ));
        if skipped_in_reset_write {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware core-disable base=0x{base:08x} stage=in-reset-configure-skip value=0x{reset_hold_ioctrl:02x} prior=0x{prereset_ioctrl:02x} reason=redundant-after-assert"
            ));
        } else {
            self.core_ctrl_write8_in_reset_logged(
                base,
                AI_IOCTRL_OFFSET,
                reset_hold_ioctrl,
                "in-reset-configure",
            )?;
        }
        bounded_spin_settle(
            "cyw43-core-in-reset-configure",
            CYW43_CORE_CONTROL_SETTLE_LOOPS,
        );
        if core_ctrl_can_defer_in_reset_readback(base, asserted_reset, skipped_in_reset_write) {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware core-disable base=0x{base:08x} stage=in-reset-ready-read-deferred io=0x{reset_hold_ioctrl:02x} reset=0x{assumed_reset:02x} reason=redundant-after-assert",
                assumed_reset = AI_RESETCTRL_BIT_RESET,
            ));
            self.log_transport_shadow("core-disable-in-reset-read-deferred");
            return Ok(());
        }
        let resetctrl =
            self.core_ctrl_read8_logged(base, AI_RESETCTRL_OFFSET, "in-reset-configure")?;
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware core-disable base=0x{base:08x} stage=in-reset-ready io=0x{reset_hold_ioctrl:02x} reset=0x{resetctrl:02x} reason={}",
            ai_core_state_reason(reset_hold_ioctrl, resetctrl),
        ));
        self.log_transport_shadow("core-disable-in-reset");
        Ok(())
    }

    fn core_reset(
        &mut self,
        base: u32,
        prereset: u8,
        reset: u8,
        postreset: u8,
    ) -> Result<(), HalError> {
        let skipped_disable = core_reset_can_skip_disable(base, prereset, reset, postreset);
        if skipped_disable {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware core-reset base=0x{base:08x} stage=skip-disable reason=held-reset-from-prior-disable"
            ));
            self.log_transport_shadow("core-reset-skip-disable");
        } else {
            self.core_disable(base, prereset, reset)?;
        }
        let reset_hold_ioctrl = core_reset_prepare_hold_value(reset);
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware core-reset base=0x{base:08x} stage=pre-clear-in-reset-configure value=0x{reset_hold_ioctrl:02x} reason=required-before-clear-reset"
        ));
        if core_reset_can_skip_pre_clear_in_reset_write(base, skipped_disable) {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware core-reset base=0x{base:08x} stage=pre-clear-in-reset-configure-skip value=0x{reset_hold_ioctrl:02x} reason=redundant-held-reset-from-prior-disable"
            ));
        } else {
            self.core_ctrl_write8_in_reset_logged(
                base,
                AI_IOCTRL_OFFSET,
                reset_hold_ioctrl,
                "pre-clear-in-reset-configure",
            )?;
        }
        if core_reset_needs_clear_reset_ht_assist(base) {
            self.settle_with_ht_clock_assist(
                "pre-clear-in-reset-keepalive",
                CYW43_CORE_CONTROL_SETTLE_LOOPS,
            )?;
        } else {
            bounded_spin_settle(
                "cyw43-core-pre-clear-in-reset-configure",
                CYW43_CORE_CONTROL_SETTLE_LOOPS,
            );
        }
        if core_reset_can_skip_pre_clear_in_reset_write(base, skipped_disable) {
            self.log_transport_shadow("core-reset-pre-clear-in-reset-skip");
        } else {
            self.log_transport_shadow("core-reset-pre-clear-in-reset");
        }
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware core-reset base=0x{base:08x} stage=clear-reset prereset=0x{prereset:02x} hold=0x{reset:02x} postreset=0x{postreset:02x}"
        ));
        let mut last_reset = AI_RESETCTRL_BIT_RESET;
        let mut cleared = false;
        let mut attempts = 0usize;
        for attempt in 0..CYW43_CORE_RESET_RETRY_LIMIT {
            attempts = attempt.saturating_add(1);
            if core_reset_needs_clear_reset_prewrite_settle(base, attempt) {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware core-reset base=0x{base:08x} stage=clear-reset-prewrite-delay loops={loops} reason=socram-fragile-first-write mode=ht-keepalive",
                    loops = CYW43_SOCRAM_CLEAR_RESET_PREWRITE_SETTLE_LOOPS,
                ));
                self.settle_with_ht_clock_assist(
                    "clear-reset-prewrite-keepalive",
                    CYW43_SOCRAM_CLEAR_RESET_PREWRITE_SETTLE_LOOPS,
                )?;
            }
            if core_reset_needs_clear_reset_ht_assist(base) {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware core-reset base=0x{base:08x} stage=clear-reset-ht-assist attempt={} reason=socram-fragile-prewrite",
                    attempt.saturating_add(1),
                ));
                self.refresh_ht_clock_assist_from_shadow_for("clear-reset-ht-assist")?;
            }
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware core-reset base=0x{base:08x} stage=clear-reset-primary attempt=1 path=cmd53-word-windowed value=0x00"
            ));
            if let Err(err) = self.core_ctrl_reset_clear_write8_logged(
                base,
                AI_RESETCTRL_OFFSET,
                0,
                "clear-reset-primary",
            ) {
                if !core_reset_can_retry_clear_reset_write(base, attempt) {
                    return Err(err);
                }
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware core-reset base=0x{base:08x} stage=clear-reset-write-retry attempt={} err={} reason=socram-fragile-first-write",
                    attempt.saturating_add(1),
                    err,
                ));
                if core_reset_clear_preserves_window_cache() {
                    self.recover_command_path_preserve_window("core-reset-clear-retry");
                } else {
                    self.recover_command_path("core-reset-clear-retry");
                }
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware core-reset base=0x{base:08x} stage=clear-reset-write-retry-delay loops={loops} reason=socram-release-edge-timeout mode=ht-keepalive",
                    loops = CYW43_SOCRAM_CLEAR_RESET_PREWRITE_SETTLE_LOOPS,
                ));
                self.settle_with_ht_clock_assist(
                    "clear-reset-retry-keepalive",
                    CYW43_SOCRAM_CLEAR_RESET_PREWRITE_SETTLE_LOOPS,
                )?;
                if core_reset_needs_clear_reset_ht_assist(base) {
                    emit_breadcrumb(format_args!(
                        "[pi4-wifi] firmware core-reset base=0x{base:08x} stage=clear-reset-retry-ht-assist attempt={} reason=socram-release-edge-prewrite",
                        attempt.saturating_add(2),
                    ));
                    self.refresh_ht_clock_assist_from_shadow_for("clear-reset-retry-ht-assist")?;
                }
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware core-reset base=0x{base:08x} stage=clear-reset-retry attempt=2 path=cmd52-byte-current-window value=0x00 cache=preserved"
                ));
                if let Err(retry_err) = self
                    .core_ctrl_reset_clear_retry_current_window_write8_logged(
                        base,
                        AI_RESETCTRL_OFFSET,
                        0,
                        "clear-reset-retry",
                    )
                {
                    if !core_reset_can_assume_clear_reset_retry_commit(
                        base,
                        AI_RESETCTRL_OFFSET,
                        attempt,
                        &retry_err,
                    ) {
                        return Err(retry_err);
                    }
                    emit_breadcrumb(format_args!(
                        "[pi4-wifi] firmware core-reset base=0x{base:08x} stage=clear-reset-retry-assumed-committed attempt=2 err={retry_err} reason=socram-release-edge-timeout"
                    ));
                    self.restore_window_cache_from_shadow(
                        "core-reset-clear-retry-assumed-committed",
                    );
                }
            }
            if attempt == 0 {
                bounded_spin_settle("cyw43-core-reset-clear", CYW43_CORE_CONTROL_SETTLE_LOOPS);
            } else {
                spin_settle(CYW43_CORE_CONTROL_SETTLE_LOOPS);
            }
            if core_ctrl_can_defer_clear_reset_readback(base, attempt) {
                last_reset = 0;
                cleared = true;
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware core-reset base=0x{base:08x} stage=clear-reset-read-deferred reset=0x{last_reset:02x} attempts={attempts} reason=socram-fragile-first-read"
                ));
                self.log_transport_shadow("core-reset-clear-read-deferred");
                break;
            }
            last_reset = self.core_ctrl_read8_logged(base, AI_RESETCTRL_OFFSET, "clear-reset")?;
            if (last_reset & AI_RESETCTRL_BIT_RESET) == 0 {
                cleared = true;
                break;
            }
        }
        if !cleared {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware core-reset base=0x{base:08x} stage=clear-reset-timeout reset=0x{last_reset:02x} attempts={attempts}"
            ));
            return Err(HalError::Unsupported("cyw43-core-reset-clear-timeout"));
        }
        let postreset_ioctrl = postreset | AI_CORE_POSTRESET_IOCTRL;
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware core-reset base=0x{base:08x} stage=clear-reset-ready reset=0x{last_reset:02x} attempts={attempts}"
        ));
        self.log_transport_shadow("core-reset-clear");
        if core_reset_needs_postreset_window_reprime(base) {
            self.invalidate_programmed_backplane_window("core-reset-postreset-window-reprime");
        }
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware core-reset base=0x{base:08x} stage=postreset-clock-en-write value=0x{postreset_ioctrl:02x}"
        ));
        if let Err(err) = self.core_ctrl_postreset_write8_logged(
            base,
            AI_IOCTRL_OFFSET,
            postreset_ioctrl,
            "postreset-clock-en-write",
        ) {
            if !core_reset_can_assume_postreset_clock_en_commit(base, AI_IOCTRL_OFFSET, &err) {
                return Err(err);
            }
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware core-reset base=0x{base:08x} stage=postreset-clock-en-assumed-committed err={err} reason=socram-postreset-write-timeout"
            ));
            self.restore_window_cache_from_shadow(
                "core-reset-postreset-clock-en-assumed-committed",
            );
        } else {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware core-reset base=0x{base:08x} stage=postreset-clock-en-write-ok value=0x{postreset_ioctrl:02x}"
            ));
        }
        bounded_spin_settle(
            "cyw43-core-postreset-clock-en",
            CYW43_CORE_CONTROL_SETTLE_LOOPS,
        );
        if core_reset_can_skip_postreset_verify(base) {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware core-reset base=0x{base:08x} stage=postreset-verify-deferred io=0x{postreset_ioctrl:02x} reset=0x{last_reset:02x} reason=upstream-socram-postreset-readback-advisory"
            ));
            self.restore_window_cache_from_shadow("core-reset-postreset-verify-deferred");
            emit_breadcrumb(format_args!(
                "[pi4-wifi] firmware core-reset base=0x{base:08x} stage=verify io=0x{postreset_ioctrl:02x} reset=0x{last_reset:02x} reason={}",
                ai_core_state_reason(postreset_ioctrl, last_reset),
            ));
            self.log_transport_shadow("core-reset-verify");
            return Ok(());
        }
        let ioctrl = match self.core_ctrl_postreset_read8_logged(
            base,
            AI_IOCTRL_OFFSET,
            "postreset-clock-en-readback",
        ) {
            Ok(value) => value,
            Err(err) => {
                if !core_reset_can_defer_postreset_clock_en_readback(base, AI_IOCTRL_OFFSET, &err) {
                    return Err(err);
                }
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware core-reset base=0x{base:08x} stage=postreset-clock-en-read-deferred io=0x{postreset_ioctrl:02x} err={err} reason={}",
                    core_reset_postreset_clock_en_read_reason(base)
                ));
                self.restore_window_cache_from_shadow(
                    "core-reset-postreset-clock-en-read-deferred",
                );
                postreset_ioctrl
            }
        };
        let resetctrl = match self.core_ctrl_postreset_read8_logged(
            base,
            AI_RESETCTRL_OFFSET,
            "postreset-clock-en-readback",
        ) {
            Ok(value) => value,
            Err(err) => {
                if !core_reset_can_defer_postreset_reset_readback(base, AI_RESETCTRL_OFFSET, &err) {
                    return Err(err);
                }
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] firmware core-reset base=0x{base:08x} stage=postreset-reset-read-deferred reset=0x{last_reset:02x} err={err} reason={}",
                    core_reset_postreset_reset_read_reason(base)
                ));
                self.restore_window_cache_from_shadow("core-reset-postreset-reset-read-deferred");
                last_reset
            }
        };
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware core-reset base=0x{base:08x} stage=verify io=0x{ioctrl:02x} reset=0x{resetctrl:02x} reason={}",
            ai_core_state_reason(ioctrl, resetctrl),
        ));
        self.log_transport_shadow("core-reset-verify");
        Ok(())
    }

    fn apply_host_bus_width(&mut self, width: SdioBusWidth) {
        let mut control = self.read8(SDHCI_HOST_CONTROL);
        control &= !SDHCI_CTRL_4BITBUS;
        if matches!(width, SdioBusWidth::FourBit) {
            control |= SDHCI_CTRL_4BITBUS;
        }
        self.write8(SDHCI_HOST_CONTROL, control);
    }

    fn compute_divider(&self, target_hz: u32, version: u16) -> u16 {
        if version >= SDHCI_SPEC_300 {
            if self.base_clock_hz <= target_hz {
                1
            } else {
                let mut div = 2u16;
                while div < 2046 && (self.base_clock_hz / u32::from(div)) > target_hz {
                    div = div.saturating_add(2);
                }
                div
            }
        } else {
            let mut div = 1u16;
            while div < 256 && (self.base_clock_hz / u32::from(div)) > target_hz {
                div = div.saturating_mul(2);
            }
            div
        }
    }

    fn wait_for_int_clock_stable(&self) -> Result<(), HalError> {
        for _ in 0..SDIO_CLOCK_STABLE_LOOPS {
            if (self.read16(SDHCI_CLOCK_CONTROL) & SDHCI_CLOCK_INT_STABLE) != 0 {
                return Ok(());
            }
            spin_loop();
        }
        Err(HalError::Unsupported("sdio-clock-stable-timeout"))
    }

    fn software_reset(&mut self, mask: u8) -> Result<(), HalError> {
        self.block_size_count_shadow = 0;
        self.transfer_mode_shadow = 0;
        self.write8(SDHCI_SOFTWARE_RESET, mask);
        for _ in 0..SDIO_HOST_RESET_LOOPS {
            if (self.read8(SDHCI_SOFTWARE_RESET) & mask) == 0 {
                return Ok(());
            }
            spin_loop();
        }
        Err(HalError::Unsupported("sdhci-reset-timeout"))
    }

    fn recover_command_path(&mut self, stage: &'static str) {
        self.software_reset(SDHCI_RESET_CMD | SDHCI_RESET_DATA).ok();
        self.write32(SDHCI_INT_STATUS, SDHCI_INT_ALL_MASK);
        self.programmed_backplane_window = None;
        emit_breadcrumb(format_args!(
            "[pi4-wifi] sdhci recover stage={stage} mask=cmd+data"
        ));
        self.log_transport_shadow(stage);
    }

    fn recover_command_path_and_refresh_transport(
        &mut self,
        stage: &'static str,
    ) -> Result<(), HalError> {
        self.recover_command_path(stage);
        self.refresh_transport_phase_for(stage)
    }

    fn recover_command_path_preserve_window(&mut self, stage: &'static str) {
        self.software_reset(SDHCI_RESET_CMD | SDHCI_RESET_DATA).ok();
        self.write32(SDHCI_INT_STATUS, SDHCI_INT_ALL_MASK);
        self.programmed_backplane_window =
            restore_programmed_backplane_window(self.last_backplane_window);
        let restored_window = self.programmed_backplane_window.unwrap_or(0);
        let shadow_window = self.last_backplane_window.unwrap_or(0);
        let function = self.last_backplane_function_addr.unwrap_or(0);
        emit_breadcrumb(format_args!(
            "[pi4-wifi] sdhci recover stage={stage} mask=cmd+data cache=preserved restored_window=0x{restored_window:08x} shadow_window=0x{shadow_window:08x} fn=0x{function:05x}"
        ));
        self.log_transport_shadow(stage);
    }

    fn recover_command_path_preserve_window_and_refresh_transport(
        &mut self,
        stage: &'static str,
    ) -> Result<(), HalError> {
        self.recover_command_path_preserve_window(stage);
        self.refresh_transport_phase_for(stage)
    }

    fn restore_window_cache_from_shadow(&mut self, stage: &'static str) {
        self.programmed_backplane_window =
            restore_programmed_backplane_window(self.last_backplane_window);
        let restored_window = self.programmed_backplane_window.unwrap_or(0);
        let shadow_window = self.last_backplane_window.unwrap_or(0);
        let function = self.last_backplane_function_addr.unwrap_or(0);
        emit_breadcrumb(format_args!(
            "[pi4-wifi] sdhci recover stage={stage} cache=restored restored_window=0x{restored_window:08x} shadow_window=0x{shadow_window:08x} fn=0x{function:05x}"
        ));
        self.log_transport_shadow(stage);
    }

    fn restore_window_cache_from_shadow_and_refresh_transport(
        &mut self,
        stage: &'static str,
    ) -> Result<(), HalError> {
        self.restore_window_cache_from_shadow(stage);
        self.refresh_transport_phase_for(stage)
    }

    fn wait_inhibit_clear(&mut self, wait_data: bool) -> Result<(), HalError> {
        let mask = if wait_data {
            SDHCI_CMD_INHIBIT | SDHCI_DATA_INHIBIT
        } else {
            SDHCI_CMD_INHIBIT
        };
        for _ in 0..SDIO_CMD_WAIT_LOOPS {
            if (self.read32(SDHCI_PRESENT_STATE) & mask) == 0 {
                return Ok(());
            }
            spin_loop();
        }
        emit_breadcrumb(format_args!(
            "[pi4-wifi] sdhci inhibit timeout wait_data={} mask=0x{mask:08x}",
            if wait_data { "yes" } else { "no" }
        ));
        self.log_host_state("inhibit-timeout");
        Err(HalError::Unsupported("sdhci-inhibit-timeout"))
    }

    fn send_command(
        &mut self,
        cmd: u16,
        arg: u32,
        response: ResponseType,
    ) -> Result<[u32; 4], HalError> {
        self.wait_inhibit_clear(matches!(response, ResponseType::ShortBusy))?;
        self.write32(SDHCI_INT_STATUS, SDHCI_INT_ALL_MASK);
        self.write32(SDHCI_ARGUMENT, arg);
        self.write16(SDHCI_TRANSFER_MODE, 0);
        self.write16(SDHCI_COMMAND, make_command(cmd, response, false));

        let status = match self.wait_int(SDHCI_INT_CMD_MASK) {
            Ok(status) => status,
            Err(err) => {
                self.log_command_state("wait", cmd, arg, 0);
                self.recover_command_path("cmd-wait");
                return Err(err);
            }
        };
        if (status & SDHCI_INT_ERROR) != 0 {
            self.log_command_state("error", cmd, arg, status);
            self.recover_command_path("cmd-error");
            return Err(HalError::Unsupported("sdhci-command-error"));
        }

        let mut resp = [0u32; 4];
        match response {
            ResponseType::None => {}
            ResponseType::Long => {
                for (index, slot) in resp.iter_mut().enumerate() {
                    *slot = self.read32(SDHCI_RESPONSE + index * 4);
                }
            }
            ResponseType::Ocr | ResponseType::Short | ResponseType::ShortBusy => {
                resp[0] = self.read32(SDHCI_RESPONSE);
            }
        }
        if matches!(response, ResponseType::ShortBusy) {
            self.wait_inhibit_clear(true)?;
        }
        Ok(resp)
    }

    fn transfer_command(
        &mut self,
        cmd: u16,
        arg: u32,
        buffer: &mut [u8],
        write: bool,
        plan: SdioTransferPlan,
        quiet_settle: bool,
    ) -> Result<(), HalError> {
        self.wait_inhibit_clear(true)?;
        self.write32(SDHCI_INT_STATUS, SDHCI_INT_ALL_MASK);
        self.write16(SDHCI_BLOCK_SIZE, plan.block_size);
        self.write16(SDHCI_BLOCK_COUNT, plan.block_count);
        self.write32(SDHCI_ARGUMENT, arg);
        self.write16(SDHCI_TRANSFER_MODE, plan.transfer_mode);
        self.write16(SDHCI_COMMAND, make_command(cmd, ResponseType::Short, true));

        let cmd_status = match self.wait_int(SDHCI_INT_CMD_MASK) {
            Ok(status) => status,
            Err(err) => {
                log_sdio_cmd53_shape("command-wait", cmd, arg, buffer.len(), plan);
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] sdhci xfer error cmd={cmd} arg=0x{arg:08x} len={} phase=command-wait err={err}",
                    buffer.len(),
                ));
                self.log_host_state("xfer-command-wait");
                self.recover_command_path("cmd-wait");
                return Err(err);
            }
        };
        if (cmd_status & SDHCI_INT_ERROR) != 0 {
            log_sdio_cmd53_shape("command", cmd, arg, buffer.len(), plan);
            emit_breadcrumb(format_args!(
                "[pi4-wifi] sdhci xfer error cmd={cmd} arg=0x{arg:08x} len={} phase=command st=0x{cmd_status:08x} why={}",
                buffer.len(),
                sdhci_status_reason(cmd_status)
            ));
            self.log_host_state("xfer-command-fail");
            self.recover_command_path("cmd-error");
            return Err(HalError::Unsupported("sdhci-transfer-command"));
        }

        let mut offset = 0usize;
        let wait_mask = sdhci_interrupt_buffer_ready_mask(write);
        let present_ready_mask = sdhci_present_buffer_ready_mask(write);
        while offset < buffer.len() {
            if (self.read32(SDHCI_PRESENT_STATE) & present_ready_mask) == 0 {
                let status = match self.wait_int(wait_mask | SDHCI_INT_ERROR) {
                    Ok(status) => status,
                    Err(err) => {
                        log_sdio_cmd53_shape("data-wait", cmd, arg, buffer.len(), plan);
                        emit_breadcrumb(format_args!(
                    "[pi4-wifi] sdhci xfer error cmd={cmd} arg=0x{arg:08x} len={} phase=data-wait err={err}",
                            buffer.len(),
                        ));
                        self.log_host_state("xfer-data-wait");
                        self.recover_command_path("data-wait");
                        return Err(err);
                    }
                };
                if (status & SDHCI_INT_ERROR) != 0 {
                    log_sdio_cmd53_shape("data", cmd, arg, buffer.len(), plan);
                    emit_breadcrumb(format_args!(
                        "[pi4-wifi] sdhci xfer error cmd={cmd} arg=0x{arg:08x} len={} phase=data st=0x{status:08x} why={}",
                        buffer.len(),
                        sdhci_status_reason(status)
                    ));
                    self.log_host_state("xfer-data-fail");
                    self.recover_command_path("data-error");
                    return Err(HalError::Unsupported("sdhci-transfer-data"));
                }
            }

            while offset < buffer.len()
                && (self.read32(SDHCI_PRESENT_STATE) & present_ready_mask) != 0
            {
                let mut word = [0u8; 4];
                let chunk_len = cmp::min(4, buffer.len() - offset);
                if write {
                    word[..chunk_len].copy_from_slice(&buffer[offset..offset + chunk_len]);
                    self.write32(SDHCI_BUFFER, u32::from_le_bytes(word));
                } else {
                    word = self.read32(SDHCI_BUFFER).to_le_bytes();
                    buffer[offset..offset + chunk_len].copy_from_slice(&word[..chunk_len]);
                }
                offset += chunk_len;
            }
        }

        let data_status = match self
            .wait_int(SDHCI_INT_DATA_END | SDHCI_INT_ERROR | SDHCI_INT_DATA_MASK)
        {
            Ok(status) => status,
            Err(err) => {
                log_sdio_cmd53_shape("finish-wait", cmd, arg, buffer.len(), plan);
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] sdhci xfer error cmd={cmd} arg=0x{arg:08x} len={} phase=finish-wait err={err}",
                        buffer.len(),
                    ));
                self.log_host_state("xfer-finish-wait");
                self.recover_command_path("finish-wait");
                return Err(err);
            }
        };
        if (data_status & SDHCI_INT_ERROR) != 0 {
            log_sdio_cmd53_shape("finish", cmd, arg, buffer.len(), plan);
            emit_breadcrumb(format_args!(
                "[pi4-wifi] sdhci xfer error cmd={cmd} arg=0x{arg:08x} len={} phase=finish st=0x{data_status:08x} why={}",
                buffer.len(),
                sdhci_status_reason(data_status)
            ));
            self.log_host_state("xfer-finish-fail");
            self.recover_command_path("finish-error");
            return Err(HalError::Unsupported("sdhci-transfer-finish"));
        }
        self.settle_transfer_data_path(cmd, arg, buffer.len(), quiet_settle)?;
        Ok(())
    }

    fn settle_transfer_data_path(
        &mut self,
        cmd: u16,
        arg: u32,
        len: usize,
        quiet: bool,
    ) -> Result<(), HalError> {
        for _ in 0..SDIO_CMD_WAIT_LOOPS {
            let present = self.read32(SDHCI_PRESENT_STATE);
            if (present & (SDHCI_DATA_INHIBIT | SDHCI_DAT_ACTIVE)) == 0 {
                return Ok(());
            }
            spin_loop();
        }
        if !quiet {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] sdhci xfer settle cmd={cmd} arg=0x{arg:08x} len={len} phase=post-data-inhibit action=data-reset"
            ));
            self.log_host_state("xfer-post-settle");
        }
        self.software_reset(SDHCI_RESET_DATA)?;
        Ok(())
    }

    fn enable_firmware_bulk_clock(
        &mut self,
        stage: &'static str,
        target_clock_hz: u32,
        restore_clock_hz: u32,
    ) -> Result<bool, HalError> {
        if target_clock_hz == 0 || restore_clock_hz == 0 || restore_clock_hz == target_clock_hz {
            return Ok(false);
        }
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware stage={stage} clock=boost request={}Hz from={}Hz",
            target_clock_hz, restore_clock_hz,
        ));
        let effective_hz = self.set_clock_hz(target_clock_hz)?;
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware stage={stage} clock=boost-ready effective={}Hz",
            effective_hz
        ));
        Ok(true)
    }

    fn restore_firmware_bulk_clock(
        &mut self,
        stage: &'static str,
        restore_clock_hz: u32,
    ) -> Result<(), HalError> {
        if restore_clock_hz == 0 || self.current_clock_hz == restore_clock_hz {
            return Ok(());
        }
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware stage={stage} clock=restore request={}Hz from={}Hz",
            restore_clock_hz, self.current_clock_hz,
        ));
        let effective_hz = self.set_clock_hz(restore_clock_hz)?;
        emit_breadcrumb(format_args!(
            "[pi4-wifi] firmware stage={stage} clock=restore-ready effective={}Hz",
            effective_hz
        ));
        Ok(())
    }

    fn with_firmware_bulk_clock<T>(
        &mut self,
        stage: &'static str,
        target_clock_hz: u32,
        f: impl FnOnce(&mut Self) -> Result<T, HalError>,
    ) -> Result<T, HalError> {
        let restore_clock_hz = self.current_clock_hz;
        let boosted = self.enable_firmware_bulk_clock(stage, target_clock_hz, restore_clock_hz)?;
        let result = f(self);
        if !boosted {
            return result;
        }

        let restore_result = self.restore_firmware_bulk_clock(stage, restore_clock_hz);
        match (result, restore_result) {
            (Ok(value), Ok(())) => Ok(value),
            (Ok(_), Err(err)) => Err(err),
            (Err(err), _) => Err(err),
        }
    }

    fn wait_int(&mut self, mask: u32) -> Result<u32, HalError> {
        for _ in 0..SDIO_DATA_WAIT_LOOPS {
            let status = self.read32(SDHCI_INT_STATUS);
            if status & mask != 0 {
                self.write32(SDHCI_INT_STATUS, status);
                return Ok(status);
            }
            spin_loop();
        }
        Err(HalError::Unsupported("sdhci-int-timeout"))
    }

    fn log_command_state(&self, stage: &'static str, cmd: u16, arg: u32, status: u32) {
        let mode = self.read16(SDHCI_TRANSFER_MODE);
        let cmd_reg = self.read16(SDHCI_COMMAND);
        let host = self.read8(SDHCI_HOST_CONTROL);
        emit_breadcrumb(format_args!(
            "[pi4-wifi] sdhci cmd {stage} cmd={cmd} arg=0x{arg:08x} st=0x{status:08x} why={}",
            sdhci_status_reason(status)
        ));
        emit_breadcrumb(format_args!(
            "[pi4-wifi] sdhci issue {stage} mode=0x{mode:04x} cmdreg=0x{cmd_reg:04x} host=0x{host:02x}",
        ));
        self.log_host_state("cmd-fail");
    }

    fn log_host_state(&self, stage: &'static str) {
        let present = self.read32(SDHCI_PRESENT_STATE);
        let power = self.read8(SDHCI_POWER_CONTROL);
        let clock = self.read16(SDHCI_CLOCK_CONTROL);
        let timeout = self.read8(SDHCI_TIMEOUT_CONTROL);
        let host = self.read8(SDHCI_HOST_CONTROL);
        let int_status = self.read32(SDHCI_INT_STATUS);
        let int_enable = self.read32(SDHCI_INT_ENABLE);
        let signal_enable = self.read32(SDHCI_SIGNAL_ENABLE);
        let caps = self.read32(SDHCI_CAPABILITIES);
        let version = self.read16(SDHCI_HOST_VERSION);
        emit_breadcrumb(format_args!(
            "[pi4-wifi] sdhci regs {stage} ps=0x{present:08x} pwr=0x{power:02x} clk=0x{clock:04x} host=0x{host:02x} to=0x{timeout:02x}",
        ));
        emit_breadcrumb(format_args!(
            "[pi4-wifi] sdhci ints {stage} stat=0x{int_status:08x} en=0x{int_enable:08x} sig=0x{signal_enable:08x}",
        ));
        emit_breadcrumb(format_args!(
            "[pi4-wifi] sdhci caps {stage} caps=0x{caps:08x} ver=0x{version:04x} hz={} width={}",
            self.current_clock_hz,
            sdio_bus_width_name(self.desired_bus_width),
        ));
        emit_breadcrumb(format_args!(
            "[pi4-wifi] sdhci flags {stage} cmdi={} dati={} datact={} card={} stable={} detect={} wp={} dat=0x{:x} iclk={} sclk={} pwron={}",
            yn((present & SDHCI_CMD_INHIBIT) != 0),
            yn((present & SDHCI_DATA_INHIBIT) != 0),
            yn((present & SDHCI_DAT_ACTIVE) != 0),
            yn((present & SDHCI_CARD_PRESENT) != 0),
            yn((present & SDHCI_CARD_STATE_STABLE) != 0),
            yn((present & SDHCI_CARD_DETECT_PIN_LEVEL) != 0),
            yn((present & SDHCI_WRITE_PROTECT) != 0),
            (present & SDHCI_DATA_LVL_MASK) >> 20,
            yn((clock & SDHCI_CLOCK_INT_STABLE) != 0),
            yn((clock & SDHCI_CLOCK_CARD_EN) != 0),
            yn((power & SDHCI_POWER_ON) != 0),
        ));
    }

    fn set_function_block_size(
        &mut self,
        function: SdioFunction,
        size: u16,
    ) -> Result<(), HalError> {
        let base = SDIO_CCCR_FBR_BASE
            .checked_mul(u32::from(function.number()))
            .ok_or(HalError::Unsupported("sdio-fbr-base"))?;
        self.io_direct_write(
            SdioFunction::Function0,
            base + SDIO_FBR_BLKSIZE,
            (size & 0xFF) as u8,
        )?;
        self.io_direct_write(
            SdioFunction::Function0,
            base + SDIO_FBR_BLKSIZE + 1,
            (size >> 8) as u8,
        )?;
        Ok(())
    }

    fn read8(&self, offset: usize) -> u8 {
        let aligned = offset & !0x3;
        let word = self.raw_read32(aligned);
        let shift = ((offset & 0x3) * 8) as u32;
        ((word >> shift) & 0xFF) as u8
    }

    fn read16(&self, offset: usize) -> u16 {
        let aligned = offset & !0x3;
        let word = self.raw_read32(aligned);
        let shift = ((offset & 0x2) * 8) as u32;
        ((word >> shift) & 0xFFFF) as u16
    }

    fn read32(&self, offset: usize) -> u32 {
        self.raw_read32(offset)
    }

    fn write8(&mut self, offset: usize, value: u8) {
        let aligned = offset & !0x3;
        let word = self.raw_read32(aligned);
        self.raw_write32(aligned, merge_u8_word(word, offset, value));
    }

    fn write16(&mut self, offset: usize, value: u16) {
        let aligned = offset & !0x3;
        let word = if offset == SDHCI_COMMAND {
            self.transfer_mode_shadow
        } else if offset == SDHCI_BLOCK_SIZE || offset == SDHCI_BLOCK_COUNT {
            self.block_size_count_shadow
        } else {
            self.raw_read32(aligned)
        };
        let new_word = merge_u16_word(word, offset, value);
        if offset == SDHCI_TRANSFER_MODE {
            self.transfer_mode_shadow = new_word;
            return;
        }
        if offset == SDHCI_BLOCK_SIZE || offset == SDHCI_BLOCK_COUNT {
            self.block_size_count_shadow = new_word;
        }
        self.raw_write32(aligned, new_word);
    }

    fn write32(&mut self, offset: usize, value: u32) {
        self.raw_write32(offset, value);
    }

    fn raw_read32(&self, offset: usize) -> u32 {
        let base = self.regs.vaddr();
        // SAFETY: `regs` is a mapped BCM2711 SDHCI window owned by the HAL, and
        // callers pass only fixed register offsets within that page.
        unsafe { ptr::read_volatile((base + offset) as *const u32) }
    }

    fn raw_write32(&mut self, offset: usize, value: u32) {
        self.wait_write_gap(offset);
        let base = self.regs.vaddr();
        // SAFETY: `regs` is a mapped BCM2711 SDHCI window owned by the HAL, and
        // callers pass only fixed register offsets within that page.
        unsafe { ptr::write_volatile((base + offset) as *mut u32, value) };
    }

    fn wait_write_gap(&self, offset: usize) {
        if offset == SDHCI_BUFFER {
            return;
        }
        for _ in 0..SDHCI_WRITE_GAP_SPIN_LOOPS {
            spin_loop();
        }
    }
}

struct MailboxRef<'a>(&'a Mailbox);

impl MailboxRef<'_> {
    fn query_clock_hz(&mut self) -> Result<u32, HalError> {
        let mut cloned = Mailbox {
            regs: self.0.regs.clone(),
            request: self.0.request.clone(),
        };
        cloned.power_on_module(POWER_DEVID_SDHCI)?;
        cloned
            .get_clock_rate(CLOCK_ID_EMMC2)
            .or_else(|_| cloned.get_clock_rate(CLOCK_ID_EMMC))
    }
}

fn map_exact<H>(
    hal: &mut H,
    candidates: &[usize],
    prefix_maps: &mut Vec<DeviceFrame>,
) -> Result<DeviceFrame, HalError>
where
    H: Hardware<Error = HalError>,
{
    for &candidate in candidates {
        if let Ok(frame) = map_device_exact(hal, candidate, prefix_maps) {
            return Ok(frame);
        }
    }
    Err(HalError::Unsupported("device-map-exact"))
}

fn map_device_exact<H>(
    hal: &mut H,
    paddr: usize,
    prefix_maps: &mut Vec<DeviceFrame>,
) -> Result<DeviceFrame, HalError>
where
    H: Hardware<Error = HalError>,
{
    let Some(coverage) = hal.device_coverage(paddr, PAGE_BITS) else {
        return Err(HalError::Unsupported("device-coverage"));
    };
    let span_bytes = coverage.limit.saturating_sub(coverage.base);
    let span_pages = cmp::max(1usize, span_bytes / PAGE_SIZE);
    let max_attempts = cmp::max(
        1usize,
        cmp::min(span_pages.saturating_add(1), MAP_EXACT_ATTEMPT_CAP),
    );

    for _ in 0..max_attempts {
        let frame = hal.map_device(paddr)?;
        let actual_paddr = page_get_address(frame.cap()).map_err(HalError::from)?;
        if actual_paddr == paddr {
            return Ok(frame);
        }
        if actual_paddr > paddr {
            return Err(HalError::Unsupported("device-map-order"));
        }
        prefix_maps.push(frame);
    }

    Err(HalError::Unsupported("device-map-exact"))
}

fn phys_to_bus(paddr: usize, alias_base: u32) -> Option<u32> {
    let phys = u32::try_from(paddr).ok()?;
    Some((phys & VC_BUS_MASK) | alias_base)
}

fn make_command(cmd: u16, response: ResponseType, data: bool) -> u16 {
    let mut flags = match response {
        ResponseType::None => SDHCI_CMD_RESP_NONE,
        ResponseType::Ocr => SDHCI_CMD_RESP_SHORT,
        ResponseType::Short => SDHCI_CMD_RESP_SHORT | SDHCI_CMD_CRC | SDHCI_CMD_INDEX,
        ResponseType::ShortBusy => SDHCI_CMD_RESP_SHORT_BUSY | SDHCI_CMD_CRC | SDHCI_CMD_INDEX,
        ResponseType::Long => SDHCI_CMD_RESP_LONG | SDHCI_CMD_CRC,
    };
    if data {
        flags |= SDHCI_CMD_DATA;
    }
    (cmd << 8) | flags
}

#[inline]
fn cmd52_argument(function: SdioFunction, addr: u32, write: bool, value: u8) -> u32 {
    ((write as u32) << 31)
        | ((function.number() as u32) << 28)
        | ((addr & 0x1_FFFF) << 9)
        | (value as u32)
}

fn sdio_transfer_plan(
    function: SdioFunction,
    len: usize,
    write: bool,
) -> Result<SdioTransferPlan, HalError> {
    let mut transfer_mode = 0u16;
    if !write {
        transfer_mode |= SDHCI_TRNS_READ;
    }
    if let Some(block_size) = sdio_function_block_size(function) {
        let block_len = usize::from(block_size);
        if len >= block_len && len % block_len == 0 {
            let block_count = u16::try_from(len / block_len)
                .map_err(|_| HalError::Unsupported("sdhci-block-count"))?;
            if block_count != 0 {
                transfer_mode |= SDHCI_TRNS_BLK_CNT_EN;
                if block_count > 1 {
                    transfer_mode |= SDHCI_TRNS_MULTI;
                }
                return Ok(SdioTransferPlan {
                    block_size,
                    block_count,
                    cmd53_count: block_count,
                    block_mode: true,
                    transfer_mode,
                });
            }
        }
    }
    let block_size = u16::try_from(len).map_err(|_| HalError::Unsupported("sdhci-block-size"))?;
    Ok(SdioTransferPlan {
        block_size,
        block_count: 0,
        cmd53_count: block_size,
        block_mode: false,
        transfer_mode,
    })
}

fn sdio_byte_mode_transfer_plan(len: usize, write: bool) -> Result<SdioTransferPlan, HalError> {
    let mut transfer_mode = 0u16;
    if !write {
        transfer_mode |= SDHCI_TRNS_READ;
    }
    transfer_mode |= SDHCI_TRNS_BLK_CNT_EN;
    let block_size = u16::try_from(len).map_err(|_| HalError::Unsupported("sdhci-block-size"))?;
    Ok(SdioTransferPlan {
        block_size,
        block_count: 1,
        cmd53_count: block_size,
        block_mode: false,
        transfer_mode,
    })
}

fn sdio_function_block_size(function: SdioFunction) -> Option<u16> {
    match function {
        SdioFunction::Function1 => Some(SDIO_FUNCTION_ENABLE_F1.block_size),
        SdioFunction::Function2 => Some(SDIO_FUNCTION_ENABLE_F2.block_size),
        _ => None,
    }
}

fn r5_status(response: u32) -> u32 {
    response & 0xCB00
}

pub fn normalize_nvram(nvram: &[u8]) -> Vec<u8> {
    let mut normalized = Vec::with_capacity(nvram.len().saturating_add(2));
    for &byte in nvram {
        if byte == b'\r' {
            continue;
        }
        normalized.push(byte);
    }
    if !normalized.ends_with(b"\n") {
        normalized.push(b'\n');
    }
    normalized.push(0);
    while normalized.len() % 4 != 0 {
        normalized.push(0);
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::{
        backplane_byte_function_addr, backplane_small_access_addr,
        backplane_transfer_function_addr, backplane_window_base, backplane_window_register_bytes,
        backplane_window_reprogram_needed, backplane_word_function_addr, bcm2711_gpfsel_offset,
        bcm2711_puppdn_offset, clear_reset_keepalive_chunk_loops, cmd52_argument,
        control_plane_promote_rearm_budget, control_plane_promote_rearm_mode_name,
        control_plane_promoted_probe_stalled_after_rearm,
        control_plane_reply_rearm_attempts_after_rearm, control_plane_reply_rearm_mode_name,
        control_plane_reply_rearm_none, control_plane_reply_rearm_pending,
        control_plane_reply_rearm_promoted_link, control_plane_reply_rearm_startup_link,
        control_plane_reply_rearm_uses_promoted_link,
        control_plane_snapshot_uses_live_sdio_core_reads,
        control_plane_startup_link_probe_stalled_after_rearm,
        control_plane_zero_frame_needs_reply_rearm, core_ctrl_access_mode_label,
        core_ctrl_current_window_addr, core_ctrl_postreset_access_mode_label,
        core_ctrl_reset_assert_access_mode_label, core_ctrl_reset_clear_access_mode_label,
        core_ctrl_trace_function_addr, core_disable_uses_upstream_socram_disable,
        core_reset_can_skip_postreset_verify, core_reset_prepare_hold_value,
        core_wait_can_defer_after_read_error, core_wait_should_raise_control_plane_clock,
        cyw43_transport_mode_name, experimental_control_plane_write_can_assume_committed,
        experimental_control_plane_write_can_promote_after_post_write_rearm_timeout,
        experimental_control_plane_write_can_retry_on_startup_link,
        experimental_control_plane_write_needs_post_write_rearm,
        experimental_function2_fifo_chunk_limit, firmware_bulk_clock_candidates,
        firmware_phase_can_retry, ht_clock_assist_shadow_is_complete, ht_clock_request_value,
        ht_clock_retry_can_cutover_to_bounded_no_ht_early,
        ht_clock_timeout_can_enter_bounded_no_ht_transport, is_armcr4_postreset_fragile_read_error,
        is_mailbox_protocol_error, mailbox_tag_name, make_command, merge_u16_word,
        next_distinct_firmware_bulk_clock_candidate, normalize_nvram, phys_to_bus, r5_status,
        required_ht_clock_bounded_no_ht_shortcut_loops, sdhci_interrupt_buffer_ready_mask,
        sdhci_present_buffer_ready_mask, sdhci_status_reason, sdio_byte_mode_transfer_plan,
        sdio_core_reg_addr, sdio_core_transfer_function_addr, sdio_core_transfer_increment_addr,
        sdio_function_ready_budget_name, sdio_function_ready_extended_polls,
        sdio_function_ready_extended_polls_for, sdio_function_ready_extended_settle_loops,
        sdio_function_ready_extended_settle_loops_for, sdio_function_ready_retry_limit_for,
        sdio_function_ready_timeout_can_continue_experimentally,
        sdio_function_ready_uses_control_plane_reply_probe_budget,
        sdio_function_ready_uses_short_probe_only_budget, sdio_transfer_addr, sdio_transfer_plan,
        setup_firmware_channel_can_assume_write_committed,
        setup_firmware_channel_uses_experimental_order, should_log_firmware_upload_progress,
        should_log_sdio_transfer_chunk, transport_phase_chipclk_value,
        update_bcm2711_gpio_function, update_bcm2711_gpio_pull,
        wait_for_firmware_ready_restore_clock_hz, HalError, ResponseType, SdioFunction,
        SdioFunctionReadyBudget, AI_CORE_POSTRESET_IOCTRL, AI_CORE_PRERESET_IOCTRL,
        AI_IOCTRL_BIT_CLOCK_EN, AI_IOCTRL_BIT_FGC, AI_IOCTRL_OFFSET, AI_RESETCTRL_BIT_RESET,
        ARMCR4_BCMA_IOCTL_CPUHALT, ARMCR4_CAP, BACKPLANE_32BIT_FLAG, BACKPLANE_ADDRESS_MASK,
        BACKPLANE_WINDOW_MASK, BCM2711_GPIO_ALT3, CYW43_ARMCR4_CORE_BASE, CYW43_CHIPCOMMON_BASE,
        CYW43_CONTROL_PLANE_CLOCK_HZ, CYW43_FIRMWARE_BULK_CLOCK_HZ,
        CYW43_FIRMWARE_PROGRESS_INTERVAL, CYW43_RAM_BASE_4345,
        CYW43_SOCRAM_CLEAR_RESET_KEEPALIVE_CHUNK_LOOPS, CYW43_SOCRAM_CORE_BASE,
        CYW43_STARTUP_CLOCK_HZ, PI4_WIFI_SDIO_PINS, PI4_WIFI_SDIO_PULLS, SBSDIO_ALP_AVAIL_REQ,
        SBSDIO_FORCE_HT, SBSDIO_HT_AVAIL, SBSDIO_HT_AVAIL_REQ, SBSDIO_WAKE_TILL_HT_AVAIL,
        SDHCI_COMMAND, SDHCI_DATA_AVAILABLE, SDHCI_INT_CRC, SDHCI_INT_DATA_AVAIL,
        SDHCI_INT_DATA_CRC, SDHCI_INT_SPACE_AVAIL, SDHCI_INT_TIMEOUT, SDHCI_SPACE_AVAILABLE,
        SDHCI_TRANSFER_MODE, SDHCI_TRNS_BLK_CNT_EN, SDHCI_TRNS_MULTI, SDHCI_TRNS_READ,
        SDHCI_WRITE_DELAY_LOOPS, SDHCI_WRITE_GAP_SPIN_LOOPS, SDIO_CCCR_BRCM_CARDCAP_CMD_NODEC,
        SDIO_FUNCTION_ENABLE_SEQUENCE, SDIO_FUNCTION_READY_POLLS_FUNCTION2_REPLY_PROBE,
        SDIO_FUNCTION_READY_SETTLE_LOOPS_FUNCTION2_REPLY_PROBE, SDIO_FUNC_ENABLE_1,
        SDIO_FUNC_ENABLE_2, SDIO_FUNC_READY_1, SDIO_FUNC_READY_2, SDIO_INT_STATUS,
        SDPCMD_REG_HOSTINTMASK, SDPCMD_REG_TOHOSTMAILBOXDATA, SDPCMD_REG_TOSBMAILBOXDATA,
        TAG_GET_CLOCK_RATE, TAG_NOTIFY_XHCI_RESET, TAG_SET_GPIO_CONFIG, TAG_SET_POWER_STATE,
    };

    #[test]
    fn normalize_nvram_appends_newline_nul_and_padding() {
        let nvram = normalize_nvram(b"aa=1\r\nbb=2");
        assert!(nvram.starts_with(b"aa=1\nbb=2\n\0"));
        assert_eq!(nvram.len() % 4, 0);
    }

    #[test]
    fn cmd_flags_encode_expected_response_modes() {
        assert_eq!(make_command(5, ResponseType::None, false) & 0x3F, 0);
        let cmd5 = make_command(5, ResponseType::Ocr, false);
        assert_ne!(cmd5 & SDHCI_CMD_RESP_SHORT, 0);
        assert_eq!(cmd5 & (SDHCI_CMD_CRC | SDHCI_CMD_INDEX), 0);
        assert_ne!(make_command(52, ResponseType::Short, false) & 0x1C, 0);
        assert_ne!(make_command(53, ResponseType::Short, true) & 0x20, 0);
    }

    #[test]
    fn sdio_transfer_plan_uses_byte_mode_for_small_transfers() {
        let read =
            sdio_transfer_plan(SdioFunction::Function1, 4, false).expect("read transfer plan");
        assert_eq!(read.block_size, 4);
        assert_eq!(read.block_count, 0);
        assert_eq!(read.cmd53_count, 4);
        assert!(!read.block_mode);
        assert_eq!(read.transfer_mode & SDHCI_TRNS_BLK_CNT_EN, 0);
        assert_ne!(read.transfer_mode & SDHCI_TRNS_READ, 0);

        let write =
            sdio_transfer_plan(SdioFunction::Function0, 64, true).expect("write transfer plan");
        assert_eq!(write.block_size, 64);
        assert_eq!(write.block_count, 0);
        assert_eq!(write.cmd53_count, 64);
        assert!(!write.block_mode);
        assert_eq!(write.transfer_mode & SDHCI_TRNS_BLK_CNT_EN, 0);
        assert_eq!(write.transfer_mode & SDHCI_TRNS_READ, 0);
    }

    #[test]
    fn sdio_transfer_addr_advances_only_for_incrementing_transfers() {
        assert_eq!(
            sdio_transfer_addr(0x1200, 0, false).expect("base address"),
            0x1200
        );
        assert_eq!(
            sdio_transfer_addr(0x1200, 8, false).expect("fixed address"),
            0x1200
        );
        assert_eq!(
            sdio_transfer_addr(0x1200, 8, true).expect("incremented address"),
            0x1208
        );
    }

    #[test]
    fn sdio_core_reg_addr_maps_mailbox_and_interrupt_registers_into_core_window() {
        assert_eq!(
            sdio_core_reg_addr(SDIO_INT_STATUS),
            CYW43_SDIO_CORE_BASE + SDIO_INT_STATUS
        );
        assert_eq!(
            sdio_core_reg_addr(SDPCMD_REG_HOSTINTMASK),
            CYW43_SDIO_CORE_BASE + SDPCMD_REG_HOSTINTMASK
        );
        assert_eq!(
            sdio_core_reg_addr(SDPCMD_REG_TOSBMAILBOXDATA),
            CYW43_SDIO_CORE_BASE + SDPCMD_REG_TOSBMAILBOXDATA
        );
        assert_eq!(
            sdio_core_reg_addr(SDPCMD_REG_TOHOSTMAILBOXDATA),
            CYW43_SDIO_CORE_BASE + SDPCMD_REG_TOHOSTMAILBOXDATA
        );
    }

    #[test]
    fn sdio_core_transfer_path_uses_incrementing_flagged_function_addrs() {
        assert!(sdio_core_transfer_increment_addr());
        assert_eq!(
            sdio_core_transfer_function_addr(SDPCMD_REG_HOSTINTMASK),
            backplane_transfer_function_addr(sdio_core_reg_addr(SDPCMD_REG_HOSTINTMASK))
        );
        assert_eq!(
            sdio_core_transfer_function_addr(SDPCMD_REG_TOHOSTMAILBOXDATA),
            backplane_transfer_function_addr(sdio_core_reg_addr(SDPCMD_REG_TOHOSTMAILBOXDATA))
        );
    }

    #[test]
    fn sdio_transfer_chunk_trace_gate_prefers_control_and_early_incrementing_chunks() {
        assert!(should_log_sdio_transfer_chunk(
            SdioFunction::Function1,
            false,
            4,
            0
        ));
        assert!(!should_log_sdio_transfer_chunk(
            SdioFunction::Function1,
            false,
            16,
            0
        ));
        assert!(should_log_sdio_transfer_chunk(
            SdioFunction::Function1,
            true,
            64,
            0
        ));
        assert!(should_log_sdio_transfer_chunk(
            SdioFunction::Function1,
            true,
            64,
            511
        ));
        assert!(!should_log_sdio_transfer_chunk(
            SdioFunction::Function1,
            true,
            64,
            1022
        ));
        assert!(!should_log_sdio_transfer_chunk(
            SdioFunction::Function0,
            false,
            4,
            0
        ));
    }

    #[test]
    fn firmware_upload_progress_logs_first_boundary_and_final_chunk() {
        assert!(should_log_firmware_upload_progress(0, 64, 1024));
        assert!(!should_log_firmware_upload_progress(64, 64, 1024));
        assert!(should_log_firmware_upload_progress(
            CYW43_FIRMWARE_PROGRESS_INTERVAL - 64,
            64,
            CYW43_FIRMWARE_PROGRESS_INTERVAL * 2,
        ));
        assert!(should_log_firmware_upload_progress(960, 64, 1024));
    }

    #[test]
    fn sdio_transfer_plan_uses_block_mode_for_function_aligned_bulk_write() {
        let write = sdio_transfer_plan(SdioFunction::Function1, 256, true)
            .expect("function1 bulk write transfer plan");
        assert_eq!(write.block_size, 64);
        assert_eq!(write.block_count, 4);
        assert_eq!(write.cmd53_count, 4);
        assert!(write.block_mode);
        assert_ne!(write.transfer_mode & SDHCI_TRNS_BLK_CNT_EN, 0);
        assert_ne!(write.transfer_mode & SDHCI_TRNS_MULTI, 0);
        assert_eq!(write.transfer_mode & SDHCI_TRNS_READ, 0);
    }

    #[test]
    fn sdio_transfer_plan_uses_block_mode_for_single_function_block_write() {
        let write = sdio_transfer_plan(SdioFunction::Function1, 64, true)
            .expect("function1 single-block write transfer plan");
        assert_eq!(write.block_size, 64);
        assert_eq!(write.block_count, 1);
        assert_eq!(write.cmd53_count, 1);
        assert!(write.block_mode);
        assert_ne!(write.transfer_mode & SDHCI_TRNS_BLK_CNT_EN, 0);
        assert_eq!(write.transfer_mode & SDHCI_TRNS_MULTI, 0);
        assert_eq!(write.transfer_mode & SDHCI_TRNS_READ, 0);
    }

    #[test]
    fn experimental_function2_fifo_chunk_limit_uses_smaller_chunks_on_no_ht_fifo_path() {
        assert_eq!(
            experimental_function2_fifo_chunk_limit(SdioFunction::Function2, false, true),
            experimental_no_ht_f2_fifo_chunk_limit()
        );
        assert_eq!(
            experimental_no_ht_f2_fifo_chunk_limit(),
            SDIO_FUNCTION_ENABLE_F1.block_size as usize
        );
        assert_eq!(
            experimental_function2_fifo_chunk_limit(SdioFunction::Function2, true, true),
            SDIO_MAX_BYTE_MODE
        );
        assert_eq!(
            experimental_function2_fifo_chunk_limit(SdioFunction::Function1, false, true),
            SDIO_MAX_BYTE_MODE
        );
        assert_eq!(
            experimental_function2_fifo_chunk_limit(SdioFunction::Function2, false, false),
            SDIO_MAX_BYTE_MODE
        );
    }

    #[test]
    fn experimental_control_plane_write_never_assumes_committed_state() {
        assert!(!experimental_control_plane_write_can_assume_committed(
            true,
            true,
            false,
            &HalError::Unsupported("sdhci-transfer-finish")
        ));
        assert!(!experimental_control_plane_write_can_assume_committed(
            false,
            true,
            false,
            &HalError::Unsupported("sdhci-transfer-finish")
        ));
        assert!(!experimental_control_plane_write_can_assume_committed(
            true,
            false,
            false,
            &HalError::Unsupported("sdhci-transfer-finish")
        ));
        assert!(!experimental_control_plane_write_can_assume_committed(
            true,
            true,
            true,
            &HalError::Unsupported("sdhci-transfer-finish")
        ));
        assert!(!experimental_control_plane_write_can_assume_committed(
            true,
            true,
            false,
            &HalError::Unsupported("sdio-cmd52-read")
        ));
    }

    #[test]
    fn experimental_control_plane_write_startup_link_retry_requires_first_high_clock_io_error() {
        assert!(experimental_control_plane_write_can_retry_on_startup_link(
            true,
            true,
            CYW43_CONTROL_PLANE_CLOCK_HZ,
            &HalError::Unsupported("sdhci-transfer-finish"),
        ));
        assert!(experimental_control_plane_write_can_retry_on_startup_link(
            true,
            true,
            CYW43_CONTROL_PLANE_CLOCK_HZ,
            &HalError::Unsupported("sdhci-transfer-data"),
        ));
        assert!(!experimental_control_plane_write_can_retry_on_startup_link(
            false,
            true,
            CYW43_CONTROL_PLANE_CLOCK_HZ,
            &HalError::Unsupported("sdhci-transfer-finish"),
        ));
        assert!(!experimental_control_plane_write_can_retry_on_startup_link(
            true,
            false,
            CYW43_CONTROL_PLANE_CLOCK_HZ,
            &HalError::Unsupported("sdhci-transfer-finish"),
        ));
        assert!(!experimental_control_plane_write_can_retry_on_startup_link(
            true,
            true,
            CYW43_STARTUP_CLOCK_HZ,
            &HalError::Unsupported("sdhci-transfer-finish"),
        ));
        assert!(!experimental_control_plane_write_can_retry_on_startup_link(
            true,
            true,
            CYW43_CONTROL_PLANE_CLOCK_HZ,
            &HalError::Unsupported("sdio-cmd52-read"),
        ));
    }

    #[test]
    fn experimental_control_plane_write_post_write_rearm_tracks_bounded_probe() {
        assert!(experimental_control_plane_write_needs_post_write_rearm(
            true, true
        ));
        assert!(!experimental_control_plane_write_needs_post_write_rearm(
            false, true
        ));
        assert!(!experimental_control_plane_write_needs_post_write_rearm(
            true, false
        ));
    }

    #[test]
    fn experimental_control_plane_write_post_write_timeout_promotion_tracks_bounded_probe() {
        assert!(
            experimental_control_plane_write_can_promote_after_post_write_rearm_timeout(
                true,
                true,
                &HalError::Unsupported("sdio-function2-ready-timeout"),
            )
        );
        assert!(
            experimental_control_plane_write_can_promote_after_post_write_rearm_timeout(
                true,
                true,
                &HalError::Unsupported("cyw43-control-plane-startup-link-reply-timeout"),
            )
        );
        assert!(
            !experimental_control_plane_write_can_promote_after_post_write_rearm_timeout(
                false,
                true,
                &HalError::Unsupported("cyw43-control-plane-startup-link-reply-timeout"),
            )
        );
        assert!(
            !experimental_control_plane_write_can_promote_after_post_write_rearm_timeout(
                true,
                false,
                &HalError::Unsupported("sdio-function2-ready-timeout"),
            )
        );
        assert!(
            !experimental_control_plane_write_can_promote_after_post_write_rearm_timeout(
                true,
                true,
                &HalError::Unsupported("sdhci-transfer-finish"),
            )
        );
    }

    #[test]
    fn startup_link_timeout_promotes_only_on_bounded_no_ht_probe() {
        assert!(control_plane_startup_link_timeout_needs_promoted_probe(
            true,
            control_plane_reply_rearm_startup_link(),
        ));
        assert!(!control_plane_startup_link_timeout_needs_promoted_probe(
            false,
            control_plane_reply_rearm_startup_link(),
        ));
        assert!(!control_plane_startup_link_timeout_needs_promoted_probe(
            true,
            control_plane_reply_rearm_promoted_link(),
        ));
    }

    #[test]
    fn control_plane_zero_frame_reply_rearm_runs_for_startup_and_promoted_paths_only() {
        assert!(control_plane_reply_rearm_pending(
            control_plane_reply_rearm_startup_link()
        ));
        assert!(control_plane_reply_rearm_pending(
            control_plane_reply_rearm_promoted_link()
        ));
        assert!(!control_plane_reply_rearm_pending(
            control_plane_reply_rearm_none()
        ));
        assert!(!control_plane_reply_rearm_uses_promoted_link(
            control_plane_reply_rearm_startup_link()
        ));
        assert!(control_plane_reply_rearm_uses_promoted_link(
            control_plane_reply_rearm_promoted_link()
        ));
        assert!(control_plane_zero_frame_needs_reply_rearm(
            control_plane_reply_rearm_startup_link(),
            false,
            0,
        ));
        assert!(control_plane_zero_frame_needs_reply_rearm(
            control_plane_reply_rearm_promoted_link(),
            false,
            1,
        ));
        assert!(!control_plane_zero_frame_needs_reply_rearm(
            control_plane_reply_rearm_promoted_link(),
            true,
            0,
        ));
        assert!(!control_plane_zero_frame_needs_reply_rearm(
            control_plane_reply_rearm_none(),
            false,
            0,
        ));
        assert!(!control_plane_zero_frame_needs_reply_rearm(
            control_plane_reply_rearm_startup_link(),
            false,
            2,
        ));
        assert_eq!(control_plane_reply_rearm_attempts_after_rearm(false, 1), 1);
        assert_eq!(control_plane_reply_rearm_attempts_after_rearm(false, 2), 2);
        assert_eq!(control_plane_reply_rearm_attempts_after_rearm(true, 2), 0);
        assert!(!control_plane_startup_link_probe_stalled_after_rearm(
            control_plane_reply_rearm_startup_link(),
            true,
            2,
        ));
        assert!(!control_plane_startup_link_probe_stalled_after_rearm(
            control_plane_reply_rearm_promoted_link(),
            false,
            2,
        ));
        assert!(control_plane_startup_link_probe_stalled_after_rearm(
            control_plane_reply_rearm_startup_link(),
            false,
            2,
        ));
        assert!(!control_plane_promoted_probe_stalled_after_rearm(
            control_plane_reply_rearm_startup_link(),
            false,
            false,
            2,
        ));
        assert!(!control_plane_promoted_probe_stalled_after_rearm(
            control_plane_reply_rearm_promoted_link(),
            false,
            false,
            2,
        ));
        assert!(!control_plane_promoted_probe_stalled_after_rearm(
            control_plane_reply_rearm_promoted_link(),
            true,
            true,
            2,
        ));
        assert!(control_plane_promoted_probe_stalled_after_rearm(
            control_plane_reply_rearm_promoted_link(),
            true,
            false,
            2,
        ));
    }

    #[test]
    fn no_ht_control_plane_snapshots_skip_live_sdio_core_reads() {
        assert!(control_plane_snapshot_uses_live_sdio_core_reads(false));
        assert!(!control_plane_snapshot_uses_live_sdio_core_reads(true));
    }

    #[test]
    fn control_plane_reply_rearm_mode_names_are_stable() {
        assert_eq!(
            control_plane_reply_rearm_mode_name(control_plane_reply_rearm_none()),
            "none"
        );
        assert_eq!(
            control_plane_reply_rearm_mode_name(control_plane_reply_rearm_startup_link()),
            "startup-link"
        );
        assert_eq!(
            control_plane_reply_rearm_mode_name(control_plane_reply_rearm_promoted_link()),
            "promoted-link"
        );
        assert_eq!(control_plane_reply_rearm_mode_name(99), "none");
    }

    #[test]
    fn control_plane_promote_rearm_mode_names_are_stable() {
        assert_eq!(control_plane_promote_rearm_mode_name(false), "strict");
        assert_eq!(
            control_plane_promote_rearm_mode_name(true),
            "speculative-empty-poll"
        );
    }

    #[test]
    fn speculative_promote_rearm_reuses_experimental_bypass_budget() {
        assert_eq!(
            control_plane_promote_rearm_budget(false),
            SdioFunctionReadyBudget::Strict
        );
        assert_eq!(
            control_plane_promote_rearm_budget(true),
            SdioFunctionReadyBudget::ExperimentalBypass
        );
    }

    #[test]
    fn sdio_byte_mode_transfer_plan_forces_single_chunk_bulk_write() {
        let write = sdio_byte_mode_transfer_plan(256, true)
            .expect("function1 bulk write byte-mode transfer plan");
        assert_eq!(write.block_size, 256);
        assert_eq!(write.block_count, 1);
        assert_eq!(write.cmd53_count, 256);
        assert!(!write.block_mode);
        assert_ne!(write.transfer_mode & SDHCI_TRNS_BLK_CNT_EN, 0);
        assert_eq!(write.transfer_mode & SDHCI_TRNS_MULTI, 0);
        assert_eq!(write.transfer_mode & SDHCI_TRNS_READ, 0);
    }

    #[test]
    fn sdio_byte_mode_transfer_plan_uses_single_host_block_for_small_writes() {
        let write = sdio_byte_mode_transfer_plan(64, true)
            .expect("function1 firmware write byte-mode transfer plan");
        assert_eq!(write.block_size, 64);
        assert_eq!(write.block_count, 1);
        assert_eq!(write.cmd53_count, 64);
        assert!(!write.block_mode);
        assert_ne!(write.transfer_mode & SDHCI_TRNS_BLK_CNT_EN, 0);
        assert_eq!(write.transfer_mode & SDHCI_TRNS_MULTI, 0);
        assert_eq!(write.transfer_mode & SDHCI_TRNS_READ, 0);
    }

    #[test]
    fn sdio_transfer_plan_uses_cmd53_byte_mode_for_nvram_tail_write() {
        let write = sdio_transfer_plan(SdioFunction::Function1, 4, true)
            .expect("function1 nvram tail write transfer plan");
        assert_eq!(write.block_size, 4);
        assert_eq!(write.block_count, 0);
        assert_eq!(write.cmd53_count, 4);
        assert!(!write.block_mode);
        assert_eq!(write.transfer_mode & SDHCI_TRNS_BLK_CNT_EN, 0);
        assert_eq!(write.transfer_mode & SDHCI_TRNS_MULTI, 0);
        assert_eq!(write.transfer_mode & SDHCI_TRNS_READ, 0);
    }

    #[test]
    fn backplane_access_modes_split_small_and_transfer_paths() {
        assert_eq!(
            backplane_small_access_addr(CYW43_CHIPCOMMON_BASE),
            CYW43_CHIPCOMMON_BASE
        );
        assert_eq!(
            backplane_byte_function_addr(CYW43_CHIPCOMMON_BASE + 0x10),
            0x0010
        );
        assert_eq!(backplane_byte_function_addr(CYW43_RAM_BASE_4345), 0x0000);
        assert_eq!(
            backplane_transfer_function_addr(CYW43_CHIPCOMMON_BASE + 0x10),
            0x8010
        );
        assert_eq!(
            backplane_transfer_function_addr(CYW43_RAM_BASE_4345),
            BACKPLANE_32BIT_FLAG
        );
        assert_eq!(
            backplane_transfer_function_addr(CYW43_RAM_BASE_4345 + 0x40),
            BACKPLANE_32BIT_FLAG | 0x0040
        );
        assert_eq!(backplane_byte_function_addr(0x0026_fffc), 0x7ffc);
        assert_eq!(
            backplane_transfer_function_addr(0x0026_fffc),
            BACKPLANE_32BIT_FLAG | 0x7ffc
        );
        assert_eq!(
            backplane_word_function_addr(CYW43_ARMCR4_CORE_BASE + ARMCR4_CAP),
            ((CYW43_ARMCR4_CORE_BASE + ARMCR4_CAP) & BACKPLANE_ADDRESS_MASK) | BACKPLANE_32BIT_FLAG
        );
        assert_eq!(
            (CYW43_ARMCR4_CORE_BASE + ARMCR4_CAP) & BACKPLANE_WINDOW_MASK,
            CYW43_ARMCR4_CORE_BASE & BACKPLANE_WINDOW_MASK
        );
        assert!(!backplane_word_increment_addr());
        assert_eq!(
            core_ctrl_function_addr(CYW43_ARMCR4_CORE_BASE + AI_IOCTRL_OFFSET),
            0x0c408
        );
        assert_eq!(
            core_ctrl_current_window_addr(CYW43_ARMCR4_CORE_BASE + AI_IOCTRL_OFFSET),
            0x04408
        );
        assert_eq!(
            core_ctrl_trace_function_addr(CYW43_ARMCR4_CORE_BASE + AI_IOCTRL_OFFSET),
            0x0c408
        );
        assert_eq!(
            core_ctrl_access_mode_label(),
            "cmd53-windowed-read32-cmd53-byte-current-window fallback=cmd53-byte-rewindow"
        );
        assert_eq!(
            core_ctrl_reset_assert_access_mode_label(),
            "cmd53-word-windowed fallback=cmd52-byte-current-window-rewindow"
        );
        assert_eq!(
            core_ctrl_reset_clear_access_mode_label(),
            "cmd53-word-windowed fallback=cmd52-byte-current-window"
        );
        assert_eq!(
            core_ctrl_reset_clear_retry_access_mode_label(),
            "cmd52-byte-current-window retry=preserved-cache"
        );
        assert_eq!(
            core_ctrl_postreset_access_mode_label(CYW43_SOCRAM_CORE_BASE, AI_IOCTRL_OFFSET),
            "cmd53-byte-current-window fallback=cmd53-byte-rewindow"
        );
        assert_eq!(
            core_ctrl_postreset_access_mode_label(CYW43_ARMCR4_CORE_BASE, AI_IOCTRL_OFFSET),
            "cmd53-byte-current-window fallback=cmd53-byte-rewindow"
        );
        assert_eq!(
            core_ctrl_postreset_read_access_mode_label(CYW43_SOCRAM_CORE_BASE, AI_IOCTRL_OFFSET),
            "cmd53-windowed-read32-cmd53-byte-current-window fallback=cmd53-byte-rewindow"
        );
        assert_eq!(
            core_ctrl_postreset_read_access_mode_label(CYW43_SOCRAM_CORE_BASE, AI_RESETCTRL_OFFSET),
            "cmd53-windowed-read32-cmd53-byte-current-window fallback=cmd53-byte-rewindow"
        );
        assert_eq!(
            core_ctrl_postreset_read_access_mode_label(CYW43_ARMCR4_CORE_BASE, AI_IOCTRL_OFFSET),
            "cmd52-byte-current-window retry=cmd52-byte-rewindow"
        );
        assert!(core_ctrl_postreset_read_uses_cmd52_current_window(
            CYW43_ARMCR4_CORE_BASE,
            AI_IOCTRL_OFFSET
        ));
        assert!(!core_ctrl_postreset_read_uses_cmd52_current_window(
            CYW43_SOCRAM_CORE_BASE,
            AI_IOCTRL_OFFSET
        ));
        assert_eq!(
            core_ctrl_in_reset_access_mode_label(CYW43_ARMCR4_CORE_BASE, AI_IOCTRL_OFFSET),
            "cmd53-word-windowed-in-reset fallback=cmd52-current-window-rewindow"
        );
        assert_eq!(
            core_ctrl_in_reset_access_mode_label(CYW43_SOCRAM_CORE_BASE, AI_IOCTRL_OFFSET),
            "cmd52-byte-current-window fallback=cmd52-byte-rewindow"
        );
        assert!(core_ctrl_in_reset_write_uses_word_path(
            CYW43_ARMCR4_CORE_BASE,
            AI_IOCTRL_OFFSET
        ));
        assert!(!core_ctrl_in_reset_write_uses_word_path(
            CYW43_SOCRAM_CORE_BASE,
            AI_IOCTRL_OFFSET
        ));
        assert!(!core_ctrl_in_reset_write_uses_word_path(
            CYW43_ARMCR4_CORE_BASE,
            AI_RESETCTRL_OFFSET
        ));
        assert!(core_reset_needs_postreset_window_reprime(
            CYW43_SOCRAM_CORE_BASE
        ));
        assert!(!core_reset_needs_postreset_window_reprime(
            CYW43_ARMCR4_CORE_BASE
        ));
        assert!(core_reset_can_skip_postreset_verify(CYW43_SOCRAM_CORE_BASE));
        assert!(!core_reset_can_skip_postreset_verify(
            CYW43_ARMCR4_CORE_BASE
        ));
        assert!(core_ctrl_can_skip_redundant_in_reset_write(
            CYW43_SOCRAM_CORE_BASE,
            true,
            AI_CORE_PRERESET_IOCTRL,
            AI_CORE_PRERESET_IOCTRL,
        ));
        assert!(!core_ctrl_can_skip_redundant_in_reset_write(
            CYW43_ARMCR4_CORE_BASE,
            true,
            ARMCR4_BCMA_IOCTL_CPUHALT | AI_CORE_PRERESET_IOCTRL,
            ARMCR4_BCMA_IOCTL_CPUHALT | AI_CORE_PRERESET_IOCTRL,
        ));
        assert!(core_ctrl_can_defer_in_reset_readback(
            CYW43_SOCRAM_CORE_BASE,
            true,
            true,
        ));
        assert!(!core_ctrl_can_defer_in_reset_readback(
            CYW43_SOCRAM_CORE_BASE,
            true,
            false,
        ));
        assert!(!core_ctrl_can_defer_in_reset_readback(
            CYW43_ARMCR4_CORE_BASE,
            true,
            true,
        ));
        assert!(core_ctrl_can_defer_clear_reset_readback(
            CYW43_SOCRAM_CORE_BASE,
            0,
        ));
        assert!(!core_ctrl_can_defer_clear_reset_readback(
            CYW43_SOCRAM_CORE_BASE,
            1,
        ));
        assert!(core_ctrl_can_defer_clear_reset_readback(
            CYW43_ARMCR4_CORE_BASE,
            0,
        ));
        assert!(!core_ctrl_can_defer_clear_reset_readback(
            CYW43_ARMCR4_CORE_BASE,
            1,
        ));
        assert!(core_reset_can_skip_disable(CYW43_SOCRAM_CORE_BASE, 0, 0, 0,));
        assert!(!core_reset_needs_clear_reset_prewrite_settle(
            CYW43_SOCRAM_CORE_BASE,
            0,
        ));
        assert!(!core_reset_needs_clear_reset_prewrite_settle(
            CYW43_SOCRAM_CORE_BASE,
            1,
        ));
        assert!(core_reset_can_retry_clear_reset_write(
            CYW43_SOCRAM_CORE_BASE,
            0,
        ));
        assert!(!core_reset_can_retry_clear_reset_write(
            CYW43_ARMCR4_CORE_BASE,
            0,
        ));
        assert!(core_reset_needs_clear_reset_ht_assist(
            CYW43_SOCRAM_CORE_BASE
        ));
        assert!(!core_reset_needs_clear_reset_ht_assist(
            CYW43_ARMCR4_CORE_BASE
        ));
        assert!(core_disable_uses_upstream_socram_disable(
            CYW43_SOCRAM_CORE_BASE,
            0,
            0,
        ));
        assert!(!core_disable_uses_upstream_socram_disable(
            CYW43_ARMCR4_CORE_BASE,
            ARMCR4_BCMA_IOCTL_CPUHALT,
            ARMCR4_BCMA_IOCTL_CPUHALT,
        ));
        assert!(is_sdhci_command_error(&HalError::Unsupported(
            "sdhci-command-error"
        )));
        assert!(!is_sdhci_command_error(&HalError::Unsupported(
            "sdhci-int-timeout"
        )));
        assert!(is_sdhci_int_timeout(&HalError::Unsupported(
            "sdhci-int-timeout"
        )));
        assert!(is_sdhci_fragile_read_error(&HalError::Unsupported(
            "sdhci-int-timeout"
        )));
        assert!(is_sdhci_fragile_read_error(&HalError::Unsupported(
            "sdhci-command-error"
        )));
        assert!(is_armcr4_postreset_fragile_read_error(
            &HalError::Unsupported("sdio-cmd52-read")
        ));
        assert!(!core_reset_can_assume_clear_reset_retry_commit(
            CYW43_SOCRAM_CORE_BASE,
            AI_RESETCTRL_OFFSET,
            0,
            &HalError::Unsupported("sdhci-command-error"),
        ));
        assert!(!core_reset_can_assume_clear_reset_retry_commit(
            CYW43_ARMCR4_CORE_BASE,
            AI_RESETCTRL_OFFSET,
            0,
            &HalError::Unsupported("sdhci-command-error"),
        ));
        assert!(!core_reset_can_assume_clear_reset_retry_commit(
            CYW43_SOCRAM_CORE_BASE,
            AI_IOCTRL_OFFSET,
            0,
            &HalError::Unsupported("sdhci-command-error"),
        ));
        assert!(!core_reset_can_assume_clear_reset_retry_commit(
            CYW43_SOCRAM_CORE_BASE,
            AI_RESETCTRL_OFFSET,
            1,
            &HalError::Unsupported("sdhci-command-error"),
        ));
        assert!(!core_reset_can_assume_clear_reset_retry_commit(
            CYW43_SOCRAM_CORE_BASE,
            AI_RESETCTRL_OFFSET,
            0,
            &HalError::Unsupported("sdhci-int-timeout"),
        ));
        assert!(!core_reset_can_assume_postreset_clock_en_commit(
            CYW43_SOCRAM_CORE_BASE,
            AI_IOCTRL_OFFSET,
            &HalError::Unsupported("sdhci-command-error"),
        ));
        assert!(!core_reset_can_assume_postreset_clock_en_commit(
            CYW43_ARMCR4_CORE_BASE,
            AI_IOCTRL_OFFSET,
            &HalError::Unsupported("sdhci-command-error"),
        ));
        assert!(!core_reset_can_assume_postreset_clock_en_commit(
            CYW43_SOCRAM_CORE_BASE,
            AI_RESETCTRL_OFFSET,
            &HalError::Unsupported("sdhci-command-error"),
        ));
        assert!(!core_reset_can_assume_postreset_clock_en_commit(
            CYW43_SOCRAM_CORE_BASE,
            AI_IOCTRL_OFFSET,
            &HalError::Unsupported("sdhci-int-timeout"),
        ));
        assert!(!core_reset_can_defer_postreset_clock_en_readback(
            CYW43_SOCRAM_CORE_BASE,
            AI_IOCTRL_OFFSET,
            &HalError::Unsupported("sdhci-command-error"),
        ));
        assert!(core_reset_can_defer_postreset_clock_en_readback(
            CYW43_ARMCR4_CORE_BASE,
            AI_IOCTRL_OFFSET,
            &HalError::Unsupported("sdhci-int-timeout"),
        ));
        assert!(core_reset_can_defer_postreset_clock_en_readback(
            CYW43_ARMCR4_CORE_BASE,
            AI_IOCTRL_OFFSET,
            &HalError::Unsupported("sdio-cmd52-read"),
        ));
        assert!(!core_reset_can_defer_postreset_clock_en_readback(
            CYW43_SOCRAM_CORE_BASE,
            AI_RESETCTRL_OFFSET,
            &HalError::Unsupported("sdhci-command-error"),
        ));
        assert!(!core_reset_can_defer_postreset_clock_en_readback(
            CYW43_SOCRAM_CORE_BASE,
            AI_IOCTRL_OFFSET,
            &HalError::Unsupported("sdhci-int-timeout"),
        ));
        assert!(!core_reset_can_defer_postreset_reset_readback(
            CYW43_SOCRAM_CORE_BASE,
            AI_RESETCTRL_OFFSET,
            &HalError::Unsupported("sdhci-command-error"),
        ));
        assert!(core_reset_can_defer_postreset_reset_readback(
            CYW43_ARMCR4_CORE_BASE,
            AI_RESETCTRL_OFFSET,
            &HalError::Unsupported("sdhci-int-timeout"),
        ));
        assert!(core_reset_can_defer_postreset_reset_readback(
            CYW43_ARMCR4_CORE_BASE,
            AI_RESETCTRL_OFFSET,
            &HalError::Unsupported("sdio-cmd52-read"),
        ));
        assert!(!core_reset_can_defer_postreset_reset_readback(
            CYW43_SOCRAM_CORE_BASE,
            AI_IOCTRL_OFFSET,
            &HalError::Unsupported("sdhci-command-error"),
        ));
        assert!(!core_reset_can_defer_postreset_reset_readback(
            CYW43_SOCRAM_CORE_BASE,
            AI_RESETCTRL_OFFSET,
            &HalError::Unsupported("sdhci-int-timeout"),
        ));
        assert!(core_wait_can_retry_after_read_error(
            CYW43_ARMCR4_CORE_BASE,
            &HalError::Unsupported("sdhci-int-timeout"),
        ));
        assert!(core_wait_can_retry_after_read_error(
            CYW43_ARMCR4_CORE_BASE,
            &HalError::Unsupported("sdio-cmd52-read"),
        ));
        assert!(core_wait_should_raise_control_plane_clock(
            CYW43_ARMCR4_CORE_BASE,
            CYW43_STARTUP_CLOCK_HZ,
            &HalError::Unsupported("sdhci-int-timeout"),
        ));
        assert!(core_wait_should_raise_control_plane_clock(
            CYW43_ARMCR4_CORE_BASE,
            CYW43_STARTUP_CLOCK_HZ,
            &HalError::Unsupported("sdio-cmd52-read"),
        ));
        assert!(!core_wait_should_raise_control_plane_clock(
            CYW43_ARMCR4_CORE_BASE,
            CYW43_CONTROL_PLANE_CLOCK_HZ,
            &HalError::Unsupported("sdhci-int-timeout"),
        ));
        assert!(!core_wait_can_defer_after_read_error(
            CYW43_ARMCR4_CORE_BASE,
            0,
            CYW43_CONTROL_PLANE_CLOCK_HZ,
            &HalError::Unsupported("sdhci-int-timeout"),
        ));
        assert!(!core_wait_can_defer_after_read_error(
            CYW43_ARMCR4_CORE_BASE,
            1,
            CYW43_CONTROL_PLANE_CLOCK_HZ,
            &HalError::Unsupported("sdhci-int-timeout"),
        ));
        assert!(core_wait_can_defer_after_read_error(
            CYW43_ARMCR4_CORE_BASE,
            2,
            CYW43_CONTROL_PLANE_CLOCK_HZ,
            &HalError::Unsupported("sdhci-int-timeout"),
        ));
        assert!(core_wait_can_defer_after_read_error(
            CYW43_ARMCR4_CORE_BASE,
            2,
            CYW43_CONTROL_PLANE_CLOCK_HZ,
            &HalError::Unsupported("sdio-cmd52-read"),
        ));
        assert!(!core_wait_can_defer_after_read_error(
            CYW43_ARMCR4_CORE_BASE,
            2,
            CYW43_STARTUP_CLOCK_HZ,
            &HalError::Unsupported("sdhci-int-timeout"),
        ));
        assert!(!core_wait_can_retry_after_read_error(
            CYW43_SOCRAM_CORE_BASE,
            &HalError::Unsupported("sdhci-int-timeout"),
        ));
        assert!(!core_wait_should_raise_control_plane_clock(
            CYW43_SOCRAM_CORE_BASE,
            CYW43_STARTUP_CLOCK_HZ,
            &HalError::Unsupported("sdhci-int-timeout"),
        ));
        assert!(!core_wait_can_defer_after_read_error(
            CYW43_SOCRAM_CORE_BASE,
            2,
            CYW43_CONTROL_PLANE_CLOCK_HZ,
            &HalError::Unsupported("sdhci-int-timeout"),
        ));
        assert_eq!(
            backplane_window_base(CYW43_SOCRAM_CORE_BASE + AI_RESETCTRL_OFFSET),
            backplane_window_base(CYW43_SOCRAM_CORE_BASE + 0x10),
        );
        assert!(chipcommon_config_is_phase_addr(
            CYW43_SOCRAM_CORE_BASE + 0x10
        ));
        assert!(chipcommon_config_is_phase_addr(
            CYW43_SOCRAM_CORE_BASE + 0x44
        ));
        assert!(!chipcommon_config_is_phase_addr(
            CYW43_SOCRAM_CORE_BASE + 0x48
        ));
        assert!(!backplane_window_differs_by_mid_byte_only(
            backplane_window_base(CYW43_SOCRAM_CORE_BASE + AI_RESETCTRL_OFFSET),
            CYW43_ARMCR4_CORE_BASE + AI_IOCTRL_OFFSET,
        ));
        assert_eq!(
            chipcommon_config_source_window(
                None,
                Some(CYW43_SOCRAM_CORE_BASE + AI_RESETCTRL_OFFSET),
                CYW43_SOCRAM_CORE_BASE + 0x10,
            ),
            Some(backplane_window_base(
                CYW43_SOCRAM_CORE_BASE + AI_RESETCTRL_OFFSET
            )),
        );
        assert!(!chipcommon_config_can_use_mid_only_window_switch(
            Some(backplane_window_base(
                CYW43_SOCRAM_CORE_BASE + AI_RESETCTRL_OFFSET
            )),
            None,
            CYW43_SOCRAM_CORE_BASE + 0x10,
        ));
        assert!(!chipcommon_config_can_use_mid_only_window_switch(
            None,
            Some(CYW43_SOCRAM_CORE_BASE + AI_RESETCTRL_OFFSET),
            CYW43_SOCRAM_CORE_BASE + 0x10,
        ));
        assert!(!chipcommon_config_can_use_mid_only_window_switch(
            None,
            None,
            CYW43_SOCRAM_CORE_BASE + 0x10,
        ));
        assert!(!chipcommon_config_can_use_mid_only_window_switch(
            Some(backplane_window_base(
                CYW43_SOCRAM_CORE_BASE + AI_RESETCTRL_OFFSET
            )),
            None,
            CYW43_SOCRAM_CORE_BASE + 0x44,
        ));
        assert!(!chipcommon_config_can_use_mid_only_window_switch(
            Some(backplane_window_base(CYW43_SOCRAM_CORE_BASE + 0x10)),
            None,
            CYW43_SOCRAM_CORE_BASE + 0x10,
        ));
        assert!(is_sdhci_io_path_error(&HalError::Unsupported(
            "sdhci-command-error"
        )));
        assert!(is_sdhci_io_path_error(&HalError::Unsupported(
            "sdhci-transfer-command"
        )));
        assert!(is_sdhci_io_path_error(&HalError::Unsupported(
            "sdhci-transfer-data"
        )));
        assert!(is_sdhci_io_path_error(&HalError::Unsupported(
            "sdhci-transfer-finish"
        )));
        assert!(!is_sdhci_io_path_error(&HalError::Unsupported(
            "sdhci-int-timeout"
        )));
        assert!(!chipcommon_config_can_assume_write_commit(
            CYW43_SOCRAM_CORE_BASE + 0x10,
            &HalError::Unsupported("sdhci-transfer-command"),
        ));
        assert!(!chipcommon_config_can_assume_window_commit(
            &HalError::Unsupported("sdhci-command-error"),
        ));
        assert!(!chipcommon_config_can_assume_window_commit(
            &HalError::Unsupported("sdhci-transfer-command"),
        ));
        assert!(!chipcommon_config_can_assume_write_commit(
            CYW43_SOCRAM_CORE_BASE + 0x44,
            &HalError::Unsupported("sdhci-command-error"),
        ));
        assert!(!chipcommon_config_can_assume_write_commit(
            CYW43_SOCRAM_CORE_BASE + 0x48,
            &HalError::Unsupported("sdhci-transfer-command"),
        ));
        assert!(!chipcommon_config_can_assume_write_commit(
            CYW43_SOCRAM_CORE_BASE + 0x10,
            &HalError::Unsupported("sdhci-int-timeout"),
        ));
        assert!(core_reset_clear_preserves_window_cache());
        assert!(!core_reset_clear_allows_immediate_rewindow_fallback());
        assert_eq!(
            restore_programmed_backplane_window(Some(CYW43_SOCRAM_CORE_BASE + AI_RESETCTRL_OFFSET)),
            Some(backplane_window_base(
                CYW43_SOCRAM_CORE_BASE + AI_RESETCTRL_OFFSET
            )),
        );
        assert_eq!(
            restore_programmed_backplane_window(Some(CYW43_SOCRAM_CORE_BASE + AI_IOCTRL_OFFSET)),
            Some(backplane_window_base(
                CYW43_SOCRAM_CORE_BASE + AI_IOCTRL_OFFSET
            )),
        );
        assert_eq!(restore_programmed_backplane_window(None), None);
        assert_eq!(core_reset_prepare_hold_value(0), AI_CORE_PRERESET_IOCTRL);
        assert_eq!(
            core_reset_prepare_hold_value(ARMCR4_BCMA_IOCTL_CPUHALT),
            ARMCR4_BCMA_IOCTL_CPUHALT | AI_CORE_PRERESET_IOCTRL
        );
        assert!(core_reset_can_skip_pre_clear_in_reset_write(
            CYW43_SOCRAM_CORE_BASE,
            true,
        ));
        assert!(!core_reset_can_skip_pre_clear_in_reset_write(
            CYW43_SOCRAM_CORE_BASE,
            false,
        ));
        assert!(!core_reset_can_skip_pre_clear_in_reset_write(
            CYW43_ARMCR4_CORE_BASE,
            true,
        ));
        assert!(core_reset_can_skip_disable(
            CYW43_ARMCR4_CORE_BASE,
            ARMCR4_BCMA_IOCTL_CPUHALT,
            0,
            0,
        ));
    }

    #[test]
    fn backplane_window_register_bytes_encode_full_low_mid_high_bytes() {
        assert_eq!(
            backplane_window_register_bytes(CYW43_ARMCR4_CORE_BASE + AI_IOCTRL_OFFSET),
            (0x00, 0x10, 0x18)
        );
        assert_eq!(
            backplane_window_register_bytes(CYW43_RAM_BASE_4345),
            (0x80, 0x19, 0x00)
        );
    }

    #[test]
    fn backplane_window_program_sequence_commits_low_last() {
        assert_eq!(
            backplane_window_program_sequence(0x80, 0x19, 0x00),
            [
                ("high", SBSDIO_FUNC1_SBADDRHIGH, 0x00),
                ("mid", SBSDIO_FUNC1_SBADDRMID, 0x19),
                ("low", SBSDIO_FUNC1_SBADDRLOW, 0x80),
            ]
        );
    }

    #[test]
    fn firmware_backplane_write_retry_is_single_shot_on_io_path_errors() {
        assert!(firmware_backplane_write_can_retry(
            &HalError::Unsupported("sdhci-command-error"),
            0,
        ));
        assert!(firmware_backplane_write_can_retry(
            &HalError::Unsupported("sdhci-transfer-command"),
            0,
        ));
        assert!(firmware_backplane_write_can_retry(
            &HalError::Unsupported("sdhci-transfer-data"),
            0,
        ));
        assert!(firmware_backplane_write_can_retry(
            &HalError::Unsupported("sdhci-transfer-finish"),
            0,
        ));
        assert!(!firmware_backplane_write_can_retry(
            &HalError::Unsupported("sdhci-transfer-command"),
            1,
        ));
        assert!(!firmware_backplane_write_can_retry(
            &HalError::Unsupported("sdhci-int-timeout"),
            0,
        ));
    }

    #[test]
    fn firmware_transfer_stays_fast_until_byte_mode_fallback() {
        assert_eq!(CYW43_FIRMWARE_TRANSFER_CHUNK, CYW43_TRANSFER_CHUNK);
        assert!(!firmware_transfer_uses_byte_mode(false));
        assert!(firmware_transfer_uses_byte_mode(true));
    }

    #[test]
    fn firmware_window_write_retry_is_single_shot_on_command_errors() {
        assert!(firmware_window_write_can_retry(
            &HalError::Unsupported("sdhci-command-error"),
            0,
        ));
        assert!(!firmware_window_write_can_retry(
            &HalError::Unsupported("sdhci-command-error"),
            1,
        ));
        assert!(!firmware_window_write_can_retry(
            &HalError::Unsupported("sdhci-transfer-command"),
            0,
        ));
    }

    #[test]
    fn firmware_phase_retry_is_single_shot_on_command_path_errors() {
        assert!(firmware_phase_can_retry(
            &HalError::Unsupported("sdhci-command-error"),
            0,
        ));
        assert!(firmware_phase_can_retry(
            &HalError::Unsupported("sdhci-int-timeout"),
            0,
        ));
        assert!(firmware_phase_can_retry(
            &HalError::Unsupported("sdio-cmd52-write"),
            0,
        ));
        assert!(firmware_phase_can_retry(
            &HalError::Unsupported("sdio-cmd52-read"),
            0,
        ));
        assert!(!firmware_phase_can_retry(
            &HalError::Unsupported("sdhci-command-error"),
            1,
        ));
        assert!(!firmware_phase_can_retry(
            &HalError::Unsupported("sdio-cmd52-write"),
            1,
        ));
        assert!(!firmware_phase_can_retry(
            &HalError::Unsupported("cyw43-firmware-ready-timeout"),
            0,
        ));
    }

    #[test]
    fn io_direct_cmd53_byte_fallback_is_function1_only() {
        assert!(!io_direct_cmd53_byte_fallback_allowed(
            SdioFunction::Function0
        ));
        assert!(io_direct_cmd53_byte_fallback_allowed(
            SdioFunction::Function1
        ));
        assert!(!io_direct_cmd53_byte_fallback_allowed(
            SdioFunction::Function2
        ));
    }

    #[test]
    fn backplane_window_cache_skips_same_window_reprogramming() {
        let socram_reset = CYW43_SOCRAM_CORE_BASE + AI_RESETCTRL_OFFSET;
        let socram_ioctrl = CYW43_SOCRAM_CORE_BASE + AI_IOCTRL_OFFSET;
        let armcr4_ioctrl = CYW43_ARMCR4_CORE_BASE + AI_IOCTRL_OFFSET;
        let socram_window = backplane_window_base(socram_reset);

        assert_eq!(socram_window, backplane_window_base(socram_ioctrl));
        assert!(!backplane_window_reprogram_needed(
            Some(socram_window),
            socram_ioctrl
        ));
        assert!(backplane_window_reprogram_needed(
            Some(socram_window),
            armcr4_ioctrl
        ));
        assert!(backplane_window_reprogram_needed(None, socram_ioctrl));
    }

    #[test]
    fn mailbox_tag_names_cover_bringup_tags() {
        assert_eq!(mailbox_tag_name(TAG_SET_POWER_STATE), "set-power-state");
        assert_eq!(mailbox_tag_name(TAG_GET_CLOCK_RATE), "get-clock-rate");
        assert_eq!(mailbox_tag_name(TAG_NOTIFY_XHCI_RESET), "notify-xhci-reset");
        assert_eq!(mailbox_tag_name(TAG_SET_GPIO_CONFIG), "set-gpio-config");
        assert_eq!(mailbox_tag_name(0xffff_ffff), "unknown");
    }

    #[test]
    fn xhci_reset_notify_uses_extended_mailbox_receive_budget() {
        assert_eq!(
            mailbox_recv_wait_spins(TAG_NOTIFY_XHCI_RESET),
            MAILBOX_WAIT_SPINS_NOTIFY_XHCI_RESET
        );
        assert!(mailbox_recv_wait_spins(TAG_NOTIFY_XHCI_RESET) < MAILBOX_WAIT_SPINS);
        assert_eq!(
            mailbox_recv_wait_spins(TAG_SET_POWER_STATE),
            MAILBOX_WAIT_SPINS
        );
    }

    #[test]
    fn xhci_reset_notify_reuses_shared_acked_request_page_labels() {
        assert_eq!(
            mailbox_request_page_actions(),
            ("reuse-shared", "alloc-shared")
        );
    }

    #[test]
    fn xhci_reset_notify_is_the_only_posted_mailbox_fallback() {
        assert_eq!(
            mailbox_posted_alias(TAG_NOTIFY_XHCI_RESET),
            Some(VC_BUS_ALIAS_BASES[0])
        );
        assert_eq!(mailbox_posted_alias(TAG_SET_POWER_STATE), None);
        assert_eq!(mailbox_posted_alias(TAG_GET_CLOCK_RATE), None);
    }

    #[test]
    fn xhci_reset_notify_skips_acked_alias_retries() {
        assert_eq!(mailbox_ack_alias_count(TAG_NOTIFY_XHCI_RESET), 1);
        assert_eq!(
            mailbox_ack_alias_count(TAG_SET_POWER_STATE),
            VC_BUS_ALIAS_BASES.len()
        );
    }

    #[test]
    fn mailbox_reply_matching_accepts_same_request_page_across_aliases() {
        assert!(mailbox_reply_matches_request_page(0x4400_0000, 0xC400_0000));
        assert!(mailbox_reply_matches_request_page(0x4400_0000, 0xC400_0008));
    }

    #[test]
    fn mailbox_reply_matching_rejects_different_request_pages() {
        assert!(!mailbox_reply_matches_request_page(
            0x4400_0000,
            0xC400_1008
        ));
    }

    #[test]
    fn function_ready_transport_refresh_is_function2_only() {
        assert_eq!(
            sdio_function_ready_transport_stage(SDIO_FUNCTION_ENABLE_F1),
            None
        );
        assert_eq!(
            sdio_function_ready_transport_stage(SDIO_FUNCTION_ENABLE_F2),
            Some("sdio-function2-ready")
        );
        assert_eq!(sdio_function_ready_retry_limit(SDIO_FUNCTION_ENABLE_F1), 0);
        assert_eq!(
            sdio_function_ready_retry_limit(SDIO_FUNCTION_ENABLE_F2),
            SDIO_FUNCTION2_READY_RETRY_LIMIT
        );
    }

    #[test]
    fn function2_ready_budget_uses_linux_style_extended_phase() {
        assert!(
            sdio_function_ready_polls(SDIO_FUNCTION_ENABLE_F2)
                < sdio_function_ready_polls(SDIO_FUNCTION_ENABLE_F1)
        );
        assert_eq!(
            sdio_function_ready_extended_polls(SDIO_FUNCTION_ENABLE_F2),
            Some(SDIO_FUNCTION_READY_POLLS_FUNCTION2_EXTENDED)
        );
        assert_eq!(
            sdio_function_ready_extended_settle_loops(SDIO_FUNCTION_ENABLE_F2),
            Some(SDIO_FUNCTION_READY_SETTLE_LOOPS_FUNCTION2_EXTENDED)
        );
        assert_eq!(
            sdio_function_ready_extended_polls(SDIO_FUNCTION_ENABLE_F1),
            None
        );
        assert_eq!(
            sdio_function_ready_extended_settle_loops(SDIO_FUNCTION_ENABLE_F1),
            None
        );
    }

    #[test]
    fn function2_ready_budget_short_probe_paths_skip_extended_wait() {
        assert!(sdio_function_ready_uses_short_probe_only_budget(
            SDIO_FUNCTION_ENABLE_F2,
            SdioFunctionReadyBudget::ExperimentalBypass,
        ));
        assert!(!sdio_function_ready_uses_control_plane_reply_probe_budget(
            SDIO_FUNCTION_ENABLE_F2,
            SdioFunctionReadyBudget::ExperimentalBypass,
        ));
        assert_eq!(
            sdio_function_ready_retry_limit_for(
                SDIO_FUNCTION_ENABLE_F2,
                SdioFunctionReadyBudget::ExperimentalBypass
            ),
            0
        );
        assert_eq!(
            sdio_function_ready_retry_limit_for(
                SDIO_FUNCTION_ENABLE_F2,
                SdioFunctionReadyBudget::ControlPlaneReplyProbe
            ),
            0
        );
        assert_eq!(
            sdio_function_ready_extended_polls_for(
                SDIO_FUNCTION_ENABLE_F2,
                SdioFunctionReadyBudget::ExperimentalBypass
            ),
            None
        );
        assert_eq!(
            sdio_function_ready_extended_polls_for(
                SDIO_FUNCTION_ENABLE_F2,
                SdioFunctionReadyBudget::ControlPlaneReplyProbe
            ),
            Some(SDIO_FUNCTION_READY_POLLS_FUNCTION2_REPLY_PROBE)
        );
        assert_eq!(
            sdio_function_ready_extended_settle_loops_for(
                SDIO_FUNCTION_ENABLE_F2,
                SdioFunctionReadyBudget::ExperimentalBypass
            ),
            None
        );
        assert_eq!(
            sdio_function_ready_extended_settle_loops_for(
                SDIO_FUNCTION_ENABLE_F2,
                SdioFunctionReadyBudget::ControlPlaneReplyProbe
            ),
            Some(SDIO_FUNCTION_READY_SETTLE_LOOPS_FUNCTION2_REPLY_PROBE)
        );
        assert_eq!(
            sdio_function_ready_retry_limit_for(
                SDIO_FUNCTION_ENABLE_F2,
                SdioFunctionReadyBudget::Strict
            ),
            SDIO_FUNCTION2_READY_RETRY_LIMIT
        );
        assert_eq!(
            sdio_function_ready_extended_polls_for(
                SDIO_FUNCTION_ENABLE_F2,
                SdioFunctionReadyBudget::Strict
            ),
            Some(SDIO_FUNCTION_READY_POLLS_FUNCTION2_EXTENDED)
        );
        assert_eq!(
            sdio_function_ready_extended_settle_loops_for(
                SDIO_FUNCTION_ENABLE_F2,
                SdioFunctionReadyBudget::Strict
            ),
            Some(SDIO_FUNCTION_READY_SETTLE_LOOPS_FUNCTION2_EXTENDED)
        );
        assert_eq!(
            sdio_function_ready_retry_limit_for(
                SDIO_FUNCTION_ENABLE_F1,
                SdioFunctionReadyBudget::ExperimentalBypass
            ),
            0
        );
        assert_eq!(
            sdio_function_ready_extended_polls_for(
                SDIO_FUNCTION_ENABLE_F1,
                SdioFunctionReadyBudget::ExperimentalBypass
            ),
            None
        );
        assert_eq!(
            sdio_function_ready_extended_settle_loops_for(
                SDIO_FUNCTION_ENABLE_F1,
                SdioFunctionReadyBudget::ExperimentalBypass
            ),
            None
        );
        assert_eq!(
            sdio_function_ready_budget_name(SdioFunctionReadyBudget::ControlPlaneReplyProbe),
            "reply-probe"
        );
    }

    #[test]
    fn function2_ready_timeout_experimental_continue_requires_f1_ready() {
        let desired = SDIO_FUNC_ENABLE_1 | SDIO_FUNC_ENABLE_2;
        assert!(sdio_function_ready_timeout_can_continue_experimentally(
            SDIO_FUNCTION_ENABLE_F2,
            desired,
            SDIO_FUNC_READY_1,
            SdioFunctionReadyBudget::ExperimentalBypass,
        ));
        assert!(!sdio_function_ready_timeout_can_continue_experimentally(
            SDIO_FUNCTION_ENABLE_F2,
            desired,
            0,
            SdioFunctionReadyBudget::ExperimentalBypass,
        ));
        assert!(!sdio_function_ready_timeout_can_continue_experimentally(
            SDIO_FUNCTION_ENABLE_F2,
            desired,
            SDIO_FUNC_READY_1 | SDIO_FUNC_READY_2,
            SdioFunctionReadyBudget::ExperimentalBypass,
        ));
        assert!(!sdio_function_ready_timeout_can_continue_experimentally(
            SDIO_FUNCTION_ENABLE_F1,
            SDIO_FUNC_ENABLE_1,
            SDIO_FUNC_READY_1,
            SdioFunctionReadyBudget::ExperimentalBypass,
        ));
        assert!(!sdio_function_ready_timeout_can_continue_experimentally(
            SDIO_FUNCTION_ENABLE_F2,
            desired,
            SDIO_FUNC_READY_1,
            SdioFunctionReadyBudget::ControlPlaneReplyProbe,
        ));
        assert!(!sdio_function_ready_timeout_can_continue_experimentally(
            SDIO_FUNCTION_ENABLE_F2,
            desired,
            SDIO_FUNC_READY_1,
            SdioFunctionReadyBudget::Strict,
        ));
    }

    #[test]
    fn setup_firmware_channel_keeps_stable_order_even_with_bypass_enabled() {
        assert!(!setup_firmware_channel_uses_experimental_order(true));
        assert!(!setup_firmware_channel_uses_experimental_order(false));
    }

    #[test]
    fn firmware_channel_writes_drop_to_startup_clock_on_fast_paths() {
        assert_eq!(
            firmware_channel_write_restore_clock_hz(true, CYW43_CONTROL_PLANE_CLOCK_HZ),
            Some(CYW43_CONTROL_PLANE_CLOCK_HZ)
        );
        assert_eq!(
            firmware_channel_write_restore_clock_hz(true, CYW43_STARTUP_CLOCK_HZ),
            None
        );
        assert_eq!(
            firmware_channel_write_restore_clock_hz(false, CYW43_CONTROL_PLANE_CLOCK_HZ),
            Some(CYW43_CONTROL_PLANE_CLOCK_HZ)
        );
    }

    #[test]
    fn setup_firmware_channel_never_assumes_critical_writes_committed() {
        assert!(!setup_firmware_channel_can_assume_write_committed(
            true,
            0,
            &HalError::Unsupported("sdhci-transfer-finish"),
        ));
        assert!(!setup_firmware_channel_can_assume_write_committed(
            false,
            0,
            &HalError::Unsupported("sdhci-transfer-finish"),
        ));
        assert!(!setup_firmware_channel_can_assume_write_committed(
            true,
            1,
            &HalError::Unsupported("sdhci-transfer-finish"),
        ));
        assert!(!setup_firmware_channel_can_assume_write_committed(
            true,
            0,
            &HalError::Unsupported("cyw43-ht-clock-timeout"),
        ));
    }

    #[test]
    fn wait_for_firmware_ready_experimental_mailbox_read_tracks_bounded_no_ht_mode() {
        assert!(wait_for_firmware_ready_uses_experimental_mailbox_read(true));
        assert!(!wait_for_firmware_ready_uses_experimental_mailbox_read(
            false
        ));
    }

    #[test]
    fn wait_for_firmware_ready_stays_on_startup_clock_only_for_bounded_no_ht_fast_path() {
        assert_eq!(
            wait_for_firmware_ready_restore_clock_hz(true, CYW43_CONTROL_PLANE_CLOCK_HZ),
            Some(CYW43_CONTROL_PLANE_CLOCK_HZ)
        );
        assert_eq!(
            wait_for_firmware_ready_restore_clock_hz(true, CYW43_STARTUP_CLOCK_HZ),
            None
        );
        assert_eq!(
            wait_for_firmware_ready_restore_clock_hz(false, CYW43_CONTROL_PLANE_CLOCK_HZ),
            None
        );
    }

    #[test]
    fn wait_for_firmware_ready_assumes_mailbox_ready_only_after_bounded_retry() {
        assert!(!wait_for_firmware_ready_can_assume_mailbox_ready(
            true,
            0,
            &HalError::Unsupported("sdhci-transfer-data"),
        ));
        assert!(!wait_for_firmware_ready_can_assume_mailbox_ready(
            false,
            1,
            &HalError::Unsupported("sdhci-transfer-data"),
        ));
        assert!(wait_for_firmware_ready_can_assume_mailbox_ready(
            true,
            1,
            &HalError::Unsupported("sdhci-transfer-data"),
        ));
        assert!(wait_for_firmware_ready_can_assume_mailbox_ready(
            true,
            1,
            &HalError::Unsupported("sdhci-int-timeout"),
        ));
        assert!(wait_for_firmware_ready_can_assume_mailbox_ready(
            true,
            1,
            &HalError::Unsupported("sdio-cmd52-read"),
        ));
        assert!(!wait_for_firmware_ready_can_assume_mailbox_ready(
            true,
            1,
            &HalError::Unsupported("cyw43-firmware-ready-timeout"),
        ));
    }

    #[test]
    fn sdhci_buffer_ready_masks_follow_transfer_direction() {
        assert_eq!(sdhci_present_buffer_ready_mask(true), SDHCI_SPACE_AVAILABLE);
        assert_eq!(sdhci_present_buffer_ready_mask(false), SDHCI_DATA_AVAILABLE);
        assert_eq!(
            sdhci_interrupt_buffer_ready_mask(true),
            SDHCI_INT_SPACE_AVAIL
        );
        assert_eq!(
            sdhci_interrupt_buffer_ready_mask(false),
            SDHCI_INT_DATA_AVAIL
        );
    }

    #[test]
    fn ht_clock_request_requests_ht_only() {
        assert_eq!(ht_clock_request_value(), SBSDIO_HT_AVAIL_REQ);
    }

    #[test]
    fn transport_phase_chipclk_refresh_preserves_ht_request_once_ready() {
        assert_eq!(transport_phase_chipclk_value(None), SBSDIO_FORCE_HT);
        assert_eq!(
            transport_phase_chipclk_value(Some(SBSDIO_FORCE_HT)),
            SBSDIO_FORCE_HT
        );
        assert_eq!(
            transport_phase_chipclk_value(Some(SBSDIO_HT_AVAIL_REQ)),
            SBSDIO_HT_AVAIL_REQ | SBSDIO_FORCE_HT
        );
        assert_eq!(
            transport_phase_chipclk_value(Some(SBSDIO_ALP_AVAIL_REQ)),
            SBSDIO_ALP_AVAIL_REQ | SBSDIO_FORCE_HT
        );
        assert_eq!(
            transport_phase_chipclk_value(Some(SBSDIO_HT_AVAIL)),
            SBSDIO_HT_AVAIL_REQ | SBSDIO_FORCE_HT
        );
        assert_eq!(
            transport_phase_chipclk_value(Some(SBSDIO_HT_AVAIL_REQ | SBSDIO_HT_AVAIL)),
            SBSDIO_HT_AVAIL_REQ | SBSDIO_FORCE_HT
        );
        assert_eq!(
            transport_phase_chipclk_value(Some(
                SBSDIO_ALP_AVAIL_REQ | SBSDIO_HT_AVAIL_REQ | SBSDIO_FORCE_HW_CLKREQ_OFF
            )),
            SBSDIO_ALP_AVAIL_REQ
                | SBSDIO_HT_AVAIL_REQ
                | SBSDIO_FORCE_HW_CLKREQ_OFF
                | SBSDIO_FORCE_HT
        );
    }

    #[test]
    fn ht_clock_alp_prime_request_keeps_force_and_retry_bits_asserted() {
        assert_eq!(
            ht_clock_alp_prime_request_value(None),
            SBSDIO_ALP_AVAIL_REQ | SBSDIO_FORCE_HT
        );
        assert_eq!(
            ht_clock_alp_prime_request_value(Some(
                SBSDIO_ALP_AVAIL_REQ | SBSDIO_HT_AVAIL_REQ | SBSDIO_FORCE_HT
            )),
            SBSDIO_ALP_AVAIL_REQ | SBSDIO_HT_AVAIL_REQ | SBSDIO_FORCE_HT
        );
        assert_eq!(
            ht_clock_alp_prime_request_value(Some(
                SBSDIO_ALP_AVAIL_REQ
                    | SBSDIO_HT_AVAIL_REQ
                    | SBSDIO_FORCE_HW_CLKREQ_OFF
                    | SBSDIO_ALP_AVAIL
            )),
            SBSDIO_ALP_AVAIL_REQ
                | SBSDIO_HT_AVAIL_REQ
                | SBSDIO_FORCE_HW_CLKREQ_OFF
                | SBSDIO_FORCE_HT
        );
    }

    #[test]
    fn firmware_bulk_clock_candidates_step_down_and_append_restore_clock() {
        assert_eq!(
            firmware_bulk_clock_candidates(400_000, true),
            [CYW43_FIRMWARE_BULK_CLOCK_HZ, 6_250_000, 3_125_000, 400_000]
        );
    }

    #[test]
    fn firmware_bulk_clock_candidates_startup_first_when_ht_not_ready() {
        assert_eq!(
            firmware_bulk_clock_candidates(400_000, false),
            [400_000, 0, 0, 0]
        );
    }

    #[test]
    fn firmware_upload_prefers_byte_mode_only_on_low_clock_non_ht_path() {
        assert!(firmware_upload_prefers_byte_mode(400_000, false));
        assert!(!firmware_upload_prefers_byte_mode(
            CYW43_FIRMWARE_BULK_CLOCK_HZ,
            true
        ));
        assert!(!firmware_upload_prefers_byte_mode(6_250_000, false));
    }

    #[test]
    fn control_plane_clock_target_raises_startup_clock_to_operational_floor() {
        assert_eq!(
            control_plane_clock_target_hz(400_000, 400_000),
            CYW43_CONTROL_PLANE_CLOCK_HZ
        );
        assert_eq!(
            control_plane_clock_target_hz(400_000, CYW43_FIRMWARE_BULK_CLOCK_HZ),
            CYW43_FIRMWARE_BULK_CLOCK_HZ
        );
        assert_eq!(
            control_plane_clock_target_hz(CYW43_CONTROL_PLANE_CLOCK_HZ, 400_000),
            CYW43_CONTROL_PLANE_CLOCK_HZ
        );
    }

    #[test]
    fn ht_clock_timeout_can_continue_when_alp_stays_up() {
        assert!(ht_clock_timeout_can_continue(
            true,
            SBSDIO_HT_AVAIL_REQ | SBSDIO_ALP_AVAIL
        ));
        assert!(!ht_clock_timeout_can_continue(true, SBSDIO_HT_AVAIL_REQ));
        assert!(!ht_clock_timeout_can_continue(true, SBSDIO_ALP_AVAIL));
        assert!(!ht_clock_timeout_can_continue(
            false,
            SBSDIO_HT_AVAIL_REQ | SBSDIO_ALP_AVAIL
        ));
    }

    #[test]
    fn ht_clock_timeout_can_enter_bounded_no_ht_transport_only_with_force_ht_and_shadow() {
        assert!(ht_clock_timeout_can_enter_bounded_no_ht_transport(
            Some(SBSDIO_FORCE_HT | SBSDIO_HT_AVAIL_REQ | SBSDIO_ALP_AVAIL),
            Some(SBSDIO_WAKE_TILL_HT_AVAIL),
            Some(SBSDIO_FUNC1_SLEEPCSR_KSO_EN),
            Some(SDIO_CCCR_BRCM_CARDCAP_CMD_NODEC),
        ));
        assert!(!ht_clock_timeout_can_enter_bounded_no_ht_transport(
            Some(SBSDIO_HT_AVAIL_REQ | SBSDIO_ALP_AVAIL),
            Some(SBSDIO_WAKE_TILL_HT_AVAIL),
            Some(SBSDIO_FUNC1_SLEEPCSR_KSO_EN),
            Some(SDIO_CCCR_BRCM_CARDCAP_CMD_NODEC),
        ));
        assert!(!ht_clock_timeout_can_enter_bounded_no_ht_transport(
            Some(SBSDIO_FORCE_HT | SBSDIO_HT_AVAIL_REQ | SBSDIO_ALP_AVAIL),
            Some(SBSDIO_WAKE_TILL_HT_AVAIL),
            None,
            Some(SDIO_CCCR_BRCM_CARDCAP_CMD_NODEC),
        ));
    }

    #[test]
    fn cyw43_transport_mode_names_are_stable() {
        assert_eq!(cyw43_transport_mode_name(false), "strict");
        assert_eq!(cyw43_transport_mode_name(true), "bounded-no-ht");
    }

    #[test]
    fn ht_clock_soft_wait_budget_is_shorter_than_required_wait_budget() {
        assert!(super::CYW43_HT_CLOCK_SOFT_WAIT_LOOPS < SDIO_INIT_WAIT_LOOPS);
    }

    #[test]
    fn stronger_required_ht_retry_uses_extended_wait_budget() {
        assert_eq!(required_ht_clock_wait_loops(false), SDIO_INIT_WAIT_LOOPS);
        assert_eq!(
            required_ht_clock_wait_loops(true),
            super::CYW43_CORE_CONTROL_SETTLE_LOOPS
        );
    }

    #[test]
    fn bounded_no_ht_shortcut_wait_budget_matches_one_soft_wait_chunk() {
        assert_eq!(
            required_ht_clock_bounded_no_ht_shortcut_loops(),
            super::CYW43_HT_CLOCK_SOFT_WAIT_LOOPS
        );
        assert!(
            required_ht_clock_bounded_no_ht_shortcut_loops() < required_ht_clock_wait_loops(true)
        );
    }

    #[test]
    fn ht_clock_progress_chunk_uses_soft_wait_budget_as_cap() {
        assert_eq!(
            ht_clock_progress_chunk_loops(super::CYW43_HT_CLOCK_SOFT_WAIT_LOOPS * 4),
            super::CYW43_HT_CLOCK_SOFT_WAIT_LOOPS
        );
        assert_eq!(ht_clock_progress_chunk_loops(1234), 1234);
        assert_eq!(ht_clock_progress_chunk_loops(0), 0);
    }

    #[test]
    fn required_ht_clock_request_keeps_force_ht_asserted() {
        assert_eq!(
            required_ht_clock_request_value(None),
            SBSDIO_ALP_AVAIL_REQ | SBSDIO_HT_AVAIL_REQ | SBSDIO_FORCE_HT
        );
        assert_eq!(
            required_ht_clock_request_value(Some(SBSDIO_HT_AVAIL_REQ | SBSDIO_ALP_AVAIL)),
            SBSDIO_ALP_AVAIL_REQ | SBSDIO_HT_AVAIL_REQ | SBSDIO_FORCE_HT
        );
        assert_eq!(
            required_ht_clock_request_value(Some(SBSDIO_FORCE_HT)),
            SBSDIO_ALP_AVAIL_REQ | SBSDIO_HT_AVAIL_REQ | SBSDIO_FORCE_HT
        );
    }

    #[test]
    fn required_ht_clock_retry_request_adds_force_hw_clkreq_off() {
        assert_eq!(
            required_ht_clock_retry_request_value(None),
            SBSDIO_ALP_AVAIL_REQ
                | SBSDIO_HT_AVAIL_REQ
                | SBSDIO_FORCE_HT
                | SBSDIO_FORCE_HW_CLKREQ_OFF
        );
        assert_eq!(
            required_ht_clock_retry_request_value(Some(SBSDIO_HT_AVAIL_REQ | SBSDIO_ALP_AVAIL)),
            SBSDIO_ALP_AVAIL_REQ
                | SBSDIO_HT_AVAIL_REQ
                | SBSDIO_FORCE_HT
                | SBSDIO_FORCE_HW_CLKREQ_OFF
        );
    }

    #[test]
    fn stronger_ht_retry_cuts_over_early_only_after_shortcut_budget_and_shadow_contract() {
        let chipclk = Some(SBSDIO_FORCE_HT | SBSDIO_HT_AVAIL_REQ | SBSDIO_ALP_AVAIL);
        let wake = Some(SBSDIO_WAKE_TILL_HT_AVAIL);
        let sleep = Some(SBSDIO_FUNC1_SLEEPCSR_KSO_EN);
        let cardcap = Some(SDIO_CCCR_BRCM_CARDCAP_CMD_NODEC);
        let shortcut = required_ht_clock_bounded_no_ht_shortcut_loops();

        assert!(!ht_clock_retry_can_cutover_to_bounded_no_ht_early(
            true,
            shortcut.saturating_sub(1),
            chipclk,
            wake,
            sleep,
            cardcap,
        ));
        assert!(ht_clock_retry_can_cutover_to_bounded_no_ht_early(
            true, shortcut, chipclk, wake, sleep, cardcap,
        ));
        assert!(!ht_clock_retry_can_cutover_to_bounded_no_ht_early(
            false, shortcut, chipclk, wake, sleep, cardcap,
        ));
        assert!(!ht_clock_retry_can_cutover_to_bounded_no_ht_early(
            true,
            shortcut,
            Some(SBSDIO_HT_AVAIL_REQ | SBSDIO_ALP_AVAIL),
            wake,
            sleep,
            cardcap,
        ));
    }

    #[test]
    fn sdio_retry_settle_budget_is_shorter_than_power_on_fallback_budget() {
        assert!(super::SDIO_CARD_INIT_RETRY_SETTLE_LOOPS < super::SDHCI_POWER_SETTLE_LOOPS);
    }

    #[test]
    fn next_distinct_firmware_bulk_clock_candidate_skips_duplicates() {
        let unique_restore = firmware_bulk_clock_candidates(400_000, true);
        assert_eq!(
            next_distinct_firmware_bulk_clock_candidate(&unique_restore, 0),
            Some(6_250_000)
        );
        assert_eq!(
            next_distinct_firmware_bulk_clock_candidate(&unique_restore, 1),
            Some(3_125_000)
        );
        assert_eq!(
            next_distinct_firmware_bulk_clock_candidate(&unique_restore, 2),
            Some(400_000)
        );
        assert_eq!(
            next_distinct_firmware_bulk_clock_candidate(&unique_restore, 3),
            None
        );

        let duplicate_restore =
            firmware_bulk_clock_candidates(CYW43_FIRMWARE_BULK_CLOCK_HZ / 2, true);
        assert_eq!(
            next_distinct_firmware_bulk_clock_candidate(&duplicate_restore, 0),
            Some(6_250_000)
        );
        assert_eq!(
            next_distinct_firmware_bulk_clock_candidate(&duplicate_restore, 1),
            Some(3_125_000)
        );
        assert_eq!(
            next_distinct_firmware_bulk_clock_candidate(&duplicate_restore, 2),
            None
        );
    }

    #[test]
    fn ht_clock_assist_constants_match_broadcom_sequence() {
        assert_eq!(SBSDIO_FORCE_HT, 0x02);
        assert_eq!(SBSDIO_WAKE_TILL_HT_AVAIL, 0x02);
        assert_eq!(SDIO_CCCR_BRCM_CARDCAP_CMD_NODEC, 0x08);
    }

    #[test]
    fn ht_clock_assist_shadow_refresh_requires_cached_registers() {
        assert!(ht_clock_assist_shadow_is_complete(
            Some(0x02),
            Some(0x01),
            Some(0x08),
        ));
        assert!(!ht_clock_assist_shadow_is_complete(
            None,
            Some(0x01),
            Some(0x08)
        ));
        assert!(!ht_clock_assist_shadow_is_complete(
            Some(0x02),
            None,
            Some(0x08)
        ));
        assert!(!ht_clock_assist_shadow_is_complete(
            Some(0x02),
            Some(0x01),
            None,
        ));
    }

    #[test]
    fn clear_reset_keepalive_chunks_match_control_settle_window() {
        assert_eq!(
            clear_reset_keepalive_chunk_loops(CYW43_SOCRAM_CLEAR_RESET_KEEPALIVE_CHUNK_LOOPS * 4),
            CYW43_SOCRAM_CLEAR_RESET_KEEPALIVE_CHUNK_LOOPS,
        );
        assert_eq!(clear_reset_keepalive_chunk_loops(1234), 1234);
        assert_eq!(clear_reset_keepalive_chunk_loops(0), 0);
    }

    #[test]
    fn pre_reset_ht_assist_prime_follows_cached_chipclk_state() {
        assert!(should_prime_ht_clock_assist_before_reset(None));
        assert!(should_prime_ht_clock_assist_before_reset(Some(0)));
        assert!(!should_prime_ht_clock_assist_before_reset(Some(
            SBSDIO_FORCE_HT
        )));
        assert!(!should_prime_ht_clock_assist_before_reset(Some(
            SBSDIO_HT_AVAIL_REQ
        )));
        assert!(!should_prime_ht_clock_assist_before_reset(Some(
            SBSDIO_HT_AVAIL
        )));
    }

    #[test]
    fn mailbox_protocol_error_match_is_exact() {
        assert!(is_mailbox_protocol_error(&HalError::Unsupported(
            "mailbox-protocol"
        )));
        assert!(!is_mailbox_protocol_error(&HalError::Unsupported(
            "mailbox-timeout"
        )));
    }

    #[test]
    fn r5_status_extracts_only_error_bits() {
        assert_eq!(r5_status(0), 0);
        assert_eq!(r5_status(0xCB00), 0xCB00);
        assert_eq!(r5_status(0xFFFF_FFFF), 0xCB00);
    }

    #[test]
    fn cmd52_argument_encodes_backplane_register_accesses() {
        assert_eq!(
            cmd52_argument(SdioFunction::Function1, 0x1000E, true, 0x08),
            0x9200_1c08
        );
        assert_eq!(
            cmd52_argument(SdioFunction::Function1, 0x1000F, true, 0x00),
            0x9200_1e00
        );
    }

    #[test]
    fn ai_core_iocontrol_values_match_upstream_sequence() {
        assert_eq!(
            AI_CORE_PRERESET_IOCTRL,
            AI_IOCTRL_BIT_FGC | AI_IOCTRL_BIT_CLOCK_EN
        );
        assert_eq!(AI_CORE_POSTRESET_IOCTRL, AI_IOCTRL_BIT_CLOCK_EN);
    }

    #[test]
    fn ai_core_is_up_matches_expected_reset_state() {
        assert!(ai_core_is_up(AI_CORE_POSTRESET_IOCTRL, 0));
        assert!(!ai_core_is_up(0, AI_RESETCTRL_BIT_RESET));
        assert!(!ai_core_is_up(AI_CORE_PRERESET_IOCTRL, 0));
    }

    #[test]
    fn ai_core_state_reason_classifies_reset_and_clock_states() {
        assert_eq!(ai_core_state_reason(AI_CORE_POSTRESET_IOCTRL, 0), "core-up");
        assert_eq!(
            ai_core_state_reason(0, AI_RESETCTRL_BIT_RESET),
            "reset-held-clock-off"
        );
        assert_eq!(
            ai_core_state_reason(AI_CORE_PRERESET_IOCTRL, AI_RESETCTRL_BIT_RESET),
            "reset-held-fgc-clock"
        );
        assert_eq!(
            ai_core_state_reason(AI_CORE_PRERESET_IOCTRL, 0),
            "fgc-still-set"
        );
        assert_eq!(
            ai_core_state_reason(
                ARMCR4_BCMA_IOCTL_CPUHALT | AI_CORE_PRERESET_IOCTRL,
                AI_RESETCTRL_BIT_RESET
            ),
            "reset-held-fgc-clock-extras"
        );
        assert_eq!(
            ai_core_state_reason(ARMCR4_BCMA_IOCTL_CPUHALT | AI_CORE_POSTRESET_IOCTRL, 0),
            "clock-en-extras"
        );
    }

    #[test]
    fn armcr4_cpuhalt_matches_upstream_handoff_sequence() {
        assert_eq!(ARMCR4_BCMA_IOCTL_CPUHALT, 0x20);
        assert_eq!(ARMCR4_BCMA_IOCTL_CPUHALT | AI_CORE_PRERESET_IOCTRL, 0x23);
        assert_eq!(ARMCR4_BCMA_IOCTL_CPUHALT | AI_CORE_POSTRESET_IOCTRL, 0x21);
    }

    #[test]
    fn wifi_gpio_transition_target_only_requests_real_power_changes() {
        assert_eq!(
            wifi_gpio_transition_target(false, WifiPowerState::Off),
            None
        );
        assert_eq!(
            wifi_gpio_transition_target(false, WifiPowerState::On),
            Some(true)
        );
        assert_eq!(wifi_gpio_transition_target(true, WifiPowerState::On), None);
        assert_eq!(
            wifi_gpio_transition_target(true, WifiPowerState::Off),
            Some(false)
        );
    }

    #[test]
    fn phys_to_bus_preserves_low_bits_and_applies_alias() {
        assert_eq!(phys_to_bus(0x3F00_B880, 0xC000_0000), Some(0xFF00_B880));
        assert_eq!(phys_to_bus(0x3F00_B880, 0x4000_0000), Some(0x7F00_B880));
    }

    #[test]
    fn sdhci_status_reason_prefers_specific_error_bits() {
        assert_eq!(sdhci_status_reason(SDHCI_INT_TIMEOUT), "timeout");
        assert_eq!(sdhci_status_reason(SDHCI_INT_CRC), "crc");
        assert_eq!(sdhci_status_reason(SDHCI_INT_DATA_CRC), "data-crc");
        assert_eq!(sdhci_status_reason(0), "unknown");
    }

    #[test]
    fn wifi_gpio_line_follows_power_state() {
        assert!(!wifi_gpio_line_enabled(WifiPowerState::Off));
        assert!(wifi_gpio_line_enabled(WifiPowerState::On));
    }

    #[test]
    fn sdhci_power_ready_requires_power_and_stable_card_state() {
        let ready_power = SDHCI_POWER_330 | SDHCI_POWER_ON;
        let ready_present = SDHCI_CARD_PRESENT | SDHCI_CARD_STATE_STABLE;
        assert!(sdhci_power_ready(ready_power, ready_present));
        assert!(!sdhci_power_ready(SDHCI_POWER_330, ready_present));
        assert!(!sdhci_power_ready(
            ready_power,
            ready_present | SDHCI_CMD_INHIBIT,
        ));
        assert!(!sdhci_power_ready(ready_power, SDHCI_CARD_PRESENT));
    }

    #[test]
    fn wifi_sdio_pinmux_matches_pi4_dtb_state() {
        let mut fsel3 = 0u32;
        let mut pud2 = 0u32;
        for &pin in &PI4_WIFI_SDIO_PINS {
            assert_eq!(bcm2711_gpfsel_offset(pin), bcm2711_gpfsel_offset(34));
            fsel3 = update_bcm2711_gpio_function(fsel3, pin, BCM2711_GPIO_ALT3);
        }
        for (&pin, &pull) in PI4_WIFI_SDIO_PINS.iter().zip(PI4_WIFI_SDIO_PULLS.iter()) {
            assert_eq!(bcm2711_puppdn_offset(pin), bcm2711_puppdn_offset(34));
            pud2 = update_bcm2711_gpio_pull(pud2, pin, pull);
        }

        assert_eq!(fsel3, 0x00ff_fc00);
        assert_eq!(pud2, 0x00000aa0);
    }

    #[test]
    fn sdhci_command_word_uses_transfer_mode_shadow() {
        let shadow = merge_u16_word(0, SDHCI_TRANSFER_MODE, 0x1234);
        let combined = merge_u16_word(shadow, SDHCI_COMMAND, 0xabcd);
        assert_eq!(combined, 0xabcd_1234);
    }

    #[test]
    fn sdhci_block_size_count_word_uses_shadow() {
        let shadow = merge_u16_word(0, SDHCI_BLOCK_SIZE, 0x0040);
        let combined = merge_u16_word(shadow, SDHCI_BLOCK_COUNT, 0x0001);
        assert_eq!(combined, 0x0001_0040);
    }

    #[test]
    fn sdhci_write_gap_spin_delay_is_bounded() {
        assert_eq!(SDHCI_WRITE_DELAY_LOOPS, 256);
        assert_eq!(SDHCI_WRITE_GAP_SPIN_LOOPS, 8192);
    }

    #[test]
    fn sdio_function_enable_sequence_brings_up_f1_then_f2() {
        assert_eq!(
            SDIO_FUNCTION_ENABLE_SEQUENCE[0].function,
            SdioFunction::Function1
        );
        assert_eq!(
            SDIO_FUNCTION_ENABLE_SEQUENCE[0].enable_bit,
            SDIO_FUNC_ENABLE_1
        );
        assert_eq!(
            SDIO_FUNCTION_ENABLE_SEQUENCE[0].ready_bit,
            SDIO_FUNC_READY_1
        );
        assert_eq!(SDIO_FUNCTION_ENABLE_SEQUENCE[0].block_size, 64);
        assert_eq!(
            SDIO_FUNCTION_ENABLE_SEQUENCE[1].function,
            SdioFunction::Function2
        );
        assert_eq!(
            SDIO_FUNCTION_ENABLE_SEQUENCE[1].enable_bit,
            SDIO_FUNC_ENABLE_2
        );
        assert_eq!(
            SDIO_FUNCTION_ENABLE_SEQUENCE[1].ready_bit,
            SDIO_FUNC_READY_2
        );
        assert_eq!(SDIO_FUNCTION_ENABLE_SEQUENCE[1].block_size, 512);
    }

    #[test]
    fn pinned_mailbox_regs_reports_cached_mapping() {
        let original = PINNED_MAILBOX_REGS.lock().take();
        {
            let mut slot = PINNED_MAILBOX_REGS.lock();
            *slot = Some(MappedRegs {
                paddr: 0xFE00_B000,
                vaddr: 0x1234_5000,
            });
        }
        assert_eq!(pinned_mailbox_regs(), Some((0xFE00_B000, 0x1234_5000)));
        let mut slot = PINNED_MAILBOX_REGS.lock();
        *slot = original;
    }
}
