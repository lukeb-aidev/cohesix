// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide a HAL-bound Broadcom GENETv5 NIC backend for Pi4 networking.
// Author: Lukas Bower

//! Broadcom GENETv5 bring-up driver for Raspberry Pi 4 profiles.
//!
//! The implementation intentionally keeps queue sizing and memory allocation
//! bounded. Runtime hardware access is restricted to HAL-provided mappings and
//! DMA frames.
#![allow(unsafe_code)]

#[cfg(target_arch = "aarch64")]
use core::arch::asm;
use core::fmt;
use core::hint::spin_loop;
use core::ops::Range;
use core::sync::atomic::{compiler_fence, fence, Ordering};

use heapless::{Deque, Vec as HeaplessVec};
use log::{debug, info, warn};
use smoltcp::phy::{self, Device, DeviceCapabilities};
use smoltcp::time::Instant;
use smoltcp::wire::EthernetAddress;

use crate::hal::{bcmgenet as genet_hal, dma, DeviceHal, HalError};
use crate::net::{ConsoleNetConfig, NetDevice, NetDeviceCounters, NetDriverError};
use crate::sel4::{RamFrame, PAGE_BITS};

const PAGE_SIZE: usize = 1 << PAGE_BITS;
const MAX_FRAME_LEN: usize = crate::net_consts::MAX_FRAME_LEN;
const RX_RING_DESCS: usize = HW_TOTAL_DESCS;
const TX_RING_DESCS: usize = HW_TOTAL_DESCS;
const RX_READY_CAP: usize = 128;

const GENET_SYS_OFF: usize = 0x0000;
const GENET_EXT_OFF: usize = 0x0080;
const GENET_RBUF_OFF: usize = 0x0300;
const GENET_UMAC_OFF: usize = 0x0800;
const GENET_RX_OFF: usize = 0x2000;
const GENET_TX_OFF: usize = 0x4000;

const SYS_PORT_CTRL: usize = GENET_SYS_OFF + 0x04;
const SYS_RBUF_FLUSH_CTRL: usize = GENET_SYS_OFF + 0x08;
const EXT_RGMII_OOB_CTRL: usize = GENET_EXT_OFF + 0x0c;
const RBUF_CTRL: usize = GENET_RBUF_OFF + 0x00;
const RBUF_TBUF_SIZE_CTRL: usize = GENET_RBUF_OFF + 0xb4;

const PORT_MODE_EXT_GPHY: u32 = 3;
const RGMII_LINK: u32 = 1 << 4;
const OOB_DISABLE: u32 = 1 << 5;
const RGMII_MODE_EN: u32 = 1 << 6;
const ID_MODE_DIS: u32 = 1 << 16;
const RBUF_ALIGN_2B: u32 = 1 << 1;

const GENET_UMAC_CMD: usize = 0x0808;
const GENET_UMAC_MAC0: usize = 0x080C;
const GENET_UMAC_MAC1: usize = 0x0810;
const UMAC_MAX_FRAME_LEN: usize = GENET_UMAC_OFF + 0x14;
const UMAC_TX_FLUSH: usize = GENET_UMAC_OFF + 0x334;
const UMAC_MIB_CTRL: usize = GENET_UMAC_OFF + 0x580;
const MDIO_CMD: usize = GENET_UMAC_OFF + 0x614;

const CMD_TX_EN: u32 = 1 << 0;
const CMD_RX_EN: u32 = 1 << 1;
const CMD_SPEED_SHIFT: u32 = 2;
const CMD_SPEED_MASK: u32 = 0x3;
const CMD_SW_RESET: u32 = 1 << 13;
const CMD_LCL_LOOP_EN: u32 = 1 << 15;
const UMAC_SPEED_10: u32 = 0;
const UMAC_SPEED_100: u32 = 1;
const UMAC_SPEED_1000: u32 = 2;

const MIB_RESET_RX: u32 = 1 << 0;
const MIB_RESET_RUNT: u32 = 1 << 1;
const MIB_RESET_TX: u32 = 1 << 2;

const MDIO_START_BUSY: u32 = 1 << 29;
const MDIO_READ_FAIL: u32 = 1 << 28;
const MDIO_RD: u32 = 2 << 26;
const MDIO_WR: u32 = 1 << 26;
const MDIO_PMD_SHIFT: u32 = 21;
const MDIO_REG_SHIFT: u32 = 16;
const MDIO_FIELD_MASK: u32 = 0x1f;
const MDIO_POLL_TRIES: usize = 10_000;
const MII_BMCR: u8 = 0;
const MII_BMSR: u8 = 1;
const MII_ADVERTISE: u8 = 4;
const MII_LPA: u8 = 5;
const MII_CTRL1000: u8 = 9;
const MII_STAT1000: u8 = 10;
const MII_PHYSID1: u8 = 2;
const MII_PHYSID2: u8 = 3;
const MII_BMSR_LSTATUS: u16 = 1 << 2;
const MII_BMSR_ANEGCOMPLETE: u16 = 1 << 5;
const MII_BMCR_SPEED100: u16 = 1 << 13;
const MII_BMCR_SPEED1000: u16 = 1 << 6;
const MII_BMCR_ANENABLE: u16 = 1 << 12;
const MII_BMCR_ANRESTART: u16 = 1 << 9;
const LPA_10HALF: u16 = 0x0020;
const LPA_10FULL: u16 = 0x0040;
const LPA_100HALF: u16 = 0x0080;
const LPA_100FULL: u16 = 0x0100;
const ADVERTISE_1000HALF: u16 = 0x0100;
const ADVERTISE_1000FULL: u16 = 0x0200;
const LPA_1000HALF: u16 = 0x0400;
const LPA_1000FULL: u16 = 0x0800;
const PHY_LINK_POLL_TRIES: usize = 2_000;
const PHY_LINK_POLL_DELAY_SPINS: usize = 50_000;

const DMA_EN: u32 = 1 << 0;
const DMA_RING_BUF_EN_SHIFT: u32 = 1;
const DMA_BUFLENGTH_MASK: u32 = 0x0fff;
const DMA_BUFLENGTH_SHIFT: u32 = 16;
const DMA_RING_SIZE_SHIFT: u32 = 16;
const DMA_OWN: u32 = 0x8000;
const DMA_EOP: u32 = 0x4000;
const DMA_SOP: u32 = 0x2000;
const DMA_TX_APPEND_CRC: u32 = 0x0040;
const DMA_TX_QTAG_SHIFT: u32 = 7;
const DMA_DEFAULT_QTAG: u32 = 0x3f;
const DMA_MAX_BURST_LENGTH: u32 = 0x8;
const DMA_DESC_SIZE: usize = 12;
const DEFAULT_Q: u32 = 0x10;
const DMA_RING_SIZE: usize = 0x40;
const DMA_RINGS_SIZE: usize = DMA_RING_SIZE * ((DEFAULT_Q as usize) + 1);
const HW_TOTAL_DESCS: usize = 256;
const GENET_RDMA_REG_OFF: usize = GENET_RX_OFF + HW_TOTAL_DESCS * DMA_DESC_SIZE;
const GENET_TDMA_REG_OFF: usize = GENET_TX_OFF + HW_TOTAL_DESCS * DMA_DESC_SIZE;
const RDMA_RING_REG_BASE: usize = GENET_RDMA_REG_OFF + (DEFAULT_Q as usize) * DMA_RING_SIZE;
const TDMA_RING_REG_BASE: usize = GENET_TDMA_REG_OFF + (DEFAULT_Q as usize) * DMA_RING_SIZE;
const RDMA_REG_BASE: usize = GENET_RDMA_REG_OFF + DMA_RINGS_SIZE;
const TDMA_REG_BASE: usize = GENET_TDMA_REG_OFF + DMA_RINGS_SIZE;
const DMA_RING_CFG: usize = 0x00;
const DMA_CTRL: usize = 0x04;
const DMA_SCB_BURST_SIZE: usize = 0x0c;
const DMA_RING_BUF_SIZE: usize = 0x10;
const DMA_START_ADDR: usize = 0x14;
const DMA_END_ADDR: usize = 0x1c;
const DMA_MBUF_DONE_THRESH: usize = 0x24;
const TDMA_FLOW_PERIOD: usize = TDMA_RING_REG_BASE + 0x28;
const TDMA_READ_PTR: usize = TDMA_RING_REG_BASE + 0x00;
const TDMA_CONS_INDEX: usize = TDMA_RING_REG_BASE + 0x08;
const TDMA_PROD_INDEX: usize = TDMA_RING_REG_BASE + 0x0c;
const TDMA_WRITE_PTR: usize = TDMA_RING_REG_BASE + 0x2c;
const RDMA_WRITE_PTR: usize = RDMA_RING_REG_BASE + 0x00;
const RDMA_PROD_INDEX: usize = RDMA_RING_REG_BASE + 0x08;
const RDMA_CONS_INDEX: usize = RDMA_RING_REG_BASE + 0x0c;
const RDMA_XON_XOFF_THRESH: usize = RDMA_RING_REG_BASE + 0x28;
const RDMA_READ_PTR: usize = RDMA_RING_REG_BASE + 0x2c;
const DMA_FC_THRESH_LO: u32 = 5;

const ENET_MAX_MTU_SIZE: usize = 1536;
const RX_BUF_LENGTH: usize = 2048;
const RX_BUF_OFFSET: usize = 2;
const TX_STALL_LOG_POLL_THRESHOLD: u32 = 8_192;
const TX_BACKPRESSURE_LOG_POLL_THRESHOLD: u32 = 8_192;
const TX_DROP_LOG_INTERVAL: u32 = 512;
const RX_IDLE_LOG_POLL_THRESHOLD: u32 = 65_536;

