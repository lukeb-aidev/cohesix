// Author: Lukas Bower

//! Minimal RTL8139 driver used for the dev-virt TCP console path.
//!
//! This driver intentionally targets the QEMU `virt` board with a single
//! `rtl8139` PCI device attached. It implements the subset of RTL8139
//! functionality required to send and receive Ethernet frames via smoltcp
//! without relying on virtio.
#![allow(unsafe_code)]

use core::ptr::{read_volatile, write_bytes, write_volatile};

use heapless::Vec as HeaplessVec;
use log::{debug, error, info, warn};
use smoltcp::phy::{self, Device, DeviceCapabilities};
use smoltcp::time::Instant;
use smoltcp::wire::EthernetAddress;

use crate::hal::pci::{PciBarKind, PciDeviceInfo};
use crate::hal::{HalError, Hardware, MapPerms, MappedRegion, PciCommandFlags};
use crate::net::{NetDevice, NetDeviceCounters, NetDriverError};
use crate::sel4::RamFrame;

const RTL8139_VENDOR_ID: u16 = 0x10ec;
const RTL8139_DEVICE_ID: u16 = 0x8139;

const RTL_REG_IDR0: usize = 0x00;
const RTL_REG_TSD0: usize = 0x10;
const RTL_REG_TSAD0: usize = 0x20;
const RTL_REG_RBSTART: usize = 0x30;
const RTL_REG_CMD: usize = 0x37;
const RTL_REG_CAPR: usize = 0x38;
const RTL_REG_CBR: usize = 0x3a;
const RTL_REG_IMR: usize = 0x3c;
const RTL_REG_ISR: usize = 0x3e;
const RTL_REG_RCR: usize = 0x44;
const RTL_REG_TCR: usize = 0x40;
const RTL_REG_CONFIG1: usize = 0x52;

const RTL_CMD_RESET: u8 = 0x10;
const RTL_CMD_RX_ENABLE: u8 = 0x08;
const RTL_CMD_TX_ENABLE: u8 = 0x04;

const RTL_ISR_ROK: u16 = 1 << 0;
const RTL_ISR_RER: u16 = 1 << 1;
const RTL_ISR_TOK: u16 = 1 << 2;
const RTL_ISR_TER: u16 = 1 << 3;

const RTL_RCR_ACCEPT_BROADCAST: u32 = 1 << 3;
const RTL_RCR_ACCEPT_PHYS: u32 = 1 << 0;
const RTL_RCR_ACCEPT_ALL_MULTICAST: u32 = 1 << 2;
const RTL_RCR_WRAP: u32 = 1 << 7;
const RTL_RCR_RBLEN_32K: u32 = 0b11 << 11;

const RX_BUFFER_LEN: usize = 32 * 1024;
const TX_SLOT_COUNT: usize = 4;
const TX_BUFFER_LEN: usize = 2048;
const MAX_FRAME_LEN: usize = 1518;

#[derive(Debug)]
pub enum DriverError {
    NoDevice,
    Hal(HalError),
}

impl From<HalError> for DriverError {
    fn from(value: HalError) -> Self {
        Self::Hal(value)
    }
}

pub struct Rtl8139Device {
    regs: MappedRegion,
    rx_buffer: RamFrame,
    tx_buffers: HeaplessVec<RamFrame, TX_SLOT_COUNT>,
    tx_cursor: usize,
    rx_offset: usize,
    mac: EthernetAddress,
    tx_drops: u32,
    rx_packets: u64,
    tx_packets: u64,
}

pub struct RxToken {
    packet: HeaplessVec<u8, MAX_FRAME_LEN>,
}

pub struct TxToken<'a> {
    device: &'a mut Rtl8139Device,
}

impl Rtl8139Device {
    pub fn new<H>(hal: &mut H) -> Result<Self, DriverError>
    where
        H: Hardware<Error = HalError>,
    {
        info!("[rtl8139] probing HAL PCI topology for RTL8139");
        let device_info = Self::locate_pci_device(hal)?;
        let bar0 = device_info
            .bars
            .get(0)
            .and_then(|bar| *bar)
            .ok_or(HalError::PciBarUnavailable)?;
        if bar0.kind == PciBarKind::IoPort {
            return Err(DriverError::from(HalError::Unsupported(
                "io-port BARs unsupported on this platform",
            )));
        }

        hal.configure_pci_device(
            device_info.addr,
            PciCommandFlags::MEMORY_SPACE | PciCommandFlags::BUS_MASTER,
        )?;
        let regs = hal.map_pci_bar(device_info.addr, bar0.index, MapPerms::RW)?;
        info!(
            "[rtl8139] pci device located at {:02x}:{:02x}.{} (bar0=0x{:x} size=0x{:x})",
            device_info.addr.bus,
            device_info.addr.device,
            device_info.addr.function,
            bar0.base,
            bar0.size
        );
        let mac = Self::read_mac(&regs);
        info!("[rtl8139] mac address: {mac}");

        let rx_buffer = hal.alloc_dma_frame()?;
        let mut tx_buffers = HeaplessVec::new();
        for _ in 0..TX_SLOT_COUNT {
            let frame = hal.alloc_dma_frame()?;
            tx_buffers.push(frame).map_err(|_| DriverError::NoDevice)?;
        }

        let mut device = Self {
            regs,
            rx_buffer,
            tx_buffers,
            tx_cursor: 0,
            rx_offset: 0,
            mac,
            tx_drops: 0,
            rx_packets: 0,
            tx_packets: 0,
        };

        device.reset();
        device.configure_rx();
        device.configure_tx();
        info!(
            "[rtl8139] init complete; rx=32KiB tx_slots={} mac={}",
            TX_SLOT_COUNT, device.mac
        );
        Ok(device)
    }

