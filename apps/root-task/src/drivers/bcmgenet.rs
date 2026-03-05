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

use core::fmt;
use core::ops::Range;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{compiler_fence, Ordering};

use heapless::Vec as HeaplessVec;
use log::{debug, info, warn};
use smoltcp::phy::{self, Device, DeviceCapabilities};
use smoltcp::time::Instant;
use smoltcp::wire::EthernetAddress;

#[cfg(any(
    all(feature = "kernel", target_os = "none"),
    feature = "cache-maintenance"
))]
use crate::hal::cache::{cache_clean, cache_invalidate};
use crate::hal::{HalError, Hardware};
use crate::net::{NetDevice, NetDeviceCounters, NetDriverError};
#[cfg(any(
    all(feature = "kernel", target_os = "none"),
    feature = "cache-maintenance"
))]
use crate::sel4::seL4_CapInitThreadVSpace;
use crate::sel4::{DeviceFrame, RamFrame, PAGE_BITS};

const PAGE_SIZE: usize = 1 << PAGE_BITS;
const MAX_FRAME_LEN: usize = crate::net_consts::MAX_FRAME_LEN;
const RING_FRAMES: usize = 8;
const MMIO_PAGE_COUNT: usize = 6;

// GENETv5 MMIO candidates observed across Pi4 firmware/alias mappings.
const GENET_MMIO_CANDIDATES: [usize; 3] = [0xFD58_0000, 0x7D58_0000, 0xFE58_0000];

const GENET_SYS_OFF: usize = 0x0000;
const GENET_RBUF_OFF: usize = 0x0300;
const GENET_UMAC_OFF: usize = 0x0800;
const GENET_RX_OFF: usize = 0x2000;
const GENET_TX_OFF: usize = 0x4000;

const SYS_RBUF_FLUSH_CTRL: usize = GENET_SYS_OFF + 0x08;
const RBUF_CTRL: usize = GENET_RBUF_OFF + 0x00;
const RBUF_TBUF_SIZE_CTRL: usize = GENET_RBUF_OFF + 0xb4;

const RBUF_ALIGN_2B: u32 = 1 << 1;

const GENET_UMAC_CMD: usize = 0x0808;
const GENET_UMAC_MAC0: usize = 0x080C;
const GENET_UMAC_MAC1: usize = 0x0810;
const UMAC_MAX_FRAME_LEN: usize = GENET_UMAC_OFF + 0x14;
const UMAC_TX_FLUSH: usize = GENET_UMAC_OFF + 0x334;
const UMAC_MIB_CTRL: usize = GENET_UMAC_OFF + 0x580;

const CMD_TX_EN: u32 = 1 << 0;
const CMD_RX_EN: u32 = 1 << 1;
const CMD_SW_RESET: u32 = 1 << 13;
const CMD_LCL_LOOP_EN: u32 = 1 << 15;

const MIB_RESET_RX: u32 = 1 << 0;
const MIB_RESET_RUNT: u32 = 1 << 1;
const MIB_RESET_TX: u32 = 1 << 2;

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

#[derive(Clone, Copy, Debug, Default)]
struct DmaDesc {
    len_status: u32,
    addr_lo: u32,
    addr_hi: u32,
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
    regs: HeaplessVec<DeviceFrame, MMIO_PAGE_COUNT>,
    mmio_base: usize,
    rx_frames: HeaplessVec<RamFrame, RING_FRAMES>,
    tx_frames: HeaplessVec<RamFrame, RING_FRAMES>,
    tx_prod_index: u16,
    tx_cons_index: u16,
    rx_cons_index: u16,
    mac: EthernetAddress,
    tx_drops: u32,
    counters: NetDeviceCounters,
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
        H: Hardware<Error = HalError>,
    {
        let (mmio_base, regs) = Self::map_registers(hal)?;
        let mut rx_frames = HeaplessVec::new();
        let mut tx_frames = HeaplessVec::new();
        for _ in 0..RING_FRAMES {
            rx_frames
                .push(hal.alloc_dma_frame_low()?)
                .map_err(|_| DriverError::QueueInit)?;
            tx_frames
                .push(hal.alloc_dma_frame_low()?)
                .map_err(|_| DriverError::QueueInit)?;
        }

        let mut device = Self {
            regs,
            mmio_base,
            rx_frames,
            tx_frames,
            tx_prod_index: 0,
            tx_cons_index: 0,
            rx_cons_index: 0,
            mac: EthernetAddress([0x02, 0x43, 0x4f, 0x48, 0x58, 0x01]),
            tx_drops: 0,
            counters: NetDeviceCounters::default(),
        };
        device.init_hardware();
        device.mac = device.read_or_default_mac();
        device.write_mac(device.mac);
        device.refresh_tx_counters();
        info!(
            "[bcmgenet] init complete mmio=0x{:016x} pages={} ring_frames={} mac={} tx_idx={} rx_idx={}",
            device.mmio_base,
            MMIO_PAGE_COUNT,
            RING_FRAMES,
            device.mac,
            device.tx_prod_index,
            device.rx_cons_index,
        );
        Ok(device)
    }