#[derive(Clone, Copy, Debug, Default)]
struct DmaDesc {
    len_status: u32,
    addr_lo: u32,
    addr_hi: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BreadcrumbReason {
    TxNoRoom,
    TxNoRoomRecovered,
    TxRingFull,
    TxConsStalled,
    TxConsRecovered,
    TxConsJump,
    TxSwIndexInvalid,
    TxHwIndexInvalid,
    TxFirstCompletion,
    RxProdStalled,
    RxProdRecovered,
    RxFirstFrame,
}

impl BreadcrumbReason {
    const fn as_str(self) -> &'static str {
        match self {
            Self::TxNoRoom => "tx-no-room",
            Self::TxNoRoomRecovered => "tx-no-room-recovered",
            Self::TxRingFull => "tx-ring-full",
            Self::TxConsStalled => "tx-cons-stalled",
            Self::TxConsRecovered => "tx-cons-recovered",
            Self::TxConsJump => "tx-cons-jump",
            Self::TxSwIndexInvalid => "tx-sw-index-invalid",
            Self::TxHwIndexInvalid => "tx-hw-index-invalid",
            Self::TxFirstCompletion => "tx-first-completion",
            Self::RxProdStalled => "rx-prod-stalled",
            Self::RxProdRecovered => "rx-prod-recovered",
            Self::RxFirstFrame => "rx-first-frame",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct DmaBreadcrumbSnapshot {
    tx_sw_prod: u16,
    tx_sw_cons: u16,
    rx_sw_cons: u16,
    tx_hw_prod: u16,
    tx_hw_cons: u16,
    rx_hw_prod: u16,
    rx_hw_cons: u16,
    tx_sw_in_flight: u16,
    tx_hw_in_flight: u16,
    tx_read_ptr: u32,
    tx_write_ptr: u32,
    rx_read_ptr: u32,
    rx_write_ptr: u32,
    umac_cmd: u32,
    oob_ctrl: u32,
    tdma_ctrl: u32,
    rdma_ctrl: u32,
    tx_cons_len_status: u32,
    tx_prod_len_status: u32,
    tx_cons_addr_lo: u32,
    tx_cons_addr_hi: u32,
    tx_prod_addr_lo: u32,
    tx_prod_addr_hi: u32,
    rx_cons_len_status: u32,
    rx_prod_len_status: u32,
    rx_cons_addr_lo: u32,
    rx_cons_addr_hi: u32,
    rx_prod_addr_lo: u32,
    rx_prod_addr_hi: u32,
    rx_ready_len: u16,
}

#[derive(Debug)]
pub enum DriverError {
    NoDevice,
    Hal(HalError),
    QueueInit,
}

impl fmt::Display for DriverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDevice => f.write_str("bcmgenet device not present"),
            Self::Hal(err) => write!(f, "{err}"),
            Self::QueueInit => f.write_str("bcmgenet queue init failed"),
        }
    }
}

impl From<HalError> for DriverError {
    fn from(value: HalError) -> Self {
        Self::Hal(value)
    }
}

impl NetDriverError for DriverError {
    fn is_absent(&self) -> bool {
        matches!(self, Self::NoDevice)
    }
}

pub struct BcmGenetDevice {
    regs: genet_hal::BcmGenetRegisters,
    mmio_base: usize,
    rx_frames: HeaplessVec<RamFrame, RX_RING_DESCS>,
    tx_frames: HeaplessVec<RamFrame, TX_RING_DESCS>,
    rx_dma_shares: HeaplessVec<Option<dma::PinnedDmaRange>, RX_RING_DESCS>,
    tx_dma_shares: HeaplessVec<Option<dma::PinnedDmaRange>, TX_RING_DESCS>,
    tx_prod_index: u16,
    tx_cons_index: u16,
    rx_cons_index: u16,
    rx_ready: Deque<HeaplessVec<u8, MAX_FRAME_LEN>, RX_READY_CAP>,
    mac: EthernetAddress,
    dma_cacheable: bool,
    tx_drops: u32,
    counters: NetDeviceCounters,
    tx_stall_polls: u32,
    tx_stall_logged: bool,
    tx_backpressure_polls: u32,
    tx_backpressure_logged: bool,
    rx_idle_polls: u32,
    rx_idle_logged: bool,
    crumb_seq: u64,
    crumb_repeat: u32,
    crumb_suppressed: u32,
    crumb_last_reason: Option<BreadcrumbReason>,
    crumb_last_snapshot: Option<DmaBreadcrumbSnapshot>,
}

pub struct RxToken {
    frame: HeaplessVec<u8, MAX_FRAME_LEN>,
}

pub struct TxToken<'a> {
    device: &'a mut BcmGenetDevice,
}

impl BcmGenetDevice {
    pub fn new<H>(hal: &mut H) -> Result<Self, DriverError>
    where
        H: DeviceHal<Error = HalError>,
    {
        let regs = genet_hal::map_registers(hal).map_err(|err| match err {
            HalError::Unsupported("bcmgenet-mmio-not-covered") => DriverError::NoDevice,
            other => DriverError::from(other),
        })?;
        let mmio_base = regs.base_paddr();
        let dma_attr = if genet_hal::dma_uncached() {
            sel4_sys::seL4_ARM_Page_Uncached
        } else {
            sel4_sys::seL4_ARM_Page_Default
        };
        let dma_cacheable = !genet_hal::dma_uncached();
        let mut rx_frames = HeaplessVec::new();
        let mut tx_frames = HeaplessVec::new();
        let mut rx_dma_shares = HeaplessVec::new();
        let mut tx_dma_shares = HeaplessVec::new();
        for _ in 0..RX_RING_DESCS {
            rx_frames
                .push(hal.alloc_dma_frame_low_attr(dma_attr)?)
                .map_err(|_| DriverError::QueueInit)?;
            rx_dma_shares
                .push(None)
                .map_err(|_| DriverError::QueueInit)?;
        }
        for _ in 0..TX_RING_DESCS {
            tx_frames
                .push(hal.alloc_dma_frame_low_attr(dma_attr)?)
                .map_err(|_| DriverError::QueueInit)?;
            tx_dma_shares
                .push(None)
                .map_err(|_| DriverError::QueueInit)?;
        }
        Self::validate_dma_frames(&rx_frames, &tx_frames)?;
        let rx_range = Self::dma_frame_range(&rx_frames).ok_or(DriverError::QueueInit)?;
        let tx_range = Self::dma_frame_range(&tx_frames).ok_or(DriverError::QueueInit)?;

        let mut device = Self {
            regs,
            mmio_base,
            rx_frames,
            tx_frames,
            rx_dma_shares,
            tx_dma_shares,
            tx_prod_index: 0,
            tx_cons_index: 0,
            rx_cons_index: 0,
            rx_ready: Deque::new(),
            mac: EthernetAddress([0x02, 0x43, 0x4f, 0x48, 0x58, 0x01]),
            dma_cacheable,
            tx_drops: 0,
            counters: NetDeviceCounters::default(),
            tx_stall_polls: 0,
            tx_stall_logged: false,
            tx_backpressure_polls: 0,
            tx_backpressure_logged: false,
            rx_idle_polls: 0,
            rx_idle_logged: false,
            crumb_seq: 0,
            crumb_repeat: 0,
            crumb_suppressed: 0,
            crumb_last_reason: None,
            crumb_last_snapshot: None,
        };
        device.init_hardware();
        device.mac = device.read_or_default_mac();
        device.write_mac(device.mac);
        device.refresh_tx_counters();
        let dma_attr_raw: usize = crate::sel4::vm_attributes_raw(dma_attr) as usize;
        info!(
            "[bcmgenet][dma] attr=0x{:08x} cacheable={} rx=[0x{:016x}..0x{:016x}) tx=[0x{:016x}..0x{:016x})",
            dma_attr_raw,
            dma_cacheable,
            rx_range.0,
            rx_range.1,
            tx_range.0,
            tx_range.1,
        );
        info!(
            "[bcmgenet] init complete mmio=0x{:016x} pages={} ring_frames={} mac={} tx_idx={} rx_idx={} dma_alias={} dma_cacheable={}",
            device.mmio_base,
            genet_hal::BCMGENET_MMIO_PAGE_COUNT,
            TX_RING_DESCS,
            device.mac,
            device.tx_prod_index,
            device.rx_cons_index,
            genet_hal::dma_address_policy_name(),
            device.dma_cacheable,
        );
        Ok(device)
    }

    fn dma_frame_range<const N: usize>(frames: &HeaplessVec<RamFrame, N>) -> Option<(u64, u64)> {
        let mut start = u64::MAX;
        let mut end = 0u64;
        for frame in frames.iter() {
            let paddr = frame.paddr() as u64;
            start = start.min(paddr);
            end = end.max(paddr.saturating_add(PAGE_SIZE as u64));
        }
        if start == u64::MAX || end <= start {
            None
        } else {
            Some((start, end))
        }
    }

