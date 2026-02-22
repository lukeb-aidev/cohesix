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

use font8x8::legacy::BASIC_LEGACY;
use spin::Mutex;
use usb_oxide::{
    find_hid_interfaces, hid_protocol, hid_subclass, scancode_to_ascii, ConfigDesc, Dma, HidDevice,
    UsbDevice, XhciCtrl,
};

use crate::bootstrap::log as boot_log;
use crate::hal::{Hardware, KernelHal};

const PAGE_SIZE: usize = 4096;
const PAGE_MASK: usize = PAGE_SIZE - 1;

const MAILBOX_PAGE_PADDR: usize = 0xFE00_B000;
const MAILBOX_READ_OFFSET: usize = 0x880;
const MAILBOX_STATUS0_OFFSET: usize = 0x898;
const MAILBOX_WRITE_OFFSET: usize = 0x8A0;
const MAILBOX_STATUS1_OFFSET: usize = 0x8B8;
const MAILBOX_EMPTY: u32 = 0x4000_0000;
const MAILBOX_FULL: u32 = 0x8000_0000;
const MAILBOX_CHANNEL_PROPERTY: u32 = 8;
const MAILBOX_RESPONSE_SUCCESS: u32 = 0x8000_0000;
const MAILBOX_VALUE_RESPONSE: u32 = 1 << 31;
const MAILBOX_WAIT_SPINS: usize = 5_000_000;

const VC_BUS_UNCACHED_BASE: u32 = 0xC000_0000;
const VC_BUS_MASK: u32 = 0x3FFF_FFFF;

const TAG_SET_PHYSICAL_SIZE: u32 = 0x0004_8003;
const TAG_SET_VIRTUAL_SIZE: u32 = 0x0004_8004;
const TAG_SET_DEPTH: u32 = 0x0004_8005;
const TAG_SET_PIXEL_ORDER: u32 = 0x0004_8006;
const TAG_ALLOCATE_BUFFER: u32 = 0x0004_0001;
const TAG_GET_PITCH: u32 = 0x0004_0008;

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

const RPI4_XHCI_MMIO_FALLBACKS: [usize; 3] = [
    0x0000_0006_0000_0000,
    0x0000_0000_FE98_0000,
    0x0000_0000_7E98_0000,
];
const XHCI_MMIO_CANDIDATE_LIMIT: usize = 4;
const XHCI_MAX_PROBE_PORTS: usize = 16;
const KEYBOARD_ATTACH_ATTEMPTS: usize = 2;
const KEYBOARD_RETRY_SPINS: usize = 200_000;

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
}

impl core::fmt::Debug for Pi4LocalSeat {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Pi4LocalSeat").finish_non_exhaustive()
    }
}

