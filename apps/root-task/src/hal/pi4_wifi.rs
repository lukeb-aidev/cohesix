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

static PINNED_MAILBOX_REGS: Mutex<Option<MappedRegs>> = Mutex::new(None);
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

pub fn preseed_mmio<H>(hal: &mut H)
where
    H: Hardware<Error = HalError>,
{
    let mailbox = preseed_register_block(hal, &MAILBOX_PAGE_PADDR_CANDIDATES, &PINNED_MAILBOX_REGS);
    let sdhci = preseed_register_block(hal, &SDHCI_PAGE_PADDR_CANDIDATES, &PINNED_SDHCI_REGS);

    match (mailbox, sdhci) {
        (true, true) => {
            boot_log::force_uart_line("[pi4-wifi] mmio preseeded mailbox=yes sdhci=yes");
        }
        (true, false) => {
            boot_log::force_uart_line("[pi4-wifi] mmio preseeded mailbox=yes sdhci=no");
        }
        (false, true) => {
            boot_log::force_uart_line("[pi4-wifi] mmio preseeded mailbox=no sdhci=yes");
        }
        (false, false) => {
            boot_log::force_uart_line("[pi4-wifi] mmio preseeded mailbox=no sdhci=no");
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
const SDHCI_SPACE_AVAILABLE: u32 = 1 << 10;
const SDHCI_DATA_AVAILABLE: u32 = 1 << 11;

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
const WIFI_RESET_SETTLE_LOOPS: usize = 2_000_000;
const WIFI_POWER_SETTLE_LOOPS: usize = 500_000;
const SDHCI_WRITE_DELAY_LOOPS: usize = 256;
const CYW43_READY_LOOPS: usize = 1_000;
const CYW43_TRANSFER_CHUNK: usize = 256;
const SDIO_MAX_BYTE_MODE: usize = 511;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResponseType {
    None,
    Short,
    ShortBusy,
    Long,
}

#[derive(Clone, Copy)]
struct CardInfo {
    rca: u16,
    ocr: u32,
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
        let host = SdioHost::new(hal, &mailbox).map_err(|err| {
            log::warn!("[pi4-wifi] hal init: sdhci failed: {err}");
            err
        })?;
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
        self.power_state = state;
        self.apply_wifi_line()
    }

    pub fn set_reset(&mut self, state: WifiResetState) -> Result<(), HalError> {
        self.reset_state = state;
        self.apply_wifi_line()?;
        if matches!(state, WifiResetState::Deasserted) {
            for _ in 0..WIFI_RESET_SETTLE_LOOPS {
                spin_loop();
            }
        }
        Ok(())
    }

    pub fn reset_host(&mut self) -> Result<(), HalError> {
        self.host.reset_controller()
    }

    pub fn set_clock_hz(&mut self, target_hz: u32) -> Result<u32, HalError> {
        self.host.set_clock_hz(target_hz)
    }

    pub fn set_bus_width(&mut self, width: SdioBusWidth) -> Result<(), HalError> {
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

    fn apply_wifi_line(&mut self) -> Result<(), HalError> {
        let enabled = matches!(self.power_state, WifiPowerState::On)
            && matches!(self.reset_state, WifiResetState::Deasserted);
        self.mailbox
            .configure_gpio_output(PI4_WIFI_GPIO, enabled as u32)?;
        if enabled {
            for _ in 0..WIFI_POWER_SETTLE_LOOPS {
                spin_loop();
            }
        }
        if !enabled {
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
        let mut payload = [module, POWER_STATE_REQ_ON | POWER_STATE_REQ_WAIT];
        self.call_tag(TAG_SET_POWER_STATE, 8, &mut payload)?;
        Ok(())
    }

    fn get_clock_rate(&mut self, clock_id: u32) -> Result<u32, HalError> {
        let mut payload = [clock_id, 0];
        self.call_tag(TAG_GET_CLOCK_RATE, 4, &mut payload)?;
        if payload[1] != 0 {
            return Ok(payload[1]);
        }

        self.call_tag(TAG_GET_MAX_CLOCK_RATE, 4, &mut payload)?;
        if payload[1] == 0 {
            return Err(HalError::Unsupported("mailbox-clock-rate"));
        }
        Ok(payload[1])
    }

    fn configure_gpio_output(&mut self, gpio: u32, state: u32) -> Result<(), HalError> {
        let mut config = [gpio, GPIO_DIR_OUT, self.gpio_polarity(gpio)?, 0, 0, state];
        self.call_tag(TAG_SET_GPIO_CONFIG, 24, &mut config)?;

        let mut level = [gpio, state];
        self.call_tag(TAG_SET_GPIO_STATE, 8, &mut level)?;
        Ok(())
    }

    fn gpio_polarity(&mut self, gpio: u32) -> Result<u32, HalError> {
        let mut config = [gpio, 0, 0, 0, 0];
        self.call_tag(TAG_GET_GPIO_CONFIG, 4, &mut config)?;
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
                        last_err = HalError::Unsupported("mailbox-protocol");
                        continue;
                    }
                    if alias_index > 0 {
                        let mut line = heapless::String::<192>::new();
                        let _ = core::fmt::Write::write_fmt(
                            &mut line,
                            format_args!(
                                "[pi4-wifi] mailbox alias fallback alias=0x{alias_base:08x}"
                            ),
                        );
                        boot_log::force_uart_line(line.as_str());
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
                    return Err(HalError::Unsupported("mailbox-protocol"));
                }
                return Ok(());
            }
        }
    }

    fn log_timeout(&self, phase: &str) {
        let status0 = self.read_reg(MAILBOX_STATUS0_OFFSET);
        let status1 = self.read_reg(MAILBOX_STATUS1_OFFSET);
        let mut line = heapless::String::<200>::new();
        let _ = core::fmt::Write::write_fmt(
            &mut line,
            format_args!(
                "[pi4-wifi] mailbox timeout phase={phase} regs=0x{regs:08x} status0=0x{status0:08x} status1=0x{status1:08x}",
                regs = self.regs.paddr()
            ),
        );
        boot_log::force_uart_line(line.as_str());
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
        let base_clock_hz = mailbox.query_clock_hz().unwrap_or(100_000_000);
        Ok(Self {
            regs,
            regs_paddr,
            base_clock_hz,
            current_clock_hz: 0,
            desired_bus_width: SdioBusWidth::OneBit,
            card: None,
        })
    }

    fn mark_power_cycled(&mut self) {
        self.card = None;
        self.current_clock_hz = 0;
    }

    fn reset_controller(&mut self) -> Result<(), HalError> {
        self.software_reset(SDHCI_RESET_ALL)?;
        self.write8(SDHCI_POWER_CONTROL, SDHCI_POWER_330 | SDHCI_POWER_ON);
        self.write8(SDHCI_TIMEOUT_CONTROL, 0x0E);
        self.write32(SDHCI_INT_STATUS, SDHCI_INT_ALL_MASK);
        self.write32(SDHCI_INT_ENABLE, SDHCI_INT_ALL_MASK);
        self.write32(SDHCI_SIGNAL_ENABLE, 0);
        self.set_clock_hz(400_000)?;
        self.apply_host_bus_width(self.desired_bus_width);
        self.card = None;
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

        self.reset_controller()?;
        self.send_command(0, 0, ResponseType::None)?;

        let mut ocr = 0u32;
        for _ in 0..SDIO_INIT_WAIT_LOOPS {
            ocr = self.send_command(SDIO_CMD5, 0, ResponseType::Short)?[0];
            if (ocr & SDIO_OCR_3V2_3V4) != 0 {
                break;
            }
            spin_loop();
        }
        if (ocr & SDIO_OCR_3V2_3V4) == 0 {
            return Err(HalError::Unsupported("sdio-ocr-timeout"));
        }

        let desired_ocr = ocr & SDIO_OCR_3V2_3V4;
        for _ in 0..SDIO_INIT_WAIT_LOOPS {
            ocr = self.send_command(SDIO_CMD5, desired_ocr, ResponseType::Short)?[0];
            if (ocr & SDIO_R4_READY) != 0 {
                break;
            }
            spin_loop();
        }
        if (ocr & SDIO_R4_READY) == 0 {
            return Err(HalError::Unsupported("sdio-card-not-ready"));
        }

        let rca = (self.send_command(SDIO_CMD3, 0, ResponseType::Short)?[0] >> 16) as u16;
        if rca == 0 {
            return Err(HalError::Unsupported("sdio-missing-rca"));
        }
        self.send_command(SDIO_CMD7, u32::from(rca) << 16, ResponseType::ShortBusy)?;
        self.card = Some(CardInfo { rca, ocr });
        self.apply_host_bus_width(self.desired_bus_width);

        if matches!(self.desired_bus_width, SdioBusWidth::FourBit) {
            self.io_direct_write(SdioFunction::Function0, SDIO_CCCR_IF, SDIO_BUS_WIDTH_4BIT)?;
        }

        Ok(())
    }

    fn io_direct_read(&mut self, function: SdioFunction, addr: u32) -> Result<u8, HalError> {
        let arg = (u32::from(function.number()) << 28) | ((addr & 0x1_FFFF) << 9);
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
        let arg = (1u32 << 31)
            | (u32::from(function.number()) << 28)
            | ((addr & 0x1_FFFF) << 9)
            | u32::from(value);
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

    fn enable_functions(&mut self) -> Result<(), HalError> {
        let ioex = self.io_direct_read(SdioFunction::Function0, SDIO_CCCR_IOEX)?;
        let desired = ioex | SDIO_FUNC_ENABLE_1 | SDIO_FUNC_ENABLE_2;
        if desired != ioex {
            self.io_direct_write(SdioFunction::Function0, SDIO_CCCR_IOEX, desired)?;
        }
        for _ in 0..SDIO_INIT_WAIT_LOOPS {
            let ready = self.io_direct_read(SdioFunction::Function0, SDIO_CCCR_IORX)?;
            if (ready & (SDIO_FUNC_READY_1 | SDIO_FUNC_READY_2))
                == (SDIO_FUNC_READY_1 | SDIO_FUNC_READY_2)
            {
                let ien = SDIO_CCCR_IEN_FUNC0 | SDIO_CCCR_IEN_FUNC1 | SDIO_CCCR_IEN_FUNC2;
                self.io_direct_write(SdioFunction::Function0, SDIO_CCCR_IENX, ien)?;
                self.set_function_block_size(SdioFunction::Function1, 64)?;
                self.set_function_block_size(SdioFunction::Function2, 256)?;
                return Ok(());
            }
            spin_loop();
        }
        Err(HalError::Unsupported("sdio-function-ready-timeout"))
    }

    fn bring_up_backplane(&mut self) -> Result<(), HalError> {
        self.io_direct_write(
            SdioFunction::Function1,
            SBSDIO_FUNC1_CHIPCLKCSR,
            SBSDIO_FORCE_HW_CLKREQ_OFF | SBSDIO_ALP_AVAIL_REQ,
        )?;
        for _ in 0..SDIO_INIT_WAIT_LOOPS {
            if (self.io_direct_read(SdioFunction::Function1, SBSDIO_FUNC1_CHIPCLKCSR)?
                & SBSDIO_ALP_AVAIL)
                != 0
            {
                break;
            }
            spin_loop();
        }
        self.io_direct_write(
            SdioFunction::Function1,
            SBSDIO_FUNC1_CHIPCLKCSR,
            SBSDIO_FORCE_HW_CLKREQ_OFF,
        )?;
        self.io_direct_write(
            SdioFunction::Function1,
            SBSDIO_FUNC1_SLEEPCSR,
            SBSDIO_FUNC1_SLEEPCSR_KSO_EN,
        )?;
        self.io_direct_write(SdioFunction::Function1, SBSDIO_FUNC1_SDIOPULLUP, 0)?;
        self.io_direct_write(SdioFunction::Function1, SBSDIO_FUNC1_WAKEUPCTRL, 1 << 1)
            .ok();
        Ok(())
    }

    fn setup_firmware_channel(&mut self) -> Result<(), HalError> {
        self.write_f1_u32(
            SDPCMD_REG_TOSBMAILBOXDATA,
            SDPCM_PROT_VERSION << HMB_DATA_VERSION_SHIFT,
        )?;
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

    fn backplane_read32(&mut self, addr: u32) -> Result<u32, HalError> {
        let bytes = self.backplane_read(addr | BACKPLANE_32BIT_FLAG, 4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn backplane_write32(&mut self, addr: u32, value: u32) -> Result<(), HalError> {
        self.backplane_write(addr | BACKPLANE_32BIT_FLAG, &value.to_le_bytes())
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
        self.io_extended(SdioFunction::Function1, addr, true, false, &mut bytes)?;
        Ok(u32::from_le_bytes(bytes))
    }

    fn write_f1_u32(&mut self, addr: u32, value: u32) -> Result<(), HalError> {
        let mut bytes = value.to_le_bytes();
        self.io_extended(SdioFunction::Function1, addr, true, true, &mut bytes)
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
        self.backplane_read32(CYW43_CHIPCOMMON_BASE)
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
        self.ensure_card_ready()?;
        self.enable_functions()?;
        self.bring_up_backplane()?;
        let _ = self.chip_id()?;
        Ok(())
    }

    fn load_firmware(&mut self, bundle: WifiFirmwareBundle<'static>) -> Result<(), HalError> {
        let ram_base = self.firmware_ram_base();
        let ram_size = self.ram_size()?;
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

        let status = self.wait_int(SDHCI_INT_CMD_MASK)?;
        if (status & SDHCI_INT_ERROR) != 0 {
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
            ResponseType::Short | ResponseType::ShortBusy => {
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
        self.wait_inhibit_clear(true)?;
        self.write32(SDHCI_INT_STATUS, SDHCI_INT_ALL_MASK);
        self.write16(
            SDHCI_BLOCK_SIZE,
            u16::try_from(buffer.len()).map_err(|_| HalError::Unsupported("sdhci-block-size"))?,
        );
        self.write16(SDHCI_BLOCK_COUNT, 1);
        self.write32(SDHCI_ARGUMENT, arg);
        let mut transfer_mode = SDHCI_TRNS_BLK_CNT_EN;
        if !write {
            transfer_mode |= SDHCI_TRNS_READ;
        }
        self.write16(SDHCI_TRANSFER_MODE, transfer_mode);
        self.write16(SDHCI_COMMAND, make_command(cmd, ResponseType::Short, true));

        let cmd_status = self.wait_int(SDHCI_INT_CMD_MASK)?;
        if (cmd_status & SDHCI_INT_ERROR) != 0 {
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
            let status = self.wait_int(wait_mask | SDHCI_INT_ERROR | SDHCI_INT_DATA_MASK)?;
            if (status & SDHCI_INT_ERROR) != 0 {
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

        let data_status =
            self.wait_int(SDHCI_INT_DATA_END | SDHCI_INT_ERROR | SDHCI_INT_DATA_MASK)?;
        if (data_status & SDHCI_INT_ERROR) != 0 {
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
        let base = self.regs.vaddr();
        unsafe { ptr::read_volatile((base + offset) as *const u8) }
    }

    fn read16(&self, offset: usize) -> u16 {
        let base = self.regs.vaddr();
        let aligned = offset & !0x3;
        let word = unsafe { ptr::read_volatile((base + aligned) as *const u32) };
        let shift = ((offset & 0x2) * 8) as u32;
        ((word >> shift) & 0xFFFF) as u16
    }

    fn read32(&self, offset: usize) -> u32 {
        let base = self.regs.vaddr();
        unsafe { ptr::read_volatile((base + offset) as *const u32) }
    }

    fn write8(&self, offset: usize, value: u8) {
        let aligned = offset & !0x3;
        let word = self.read32(aligned);
        let shift = ((offset & 0x3) * 8) as u32;
        let mask = !(0xFFu32 << shift);
        self.write32(aligned, (word & mask) | (u32::from(value) << shift));
    }

    fn write16(&self, offset: usize, value: u16) {
        let aligned = offset & !0x3;
        let word = self.read32(aligned);
        let shift = ((offset & 0x2) * 8) as u32;
        let mask = !(0xFFFFu32 << shift);
        let new_word = (word & mask) | (u32::from(value) << shift);
        let base = self.regs.vaddr();
        unsafe { ptr::write_volatile((base + aligned) as *mut u32, new_word) };
        self.write_delay(aligned);
    }

    fn write32(&self, offset: usize, value: u32) {
        let base = self.regs.vaddr();
        unsafe { ptr::write_volatile((base + offset) as *mut u32, value) };
        self.write_delay(offset);
    }

    fn write_delay(&self, offset: usize) {
        if offset == SDHCI_BUFFER {
            return;
        }
        for _ in 0..SDHCI_WRITE_DELAY_LOOPS {
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
        ResponseType::Short => SDHCI_CMD_RESP_SHORT | SDHCI_CMD_CRC | SDHCI_CMD_INDEX,
        ResponseType::ShortBusy => SDHCI_CMD_RESP_SHORT_BUSY | SDHCI_CMD_CRC | SDHCI_CMD_INDEX,
        ResponseType::Long => SDHCI_CMD_RESP_LONG | SDHCI_CMD_CRC,
    };
    if data {
        flags |= SDHCI_CMD_DATA;
    }
    (cmd << 8) | flags
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
    use super::{make_command, normalize_nvram, phys_to_bus, r5_status, ResponseType};

    #[test]
    fn normalize_nvram_appends_newline_nul_and_padding() {
        let nvram = normalize_nvram(b"aa=1\r\nbb=2");
        assert!(nvram.starts_with(b"aa=1\nbb=2\n\0"));
        assert_eq!(nvram.len() % 4, 0);
    }

    #[test]
    fn cmd_flags_encode_expected_response_modes() {
        assert_eq!(make_command(5, ResponseType::None, false) & 0x3F, 0);
        assert_ne!(make_command(52, ResponseType::Short, false) & 0x1C, 0);
        assert_ne!(make_command(53, ResponseType::Short, true) & 0x20, 0);
    }

    #[test]
    fn r5_status_extracts_only_error_bits() {
        assert_eq!(r5_status(0), 0);
        assert_eq!(r5_status(0xCB00), 0xCB00);
        assert_eq!(r5_status(0xFFFF_FFFF), 0xCB00);
    }

    #[test]
    fn phys_to_bus_preserves_low_bits_and_applies_alias() {
        assert_eq!(phys_to_bus(0x3F00_B880, 0xC000_0000), Some(0xFF00_B880));
        assert_eq!(phys_to_bus(0x3F00_B880, 0x4000_0000), Some(0x7F00_B880));
    }
}