    fn validate_dma_frames(
        rx_frames: &HeaplessVec<RamFrame, RX_RING_DESCS>,
        tx_frames: &HeaplessVec<RamFrame, TX_RING_DESCS>,
    ) -> Result<(), DriverError> {
        let page_mask = (PAGE_SIZE as u64).saturating_sub(1);
        for (idx, frame) in rx_frames.iter().enumerate() {
            let paddr = frame.paddr() as u64;
            let vaddr = frame.ptr().as_ptr() as usize;
            if paddr == 0 || (paddr & page_mask) != 0 || (vaddr & (PAGE_SIZE - 1)) != 0 {
                warn!(
                    "[bcmgenet] invalid RX DMA frame idx={} paddr=0x{:016x} vaddr=0x{:016x}",
                    idx, paddr, vaddr
                );
                return Err(DriverError::QueueInit);
            }
        }
        for (idx, frame) in tx_frames.iter().enumerate() {
            let paddr = frame.paddr() as u64;
            let vaddr = frame.ptr().as_ptr() as usize;
            if paddr == 0 || (paddr & page_mask) != 0 || (vaddr & (PAGE_SIZE - 1)) != 0 {
                warn!(
                    "[bcmgenet] invalid TX DMA frame idx={} paddr=0x{:016x} vaddr=0x{:016x}",
                    idx, paddr, vaddr
                );
                return Err(DriverError::QueueInit);
            }
        }
        for i in 0..rx_frames.len() {
            let paddr_i = rx_frames[i].paddr() as u64;
            for j in (i + 1)..rx_frames.len() {
                if paddr_i == rx_frames[j].paddr() as u64 {
                    warn!(
                        "[bcmgenet] duplicate RX DMA frame paddr=0x{:016x} i={} j={}",
                        paddr_i, i, j
                    );
                    return Err(DriverError::QueueInit);
                }
            }
            for j in 0..tx_frames.len() {
                if paddr_i == tx_frames[j].paddr() as u64 {
                    warn!(
                        "[bcmgenet] RX/TX DMA frame alias paddr=0x{:016x} rx={} tx={}",
                        paddr_i, i, j
                    );
                    return Err(DriverError::QueueInit);
                }
            }
        }
        for i in 0..tx_frames.len() {
            let paddr_i = tx_frames[i].paddr() as u64;
            for j in (i + 1)..tx_frames.len() {
                if paddr_i == tx_frames[j].paddr() as u64 {
                    warn!(
                        "[bcmgenet] duplicate TX DMA frame paddr=0x{:016x} i={} j={}",
                        paddr_i, i, j
                    );
                    return Err(DriverError::QueueInit);
                }
            }
        }
        Ok(())
    }

    fn init_hardware(&mut self) {
        self.disable_dma();
        self.init_umac();
        self.init_phy_link();
        self.init_rx_ring();
        self.init_tx_ring();
        self.init_rx_descriptors();
        self.init_tx_descriptors();
        self.enable_dma();
        let cmd = self.read_reg32(GENET_UMAC_CMD);
        self.write_reg32(GENET_UMAC_CMD, cmd | CMD_TX_EN | CMD_RX_EN);
    }

    fn init_umac(&mut self) {
        let mut flush = self.read_reg32(SYS_RBUF_FLUSH_CTRL);
        flush |= 1 << 1;
        self.write_reg32(SYS_RBUF_FLUSH_CTRL, flush);
        for _ in 0..10_000 {
            spin_loop();
        }
        flush &= !(1 << 1);
        self.write_reg32(SYS_RBUF_FLUSH_CTRL, flush);
        for _ in 0..10_000 {
            spin_loop();
        }
        self.write_reg32(SYS_RBUF_FLUSH_CTRL, 0);
        for _ in 0..10_000 {
            spin_loop();
        }

        self.write_reg32(GENET_UMAC_CMD, 0);
        self.write_reg32(GENET_UMAC_CMD, CMD_SW_RESET | CMD_LCL_LOOP_EN);
        for _ in 0..2_000 {
            spin_loop();
        }
        self.write_reg32(GENET_UMAC_CMD, 0);
        self.write_reg32(UMAC_MIB_CTRL, MIB_RESET_RX | MIB_RESET_TX | MIB_RESET_RUNT);
        self.write_reg32(UMAC_MIB_CTRL, 0);
        self.write_reg32(UMAC_MAX_FRAME_LEN, ENET_MAX_MTU_SIZE as u32);
        let rbuf_ctrl = self.read_reg32(RBUF_CTRL) | RBUF_ALIGN_2B;
        self.write_reg32(RBUF_CTRL, rbuf_ctrl);
        self.write_reg32(RBUF_TBUF_SIZE_CTRL, 1);
        self.write_reg32(SYS_RBUF_FLUSH_CTRL, 0);
        self.write_reg32(UMAC_TX_FLUSH, 0);
    }

    fn init_phy_link(&mut self) {
        self.write_reg32(SYS_PORT_CTRL, PORT_MODE_EXT_GPHY);
        let mut oob = self.read_reg32(EXT_RGMII_OOB_CTRL);
        oob &= !OOB_DISABLE;
        oob |= RGMII_LINK | RGMII_MODE_EN | ID_MODE_DIS;
        self.write_reg32(EXT_RGMII_OOB_CTRL, oob);

        // Keep PHY/MAC aligned even when autoneg takes longer at boot.
        // A mismatched MAC speed can present as TX queue stalls + no RX.
        let mut speed = self.current_umac_speed();
        let initial_speed = speed;
        let mut link_up = false;
        let mut autoneg_complete = false;
        let mut resolved_speed = None;
        if let Some(phy_addr) = self.discover_phy_addr() {
            if let Ok(bmcr) = self.mdio_read(phy_addr, MII_BMCR) {
                if (bmcr & MII_BMCR_ANENABLE) == 0 {
                    let mut next = bmcr | MII_BMCR_ANENABLE | MII_BMCR_ANRESTART;
                    // Ensure forced-speed bits do not conflict with autoneg.
                    next &= !(MII_BMCR_SPEED100 | MII_BMCR_SPEED1000);
                    let _ = self.mdio_write(phy_addr, MII_BMCR, next);
                }
            }

            for _ in 0..PHY_LINK_POLL_TRIES {
                // BMSR link bit is latch-low, so read twice to observe current state.
                let _ = self.mdio_read(phy_addr, MII_BMSR);
                let Ok(status) = self.mdio_read(phy_addr, MII_BMSR) else {
                    for _ in 0..PHY_LINK_POLL_DELAY_SPINS {
                        spin_loop();
                    }
                    continue;
                };
                link_up = (status & MII_BMSR_LSTATUS) != 0;
                autoneg_complete = (status & MII_BMSR_ANEGCOMPLETE) != 0;
                if let Some(resolved) = self.resolve_phy_speed(phy_addr) {
                    resolved_speed = Some(resolved);
                }
                if link_up && (autoneg_complete || resolved_speed.is_some()) {
                    break;
                }
                for _ in 0..PHY_LINK_POLL_DELAY_SPINS {
                    spin_loop();
                }
            }

            if let Some(resolved) = resolved_speed {
                speed = resolved;
            } else if let Ok(bmcr) = self.mdio_read(phy_addr, MII_BMCR) {
                speed = decode_bmcr_speed(bmcr);
            } else if link_up {
                // If link is up but speed cannot be resolved yet, prefer 1000M over
                // stale/reset 10M to avoid a persistent MAC/PHY mismatch.
                speed = UMAC_SPEED_1000;
            } else {
                speed = initial_speed;
            }
            info!(
                "[bcmgenet] phy addr={} link_up={} autoneg={} speed={} fallback={} resolved={}",
                phy_addr,
                link_up,
                autoneg_complete,
                speed_label(speed),
                speed_label(initial_speed),
                if resolved_speed.is_some() { 1 } else { 0 },
            );
        } else {
            warn!(
                "[bcmgenet] no MDIO PHY discovered; retaining UMAC speed={}",
                speed_label(speed)
            );
        }
        self.set_umac_speed(speed);
    }

    fn discover_phy_addr(&self) -> Option<u8> {
        for addr in 0..32u8 {
            let Ok(id1) = self.mdio_read(addr, MII_PHYSID1) else {
                continue;
            };
            let Ok(id2) = self.mdio_read(addr, MII_PHYSID2) else {
                continue;
            };
            if id1 == 0 || id1 == u16::MAX || id2 == 0 || id2 == u16::MAX {
                continue;
            }
            return Some(addr);
        }
        None
    }

    fn mdio_wait_idle(&self) -> bool {
        for _ in 0..MDIO_POLL_TRIES {
            if (self.read_reg32(MDIO_CMD) & MDIO_START_BUSY) == 0 {
                return true;
            }
        }
        false
    }

    fn mdio_read(&self, phy_addr: u8, reg: u8) -> Result<u16, ()> {
        if !self.mdio_wait_idle() {
            return Err(());
        }
        let mut cmd = MDIO_RD
            | ((u32::from(phy_addr) & MDIO_FIELD_MASK) << MDIO_PMD_SHIFT)
            | ((u32::from(reg) & MDIO_FIELD_MASK) << MDIO_REG_SHIFT);
        self.write_reg32(MDIO_CMD, cmd);
        cmd |= MDIO_START_BUSY;
        self.write_reg32(MDIO_CMD, cmd);
        if !self.mdio_wait_idle() {
            return Err(());
        }
        let value = self.read_reg32(MDIO_CMD);
        if (value & MDIO_READ_FAIL) != 0 {
            return Err(());
        }
        Ok((value & 0xffff) as u16)
    }

    fn mdio_write(&self, phy_addr: u8, reg: u8, value: u16) -> Result<(), ()> {
        if !self.mdio_wait_idle() {
            return Err(());
        }
        let cmd = MDIO_WR
            | ((u32::from(phy_addr) & MDIO_FIELD_MASK) << MDIO_PMD_SHIFT)
            | ((u32::from(reg) & MDIO_FIELD_MASK) << MDIO_REG_SHIFT)
            | u32::from(value);
        self.write_reg32(MDIO_CMD, cmd | MDIO_START_BUSY);
        if !self.mdio_wait_idle() {
            return Err(());
        }
        let status = self.read_reg32(MDIO_CMD);
        if (status & MDIO_READ_FAIL) != 0 {
            return Err(());
        }
        Ok(())
    }