impl Pi4LocalSeat {
    /// Initialize the Pi4 local-seat backend.
    pub fn new(
        hal: &mut KernelHal<'_>,
        xhci_mmio_hint: Option<usize>,
    ) -> Result<Self, Pi4SeatError> {
        let mut display = HdmiTextSink::new(hal)?;
        display.write_line("[cohesix] local-seat HDMI online");

        let mut keyboard = None;
        let mut keyboard_error = None;
        for attempt in 1..=KEYBOARD_ATTACH_ATTEMPTS {
            match UsbKeyboard::new(hal, xhci_mmio_hint) {
                Ok(found) => {
                    keyboard = Some(found);
                    if attempt > 1 {
                        let mut line = heapless::String::<160>::new();
                        let _ = core::fmt::Write::write_fmt(
                            &mut line,
                            format_args!("[local-seat] pi4 keyboard attached on retry={attempt}"),
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

        if keyboard.is_some() {
            display.write_line("[cohesix] local-seat USB keyboard online");
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
            display.write_line("[cohesix] local-seat USB keyboard unavailable");
        }
        Ok(Self { display, keyboard })
    }

    /// Mirror one rendered line to HDMI.
    pub fn write_line(&mut self, line: &str) {
        self.display.write_line(line);
    }

    /// Poll USB keyboard and write canonical bytes into `out`.
    pub fn poll_keyboard_bytes(&mut self, out: &mut [u8]) -> usize {
        match self.keyboard.as_mut() {
            Some(keyboard) => keyboard.poll_bytes(out),
            None => 0,
        }
    }
}

struct Mailbox {
    regs: crate::sel4::DeviceFrame,
    request: crate::sel4::RamFrame,
}

impl Mailbox {
    fn new(hal: &mut KernelHal<'_>) -> Result<Self, Pi4SeatError> {
        let regs = hal
            .map_device(MAILBOX_PAGE_PADDR)
            .map_err(|_| Pi4SeatError::MailboxMap)?;
        let request = hal
            .alloc_dma_frame_low()
            .map_err(|_| Pi4SeatError::MailboxDma)?;
        Ok(Self { regs, request })
    }

    fn call_tag(
        &mut self,
        tag: u32,
        request_len_bytes: u32,
        payload: &mut [u32],
    ) -> Result<(), Pi4SeatError> {
        let words = {
            let bytes = self.request.as_mut_slice();
            // SAFETY: The DMA request page is 4-byte aligned and sized to PAGE_SIZE.
            unsafe {
                core::slice::from_raw_parts_mut(bytes.as_mut_ptr().cast::<u32>(), PAGE_SIZE / 4)
            }
        };

        let total_words = 2usize
            .saturating_add(3)
            .saturating_add(payload.len())
            .saturating_add(1);
        if total_words > words.len() {
            return Err(Pi4SeatError::MailboxProtocol);
        }

        words[0] = (total_words * mem::size_of::<u32>()) as u32;
        words[1] = 0;
        words[2] = tag;
        words[3] = (payload.len() * mem::size_of::<u32>()) as u32;
        words[4] = request_len_bytes;
        words[5..5 + payload.len()].copy_from_slice(payload);
        words[5 + payload.len()] = 0;

        let request_bus = phys_to_bus(self.request.paddr()).ok_or(Pi4SeatError::MailboxDma)?;
        self.mailbox_send(request_bus)?;

        if words[1] != MAILBOX_RESPONSE_SUCCESS {
            return Err(Pi4SeatError::MailboxProtocol);
        }
        if words[2] != tag {
            return Err(Pi4SeatError::MailboxProtocol);
        }
        if (words[4] & MAILBOX_VALUE_RESPONSE) == 0 {
            return Err(Pi4SeatError::MailboxProtocol);
        }

        payload.copy_from_slice(&words[5..5 + payload.len()]);
        Ok(())
    }

    fn mailbox_send(&self, data: u32) -> Result<(), Pi4SeatError> {
        let mut wait = 0usize;
        while self.read_reg(MAILBOX_STATUS1_OFFSET) & MAILBOX_FULL != 0 {
            wait = wait.saturating_add(1);
            if wait >= MAILBOX_WAIT_SPINS {
                return Err(Pi4SeatError::MailboxTimeout);
            }
            spin_loop();
        }

        self.write_reg(
            MAILBOX_WRITE_OFFSET,
            (data & !0xF) | (MAILBOX_CHANNEL_PROPERTY & 0xF),
        );

        wait = 0;
        loop {
            while self.read_reg(MAILBOX_STATUS0_OFFSET) & MAILBOX_EMPTY != 0 {
                wait = wait.saturating_add(1);
                if wait >= MAILBOX_WAIT_SPINS {
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

    fn read_reg(&self, offset: usize) -> u32 {
        let base = self.regs.ptr().as_ptr() as usize;
        // SAFETY: Register block was mapped as device memory by HAL.
        unsafe { ptr::read_volatile((base + offset) as *const u32) }
    }

    fn write_reg(&self, offset: usize, value: u32) {
        let base = self.regs.ptr().as_ptr() as usize;
        // SAFETY: Register block was mapped as device memory by HAL.
        unsafe {
            ptr::write_volatile((base + offset) as *mut u32, value);
        }
    }
}

struct HdmiTextSink {
    width: usize,
    height: usize,
    pitch: usize,
    cols: usize,
    rows: usize,
    row: usize,
    col: usize,
    framebuffer: *mut u8,
    mappings: Vec<crate::sel4::DeviceFrame>,
}

impl HdmiTextSink {
    fn new(hal: &mut KernelHal<'_>) -> Result<Self, Pi4SeatError> {
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
        let page_base = fb_phys & !PAGE_MASK;
        let page_offset = fb_phys & PAGE_MASK;
        let map_len = page_offset.saturating_add(fb_size);
        let page_count = div_ceil(map_len, PAGE_SIZE);
        if page_count == 0 {
            return Err(Pi4SeatError::FramebufferUnavailable);
        }

        let mut mappings = Vec::with_capacity(page_count);
        for page in 0..page_count {
            let paddr = page_base.saturating_add(page.saturating_mul(PAGE_SIZE));
            let frame = hal
                .map_device(paddr)
                .map_err(|_| Pi4SeatError::FramebufferMap)?;
            mappings.push(frame);
        }

        let first = mappings
            .first()
            .ok_or(Pi4SeatError::FramebufferMap)?
            .ptr()
            .as_ptr() as usize;
        for (idx, frame) in mappings.iter().enumerate() {
            let expected = first.saturating_add(idx.saturating_mul(PAGE_SIZE));
            let got = frame.ptr().as_ptr() as usize;
            if got != expected {
                return Err(Pi4SeatError::FramebufferMap);
            }
        }

        let framebuffer = (first + page_offset) as *mut u8;

        let width = virt[0] as usize;
        let height = virt[1] as usize;
        if width == 0 || height == 0 {
            return Err(Pi4SeatError::FramebufferUnavailable);
        }

        let mut sink = Self {
            width,
            height,
            pitch: pitch[0] as usize,
            cols: cmp::max(1, width / CHAR_WIDTH),
            rows: cmp::max(1, height / CHAR_HEIGHT),
            row: 0,
            col: 0,
            framebuffer,
            mappings,
        };

        sink.clear_screen();
        Ok(sink)
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
        let byte_off = y
            .saturating_mul(self.pitch)
            .saturating_add(x.saturating_mul(mem::size_of::<u32>()));
        let addr = (self.framebuffer as usize).saturating_add(byte_off) as *mut u32;
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

struct UsbKeyboard {
    hid: HidDevice<SeatDma>,
    last_keys: [u8; 6],
    poll_error_logged: bool,
}

impl UsbKeyboard {
    fn new(hal: &mut KernelHal<'_>, xhci_mmio_hint: Option<usize>) -> Result<Self, Pi4SeatError> {
        let mut candidates = [0usize; XHCI_MMIO_CANDIDATE_LIMIT];
        let mut candidate_count = 0usize;
        if let Some(hint) = xhci_mmio_hint {
            if hint != 0 {
                candidates[candidate_count] = hint;
                candidate_count = candidate_count.saturating_add(1);
            }
        }
        for fallback in RPI4_XHCI_MMIO_FALLBACKS {
            if candidates[..candidate_count].contains(&fallback) {
                continue;
            }
            if candidate_count >= candidates.len() {
                break;
            }
            candidates[candidate_count] = fallback;
            candidate_count = candidate_count.saturating_add(1);
        }

        let mut saw_controller = false;
        let mut saw_keyboard_init_error = false;
        for &mmio_base in &candidates[..candidate_count] {
            let dma = SeatDma::new(hal);
            let ctrl = match XhciCtrl::new(mmio_base, dma) {
                Ok(ctrl) => {
                    saw_controller = true;
                    Arc::new(ctrl)
                }
                Err(_) => continue,
            };

            let max_ports = cmp::min(ctrl.max_ports() as usize, XHCI_MAX_PROBE_PORTS);
            for port in 0..max_ports {
                if !ctrl.port_connected(port as u8) {
                    continue;
                }

                let mut device = match UsbDevice::new(ctrl.clone(), port as u8) {
                    Ok(device) => device,
                    Err(_) => continue,
                };

                if device.get_device_descriptor().is_err() {
                    continue;
                }

                let config_blob = match device.get_config_descriptor(0) {
                    Ok(config_blob) => config_blob,
                    Err(_) => continue,
                };
                let Some(config) = read_config_desc(&config_blob) else {
                    continue;
                };
                if device.set_configuration(config.configuration).is_err() {
                    continue;
                }

                let device = Arc::new(device);
                let interfaces = find_hid_interfaces(&config_blob);
                for (iface, ep_in) in interfaces {
                    if iface.interface_subclass != hid_subclass::BOOT
                        || iface.interface_protocol != hid_protocol::KEYBOARD
                    {
                        continue;
                    }
                    let hid = match HidDevice::from_interface(device.clone(), &iface, &ep_in) {
                        Ok(hid) => hid,
                        Err(_) => {
                            saw_keyboard_init_error = true;
                            continue;
                        }
                    };
                    if hid.queue_read().is_err() {
                        saw_keyboard_init_error = true;
                        continue;
                    }
                    hid.device().ctrl().host().seal_runtime();
                    return Ok(Self {
                        hid,
                        last_keys: [0; 6],
                        poll_error_logged: false,
                    });
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

    fn poll_bytes(&mut self, out: &mut [u8]) -> usize {
        if out.is_empty() {
            return 0;
        }

        let report = match self.hid.poll_keyboard_checked() {
            Ok(Some(report)) => {
                self.poll_error_logged = false;
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
    fn new(hal: &mut KernelHal<'_>) -> Self {
        Self {
            state: Mutex::new(SeatDmaState {
                hal_ptr: hal as *mut _ as usize,
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
            return None;
        }
        if !align.is_power_of_two() {
            return None;
        }

        let page_count = div_ceil(size, PAGE_SIZE);
        let hal = hal_from_ptr(state.hal_ptr)?;

        let mut frames = Vec::with_capacity(page_count);
        let mut expected_phys = 0usize;
        let mut expected_virt = 0usize;
        for idx in 0..page_count {
            let frame = hal
                .alloc_dma_frame_attr(sel4_sys::seL4_ARM_Page_Uncached)
                .ok()?;
            let phys = frame.paddr();
            let virt = frame.ptr().as_ptr() as usize;
            if idx == 0 {
                expected_phys = phys;
                expected_virt = virt;
            } else {
                let next_phys = expected_phys.saturating_add(idx.saturating_mul(PAGE_SIZE));
                let next_virt = expected_virt.saturating_add(idx.saturating_mul(PAGE_SIZE));
                if phys != next_phys || virt != next_virt {
                    return None;
                }
            }
            frames.push(frame);
        }

        if (expected_virt & (align - 1)) != 0 {
            return None;
        }

        state.regions.push(PhysRegion {
            virt_start: expected_virt,
            phys_start: expected_phys,
            length: page_count.saturating_mul(PAGE_SIZE),
            size,
            align,
            backing: RegionBacking::Dma(frames),
        });
        Some(expected_virt)
    }

    fn map_mmio_locked(state: &mut SeatDmaState, phys: usize, size: usize) -> Option<usize> {
        if state.sealed || size == 0 {
            return None;
        }

        let page_base = phys & !PAGE_MASK;
        let page_offset = phys & PAGE_MASK;
        let map_len = page_offset.saturating_add(size);
        let page_count = div_ceil(map_len, PAGE_SIZE);
        let hal = hal_from_ptr(state.hal_ptr)?;

        let mut frames = Vec::with_capacity(page_count);
        let mut first_virt = 0usize;
        for idx in 0..page_count {
            let page_phys = page_base.saturating_add(idx.saturating_mul(PAGE_SIZE));
            let frame = hal.map_device(page_phys).ok()?;
            let virt = frame.ptr().as_ptr() as usize;
            if idx == 0 {
                first_virt = virt;
            } else {
                let next_virt = first_virt.saturating_add(idx.saturating_mul(PAGE_SIZE));
                if virt != next_virt {
                    return None;
                }
            }
            frames.push(frame);
        }

        let virt = first_virt.saturating_add(page_offset);
        state.regions.push(PhysRegion {
            virt_start: virt,
            phys_start: phys,
            length: map_len,
            size,
            align: PAGE_SIZE,
            backing: RegionBacking::Mmio(frames),
        });
        Some(virt)
    }

    fn virt_to_phys_locked(state: &SeatDmaState, va: usize) -> usize {
        for region in &state.regions {
            let start = region.virt_start;
            let end = start.saturating_add(region.length);
            if (start..end).contains(&va) {
                return region.phys_start.saturating_add(va.saturating_sub(start));
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
fn phys_to_bus(phys: usize) -> Option<u32> {
    if phys > VC_BUS_MASK as usize {
        return None;
    }
    Some((phys as u32 & VC_BUS_MASK) | VC_BUS_UNCACHED_BASE)
}

#[inline]
fn bus_to_phys(bus: u32) -> usize {
    (bus & VC_BUS_MASK) as usize
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
fn read_config_desc(config_blob: &[u8]) -> Option<ConfigDesc> {
    if config_blob.len() < mem::size_of::<ConfigDesc>() {
        return None;
    }
    // SAFETY: The descriptor bytes may be unaligned in the returned USB blob.
    Some(unsafe { ptr::read_unaligned(config_blob.as_ptr().cast::<ConfigDesc>()) })
}
