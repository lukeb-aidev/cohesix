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
    HalError, Hardware, SdioBusWidth, SdioFunction, WifiFirmwareBundle, WifiPowerState,
    WifiResetState,
};
use crate::bootstrap::log as boot_log;
use crate::rust_alloc::vec::Vec;
use crate::sel4::{page_get_address, DeviceFrame, RamFrame, PAGE_BITS};
use spin::Mutex;

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
const MAILBOX_DRAIN_LIMIT: usize = 64;
const MAP_EXACT_ATTEMPT_CAP: usize = 2048;

const TAG_SET_POWER_STATE: u32 = 0x0002_8001;
const TAG_GET_CLOCK_RATE: u32 = 0x0003_0002;
const TAG_GET_MAX_CLOCK_RATE: u32 = 0x0003_0004;
const TAG_GET_GPIO_STATE: u32 = 0x0003_0041;
const TAG_SET_GPIO_STATE: u32 = 0x0003_8041;
const TAG_GET_GPIO_CONFIG: u32 = 0x0003_0043;
const TAG_SET_GPIO_CONFIG: u32 = 0x0003_8043;

const POWER_STATE_REQ_ON: u32 = 1 << 0;
const POWER_STATE_REQ_WAIT: u32 = 1 << 1;
const POWER_DEVID_SDHCI: u32 = 0;
const CLOCK_ID_EMMC: u32 = 1;
const CLOCK_ID_EMMC2: u32 = 12;

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

fn bounded_spin_settle(stage: &'static str, loops: usize) {
    emit_breadcrumb(format_args!(
        "[pi4-wifi] settle stage={stage} loops={loops}"
    ));
    for _ in 0..loops {
        spin_loop();
    }
}

fn mailbox_tag_name(tag: u32) -> &'static str {
    match tag {
        TAG_SET_POWER_STATE => "set-power-state",
        TAG_GET_CLOCK_RATE => "get-clock-rate",
        TAG_GET_MAX_CLOCK_RATE => "get-max-clock-rate",
        TAG_GET_GPIO_STATE => "get-gpio-state",
        TAG_SET_GPIO_STATE => "set-gpio-state",
        TAG_GET_GPIO_CONFIG => "get-gpio-config",
        TAG_SET_GPIO_CONFIG => "set-gpio-config",
        _ => "unknown",
    }
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
const SDIO_BUS_WIDTH_1BIT: u8 = 0x00;
const SDIO_BUS_WIDTH_4BIT: u8 = 0x02;
const SDIO_CCCR_FBR_BASE: u32 = 0x100;
const SDIO_FBR_BLKSIZE: u32 = 0x10;
const SDIO_FUNC_ENABLE_1: u8 = 0x02;
const SDIO_FUNC_ENABLE_2: u8 = 0x04;
const SDIO_FUNC_READY_1: u8 = 0x02;
const SDIO_FUNC_READY_2: u8 = 0x04;
const SDIO_FUNCTION_READY_POLLS: usize = 64;
const SDIO_FUNCTION_READY_SETTLE_LOOPS: usize = 200_000;
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
const SBSDIO_FORCE_HW_CLKREQ_OFF: u8 = 0x20;
const SBSDIO_ALP_AVAIL: u8 = 0x40;
const SBSDIO_HT_AVAIL: u8 = 0x80;
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

const SDIO_INIT_WAIT_LOOPS: usize = 50_000;
const SDIO_HOST_RESET_LOOPS: usize = 50_000;
const SDIO_CLOCK_STABLE_LOOPS: usize = 50_000;
const SDIO_CMD_WAIT_LOOPS: usize = 200_000;
const SDIO_DATA_WAIT_LOOPS: usize = 200_000;
const SDIO_CARD_INIT_ATTEMPTS: usize = 2;
const SDHCI_POWER_OFF_SETTLE_LOOPS: usize = 500_000;
const SDHCI_POWER_SETTLE_LOOPS: usize = 20_000_000;
const SDIO_CARD_INIT_RETRY_SETTLE_LOOPS: usize = 20_000_000;
const WIFI_RESET_SETTLE_LOOPS: usize = 20_000_000;
const WIFI_POWER_SETTLE_LOOPS: usize = 500_000;
const WIFI_POWER_DROP_SETTLE_LOOPS: usize = 20_000_000;
const SDHCI_WRITE_DELAY_LOOPS: usize = 256;
const SDHCI_WRITE_GAP_SPIN_LOOPS: usize = SDHCI_WRITE_DELAY_LOOPS * 32;
const CYW43_READY_LOOPS: usize = 1_000;
const CYW43_TRANSFER_CHUNK: usize = 256;
const SDIO_MAX_BYTE_MODE: usize = 511;

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
    transfer_mode: u16,
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
    block_size: 256,
    timeout_error: "sdio-function2-ready-timeout",
};