    fn resolve_phy_speed(&self, phy_addr: u8) -> Option<u32> {
        let adv_1000 = self.mdio_read(phy_addr, MII_CTRL1000).ok()?;
        let lpa_1000 = self.mdio_read(phy_addr, MII_STAT1000).ok()?;
        if (adv_1000 & ADVERTISE_1000FULL) != 0 && (lpa_1000 & LPA_1000FULL) != 0 {
            return Some(UMAC_SPEED_1000);
        }
        if (adv_1000 & ADVERTISE_1000HALF) != 0 && (lpa_1000 & LPA_1000HALF) != 0 {
            return Some(UMAC_SPEED_1000);
        }

        let adv = self.mdio_read(phy_addr, MII_ADVERTISE).ok()?;
        let lpa = self.mdio_read(phy_addr, MII_LPA).ok()?;
        if (adv & LPA_100FULL) != 0 && (lpa & LPA_100FULL) != 0 {
            return Some(UMAC_SPEED_100);
        }
        if (adv & LPA_100HALF) != 0 && (lpa & LPA_100HALF) != 0 {
            return Some(UMAC_SPEED_100);
        }
        if (adv & LPA_10FULL) != 0 && (lpa & LPA_10FULL) != 0 {
            return Some(UMAC_SPEED_10);
        }
        if (adv & LPA_10HALF) != 0 && (lpa & LPA_10HALF) != 0 {
            return Some(UMAC_SPEED_10);
        }
        None
    }

    fn current_umac_speed(&self) -> u32 {
        (self.read_reg32(GENET_UMAC_CMD) >> CMD_SPEED_SHIFT) & CMD_SPEED_MASK
    }

    fn set_umac_speed(&self, speed: u32) {
        let mut cmd = self.read_reg32(GENET_UMAC_CMD);
        cmd &= !(CMD_SPEED_MASK << CMD_SPEED_SHIFT);
        cmd |= (speed & CMD_SPEED_MASK) << CMD_SPEED_SHIFT;
        self.write_reg32(GENET_UMAC_CMD, cmd);
    }

    fn disable_dma(&mut self) {
        let tdma_ctrl = self.read_reg32(TDMA_REG_BASE + DMA_CTRL);
        self.write_reg32(TDMA_REG_BASE + DMA_CTRL, tdma_ctrl & !DMA_EN);
        let rdma_ctrl = self.read_reg32(RDMA_REG_BASE + DMA_CTRL);
        self.write_reg32(RDMA_REG_BASE + DMA_CTRL, rdma_ctrl & !DMA_EN);
        self.write_reg32(UMAC_TX_FLUSH, 1);
        for _ in 0..10_000 {
            spin_loop();
        }
        self.write_reg32(UMAC_TX_FLUSH, 0);
    }

    fn enable_dma(&mut self) {
        let dma_ctrl = (1u32 << (DEFAULT_Q + DMA_RING_BUF_EN_SHIFT)) | DMA_EN;
        self.write_reg32(TDMA_REG_BASE + DMA_CTRL, dma_ctrl);
        let rdma_ctrl = self.read_reg32(RDMA_REG_BASE + DMA_CTRL);
        self.write_reg32(RDMA_REG_BASE + DMA_CTRL, rdma_ctrl | dma_ctrl);
    }

    fn init_rx_ring(&mut self) {
        let ring_len = self.rx_ring_len();
        self.write_reg32(RDMA_REG_BASE + DMA_SCB_BURST_SIZE, DMA_MAX_BURST_LENGTH);
        self.write_reg32(RDMA_RING_REG_BASE + DMA_START_ADDR, 0);
        self.write_reg32(RDMA_READ_PTR, 0);
        self.write_reg32(RDMA_WRITE_PTR, 0);
        self.write_reg32(RDMA_RING_REG_BASE + DMA_END_ADDR, ring_end_addr(ring_len));
        let prod = self.read_reg32(RDMA_PROD_INDEX) as u16;
        self.rx_cons_index = prod;
        self.write_reg32(RDMA_CONS_INDEX, prod as u32);
        info!(
            "[bcmgenet] rx ring init prod={} cons={} slot={} ring_len={}",
            prod,
            self.rx_cons_index,
            ring_slot(self.rx_cons_index, ring_len),
            ring_len
        );
        self.write_reg32(
            RDMA_RING_REG_BASE + DMA_RING_BUF_SIZE,
            ring_buffer_size(ring_len),
        );
        self.write_reg32(RDMA_XON_XOFF_THRESH, dma_fc_thresh_value(ring_len));
        self.write_reg32(RDMA_REG_BASE + DMA_RING_CFG, 1u32 << DEFAULT_Q);
    }

    fn init_tx_ring(&mut self) {
        let ring_len = self.tx_ring_len();
        self.write_reg32(TDMA_REG_BASE + DMA_SCB_BURST_SIZE, DMA_MAX_BURST_LENGTH);
        self.write_reg32(TDMA_RING_REG_BASE + DMA_START_ADDR, 0);
        self.write_reg32(TDMA_READ_PTR, 0);
        self.write_reg32(TDMA_WRITE_PTR, 0);
        self.write_reg32(TDMA_RING_REG_BASE + DMA_END_ADDR, ring_end_addr(ring_len));
        let cons = self.read_reg32(TDMA_CONS_INDEX) as u16;
        self.tx_cons_index = cons;
        self.tx_prod_index = cons;
        self.write_reg32(TDMA_PROD_INDEX, cons as u32);
        info!(
            "[bcmgenet] tx ring init cons={} prod={} slot={} ring_len={}",
            self.tx_cons_index,
            self.tx_prod_index,
            ring_slot(self.tx_prod_index, ring_len),
            ring_len
        );
        self.write_reg32(TDMA_RING_REG_BASE + DMA_MBUF_DONE_THRESH, 1);
        self.write_reg32(TDMA_FLOW_PERIOD, 0);
        self.write_reg32(
            TDMA_RING_REG_BASE + DMA_RING_BUF_SIZE,
            ring_buffer_size(ring_len),
        );
        self.write_reg32(TDMA_REG_BASE + DMA_RING_CFG, 1u32 << DEFAULT_Q);
    }

    fn init_rx_descriptors(&mut self) {
        for slot in 0..self.rx_ring_len() {
            self.rearm_rx_slot(slot);
        }
    }

    fn init_tx_descriptors(&mut self) {
        for slot in 0..self.tx_ring_len() {
            self.write_tx_desc(slot, 0, 0);
        }
    }

    fn read_or_default_mac(&self) -> EthernetAddress {
        let mac0 = self.read_reg32(GENET_UMAC_MAC0);
        let mac1 = self.read_reg32(GENET_UMAC_MAC1);
        let mac = EthernetAddress([
            ((mac0 >> 24) & 0xff) as u8,
            ((mac0 >> 16) & 0xff) as u8,
            ((mac0 >> 8) & 0xff) as u8,
            (mac0 & 0xff) as u8,
            ((mac1 >> 8) & 0xff) as u8,
            (mac1 & 0xff) as u8,
        ]);
        if mac.0 == [0; 6] {
            EthernetAddress([0x02, 0x43, 0x4f, 0x48, 0x58, 0x01])
        } else {
            mac
        }
    }

    fn write_mac(&self, mac: EthernetAddress) {
        let bytes = mac.0;
        let mac0 = (u32::from(bytes[0]) << 24)
            | (u32::from(bytes[1]) << 16)
            | (u32::from(bytes[2]) << 8)
            | u32::from(bytes[3]);
        let mac1 = (u32::from(bytes[4]) << 8) | u32::from(bytes[5]);
        self.write_reg32(GENET_UMAC_MAC0, mac0);
        self.write_reg32(GENET_UMAC_MAC1, mac1);
    }

    fn tx_ring_len(&self) -> usize {
        self.tx_frames.len()
    }

    fn rx_ring_len(&self) -> usize {
        self.rx_frames.len()
    }

    fn read_tx_desc(&self, slot: usize) -> Option<DmaDesc> {
        let base = tx_desc_offset(slot)?;
        Some(DmaDesc {
            len_status: self.read_reg32(base),
            addr_lo: self.read_reg32(base + 0x04),
            addr_hi: self.read_reg32(base + 0x08),
        })
    }

    fn read_rx_desc(&self, slot: usize) -> Option<DmaDesc> {
        let base = rx_desc_offset(slot)?;
        Some(DmaDesc {
            len_status: self.read_reg32(base),
            addr_lo: self.read_reg32(base + 0x04),
            addr_hi: self.read_reg32(base + 0x08),
        })
    }

    fn write_tx_desc(&mut self, slot: usize, paddr: u64, len_status: u32) {
        if let Some(base) = tx_desc_offset(slot) {
            self.write_reg32(base + 0x04, paddr as u32);
            self.write_reg32(base + 0x08, (paddr >> 32) as u32);
            self.write_reg32(base, len_status);
        }
    }

    fn write_rx_desc(&mut self, slot: usize, paddr: u64, len_status: u32) {
        if let Some(base) = rx_desc_offset(slot) {
            self.write_reg32(base + 0x04, paddr as u32);
            self.write_reg32(base + 0x08, (paddr >> 32) as u32);
            self.write_reg32(base, len_status);
        }
    }

    fn refresh_tx_counters(&mut self) {
        let ring_len = self.tx_ring_len();
        let in_flight = ring_distance(self.tx_prod_index, self.tx_cons_index) as usize;
        if in_flight > ring_len {
            self.counters.tx_invalid_used_state =
                self.counters.tx_invalid_used_state.saturating_add(1);
            self.counters.tx_in_flight = ring_len as u64;
            self.counters.tx_free = 0;
            self.log_dma_breadcrumb(BreadcrumbReason::TxSwIndexInvalid);
            return;
        }
        self.counters.tx_in_flight = in_flight as u64;
        self.counters.tx_free = ring_len.saturating_sub(in_flight) as u64;
    }