    fn locate_pci_device<H>(hal: &mut H) -> Result<PciDeviceInfo, DriverError>
    where
        H: Hardware<Error = HalError>,
    {
        let topology = hal.pci_topology().ok_or(HalError::NoPci)?;
        topology
            .find_by_vendor_device(RTL8139_VENDOR_ID, RTL8139_DEVICE_ID)
            .copied()
            .ok_or_else(|| {
                error!("[rtl8139] pci scan failed; device not present");
                DriverError::NoDevice
            })
    }

    fn read_mac(regs: &MappedRegion) -> EthernetAddress {
        let ptr = regs.ptr().as_ptr();
        let mut bytes = [0u8; 6];
        for i in 0..6 {
            bytes[i] = unsafe { read_volatile(ptr.add(RTL_REG_IDR0 + i)) };
        }
        EthernetAddress(bytes)
    }

    fn reset(&mut self) {
        unsafe {
            write_volatile(self.regs.ptr().as_ptr().add(RTL_REG_CMD), RTL_CMD_RESET);
        }
        for _ in 0..1000 {
            let val = unsafe { read_volatile(self.regs.ptr().as_ptr().add(RTL_REG_CMD)) };
            if val & RTL_CMD_RESET == 0 {
                break;
            }
        }
        unsafe {
            write_volatile(self.regs.ptr().as_ptr().add(RTL_REG_CONFIG1), 0);
        }
    }

    fn configure_rx(&mut self) {
        unsafe {
            write_volatile(
                self.regs.ptr().as_ptr().add(RTL_REG_RBSTART) as *mut u32,
                self.rx_buffer.paddr() as u32,
            );
            write_volatile(
                self.regs.ptr().as_ptr().add(RTL_REG_RCR) as *mut u32,
                RTL_RCR_RBLEN_32K
                    | RTL_RCR_ACCEPT_BROADCAST
                    | RTL_RCR_ACCEPT_PHYS
                    | RTL_RCR_ACCEPT_ALL_MULTICAST
                    | RTL_RCR_WRAP,
            );
            write_volatile(
                self.regs.ptr().as_ptr().add(RTL_REG_IMR) as *mut u16,
                RTL_ISR_ROK | RTL_ISR_RER,
            );
            write_volatile(
                self.regs.ptr().as_ptr().add(RTL_REG_CMD),
                RTL_CMD_RX_ENABLE | RTL_CMD_TX_ENABLE,
            );
        }
        self.rx_offset = 0;
    }

    fn configure_tx(&mut self) {
        for (idx, buffer) in self.tx_buffers.iter().enumerate() {
            unsafe {
                write_volatile(
                    self.regs.ptr().as_ptr().add(RTL_REG_TSAD0 + idx * 4) as *mut u32,
                    buffer.paddr() as u32,
                );
            }
        }
    }

    fn poll_rx(&mut self) -> Option<HeaplessVec<u8, MAX_FRAME_LEN>> {
        let isr = unsafe { read_volatile(self.regs.ptr().as_ptr().add(RTL_REG_ISR) as *const u16) };
        if isr & (RTL_ISR_ROK | RTL_ISR_RER) == 0 {
            return None;
        }
        unsafe {
            write_volatile(self.regs.ptr().as_ptr().add(RTL_REG_ISR) as *mut u16, isr);
        }
        let cbr = unsafe { read_volatile(self.regs.ptr().as_ptr().add(RTL_REG_CBR) as *const u16) }
            as usize;
        if cbr == self.rx_offset {
            return None;
        }
        let buf_ptr = self.rx_buffer.ptr().as_ptr();
        let offset = self.rx_offset % RX_BUFFER_LEN;
        let status = unsafe { read_volatile(buf_ptr.add(offset) as *const u16) } as usize;
        let len = unsafe { read_volatile(buf_ptr.add(offset + 2) as *const u16) } as usize;
        if len == 0 || len > MAX_FRAME_LEN {
            warn!("[rtl8139] rx frame len out of range: {len}");
            self.rx_offset = (self.rx_offset + 4 + len + 3) & !3;
            unsafe {
                write_volatile(
                    self.regs.ptr().as_ptr().add(RTL_REG_CAPR) as *mut u16,
                    (self.rx_offset as u16).wrapping_sub(16),
                );
            }
            return None;
        }
        let mut packet = HeaplessVec::<u8, MAX_FRAME_LEN>::new();
        for i in 0..len {
            let byte = unsafe { read_volatile(buf_ptr.add(offset + 4 + i)) };
            packet.push(byte).ok();
        }
        let ok = status & RTL_ISR_ROK as usize != 0;
        if !ok {
            warn!("[rtl8139] rx status error=0x{status:04x}");
        } else {
            debug!(
                "[rtl8139] RX frame len={} first={:02x?}",
                len,
                &packet[..packet.len().min(8)]
            );
        }
        self.rx_offset = (self.rx_offset + 4 + len + 3) & !3;
        unsafe {
            write_volatile(
                self.regs.ptr().as_ptr().add(RTL_REG_CAPR) as *mut u16,
                (self.rx_offset as u16).wrapping_sub(16),
            );
        }
        Some(packet)
    }