    fn map_registers<H>(
        hal: &mut H,
    ) -> Result<(usize, HeaplessVec<DeviceFrame, MMIO_PAGE_COUNT>), DriverError>
    where
        H: Hardware<Error = HalError>,
    {
        for candidate in GENET_MMIO_CANDIDATES {
            if !Self::candidate_covered(hal, candidate) {
                continue;
            }

            let mut regs = HeaplessVec::new();
            let mut failed = false;
            for page in 0..MMIO_PAGE_COUNT {
                let Some(offset) = page.checked_mul(PAGE_SIZE) else {
                    failed = true;
                    break;
                };
                let Some(paddr) = candidate.checked_add(offset) else {
                    failed = true;
                    break;
                };
                match hal.map_device(paddr) {
                    Ok(frame) => {
                        if regs.push(frame).is_err() {
                            failed = true;
                            break;
                        }
                    }
                    Err(err) => {
                        failed = true;
                        warn!(
                            "[bcmgenet] map_device failed mmio=0x{:016x} page={} err={}",
                            candidate, page, err
                        );
                        break;
                    }
                }
            }

            if !failed && regs.len() == MMIO_PAGE_COUNT {
                return Ok((candidate, regs));
            }

            if failed {
                warn!(
                    "[bcmgenet] candidate 0x{:016x} mapping incomplete; trying next alias",
                    candidate
                );
            } else {
                return Err(DriverError::QueueInit);
            }
        }
        Err(DriverError::NoDevice)
    }

    fn candidate_covered<H>(hal: &H, base: usize) -> bool
    where
        H: Hardware<Error = HalError>,
    {
        for page in 0..MMIO_PAGE_COUNT {
            let Some(offset) = page.checked_mul(PAGE_SIZE) else {
                return false;
            };
            let Some(paddr) = base.checked_add(offset) else {
                return false;
            };
            if hal.device_coverage(paddr, PAGE_BITS).is_none() {
                return false;
            }
        }
        true
    }

    fn init_hardware(&mut self) {
        self.disable_dma();
        self.init_umac();
        self.init_rx_ring();
        self.init_tx_ring();
        self.init_rx_descriptors();
        self.init_tx_descriptors();
        self.enable_dma();
        let cmd = self.read_reg32(GENET_UMAC_CMD);
        self.write_reg32(GENET_UMAC_CMD, cmd | CMD_TX_EN | CMD_RX_EN);
    }

    fn init_umac(&mut self) {
        let mut cmd = self.read_reg32(GENET_UMAC_CMD);
        cmd &= !(CMD_TX_EN | CMD_RX_EN);
        self.write_reg32(GENET_UMAC_CMD, cmd);

        // Keep link speed/duplex state firmware selected and only reset datapath blocks.
        self.write_reg32(GENET_UMAC_CMD, CMD_SW_RESET | CMD_LCL_LOOP_EN);
        self.write_reg32(GENET_UMAC_CMD, cmd);
        self.write_reg32(UMAC_MIB_CTRL, MIB_RESET_RX | MIB_RESET_TX | MIB_RESET_RUNT);
        self.write_reg32(UMAC_MIB_CTRL, 0);
        self.write_reg32(UMAC_MAX_FRAME_LEN, ENET_MAX_MTU_SIZE as u32);
        let rbuf_ctrl = self.read_reg32(RBUF_CTRL) | RBUF_ALIGN_2B;
        self.write_reg32(RBUF_CTRL, rbuf_ctrl);
        self.write_reg32(RBUF_TBUF_SIZE_CTRL, 1);
        self.write_reg32(SYS_RBUF_FLUSH_CTRL, 0);
        self.write_reg32(UMAC_TX_FLUSH, 0);
    }