    fn tx_in_flight(&self) -> usize {
        ring_distance(self.tx_prod_index, self.tx_cons_index) as usize
    }

    fn tx_has_room(&mut self) -> bool {
        if self.tx_ring_len() == 0 {
            return false;
        }
        self.poll_tx_completions();
        if self.tx_in_flight() < self.tx_ring_len() {
            if self.tx_backpressure_logged {
                self.log_dma_breadcrumb(BreadcrumbReason::TxNoRoomRecovered);
            }
            self.tx_backpressure_polls = 0;
            self.tx_backpressure_logged = false;
            true
        } else {
            self.tx_backpressure_polls = self.tx_backpressure_polls.saturating_add(1);
            if self.tx_backpressure_polls >= TX_BACKPRESSURE_LOG_POLL_THRESHOLD
                && !self.tx_backpressure_logged
            {
                self.log_dma_breadcrumb(BreadcrumbReason::TxNoRoom);
                self.tx_backpressure_logged = true;
            }
            false
        }
    }

    fn capture_breadcrumb_snapshot(&self) -> DmaBreadcrumbSnapshot {
        let tx_ring_len = self.tx_ring_len();
        let rx_ring_len = self.rx_ring_len();
        let tx_hw_prod = self.read_reg32(TDMA_PROD_INDEX) as u16;
        let tx_hw_cons = self.read_reg32(TDMA_CONS_INDEX) as u16;
        let rx_hw_prod = self.read_reg32(RDMA_PROD_INDEX) as u16;
        let rx_hw_cons = self.read_reg32(RDMA_CONS_INDEX) as u16;
        let tx_cons_slot = ring_slot(self.tx_cons_index, tx_ring_len);
        let tx_prod_slot = ring_slot(self.tx_prod_index, tx_ring_len);
        let rx_cons_slot = ring_slot(self.rx_cons_index, rx_ring_len);
        let rx_prod_slot = ring_slot(rx_hw_prod, rx_ring_len);
        let tx_cons_desc = self.read_tx_desc(tx_cons_slot).unwrap_or_default();
        let tx_prod_desc = self.read_tx_desc(tx_prod_slot).unwrap_or_default();
        let rx_cons_desc = self.read_rx_desc(rx_cons_slot).unwrap_or_default();
        let rx_prod_desc = self.read_rx_desc(rx_prod_slot).unwrap_or_default();

        DmaBreadcrumbSnapshot {
            tx_sw_prod: self.tx_prod_index,
            tx_sw_cons: self.tx_cons_index,
            rx_sw_cons: self.rx_cons_index,
            tx_hw_prod,
            tx_hw_cons,
            rx_hw_prod,
            rx_hw_cons,
            tx_sw_in_flight: ring_distance(self.tx_prod_index, self.tx_cons_index),
            tx_hw_in_flight: ring_distance(tx_hw_prod, tx_hw_cons),
            tx_read_ptr: self.read_reg32(TDMA_READ_PTR),
            tx_write_ptr: self.read_reg32(TDMA_WRITE_PTR),
            rx_read_ptr: self.read_reg32(RDMA_READ_PTR),
            rx_write_ptr: self.read_reg32(RDMA_WRITE_PTR),
            umac_cmd: self.read_reg32(GENET_UMAC_CMD),
            oob_ctrl: self.read_reg32(EXT_RGMII_OOB_CTRL),
            tdma_ctrl: self.read_reg32(TDMA_REG_BASE + DMA_CTRL),
            rdma_ctrl: self.read_reg32(RDMA_REG_BASE + DMA_CTRL),
            tx_cons_len_status: tx_cons_desc.len_status,
            tx_prod_len_status: tx_prod_desc.len_status,
            tx_cons_addr_lo: tx_cons_desc.addr_lo,
            tx_cons_addr_hi: tx_cons_desc.addr_hi,
            tx_prod_addr_lo: tx_prod_desc.addr_lo,
            tx_prod_addr_hi: tx_prod_desc.addr_hi,
            rx_cons_len_status: rx_cons_desc.len_status,
            rx_prod_len_status: rx_prod_desc.len_status,
            rx_cons_addr_lo: rx_cons_desc.addr_lo,
            rx_cons_addr_hi: rx_cons_desc.addr_hi,
            rx_prod_addr_lo: rx_prod_desc.addr_lo,
            rx_prod_addr_hi: rx_prod_desc.addr_hi,
            rx_ready_len: self.rx_ready.len() as u16,
        }
    }

    fn log_dma_breadcrumb(&mut self, reason: BreadcrumbReason) {
        let snapshot = self.capture_breadcrumb_snapshot();
        let repeated = self.crumb_last_reason == Some(reason)
            && self
                .crumb_last_snapshot
                .map_or(false, |prev| prev == snapshot);
        if repeated {
            self.crumb_repeat = self.crumb_repeat.saturating_add(1);
            self.crumb_suppressed = self.crumb_suppressed.saturating_add(1);
            if !should_emit_repeated_breadcrumb(self.crumb_repeat) {
                return;
            }
        } else {
            self.crumb_repeat = 0;
        }

        self.crumb_seq = self.crumb_seq.saturating_add(1);
        let suppressed = core::mem::replace(&mut self.crumb_suppressed, 0);
        let tx_ring_len = self.tx_ring_len();
        let tx_sw_overflow = usize::from(snapshot.tx_sw_in_flight) > tx_ring_len;
        let tx_hw_overflow = usize::from(snapshot.tx_hw_in_flight) > tx_ring_len;

        warn!(
            "[bcmgenet][crumb] seq={} r={} rpt={} sup={} drops={} txblk={} rxidle={}",
            self.crumb_seq,
            reason.as_str(),
            self.crumb_repeat,
            suppressed,
            self.tx_drops,
            self.counters.tx_alloc_blocked_inflight,
            self.rx_idle_polls,
        );
        warn!(
            "[bcmgenet][crumb] sw tx={}/{} in={} ov={} hw tx={}/{} in={} ov={} sw rx={} hw rx={}/{} q={} txptr={:08x}/{:08x} rxptr={:08x}/{:08x}",
            snapshot.tx_sw_prod,
            snapshot.tx_sw_cons,
            snapshot.tx_sw_in_flight,
            u8::from(tx_sw_overflow),
            snapshot.tx_hw_prod,
            snapshot.tx_hw_cons,
            snapshot.tx_hw_in_flight,
            u8::from(tx_hw_overflow),
            snapshot.rx_sw_cons,
            snapshot.rx_hw_prod,
            snapshot.rx_hw_cons,
            snapshot.rx_ready_len,
            snapshot.tx_read_ptr,
            snapshot.tx_write_ptr,
            snapshot.rx_read_ptr,
            snapshot.rx_write_ptr,
        );
        warn!(
            "[bcmgenet][crumb] desc tx(cons=0x{:08x} prod=0x{:08x}) rx(cons=0x{:08x} prod=0x{:08x}) own(cons={},prod={}) umac={:08x} tdma={:08x} rdma={:08x} oob={:08x}",
            snapshot.tx_cons_len_status,
            snapshot.tx_prod_len_status,
            snapshot.rx_cons_len_status,
            snapshot.rx_prod_len_status,
            u8::from((snapshot.rx_cons_len_status & DMA_OWN) != 0),
            u8::from((snapshot.rx_prod_len_status & DMA_OWN) != 0),
            snapshot.umac_cmd,
            snapshot.tdma_ctrl,
            snapshot.rdma_ctrl,
            snapshot.oob_ctrl,
        );
        warn!(
            "[bcmgenet][crumb] addr tx(cons=0x{:016x} prod=0x{:016x}) rx(cons=0x{:016x} prod=0x{:016x})",
            desc_dma_addr(snapshot.tx_cons_addr_hi, snapshot.tx_cons_addr_lo),
            desc_dma_addr(snapshot.tx_prod_addr_hi, snapshot.tx_prod_addr_lo),
            desc_dma_addr(snapshot.rx_cons_addr_hi, snapshot.rx_cons_addr_lo),
            desc_dma_addr(snapshot.rx_prod_addr_hi, snapshot.rx_prod_addr_lo),
        );
        self.crumb_last_reason = Some(reason);
        self.crumb_last_snapshot = Some(snapshot);
    }

    fn poll_tx_completions(&mut self) {
        let ring_len = self.tx_ring_len();
        if ring_len == 0 {
            self.refresh_tx_counters();
            return;
        }
        let new_cons = self.read_reg32(TDMA_CONS_INDEX) as u16;
        let hw_prod = self.read_reg32(TDMA_PROD_INDEX) as u16;
        let hw_in_flight = ring_distance(hw_prod, new_cons) as usize;
        if hw_in_flight > ring_len {
            self.counters.tx_invalid_used_state =
                self.counters.tx_invalid_used_state.saturating_add(1);
            self.log_dma_breadcrumb(BreadcrumbReason::TxHwIndexInvalid);
        }
        let completed = ring_distance(new_cons, self.tx_cons_index);
        if completed == 0 {
            if self.tx_in_flight() > 0 {
                self.tx_stall_polls = self.tx_stall_polls.saturating_add(1);
                if self.tx_stall_polls >= TX_STALL_LOG_POLL_THRESHOLD && !self.tx_stall_logged {
                    self.log_dma_breadcrumb(BreadcrumbReason::TxConsStalled);
                    self.tx_stall_logged = true;
                }
            } else {
                self.tx_stall_polls = 0;
                self.tx_stall_logged = false;
            }
            self.refresh_tx_counters();
            return;
        }
        if completed as usize > ring_len {
            self.counters.tx_invalid_used_state =
                self.counters.tx_invalid_used_state.saturating_add(1);
            self.log_dma_breadcrumb(BreadcrumbReason::TxConsJump);
        } else {
            let prev_complete = self.counters.tx_complete;
            for completed_offset in 0..completed as usize {
                let slot = ring_slot(
                    self.tx_cons_index.wrapping_add(completed_offset as u16),
                    ring_len,
                );
                self.unshare_tx_slot(slot);
            }
            self.counters.tx_used_advances = self
                .counters
                .tx_used_advances
                .saturating_add(completed as u64);
            self.counters.tx_complete = self.counters.tx_complete.saturating_add(completed as u64);
            if prev_complete == 0 {
                self.log_dma_breadcrumb(BreadcrumbReason::TxFirstCompletion);
            }
        }
        self.tx_cons_index = new_cons;
        if self.tx_stall_logged {
            self.log_dma_breadcrumb(BreadcrumbReason::TxConsRecovered);
        }
        self.tx_stall_logged = false;
        self.tx_stall_polls = 0;
        self.refresh_tx_counters();
    }