const SDIO_FUNCTION_ENABLE_SEQUENCE: [SdioFunctionEnableStep; 2] =
    [SDIO_FUNCTION_ENABLE_F1, SDIO_FUNCTION_ENABLE_F2];

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
        let was_enabled = wifi_gpio_line_enabled(self.power_state);
        self.reset_state = state;
        self.apply_wifi_line(was_enabled)?;
        if matches!(state, WifiResetState::Deasserted) {
            bounded_spin_settle("wifi-reset-deassert", WIFI_RESET_SETTLE_LOOPS);
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

    fn apply_wifi_line(&mut self, was_enabled: bool) -> Result<(), HalError> {
        let enabled = wifi_gpio_line_enabled(self.power_state);
        emit_breadcrumb(format_args!(
            "[pi4-wifi] gpio wl-on={} power={} reset={}",
            enabled as u8,
            wifi_power_state_name(self.power_state),
            wifi_reset_state_name(self.reset_state),
        ));
        if was_enabled != enabled {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] gpio wl-on transition {}->{}",
                was_enabled as u8, enabled as u8
            ));
        }
        self.mailbox
            .configure_gpio_output(PI4_WIFI_GPIO, enabled as u32)?;
        if !was_enabled && enabled {
            bounded_spin_settle("wifi-power-on", WIFI_POWER_SETTLE_LOOPS);
        } else if was_enabled && !enabled {
            bounded_spin_settle("wifi-power-off", WIFI_POWER_DROP_SETTLE_LOOPS);
            self.host.mark_power_cycled();
        }
        Ok(())
    }
}

struct Mailbox {
    regs: MappedRegs,
    request: RamFrame,
}

impl Mailbox {
    fn new<H>(hal: &mut H) -> Result<Self, HalError>
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
        let request = hal
            .alloc_dma_frame_low_attr(sel4_sys::seL4_ARM_Page_Uncached)
            .map_err(|_| HalError::Unsupported("mailbox-dma"))?;
        Ok(Self { regs, request })
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
        let original_payload = payload.to_vec();
        let words = {
            let bytes = self.request.as_mut_slice();
            unsafe {
                core::slice::from_raw_parts_mut(bytes.as_mut_ptr().cast::<u32>(), PAGE_SIZE / 4)
            }
        };

