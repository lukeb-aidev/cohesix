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

use crate::hal::{HalError, Hardware};
use crate::sel4::{DeviceFrame, RamFrame};

const PCI_VENDOR_ID: u16 = 0x10ec;
const PCI_DEVICE_ID: u16 = 0x8139;
const PCI_ECAM_BASE: usize = 0x3000_0000;
const PCI_ECAM_SIZE: usize = 0x1000_0000;
const PCI_MAX_BUS: usize = 1;
const PCI_MAX_DEVICE: usize = 32;

const PCI_CONFIG_VENDOR_DEVICE: usize = 0x00;
const PCI_CONFIG_COMMAND: usize = 0x04;
const PCI_CONFIG_BAR0: usize = 0x10;

const PCI_COMMAND_IO: u16 = 1 << 0;
const PCI_COMMAND_BUS_MASTER: u16 = 1 << 2;

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
    PciWindowUnavailable,
}

impl From<HalError> for DriverError {
    fn from(value: HalError) -> Self {
        Self::Hal(value)
    }
}

pub struct Rtl8139Device {
    regs: DeviceFrame,
    rx_buffer: RamFrame,
    tx_buffers: HeaplessVec<RamFrame, TX_SLOT_COUNT>,
    tx_cursor: usize,
    rx_offset: usize,
    mac: EthernetAddress,
    tx_drops: u32,
}

struct RxToken<'a> {
    device: &'a mut Rtl8139Device,
    packet: HeaplessVec<u8, MAX_FRAME_LEN>,
}

struct TxToken<'a> {
    device: &'a mut Rtl8139Device,
}

impl Rtl8139Device {
    pub fn new<H>(hal: &mut H) -> Result<Self, DriverError>
    where
        H: Hardware<Error = HalError>,
    {
        info!("[rtl8139] probing PCI ecam for RTL8139");
        let (cfg_frame, bus, device) = Self::probe_pci(hal)?;
        info!(
            "[rtl8139] pci device located at bus={} slot={} (ecam=0x{:x})",
            bus,
            device,
            cfg_frame.paddr()
        );
        let base_addr = Self::allocate_io_base(hal, &cfg_frame)?;
        info!("[rtl8139] mapped io base=0x{base_addr:08x}");
        let regs = hal.map_device(base_addr)?;
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

    fn probe_pci<H>(hal: &mut H) -> Result<(DeviceFrame, usize, usize), DriverError>
    where
        H: Hardware<Error = HalError>,
    {
        for bus in 0..PCI_MAX_BUS {
            for device in 0..PCI_MAX_DEVICE {
                let cfg_addr =
                    Self::ecam_addr(bus, device, 0).ok_or(DriverError::PciWindowUnavailable)?;
                if cfg_addr >= PCI_ECAM_BASE + PCI_ECAM_SIZE {
                    continue;
                }
                let cfg_frame = hal.map_device(cfg_addr)?;
                let vendor_device = unsafe {
                    read_volatile(
                        cfg_frame.ptr().as_ptr().add(PCI_CONFIG_VENDOR_DEVICE) as *const u32
                    )
                };
                let vendor = vendor_device as u16;
                let device_id = (vendor_device >> 16) as u16;
                if vendor == PCI_VENDOR_ID && device_id == PCI_DEVICE_ID {
                    return Ok((cfg_frame, bus, device));
                }
            }
        }
        error!("[rtl8139] pci scan failed; device not present");
        Err(DriverError::NoDevice)
    }

    fn ecam_addr(bus: usize, device: usize, function: usize) -> Option<usize> {
        let offset = (bus << 20) | (device << 15) | (function << 12);
        PCI_ECAM_BASE.checked_add(offset)
    }

    fn allocate_io_base<H>(hal: &mut H, cfg: &DeviceFrame) -> Result<usize, DriverError>
    where
        H: Hardware<Error = HalError>,
    {
        let io_base = 0x3eff_0000usize;
        unsafe {
            let cfg_ptr = cfg.ptr().as_ptr();
            let bar = io_base | 1u32 as usize;
            write_volatile(cfg_ptr.add(PCI_CONFIG_BAR0) as *mut u32, bar as u32);
            write_volatile(
                cfg_ptr.add(PCI_CONFIG_COMMAND) as *mut u16,
                PCI_COMMAND_IO | PCI_COMMAND_BUS_MASTER,
            );
        }
        hal.device_coverage(io_base, 16)
            .ok_or(DriverError::PciWindowUnavailable)?;
        Ok(io_base)
    }

    fn read_mac(regs: &DeviceFrame) -> EthernetAddress {
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

    pub fn debug_snapshot(&self) {
        let isr = unsafe { read_volatile(self.regs.ptr().as_ptr().add(RTL_REG_ISR) as *const u16) };
        let cbr = unsafe { read_volatile(self.regs.ptr().as_ptr().add(RTL_REG_CBR) as *const u16) };
        debug!(
            "[rtl8139] snapshot: isr=0x{isr:04x} cbr={cbr} rx_offset={}",
            self.rx_offset
        );
    }
}

impl<'a> phy::RxToken for RxToken<'a> {
    fn consume<R, F>(self, _timestamp: Instant, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut buffer = self.packet;
        let result = f(&mut buffer[..]);
        result
    }
}

impl<'a> phy::TxToken for TxToken<'a> {
    fn consume<R, F>(self, _timestamp: Instant, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut temp = [0u8; TX_BUFFER_LEN];
        let filled = &mut temp[..len.min(TX_BUFFER_LEN)];
        let result = f(filled);
        if let Err(err) = self.device.transmit(filled) {
            warn!("[rtl8139] tx error: {err:?}");
        }
        result
    }
}

impl Device for Rtl8139Device {
    type RxToken<'a>
        = RxToken<'a>
    where
        Self: 'a;
    type TxToken<'a>
        = TxToken<'a>
    where
        Self: 'a;

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = MAX_FRAME_LEN as u16;
        caps
    }

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let packet = self.poll_rx()?;
        Some((
            RxToken {
                device: self,
                packet,
            },
            TxToken { device: self },
        ))
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
            Self::PciWindowUnavailable => f.write_str("rtl8139 pci window unavailable"),
        }
    }
}