    fn share_dma_for_device(
        &self,
        vaddr: usize,
        paddr: usize,
        len: usize,
        label: &'static str,
    ) -> Result<dma::PinnedDmaRange, DriverError> {
        if len == 0 {
            return Err(DriverError::QueueInit);
        }
        // Ensure payload writes are not reordered past descriptor publication.
        compiler_fence(Ordering::Release);
        dma::pin(vaddr, paddr, len, label).map_err(|err| {
            warn!(
                "[bcmgenet] DMA share failed label={label} vaddr=0x{vaddr:016x} paddr=0x{paddr:016x} len={len} err={err:?}"
            );
            DriverError::QueueInit
        })
    }

    fn unshare_dma_range(range: dma::PinnedDmaRange) {
        if let Err(err) = dma::unpin(&range) {
            warn!(
                "[bcmgenet] DMA unshare failed label={} vaddr=0x{:016x} paddr=0x{:016x} len={} err={err}",
                range.label(),
                range.vaddr(),
                range.paddr(),
                range.len(),
            );
        }
    }

    fn unshare_rx_slot(&mut self, slot: usize) {
        if let Some(share) = self.rx_dma_shares.get_mut(slot).and_then(Option::take) {
            Self::unshare_dma_range(share);
        }
    }

    fn unshare_tx_slot(&mut self, slot: usize) {
        if let Some(share) = self.tx_dma_shares.get_mut(slot).and_then(Option::take) {
            Self::unshare_dma_range(share);
        }
    }

    fn sync_dma_for_cpu(
        &self,
        vaddr: usize,
        paddr: usize,
        len: usize,
        label: &'static str,
    ) -> Result<(), DriverError> {
        if len == 0 {
            return Ok(());
        }
        compiler_fence(Ordering::SeqCst);
        if let Err(err) = dma::sync_for_cpu(vaddr, paddr, len, label) {
            warn!(
                "[bcmgenet] DMA sync failed label={label} vaddr=0x{vaddr:016x} paddr=0x{paddr:016x} len={len} err={err:?}"
            );
            return Err(DriverError::QueueInit);
        }
        dma_load_barrier();
        Ok(())
    }

    fn rearm_rx_slot(&mut self, slot: usize) {
        self.unshare_rx_slot(slot);
        let Some(frame) = self.rx_frames.get(slot) else {
            return;
        };
        let frame_ptr = frame.ptr().as_ptr() as usize;
        let frame_paddr = frame.paddr();
        let Ok(range) =
            self.share_dma_for_device(frame_ptr, frame_paddr, RX_BUF_LENGTH, "bcmgenet-rx-rearm")
        else {
            return;
        };
        let Some(share) = self.rx_dma_shares.get_mut(slot) else {
            Self::unshare_dma_range(range);
            return;
        };
        *share = Some(range);
        let frame_dma = genet_hal::dma_bus_addr(frame_paddr as u64);
        self.write_rx_desc(slot, frame_dma, rx_owned_len_status());
    }

    fn advance_rx_consumer(&mut self) {
        self.rx_cons_index = self.rx_cons_index.wrapping_add(1);
        tx_doorbell_barrier();
        self.write_reg32(RDMA_CONS_INDEX, self.rx_cons_index as u32);
    }

    fn transmit(&mut self, packet: &[u8]) -> Result<(), DriverError> {
        if packet.is_empty() {
            self.tx_drops = self.tx_drops.saturating_add(1);
            self.counters.tx_zero_len_attempt = self.counters.tx_zero_len_attempt.saturating_add(1);
            return Ok(());
        }
        if packet.len() > MAX_FRAME_LEN || packet.len() > RX_BUF_LENGTH {
            self.tx_drops = self.tx_drops.saturating_add(1);
            warn!("[bcmgenet] drop oversized tx len={}", packet.len());
            return Ok(());
        }
        if self.tx_frames.is_empty() {
            return Err(DriverError::QueueInit);
        }

        self.poll_tx_completions();
        let ring_len = self.tx_ring_len();
        let in_flight = ring_distance(self.tx_prod_index, self.tx_cons_index) as usize;
        if in_flight >= ring_len {
            self.tx_drops = self.tx_drops.saturating_add(1);
            self.counters.tx_alloc_blocked_inflight =
                self.counters.tx_alloc_blocked_inflight.saturating_add(1);
            if should_log_tx_drop(self.tx_drops) {
                self.log_dma_breadcrumb(BreadcrumbReason::TxRingFull);
            }
            self.refresh_tx_counters();
            return Ok(());
        }

        let slot = ring_slot(self.tx_prod_index, self.tx_ring_len());
        self.unshare_tx_slot(slot);
        let (frame_ptr, frame_paddr) = {
            let frame = self.tx_frames.get_mut(slot).ok_or(DriverError::QueueInit)?;
            let frame_ptr = frame.ptr().as_ptr() as usize;
            let frame_paddr = frame.paddr() as u64;
            let buf = frame.as_mut_slice();
            buf[..packet.len()].copy_from_slice(packet);
            (frame_ptr, frame_paddr)
        };
        self.submit_tx_slot(
            slot,
            frame_ptr,
            frame_paddr,
            packet.len(),
            &packet[..packet.len().min(8)],
        )
    }

    fn transmit_in_place<R, F>(&mut self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        if len == 0 || len > MAX_FRAME_LEN || len > RX_BUF_LENGTH || self.tx_frames.is_empty() {
            let mut temp = [0u8; MAX_FRAME_LEN];
            let fill = &mut temp[..len.min(MAX_FRAME_LEN)];
            let result = f(fill);
            self.tx_drops = self.tx_drops.saturating_add(1);
            if len == 0 {
                self.counters.tx_zero_len_attempt =
                    self.counters.tx_zero_len_attempt.saturating_add(1);
            } else if len > MAX_FRAME_LEN || len > RX_BUF_LENGTH {
                warn!("[bcmgenet] drop oversized tx len={}", len);
            }
            self.refresh_tx_counters();
            return result;
        }

        self.poll_tx_completions();
        let ring_len = self.tx_ring_len();
        let in_flight = ring_distance(self.tx_prod_index, self.tx_cons_index) as usize;
        if in_flight >= ring_len {
            let mut temp = [0u8; MAX_FRAME_LEN];
            let result = f(&mut temp[..len]);
            self.tx_drops = self.tx_drops.saturating_add(1);
            self.counters.tx_alloc_blocked_inflight =
                self.counters.tx_alloc_blocked_inflight.saturating_add(1);
            if should_log_tx_drop(self.tx_drops) {
                self.log_dma_breadcrumb(BreadcrumbReason::TxRingFull);
            }
            self.refresh_tx_counters();
            return result;
        }

        let slot = ring_slot(self.tx_prod_index, self.tx_ring_len());
        self.unshare_tx_slot(slot);
        let first_len = len.min(8);
        let mut first = [0u8; 8];
        let Some(frame) = self.tx_frames.get_mut(slot) else {
            let mut temp = [0u8; MAX_FRAME_LEN];
            let result = f(&mut temp[..len]);
            self.tx_drops = self.tx_drops.saturating_add(1);
            self.refresh_tx_counters();
            return result;
        };
        let frame_ptr = frame.ptr().as_ptr() as usize;
        let frame_paddr = frame.paddr() as u64;
        let result = {
            let fill = &mut frame.as_mut_slice()[..len];
            let result = f(fill);
            first[..first_len].copy_from_slice(&fill[..first_len]);
            result
        };
        if let Err(err) =
            self.submit_tx_slot(slot, frame_ptr, frame_paddr, len, &first[..first_len])
        {
            self.tx_drops = self.tx_drops.saturating_add(1);
            warn!("[bcmgenet] tx error: {err}");
            self.refresh_tx_counters();
        }
        result
    }

    fn submit_tx_slot(
        &mut self,
        slot: usize,
        frame_ptr: usize,
        frame_paddr: u64,
        packet_len: usize,
        first: &[u8],
    ) -> Result<(), DriverError> {
        let range = self.share_dma_for_device(
            frame_ptr,
            frame_paddr as usize,
            packet_len,
            "bcmgenet-tx-submit",
        )?;
        let Some(share) = self.tx_dma_shares.get_mut(slot) else {
            Self::unshare_dma_range(range);
            return Err(DriverError::QueueInit);
        };
        *share = Some(range);

        let frame_dma = genet_hal::dma_bus_addr(frame_paddr);
        self.write_tx_desc(slot, frame_dma, encode_tx_len_status(packet_len));
        tx_doorbell_barrier();
        self.tx_prod_index = self.tx_prod_index.wrapping_add(1);
        self.write_reg32(TDMA_PROD_INDEX, self.tx_prod_index as u32);

        self.counters.tx_packets = self.counters.tx_packets.saturating_add(1);
        self.counters.tx_submit = self.counters.tx_submit.saturating_add(1);
        self.refresh_tx_counters();
        debug!(
            "[bcmgenet] tx len={} slot={} prod={} cons={} first={:02x?}",
            packet_len, slot, self.tx_prod_index, self.tx_cons_index, first,
        );
        Ok(())
    }