    fn transmit(&mut self, packet: &[u8]) -> Result<(), DriverError> {
        if packet.len() > TX_BUFFER_LEN {
            self.tx_drops = self.tx_drops.saturating_add(1);
            warn!("[rtl8139] drop oversized tx len={}", packet.len());
            return Ok(());
        }
        let slot = self.tx_cursor % TX_SLOT_COUNT;
        let buffer = &self.tx_buffers[slot];
        unsafe {
            let dst = buffer.ptr().as_ptr();
            write_bytes(dst, 0, TX_BUFFER_LEN);
            core::ptr::copy_nonoverlapping(packet.as_ptr(), dst, packet.len());
            write_volatile(
                self.regs.ptr().as_ptr().add(RTL_REG_TSD0 + slot * 4) as *mut u32,
                packet.len() as u32,
            );
        }
        self.tx_cursor = self.tx_cursor.wrapping_add(1);
        debug!(
            "[rtl8139] TX len={} slot={} first={:02x?}",
            packet.len(),
            slot,
            &packet[..packet.len().min(8)]
        );
        Ok(())
    }

    pub fn mac(&self) -> EthernetAddress {
        self.mac
    }

    pub fn tx_drop_count(&self) -> u32 {
        self.tx_drops
    }

    pub fn debug_snapshot(&self) {
        let isr = unsafe { read_volatile(self.regs.ptr().as_ptr().add(RTL_REG_ISR) as *const u16) };
        let cbr = unsafe { read_volatile(self.regs.ptr().as_ptr().add(RTL_REG_CBR) as *const u16) };
        debug!(
            "[rtl8139] snapshot: isr=0x{isr:04x} cbr={cbr} rx_offset={}",
            self.rx_offset
        );
    }
}

impl phy::RxToken for RxToken {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        let buffer = self.packet;
        let result = f(&buffer[..]);
        result
    }
}

impl<'a> phy::TxToken for TxToken<'a> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut temp = [0u8; TX_BUFFER_LEN];
        let filled = &mut temp[..len.min(TX_BUFFER_LEN)];
        let result = f(filled);
        if let Err(err) = self.device.transmit(filled) {
            warn!("[rtl8139] tx error: {err:?}");
        }
        self.device.tx_packets = self.device.tx_packets.saturating_add(1);
        result
    }
}

impl Device for Rtl8139Device {
    type RxToken<'a> = RxToken;
    type TxToken<'a>
        = TxToken<'a>
    where
        Self: 'a;

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = MAX_FRAME_LEN;
        caps
    }

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let packet = self.poll_rx()?;
        self.rx_packets = self.rx_packets.saturating_add(1);
        Some((RxToken { packet }, TxToken { device: self }))
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        Some(TxToken { device: self })
    }
}

impl core::fmt::Display for DriverError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoDevice => f.write_str("rtl8139 device not found"),
            Self::Hal(err) => write!(f, "{err}"),
        }
    }
}

impl NetDriverError for DriverError {
    fn is_absent(&self) -> bool {
        matches!(
            self,
            Self::NoDevice
                | Self::Hal(
                    HalError::NoPci | HalError::InvalidPciAddress | HalError::PciBarUnavailable
                )
        )
    }
}

impl NetDevice for Rtl8139Device {
    type Error = DriverError;

    fn create<H>(hal: &mut H) -> Result<Self, Self::Error>
    where
        H: Hardware<Error = HalError>,
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

    fn counters(&self) -> NetDeviceCounters {
        NetDeviceCounters {
            rx_packets: self.rx_packets,
            tx_packets: self.tx_packets,
            rx_used_advances: self.rx_packets,
            tx_used_advances: self.tx_packets,
            tx_submit: 0,
            tx_complete: 0,
            tx_free: 0,
            tx_in_flight: 0,
            tx_double_submit: 0,
            tx_zero_len_attempt: 0,
            tx_dup_publish_blocked: 0,
            tx_dup_used_ignored: 0,
            tx_invalid_used_state: 0,
            tx_alloc_blocked_inflight: 0,
        }
    }

    fn name() -> &'static str
    where
        Self: Sized,
    {
        "rtl8139"
    }

    fn debug_snapshot(&mut self) {
        Rtl8139Device::debug_snapshot(self);
    }
}