        let mut last_err = HalError::Unsupported("mailbox-protocol");
        for (alias_index, &alias_base) in VC_BUS_ALIAS_BASES.iter().enumerate() {
            self.encode_request(words, tag, request_len_bytes, &original_payload)?;
            let request_bus = phys_to_bus(self.request.paddr(), alias_base)
                .ok_or(HalError::Unsupported("mailbox-bus-alias"))?;
            match self.send(request_bus) {
                Ok(()) => {
                    if words[1] != MAILBOX_RESPONSE_SUCCESS
                        || words[2] != tag
                        || (words[4] & MAILBOX_VALUE_RESPONSE) == 0
                    {
                        self.log_protocol_reply(tag, alias_base, words);
                        last_err = HalError::Unsupported("mailbox-protocol");
                        continue;
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

    fn send(&self, data: u32) -> Result<(), HalError> {
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

        self.write_reg(
            MAILBOX_WRITE_OFFSET,
            (data & !0xF) | (MAILBOX_CHANNEL_PROPERTY & 0xF),
        );
        fence(Ordering::SeqCst);

        wait = 0;
        loop {
            while self.read_reg(MAILBOX_STATUS0_OFFSET) & MAILBOX_EMPTY != 0 {
                wait = wait.saturating_add(1);
                if wait >= MAILBOX_WAIT_SPINS {
                    self.log_timeout("recv");
                    return Err(HalError::Unsupported("mailbox-timeout"));
                }
                spin_loop();
            }

            let value = self.read_reg(MAILBOX_READ_OFFSET);
            if (value & 0xF) == MAILBOX_CHANNEL_PROPERTY {
                if (value & !0xF) != (data & !0xF) {
                    emit_breadcrumb(format_args!(
                        "[pi4-wifi] mailbox reply mismatch expected=0x{:08x} actual=0x{value:08x}",
                        data & !0xF
                    ));
                    return Err(HalError::Unsupported("mailbox-protocol"));
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

struct SdioHost {
    regs: MappedRegs,
    regs_paddr: usize,
    base_clock_hz: u32,
    current_clock_hz: u32,
    desired_bus_width: SdioBusWidth,
    card: Option<CardInfo>,
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
            desired_bus_width: SdioBusWidth::OneBit,
            card: None,
            transfer_mode_shadow: 0,
        })
    }

    fn mark_power_cycled(&mut self) {
        self.card = None;
        self.current_clock_hz = 0;
    }

    fn reset_controller(&mut self) -> Result<(), HalError> {
        self.write16(SDHCI_CLOCK_CONTROL, 0);
        self.write8(SDHCI_POWER_CONTROL, 0);
        bounded_spin_settle("sdhci-power-off", SDHCI_POWER_OFF_SETTLE_LOOPS);
        self.software_reset(SDHCI_RESET_ALL)?;
        self.write8(SDHCI_POWER_CONTROL, SDHCI_POWER_330 | SDHCI_POWER_ON);
        bounded_spin_settle("sdhci-power-on", SDHCI_POWER_SETTLE_LOOPS);
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
        self.log_host_state("after-reset");
        Ok(())
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
        let arg = cmd52_argument(function, addr, false, 0);
        let resp = self.send_command(SDIO_CMD52, arg, ResponseType::Short)?[0];
        if r5_status(resp) != 0 {
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
        if r5_status(resp) != 0 {
            return Err(HalError::Unsupported("sdio-cmd52-write"));
        }
        Ok(())
    }

    fn io_extended(
        &mut self,
        function: SdioFunction,
        addr: u32,
        increment_addr: bool,
        write: bool,
        buffer: &mut [u8],
    ) -> Result<(), HalError> {
        if buffer.is_empty() {
            return Ok(());
        }

        let mut offset = 0usize;
        while offset < buffer.len() {
            let chunk_len = cmp::min(buffer.len() - offset, SDIO_MAX_BYTE_MODE);
            let chunk = &mut buffer[offset..offset + chunk_len];
            let arg = (u32::from(write) << 31)
                | (u32::from(function.number()) << 28)
                | (u32::from(increment_addr) << 26)
                | ((addr & 0x1_FFFF) << 9)
                | u32::try_from(chunk_len).map_err(|_| HalError::Unsupported("sdio-cmd53-len"))?;
            self.transfer_command(SDIO_CMD53, arg, chunk, write)?;
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

    fn enable_function1(&mut self) -> Result<(), HalError> {
        emit_breadcrumb(format_args!("[pi4-wifi] sdio enable-function1 begin"));
        self.read_function_enable_state("before-f1")?;
        self.enable_function_step(SDIO_FUNCTION_ENABLE_F1)?;
        let (ioex, ready) = self.read_function_enable_state("after-f1")?;
        emit_breadcrumb(format_args!(
            "[pi4-wifi] sdio enable-function1 ready ioex=0x{ioex:02x} ready=0x{ready:02x}"
        ));
        Ok(())
    }

    fn enable_function2(&mut self) -> Result<(), HalError> {
        emit_breadcrumb(format_args!("[pi4-wifi] sdio enable-function2 begin"));
        self.read_function_enable_state("before-f2")?;
        self.enable_function_step(SDIO_FUNCTION_ENABLE_F2)?;
        let ien = SDIO_CCCR_IEN_FUNC0 | SDIO_CCCR_IEN_FUNC1 | SDIO_CCCR_IEN_FUNC2;
        self.io_direct_write(SdioFunction::Function0, SDIO_CCCR_IENX, ien)?;
        let (ioex, ready) = self.read_function_enable_state("after-f2")?;
        emit_breadcrumb(format_args!(
            "[pi4-wifi] sdio enable-function2 ready ioex=0x{ioex:02x} ready=0x{ready:02x} ien=0x{ien:02x}"
        ));
        Ok(())
    }

    fn enable_function_step(&mut self, step: SdioFunctionEnableStep) -> Result<(), HalError> {
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

        let mut last_ready = u8::MAX;
        for poll in 0..SDIO_FUNCTION_READY_POLLS {
            let ready = self.io_direct_read(SdioFunction::Function0, SDIO_CCCR_IORX)?;
            if poll == 0 || poll + 1 == SDIO_FUNCTION_READY_POLLS || ready != last_ready {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] sdio function-ready fn={} poll={}/{} ready=0x{ready:02x} need=0x{need:02x}",
                    function_number,
                    poll + 1,
                    SDIO_FUNCTION_READY_POLLS,
                    need = step.ready_bit
                ));
                last_ready = ready;
            }
            if (ready & step.ready_bit) == step.ready_bit {
                self.set_function_block_size(step.function, step.block_size)?;
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] sdio function-ready fn={} block={} ready=0x{ready:02x}",
                    function_number, step.block_size
                ));
                return Ok(());
            }
            if poll + 1 != SDIO_FUNCTION_READY_POLLS {
                for _ in 0..SDIO_FUNCTION_READY_SETTLE_LOOPS {
                    spin_loop();
                }
            }
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
        let mut alp_ready = false;
        for _ in 0..SDIO_INIT_WAIT_LOOPS {
            let chipclk = self.io_direct_read(SdioFunction::Function1, SBSDIO_FUNC1_CHIPCLKCSR)?;
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
        emit_breadcrumb(format_args!(
            "[pi4-wifi] cyw43 backplane stage=misc-config deferred reason=minimal-sdio-bringup"
        ));
        emit_breadcrumb(format_args!("[pi4-wifi] cyw43 backplane ready"));
        Ok(())
    }

    fn setup_firmware_channel(&mut self) -> Result<(), HalError> {
        emit_breadcrumb(format_args!("[pi4-wifi] firmware channel begin"));
        self.write_f1_u32(
            SDPCMD_REG_TOSBMAILBOXDATA,
            SDPCM_PROT_VERSION << HMB_DATA_VERSION_SHIFT,
        )?;
        self.enable_function2()?;
        self.write_f1_u32(SDPCMD_REG_HOSTINTMASK, HOSTINTMASK)?;
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
        emit_breadcrumb(format_args!("[pi4-wifi] firmware channel ready"));
        Ok(())
    }

    fn wait_for_firmware_ready(&mut self) -> Result<(), HalError> {
        for _ in 0..CYW43_READY_LOOPS {
            let value = self.read_f1_u32(SDPCMD_REG_TOHOSTMAILBOXDATA)?;
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
    }

    fn backplane_read8(&mut self, addr: u32) -> Result<u8, HalError> {
        Ok(self.backplane_read(addr, 1)?[0])
    }

    fn backplane_write8(&mut self, addr: u32, value: u8) -> Result<(), HalError> {
        self.backplane_write(addr, &[value])
    }

    fn backplane_read16(&mut self, addr: u32) -> Result<u16, HalError> {
        Ok((self.backplane_read32(addr)? & 0xFFFF) as u16)
    }

    fn backplane_write16(&mut self, addr: u32, value: u16) -> Result<(), HalError> {
        self.backplane_write(addr, &value.to_le_bytes())
    }

    fn backplane_read32(&mut self, addr: u32) -> Result<u32, HalError> {
        let mut bytes = [0u8; 4];
        self.with_backplane_window(addr | BACKPLANE_32BIT_FLAG, |this, function_addr| {
            this.io_extended(
                SdioFunction::Function1,
                function_addr,
                true,
                false,
                &mut bytes,
            )
        })?;
        Ok(u32::from_le_bytes(bytes))
    }

    fn backplane_write32(&mut self, addr: u32, value: u32) -> Result<(), HalError> {
        let mut bytes = value.to_le_bytes();
        self.with_backplane_window(addr | BACKPLANE_32BIT_FLAG, |this, function_addr| {
            this.io_extended(
                SdioFunction::Function1,
                function_addr,
                true,
                true,
                &mut bytes,
            )
        })
    }

    fn backplane_read(&mut self, addr: u32, len: usize) -> Result<[u8; 4], HalError> {
        let mut result = [0u8; 4];
        let read_len = len.min(result.len());
        self.with_backplane_window(addr, |this, function_addr| {
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
            self.with_backplane_window(chunk_addr, |this, function_addr| {
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
            self.with_backplane_window(chunk_addr, |this, function_addr| {
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

    fn with_backplane_window<T>(
        &mut self,
        addr: u32,
        f: impl FnOnce(&mut Self, u32) -> Result<T, HalError>,
    ) -> Result<T, HalError> {
        let window = addr & BACKPLANE_WINDOW_MASK;
        self.io_direct_write(
            SdioFunction::Function1,
            SBSDIO_FUNC1_SBADDRLOW,
            ((window >> 8) & 0x80) as u8,
        )?;
        self.io_direct_write(
            SdioFunction::Function1,
            SBSDIO_FUNC1_SBADDRMID,
            ((window >> 16) & 0xFF) as u8,
        )?;
        self.io_direct_write(
            SdioFunction::Function1,
            SBSDIO_FUNC1_SBADDRHIGH,
            ((window >> 24) & 0xFF) as u8,
        )?;
        f(
            self,
            (addr & BACKPLANE_ADDRESS_MASK) | (addr & BACKPLANE_32BIT_FLAG),
        )
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

    fn write_f1_u32(&mut self, addr: u32, value: u32) -> Result<(), HalError> {
        for (index, value) in value.to_le_bytes().into_iter().enumerate() {
            let byte_addr = addr
                .checked_add(index as u32)
                .ok_or(HalError::Unsupported("f1-write32-overflow"))?;
            self.io_direct_write(SdioFunction::Function1, byte_addr, value)?;
        }
        Ok(())
    }

    fn next_frame_len(&mut self) -> Result<usize, HalError> {
        let lo =
            usize::from(self.io_direct_read(SdioFunction::Function1, SBSDIO_FUNC1_RFRAMEBCLO)?);
        let hi =
            usize::from(self.io_direct_read(SdioFunction::Function1, SBSDIO_FUNC1_RFRAMEBCHI)?);
        Ok((hi << 8) | lo)
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
        self.io_extended(SdioFunction::Function2, 0, false, true, frame)
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
        self.core_disable(CYW43_ARMCR4_CORE_BASE)?;
        self.core_disable(CYW43_SOCRAM_CORE_BASE)?;
        self.core_reset(CYW43_SOCRAM_CORE_BASE)?;
        self.backplane_write32(0x1800_4000 + 0x10, 3)?;
        self.backplane_write32(0x1800_4000 + 0x44, 0)?;
        self.backplane_write(ram_base, bundle.firmware)?;
        let nvram = normalize_nvram(bundle.nvram);
        let nvram_offset = ram_base
            .checked_add(ram_size)
            .and_then(|value| value.checked_sub(4))
            .and_then(|value| value.checked_sub(u32::try_from(nvram.len()).ok()?))
            .ok_or(HalError::Unsupported("cyw43-nvram-range"))?;
        self.backplane_write(nvram_offset, &nvram)?;
        let nvram_words =
            u32::try_from(nvram.len() / 4).map_err(|_| HalError::Unsupported("cyw43-nvram-len"))?;
        let nvram_magic = (!nvram_words << 16) | nvram_words;
        self.backplane_write32(
            ram_base
                .checked_add(ram_size)
                .and_then(|value| value.checked_sub(4))
                .ok_or(HalError::Unsupported("cyw43-nvram-tail"))?,
            nvram_magic,
        )?;
        self.core_reset(CYW43_ARMCR4_CORE_BASE)?;
        self.wait_for_ht_clock()?;
        self.setup_firmware_channel()?;
        self.wait_for_firmware_ready()?;
        emit_breadcrumb(format_args!("[pi4-wifi] firmware load ready"));
        Ok(())
    }

    fn wait_for_ht_clock(&mut self) -> Result<(), HalError> {
        self.io_direct_write(
            SdioFunction::Function1,
            SBSDIO_FUNC1_CHIPCLKCSR,
            SBSDIO_HT_AVAIL_REQ,
        )?;
        for _ in 0..SDIO_INIT_WAIT_LOOPS {
            if (self.io_direct_read(SdioFunction::Function1, SBSDIO_FUNC1_CHIPCLKCSR)?
                & SBSDIO_HT_AVAIL)
                != 0
            {
                return Ok(());
            }
            spin_loop();
        }
        Err(HalError::Unsupported("cyw43-ht-clock-timeout"))
    }

    fn core_disable(&mut self, base: u32) -> Result<(), HalError> {
        let reset = self.backplane_read8(base + AI_RESETCTRL_OFFSET)?;
        if (reset & AI_RESETCTRL_BIT_RESET) != 0 {
            return Ok(());
        }
        self.backplane_write8(base + AI_IOCTRL_OFFSET, 0)?;
        self.backplane_write8(base + AI_RESETCTRL_OFFSET, AI_RESETCTRL_BIT_RESET)?;
        Ok(())
    }

    fn core_reset(&mut self, base: u32) -> Result<(), HalError> {
        self.core_disable(base)?;
        self.backplane_write8(
            base + AI_IOCTRL_OFFSET,
            AI_IOCTRL_BIT_FGC | AI_IOCTRL_BIT_CLOCK_EN,
        )?;
        self.backplane_write8(base + AI_RESETCTRL_OFFSET, 0)?;
        self.backplane_write8(base + AI_IOCTRL_OFFSET, AI_IOCTRL_BIT_CLOCK_EN)?;
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
        self.write8(SDHCI_SOFTWARE_RESET, mask);
        for _ in 0..SDIO_HOST_RESET_LOOPS {
            if (self.read8(SDHCI_SOFTWARE_RESET) & mask) == 0 {
                return Ok(());
            }
            spin_loop();
        }
        Err(HalError::Unsupported("sdhci-reset-timeout"))
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
                return Err(err);
            }
        };
        if (status & SDHCI_INT_ERROR) != 0 {
            self.log_command_state("error", cmd, arg, status);
            self.software_reset(SDHCI_RESET_CMD).ok();
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
    ) -> Result<(), HalError> {
        let plan = sdio_transfer_plan(buffer.len(), write)?;
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
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] sdhci xfer error cmd={cmd} arg=0x{arg:08x} len={} phase=command-wait err={err}",
                    buffer.len(),
                ));
                self.log_host_state("xfer-command-wait");
                self.software_reset(SDHCI_RESET_CMD).ok();
                return Err(err);
            }
        };
        if (cmd_status & SDHCI_INT_ERROR) != 0 {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] sdhci xfer error cmd={cmd} arg=0x{arg:08x} len={} phase=command st=0x{cmd_status:08x} why={}",
                buffer.len(),
                sdhci_status_reason(cmd_status)
            ));
            self.log_host_state("xfer-command-fail");
            self.software_reset(SDHCI_RESET_CMD).ok();
            return Err(HalError::Unsupported("sdhci-transfer-command"));
        }

        let mut offset = 0usize;
        while offset < buffer.len() {
            let wait_mask = if write {
                SDHCI_INT_SPACE_AVAIL
            } else {
                SDHCI_INT_DATA_AVAIL
            };
            let status = match self.wait_int(wait_mask | SDHCI_INT_ERROR | SDHCI_INT_DATA_MASK) {
                Ok(status) => status,
                Err(err) => {
                    emit_breadcrumb(format_args!(
                        "[pi4-wifi] sdhci xfer error cmd={cmd} arg=0x{arg:08x} len={} phase=data-wait err={err}",
                        buffer.len(),
                    ));
                    self.log_host_state("xfer-data-wait");
                    self.software_reset(SDHCI_RESET_DATA).ok();
                    return Err(err);
                }
            };
            if (status & SDHCI_INT_ERROR) != 0 {
                emit_breadcrumb(format_args!(
                    "[pi4-wifi] sdhci xfer error cmd={cmd} arg=0x{arg:08x} len={} phase=data st=0x{status:08x} why={}",
                    buffer.len(),
                    sdhci_status_reason(status)
                ));
                self.log_host_state("xfer-data-fail");
                self.software_reset(SDHCI_RESET_DATA).ok();
                return Err(HalError::Unsupported("sdhci-transfer-data"));
            }
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

        let data_status = match self
            .wait_int(SDHCI_INT_DATA_END | SDHCI_INT_ERROR | SDHCI_INT_DATA_MASK)
        {
            Ok(status) => status,
            Err(err) => {
                emit_breadcrumb(format_args!(
                        "[pi4-wifi] sdhci xfer error cmd={cmd} arg=0x{arg:08x} len={} phase=finish-wait err={err}",
                        buffer.len(),
                    ));
                self.log_host_state("xfer-finish-wait");
                self.software_reset(SDHCI_RESET_DATA).ok();
                return Err(err);
            }
        };
        if (data_status & SDHCI_INT_ERROR) != 0 {
            emit_breadcrumb(format_args!(
                "[pi4-wifi] sdhci xfer error cmd={cmd} arg=0x{arg:08x} len={} phase=finish st=0x{data_status:08x} why={}",
                buffer.len(),
                sdhci_status_reason(data_status)
            ));
            self.log_host_state("xfer-finish-fail");
            self.software_reset(SDHCI_RESET_DATA).ok();
            return Err(HalError::Unsupported("sdhci-transfer-finish"));
        }
        Ok(())
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
        } else {
            self.raw_read32(aligned)
        };
        let new_word = merge_u16_word(word, offset, value);
        if offset == SDHCI_TRANSFER_MODE {
            self.transfer_mode_shadow = new_word;
            return;
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

fn sdio_transfer_plan(len: usize, write: bool) -> Result<SdioTransferPlan, HalError> {
    let block_size = u16::try_from(len).map_err(|_| HalError::Unsupported("sdhci-block-size"))?;
    let mut transfer_mode = 0u16;
    if !write {
        transfer_mode |= SDHCI_TRNS_READ;
    }
    Ok(SdioTransferPlan {
        block_size,
        block_count: 0,
        transfer_mode,
    })
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
        bcm2711_gpfsel_offset, bcm2711_puppdn_offset, cmd52_argument, is_mailbox_protocol_error,
        mailbox_tag_name, make_command, merge_u16_word, normalize_nvram, phys_to_bus, r5_status,
        sdhci_status_reason, sdio_transfer_plan, update_bcm2711_gpio_function,
        update_bcm2711_gpio_pull, HalError, ResponseType, SdioFunction, BCM2711_GPIO_ALT3,
        PI4_WIFI_SDIO_PINS, PI4_WIFI_SDIO_PULLS, SDHCI_COMMAND, SDHCI_INT_CRC, SDHCI_INT_DATA_CRC,
        SDHCI_INT_TIMEOUT, SDHCI_TRANSFER_MODE, SDHCI_TRNS_BLK_CNT_EN, SDHCI_TRNS_READ,
        SDHCI_WRITE_DELAY_LOOPS, SDHCI_WRITE_GAP_SPIN_LOOPS, SDIO_FUNCTION_ENABLE_SEQUENCE,
        SDIO_FUNC_ENABLE_1, SDIO_FUNC_ENABLE_2, SDIO_FUNC_READY_1, SDIO_FUNC_READY_2,
        TAG_GET_CLOCK_RATE, TAG_SET_GPIO_CONFIG, TAG_SET_POWER_STATE,
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
    fn sdio_transfer_plan_uses_single_byte_mode_transfers() {
        let read = sdio_transfer_plan(4, false).expect("read transfer plan");
        assert_eq!(read.block_size, 4);
        assert_eq!(read.block_count, 0);
        assert_eq!(read.transfer_mode & SDHCI_TRNS_BLK_CNT_EN, 0);
        assert_ne!(read.transfer_mode & SDHCI_TRNS_READ, 0);

        let write = sdio_transfer_plan(64, true).expect("write transfer plan");
        assert_eq!(write.block_size, 64);
        assert_eq!(write.block_count, 0);
        assert_eq!(write.transfer_mode & SDHCI_TRNS_BLK_CNT_EN, 0);
        assert_eq!(write.transfer_mode & SDHCI_TRNS_READ, 0);
    }

    #[test]
    fn backplane_word_access_sets_32bit_flag() {
        assert_eq!(CYW43_CHIPCOMMON_BASE | BACKPLANE_32BIT_FLAG, 0x1800_8000);
        assert_eq!(
            (CYW43_ARMCR4_CORE_BASE + ARMCR4_CAP) | BACKPLANE_32BIT_FLAG,
            0x1810_b004
        );
    }

    #[test]
    fn mailbox_tag_names_cover_bringup_tags() {
        assert_eq!(mailbox_tag_name(TAG_SET_POWER_STATE), "set-power-state");
        assert_eq!(mailbox_tag_name(TAG_GET_CLOCK_RATE), "get-clock-rate");
        assert_eq!(mailbox_tag_name(TAG_SET_GPIO_CONFIG), "set-gpio-config");
        assert_eq!(mailbox_tag_name(0xffff_ffff), "unknown");
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
        assert_eq!(SDIO_FUNCTION_ENABLE_SEQUENCE[1].block_size, 256);
    }
}