    fn poll_rx(&mut self) -> Option<HeaplessVec<u8, MAX_FRAME_LEN>> {
        self.poll_tx_completions();
        self.drain_rx_ready();
        self.rx_ready.pop_front()
    }

    fn drain_rx_ready(&mut self) {
        if self.rx_frames.is_empty() {
            return;
        }

        let mut budget = self.rx_ring_len();
        while budget > 0 && self.rx_ready.len() < RX_READY_CAP {
            let prod = self.read_reg32(RDMA_PROD_INDEX) as u16;
            if prod == self.rx_cons_index {
                self.rx_idle_polls = self.rx_idle_polls.saturating_add(1);
                if should_log_rx_idle(self.rx_idle_polls, self.rx_idle_logged) {
                    self.log_dma_breadcrumb(BreadcrumbReason::RxProdStalled);
                    self.rx_idle_logged = true;
                }
                break;
            }
            if self.rx_idle_logged {
                self.log_dma_breadcrumb(BreadcrumbReason::RxProdRecovered);
            }
            self.rx_idle_logged = false;
            self.rx_idle_polls = 0;

            let slot = ring_slot(self.rx_cons_index, self.rx_ring_len());
            let Some(desc) = self.read_rx_desc(slot) else {
                break;
            };
            let length = decode_rx_length(desc.len_status);
            let mut maybe_frame = None;
            if length <= RX_BUF_OFFSET || length > RX_BUF_LENGTH {
                warn!(
                    "[bcmgenet] rx len invalid len={} slot={} len_status=0x{:08x} addr=0x{:08x}{:08x}",
                    length, slot, desc.len_status, desc.addr_hi, desc.addr_lo
                );
            } else {
                let mut frame = HeaplessVec::<u8, MAX_FRAME_LEN>::new();
                if let Some(source) = self.rx_frames.get(slot) {
                    let source_ptr = source.ptr().as_ptr() as usize;
                    if self
                        .sync_dma_for_cpu(
                            source_ptr,
                            source.paddr(),
                            length,
                            "bcmgenet-rx-complete",
                        )
                        .is_ok()
                    {
                        let payload_len = length.saturating_sub(RX_BUF_OFFSET).min(MAX_FRAME_LEN);
                        let payload_start = RX_BUF_OFFSET;
                        let payload_end = payload_start.saturating_add(payload_len);
                        let src_slice = source.as_slice();
                        if payload_end <= src_slice.len()
                            && frame
                                .extend_from_slice(&src_slice[payload_start..payload_end])
                                .is_ok()
                        {
                            maybe_frame = Some(frame);
                        }
                    }
                }
            }

            self.unshare_rx_slot(slot);
            self.rearm_rx_slot(slot);
            self.advance_rx_consumer();
            self.counters.rx_used_advances = self.counters.rx_used_advances.saturating_add(1);
            if let Some(frame) = maybe_frame {
                self.counters.rx_packets = self.counters.rx_packets.saturating_add(1);
                if self.counters.rx_packets == 1 {
                    self.log_dma_breadcrumb(BreadcrumbReason::RxFirstFrame);
                }
                debug!(
                    "[bcmgenet] rx len={} slot={} prod={} cons={} first={:02x?}",
                    frame.len(),
                    slot,
                    prod,
                    self.rx_cons_index,
                    &frame[..frame.len().min(8)]
                );
                let _ = self.rx_ready.push_back(frame);
            }
            budget = budget.saturating_sub(1);
        }
    }

    fn read_reg32(&self, offset: usize) -> u32 {
        match self.regs.read_u32(offset) {
            Ok(value) => value,
            Err(err) => {
                debug_assert!(false, "invalid GENET register read offset 0x{offset:x}");
                warn!("[bcmgenet] invalid HAL register read offset=0x{offset:x} err={err}");
                0
            }
        }
    }

    fn write_reg32(&self, offset: usize, value: u32) {
        if let Err(err) = self.regs.write_u32(offset, value) {
            debug_assert!(false, "invalid GENET register write offset 0x{offset:x}");
            warn!("[bcmgenet] invalid HAL register write offset=0x{offset:x} err={err}");
        }
    }
}

impl Drop for BcmGenetDevice {
    fn drop(&mut self) {
        for slot in 0..self.rx_dma_shares.len() {
            self.unshare_rx_slot(slot);
        }
        for slot in 0..self.tx_dma_shares.len() {
            self.unshare_tx_slot(slot);
        }
    }
}

const fn ring_slot(index: u16, slots: usize) -> usize {
    if slots == 0 {
        0
    } else {
        (index as usize) % slots
    }
}

const fn ring_distance(newer: u16, older: u16) -> u16 {
    newer.wrapping_sub(older)
}

const fn desc_dma_addr(addr_hi: u32, addr_lo: u32) -> u64 {
    ((addr_hi as u64) << 32) | (addr_lo as u64)
}

const fn should_log_tx_drop(tx_drops: u32) -> bool {
    tx_drops == 1 || (tx_drops % TX_DROP_LOG_INTERVAL) == 0
}

const fn should_log_rx_idle(rx_idle_polls: u32, already_logged: bool) -> bool {
    !already_logged && rx_idle_polls >= RX_IDLE_LOG_POLL_THRESHOLD
}

fn should_emit_repeated_breadcrumb(repeat_count: u32) -> bool {
    repeat_count == 1 || (repeat_count >= 64 && repeat_count.is_power_of_two())
}

const fn rx_owned_len_status() -> u32 {
    ((RX_BUF_LENGTH as u32) << DMA_BUFLENGTH_SHIFT) | DMA_OWN
}

const fn encode_tx_len_status(len: usize) -> u32 {
    ((len as u32) << DMA_BUFLENGTH_SHIFT)
        | (DMA_DEFAULT_QTAG << DMA_TX_QTAG_SHIFT)
        | DMA_TX_APPEND_CRC
        | DMA_SOP
        | DMA_EOP
}

const fn decode_rx_length(len_status: u32) -> usize {
    ((len_status >> DMA_BUFLENGTH_SHIFT) & DMA_BUFLENGTH_MASK) as usize
}

const fn ring_end_addr(ring_descs: usize) -> u32 {
    let words = ring_descs.saturating_mul(DMA_DESC_SIZE) / 4;
    words.saturating_sub(1) as u32
}

const fn ring_buffer_size(ring_descs: usize) -> u32 {
    ((ring_descs as u32) << DMA_RING_SIZE_SHIFT) | RX_BUF_LENGTH as u32
}

const fn dma_fc_thresh_value(ring_descs: usize) -> u32 {
    (DMA_FC_THRESH_LO << 16) | ((ring_descs as u32) >> 4)
}

const fn rx_desc_offset(slot: usize) -> Option<usize> {
    match slot.checked_mul(DMA_DESC_SIZE) {
        Some(offset) => GENET_RX_OFF.checked_add(offset),
        None => None,
    }
}

const fn tx_desc_offset(slot: usize) -> Option<usize> {
    match slot.checked_mul(DMA_DESC_SIZE) {
        Some(offset) => GENET_TX_OFF.checked_add(offset),
        None => None,
    }
}

const fn decode_bmcr_speed(bmcr: u16) -> u32 {
    let speed_100 = (bmcr & MII_BMCR_SPEED100) != 0;
    let speed_1000 = (bmcr & MII_BMCR_SPEED1000) != 0;
    match (speed_100, speed_1000) {
        (false, false) => UMAC_SPEED_10,
        (true, false) => UMAC_SPEED_100,
        (false, true) | (true, true) => UMAC_SPEED_1000,
    }
}

const fn speed_label(speed: u32) -> &'static str {
    match speed {
        UMAC_SPEED_10 => "10M",
        UMAC_SPEED_100 => "100M",
        UMAC_SPEED_1000 => "1000M",
        _ => "unknown",
    }
}

#[inline(always)]
fn tx_doorbell_barrier() {
    fence(Ordering::SeqCst);
    #[cfg(target_arch = "aarch64")]
    unsafe {
        // SAFETY: Barrier has no memory operands and only orders prior descriptor
        // writes/cleans before the MMIO doorbell write on this core.
        asm!("dmb oshst", options(nostack, preserves_flags));
    }
}

#[inline(always)]
fn dma_load_barrier() {
    compiler_fence(Ordering::Acquire);
    #[cfg(target_arch = "aarch64")]
    unsafe {
        // SAFETY: Barrier has no memory operands and only orders prior DMA/device
        // writes before subsequent CPU reads on this core.
        asm!("dmb oshld", options(nostack, preserves_flags));
    }
    #[cfg(not(target_arch = "aarch64"))]
    fence(Ordering::Acquire);
}

impl phy::RxToken for RxToken {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        f(self.frame.as_slice())
    }
}

impl<'a> phy::TxToken for TxToken<'a> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        self.device.transmit_in_place(len, f)
    }
}