    fn disable_dma(&mut self) {
        let tdma_ctrl = self.read_reg32(TDMA_REG_BASE + DMA_CTRL);
        self.write_reg32(TDMA_REG_BASE + DMA_CTRL, tdma_ctrl & !DMA_EN);
        let rdma_ctrl = self.read_reg32(RDMA_REG_BASE + DMA_CTRL);
        self.write_reg32(RDMA_REG_BASE + DMA_CTRL, rdma_ctrl & !DMA_EN);
        self.write_reg32(UMAC_TX_FLUSH, 1);
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
            return;
        }
        self.counters.tx_in_flight = in_flight as u64;
        self.counters.tx_free = ring_len.saturating_sub(in_flight) as u64;
    }

    fn poll_tx_completions(&mut self) {
        let new_cons = self.read_reg32(TDMA_CONS_INDEX) as u16;
        let completed = ring_distance(new_cons, self.tx_cons_index);
        if completed == 0 {
            self.refresh_tx_counters();
            return;
        }
        if completed as usize > self.tx_ring_len() {
            self.counters.tx_invalid_used_state =
                self.counters.tx_invalid_used_state.saturating_add(1);
        } else {
            self.counters.tx_used_advances = self
                .counters
                .tx_used_advances
                .saturating_add(completed as u64);
            self.counters.tx_complete = self.counters.tx_complete.saturating_add(completed as u64);
        }
        self.tx_cons_index = new_cons;
        self.refresh_tx_counters();
    }

    fn clean_cache_for_device(&self, vaddr: usize, len: usize) {
        if len == 0 {
            return;
        }
        #[cfg(any(
            all(feature = "kernel", target_os = "none"),
            feature = "cache-maintenance"
        ))]
        if let Err(err) = cache_clean(seL4_CapInitThreadVSpace, vaddr, len) {
            warn!("[bcmgenet] cache clean failed vaddr=0x{vaddr:016x} len={len} err={err}");
        }
        #[cfg(not(any(
            all(feature = "kernel", target_os = "none"),
            feature = "cache-maintenance"
        )))]
        let _ = (vaddr, len);
    }

    fn invalidate_cache_for_cpu(&self, vaddr: usize, len: usize) {
        if len == 0 {
            return;
        }
        #[cfg(any(
            all(feature = "kernel", target_os = "none"),
            feature = "cache-maintenance"
        ))]
        if let Err(err) = cache_invalidate(seL4_CapInitThreadVSpace, vaddr, len) {
            warn!("[bcmgenet] cache invalidate failed vaddr=0x{vaddr:016x} len={len} err={err}");
        }
        #[cfg(not(any(
            all(feature = "kernel", target_os = "none"),
            feature = "cache-maintenance"
        )))]
        let _ = (vaddr, len);
    }

    fn rearm_rx_slot(&mut self, slot: usize) {
        let Some(frame) = self.rx_frames.get(slot) else {
            return;
        };
        let frame_ptr = frame.ptr().as_ptr() as usize;
        self.clean_cache_for_device(frame_ptr, RX_BUF_LENGTH);
        self.write_rx_desc(slot, frame.paddr() as u64, rx_owned_len_status());
    }

    fn advance_rx_consumer(&mut self) {
        self.rx_cons_index = self.rx_cons_index.wrapping_add(1);
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
        let in_flight = ring_distance(self.tx_prod_index, self.tx_cons_index) as usize;
        if in_flight >= self.tx_ring_len() {
            self.tx_drops = self.tx_drops.saturating_add(1);
            self.counters.tx_alloc_blocked_inflight =
                self.counters.tx_alloc_blocked_inflight.saturating_add(1);
            self.refresh_tx_counters();
            return Ok(());
        }

        let slot = ring_slot(self.tx_prod_index, self.tx_ring_len());
        let frame = self.tx_frames.get_mut(slot).ok_or(DriverError::QueueInit)?;
        let frame_ptr = frame.ptr().as_ptr() as usize;
        let frame_paddr = frame.paddr() as u64;
        let buf = frame.as_mut_slice();
        buf[..packet.len()].copy_from_slice(packet);
        self.clean_cache_for_device(frame_ptr, packet.len());

        self.write_tx_desc(slot, frame_paddr, encode_tx_len_status(packet.len()));
        compiler_fence(Ordering::Release);
        self.tx_prod_index = self.tx_prod_index.wrapping_add(1);
        self.write_reg32(TDMA_PROD_INDEX, self.tx_prod_index as u32);

        self.counters.tx_packets = self.counters.tx_packets.saturating_add(1);
        self.counters.tx_submit = self.counters.tx_submit.saturating_add(1);
        self.refresh_tx_counters();
        debug!(
            "[bcmgenet] tx len={} slot={} prod={} cons={} first={:02x?}",
            packet.len(),
            slot,
            self.tx_prod_index,
            self.tx_cons_index,
            &packet[..packet.len().min(8)]
        );
        Ok(())
    }

    fn poll_rx(&mut self) -> Option<HeaplessVec<u8, MAX_FRAME_LEN>> {
        self.poll_tx_completions();
        if self.rx_frames.is_empty() {
            return None;
        }

        let prod = self.read_reg32(RDMA_PROD_INDEX) as u16;
        if prod == self.rx_cons_index {
            return None;
        }

        let slot = ring_slot(self.rx_cons_index, self.rx_ring_len());
        let desc = self.read_rx_desc(slot)?;
        let length = decode_rx_length(desc.len_status);
        if length <= RX_BUF_OFFSET || length > RX_BUF_LENGTH {
            warn!(
                "[bcmgenet] rx len invalid len={} slot={} len_status=0x{:08x} addr=0x{:08x}{:08x}",
                length, slot, desc.len_status, desc.addr_hi, desc.addr_lo
            );
            self.rearm_rx_slot(slot);
            self.advance_rx_consumer();
            self.counters.rx_used_advances = self.counters.rx_used_advances.saturating_add(1);
            return None;
        }

        let mut frame = HeaplessVec::<u8, MAX_FRAME_LEN>::new();
        {
            let source = self.rx_frames.get(slot)?;
            let source_ptr = source.ptr().as_ptr() as usize;
            self.invalidate_cache_for_cpu(source_ptr, length);
            let payload_len = length.saturating_sub(RX_BUF_OFFSET).min(MAX_FRAME_LEN);
            let payload_start = RX_BUF_OFFSET;
            let payload_end = payload_start.saturating_add(payload_len);
            let src_slice = source.as_slice();
            if payload_end > src_slice.len() {
                self.rearm_rx_slot(slot);
                self.advance_rx_consumer();
                self.counters.rx_used_advances = self.counters.rx_used_advances.saturating_add(1);
                return None;
            }
            if frame
                .extend_from_slice(&src_slice[payload_start..payload_end])
                .is_err()
            {
                self.rearm_rx_slot(slot);
                self.advance_rx_consumer();
                self.counters.rx_used_advances = self.counters.rx_used_advances.saturating_add(1);
                return None;
            }
        }

        self.rearm_rx_slot(slot);
        self.advance_rx_consumer();
        self.counters.rx_packets = self.counters.rx_packets.saturating_add(1);
        self.counters.rx_used_advances = self.counters.rx_used_advances.saturating_add(1);
        debug!(
            "[bcmgenet] rx len={} slot={} prod={} cons={} first={:02x?}",
            frame.len(),
            slot,
            prod,
            self.rx_cons_index,
            &frame[..frame.len().min(8)]
        );
        Some(frame)
    }

    fn reg_ptr(&self, offset: usize) -> *mut u32 {
        let page = offset / PAGE_SIZE;
        let page_offset = offset % PAGE_SIZE;
        debug_assert!(page_offset + core::mem::size_of::<u32>() <= PAGE_SIZE);
        let frame = self
            .regs
            .get(page)
            .expect("GENET MMIO page missing for register access");
        unsafe { frame.ptr().as_ptr().add(page_offset).cast::<u32>() }
    }

    fn read_reg32(&self, offset: usize) -> u32 {
        unsafe { read_volatile(self.reg_ptr(offset).cast_const()) }
    }

    fn write_reg32(&self, offset: usize, value: u32) {
        unsafe { write_volatile(self.reg_ptr(offset), value) };
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
        let mut temp = [0u8; MAX_FRAME_LEN];
        let fill = &mut temp[..len.min(MAX_FRAME_LEN)];
        let result = f(fill);
        if let Err(err) = self.device.transmit(fill) {
            warn!("[bcmgenet] tx error: {err}");
        }
        result
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
        self.poll_tx_completions();
        Some(TxToken { device: self })
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
        let start = self.rx_frames.first()?.ptr().as_ptr() as usize;
        Some(start..start.saturating_add(PAGE_SIZE))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        decode_rx_length, encode_tx_len_status, ring_distance, ring_slot, rx_owned_len_status,
        DMA_BUFLENGTH_SHIFT, DMA_DEFAULT_QTAG, DMA_EOP, DMA_OWN, DMA_SOP, DMA_TX_APPEND_CRC,
        DMA_TX_QTAG_SHIFT, RING_FRAMES, RX_BUF_LENGTH,
    };

    #[test]
    fn ring_slot_wraps_descriptor_count() {
        assert_eq!(ring_slot(0, RING_FRAMES), 0);
        assert_eq!(ring_slot(7, RING_FRAMES), 7);
        assert_eq!(ring_slot(8, RING_FRAMES), 0);
        assert_eq!(ring_slot(15, RING_FRAMES), 7);
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
}