impl Device for BcmGenetDevice {
    type RxToken<'a>
        = RxToken
    where
        Self: 'a;
    type TxToken<'a>
        = TxToken<'a>
    where
        Self: 'a;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        self.poll_rx()
            .map(|frame| (RxToken { frame }, TxToken { device: self }))
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        if self.tx_has_room() {
            Some(TxToken { device: self })
        } else {
            None
        }
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = MAX_FRAME_LEN;
        caps.medium = smoltcp::phy::Medium::Ethernet;
        caps
    }
}

impl NetDevice for BcmGenetDevice {
    type Error = DriverError;

    fn create<H>(hal: &mut H) -> Result<Self, Self::Error>
    where
        H: crate::hal::Hardware<Error = crate::hal::HalError>,
        Self: Sized,
    {
        Self::new(hal)
    }

    fn create_with_stage<H>(
        hal: &mut H,
        _config: &ConsoleNetConfig,
        _stage: crate::net::NetStage,
    ) -> Result<Self, Self::Error>
    where
        H: crate::hal::Hardware<Error = crate::hal::HalError>,
        Self: Sized,
    {
        Self::new(hal)
    }

    fn mac(&self) -> EthernetAddress {
        self.mac
    }

    fn tx_drop_count(&self) -> u32 {
        self.tx_drops
    }

    fn name() -> &'static str
    where
        Self: Sized,
    {
        "bcmgenet-v5"
    }

    fn debug_snapshot(&mut self) {
        self.poll_tx_completions();
        let cmd = self.read_reg32(GENET_UMAC_CMD);
        let tx_prod = self.read_reg32(TDMA_PROD_INDEX) as u16;
        let tx_cons = self.read_reg32(TDMA_CONS_INDEX) as u16;
        let rx_prod = self.read_reg32(RDMA_PROD_INDEX) as u16;
        let rx_cons = self.read_reg32(RDMA_CONS_INDEX) as u16;
        let tx_desc = self.read_tx_desc(ring_slot(self.tx_cons_index, self.tx_ring_len()));
        let rx_desc = self.read_rx_desc(ring_slot(self.rx_cons_index, self.rx_ring_len()));
        debug!(
            "[bcmgenet] snapshot mmio=0x{:016x} cmd=0x{:08x} tx(prod={},cons={},inflight={}) rx(prod={},cons={}) tx_desc={:?} rx_desc={:?} tx_drops={}",
            self.mmio_base,
            cmd,
            tx_prod,
            tx_cons,
            self.counters.tx_in_flight,
            rx_prod,
            rx_cons,
            tx_desc,
            rx_desc,
            self.tx_drops
        );
    }

    fn counters(&self) -> NetDeviceCounters {
        self.counters
    }

    fn buffer_bounds(&self) -> Option<Range<usize>> {
        let mut start = usize::MAX;
        let mut end = 0usize;
        for frame in self.rx_frames.iter().chain(self.tx_frames.iter()) {
            let frame_start = frame.ptr().as_ptr() as usize;
            let frame_end = frame_start.saturating_add(PAGE_SIZE);
            start = start.min(frame_start);
            end = end.max(frame_end);
        }
        if start == usize::MAX || end <= start {
            None
        } else {
            Some(start..end)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::hal::bcmgenet as genet_hal;

    use super::{
        decode_bmcr_speed, decode_rx_length, encode_tx_len_status, ring_distance, ring_slot,
        rx_owned_len_status, should_emit_repeated_breadcrumb, should_log_rx_idle,
        should_log_tx_drop, DMA_BUFLENGTH_SHIFT, DMA_DEFAULT_QTAG, DMA_EOP, DMA_OWN, DMA_SOP,
        DMA_TX_APPEND_CRC, DMA_TX_QTAG_SHIFT, HW_TOTAL_DESCS, MII_BMCR_SPEED100,
        MII_BMCR_SPEED1000, RX_BUF_LENGTH, RX_READY_CAP, RX_RING_DESCS, TX_RING_DESCS,
        UMAC_SPEED_10, UMAC_SPEED_100, UMAC_SPEED_1000,
    };

    #[test]
    fn ring_slot_wraps_descriptor_count() {
        assert_eq!(ring_slot(0, 8), 0);
        assert_eq!(ring_slot(7, 8), 7);
        assert_eq!(ring_slot(8, 8), 0);
        assert_eq!(ring_slot(15, 8), 7);
    }

    #[test]
    fn ring_slot_handles_empty_ring() {
        assert_eq!(ring_slot(42, 0), 0);
    }

    #[test]
    fn ring_distance_handles_wrap() {
        assert_eq!(ring_distance(10, 7), 3);
        assert_eq!(ring_distance(0, u16::MAX), 1);
    }

    #[test]
    fn tx_len_status_sets_required_bits() {
        let length = 512usize;
        let len_status = encode_tx_len_status(length);
        assert_eq!((len_status >> DMA_BUFLENGTH_SHIFT) as usize, length);
        assert_ne!(len_status & DMA_SOP, 0);
        assert_ne!(len_status & DMA_EOP, 0);
        assert_ne!(len_status & DMA_TX_APPEND_CRC, 0);
        assert_eq!(
            (len_status >> DMA_TX_QTAG_SHIFT) & DMA_DEFAULT_QTAG,
            DMA_DEFAULT_QTAG
        );
    }

    #[test]
    fn rx_len_status_round_trip() {
        let len_status = rx_owned_len_status();
        assert_eq!(decode_rx_length(len_status), RX_BUF_LENGTH);
        assert_ne!(len_status & DMA_OWN, 0);
    }

    #[test]
    fn pi4_genet_buffers_match_large_board_profile() {
        assert_eq!(RX_RING_DESCS, HW_TOTAL_DESCS);
        assert_eq!(TX_RING_DESCS, HW_TOTAL_DESCS);
        assert_eq!(RX_RING_DESCS, 256);
        assert_eq!(TX_RING_DESCS, 256);
        assert!(RX_READY_CAP >= 128);
        assert!(RX_RING_DESCS * RX_BUF_LENGTH >= 512 * 1024);
        assert!(TX_RING_DESCS * RX_BUF_LENGTH >= 512 * 1024);
    }

    #[test]
    fn tx_token_uses_dma_slot_staging_path() {
        let source = include_str!("bcmgenet.rs");
        let token_impl = source
            .find("impl<'a> phy::TxToken for TxToken<'a>")
            .expect("GENET TxToken impl remains present");
        let token_body = &source[token_impl..];
        assert!(token_body.contains("self.device.transmit_in_place(len, f)"));
        assert!(source.contains("fn transmit_in_place"));
        assert!(source.contains("fn submit_tx_slot"));
    }

    #[test]
    fn rx_ready_queue_uses_bounded_o1_dequeue() {
        let source = include_str!("bcmgenet.rs");
        let implementation = source
            .split("mod tests")
            .next()
            .expect("implementation section remains present");
        assert!(implementation
            .contains("rx_ready: Deque<HeaplessVec<u8, MAX_FRAME_LEN>, RX_READY_CAP>"));
        assert!(implementation.contains("self.rx_ready.pop_front()"));
        assert!(implementation.contains("self.rx_ready.push_back(frame)"));
        assert!(!implementation.contains("self.rx_ready.remove(0)"));
    }

    #[test]
    fn bmcr_speed_decode_matches_umac_encoding() {
        assert_eq!(decode_bmcr_speed(0), UMAC_SPEED_10);
        assert_eq!(decode_bmcr_speed(MII_BMCR_SPEED100), UMAC_SPEED_100);
        assert_eq!(decode_bmcr_speed(MII_BMCR_SPEED1000), UMAC_SPEED_1000);
        assert_eq!(
            decode_bmcr_speed(MII_BMCR_SPEED100 | MII_BMCR_SPEED1000),
            UMAC_SPEED_1000
        );
    }

    #[test]
    fn tx_drop_breadcrumb_is_rate_limited() {
        assert!(should_log_tx_drop(1));
        assert!(!should_log_tx_drop(2));
        assert!(should_log_tx_drop(512));
        assert!(!should_log_tx_drop(513));
    }

    #[test]
    fn rx_idle_breadcrumb_requires_threshold_and_repeat_window() {
        assert!(!should_log_rx_idle(1, false));
        assert!(!should_log_rx_idle(65_535, false));
        assert!(should_log_rx_idle(65_536, false));
        assert!(!should_log_rx_idle(70_000, true));
        assert!(!should_log_rx_idle(131_072, true));
    }

    #[test]
    fn repeated_breadcrumbs_emit_only_at_escalation_points() {
        assert!(!should_emit_repeated_breadcrumb(0));
        assert!(should_emit_repeated_breadcrumb(1));
        assert!(!should_emit_repeated_breadcrumb(2));
        assert!(!should_emit_repeated_breadcrumb(63));
        assert!(should_emit_repeated_breadcrumb(64));
        assert!(should_emit_repeated_breadcrumb(128));
        assert!(!should_emit_repeated_breadcrumb(96));
    }

    #[test]
    fn dma_phys_addresses_use_pi4_bus_alias_window() {
        if genet_hal::dma_address_policy_name() == "vc-0xc0000000" {
            assert_eq!(genet_hal::dma_bus_addr(0x0000_0000), 0xC000_0000);
            assert_eq!(genet_hal::dma_bus_addr(0x0400_0000), 0xC400_0000);
            assert_eq!(genet_hal::dma_bus_addr(0x3FFF_FFFF), 0xFFFF_FFFF);
        } else {
            assert_eq!(genet_hal::dma_bus_addr(0x0000_0000), 0x0000_0000);
            assert_eq!(genet_hal::dma_bus_addr(0x0400_0000), 0x0400_0000);
            assert_eq!(genet_hal::dma_bus_addr(0x3FFF_FFFF), 0x3FFF_FFFF);
        }
        assert_eq!(genet_hal::dma_bus_addr(0x4000_0000), 0x4000_0000);
    }
}
