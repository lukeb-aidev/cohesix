// Author: Lukas Bower
// Purpose: Vendored usb-oxide source with Cohesix-specific timeout hardening for Pi4 local-seat initialization.
// Copyright 2026 Lukas Bower
use crate::{
    Dma, Result, UsbError, reg,
    ring::{EventRing, PhysMem, Ring, Trb, completion, trb_type},
};

use alloc::{boxed::Box, sync::Arc};
use core::hint::spin_loop;
use spin::Mutex;

const MMIO_INIT_SIZE: usize = 0x1000;
const CMD_RING_SIZE: usize = 256;
const EVENT_RING_SIZE: usize = 256;
const STOP_WAIT_SPINS: usize = 10_000_000;
const RESET_WAIT_SPINS: usize = 10_000_000;
const READY_WAIT_SPINS: usize = 10_000_000;
const COMMAND_WAIT_SPINS: usize = 20_000_000;
const PORT_RESET_WAIT_SPINS: usize = 10_000_000;
const DROP_HALT_WAIT_SPINS: usize = 1_000_000;

/// xHCI Controller
pub struct XhciCtrl<H: Dma> {
    mmio: usize,
    mmio_size: usize,
    cap_length: u8,
    op_base: usize,
    rt_base: usize,
    db_offset: u32,
    max_slots: u8,
    max_ports: u8,
    dcbaa: PhysMem<H>,
    scratchpad: Option<PhysMem<H>>,
    cmd_ring: Mutex<Box<Ring<H>>>,
    event_ring: Mutex<Box<EventRing<H>>>,
    host: Arc<H>,
}

impl<H: Dma> XhciCtrl<H> {
    /// Create and initialize a new xHCI controller
    pub fn new(mmio_phys: usize, host: H) -> Result<Self> {
        let host = Arc::new(host);

        // Initial map to read capability registers
        let init_mmio = unsafe {
            host.map_mmio(mmio_phys, MMIO_INIT_SIZE)
        }.ok_or(UsbError::MapFail)?;

        let cap_length = unsafe { (init_mmio as *const u8).read_volatile() };
        let hcs1: u32 = unsafe { ((init_mmio + reg::HCSPARAMS1) as *const u32).read_volatile() };
        let hcs2: u32 = unsafe { ((init_mmio + reg::HCSPARAMS2) as *const u32).read_volatile() };
        let db_offset: u32 = unsafe { ((init_mmio + reg::DBOFF) as *const u32).read_volatile() };
        let rts_offset: u32 = unsafe { ((init_mmio + reg::RTSOFF) as *const u32).read_volatile() };

        let max_slots = (hcs1 & 0xff) as u8;
        let max_ports = ((hcs1 >> 24) & 0xff) as u8;
        let max_scratchpad = ((hcs2 >> 27) & 0x1f) | (((hcs2 >> 21) & 0x1f) << 5);

        // Calculate total MMIO size needed
        let mmio_size = (rts_offset as usize + 0x20 + 0x20)
            .max(db_offset as usize + (max_slots as usize + 1) * 4)
            .max(0x10000);

        unsafe {
            host.unmap_mmio(init_mmio, MMIO_INIT_SIZE);
        }

        // Remap with full size
        let mmio = unsafe {
            host.map_mmio(mmio_phys, mmio_size)
        }.ok_or(UsbError::MapFail)?;

        let op_base = mmio + cap_length as usize;
        let rt_base = mmio + rts_offset as usize;

        // Allocate DCBAA (Device Context Base Address Array)
        // xHCI spec requires 64-byte alignment for DCBAA
        let dcbaa = PhysMem::alloc(&*host, (max_slots as usize + 1) * 8, 64)?;

        // Allocate scratchpad if needed
        let scratchpad = if max_scratchpad > 0 {
            // xHCI spec requires 64-byte alignment for scratchpad array
            let sp_array = PhysMem::alloc(&*host, max_scratchpad as usize * 8, 64)?;
            // Scratchpad buffers must be page-aligned
            let sp_bufs = PhysMem::alloc(
                &*host,
                max_scratchpad as usize * host.page_size(),
                host.page_size(),
            )?;

            // Fill scratchpad array with buffer addresses
            let array_ptr = sp_array.as_ptr::<u64>();
            for i in 0..max_scratchpad as usize {
                let buf_phys = sp_bufs.phys(&*host) + (i * host.page_size()) as u64;
                unsafe {
                    array_ptr.add(i).write_volatile(buf_phys);
                }
            }

            // Point DCBAA[0] to scratchpad array
            unsafe {
                dcbaa.as_ptr::<u64>().write_volatile(sp_array.phys(&*host));
            }

            // Keep sp_bufs alive, sp_array is referenced via DCBAA[0]
            Some(sp_bufs)
        } else {
            None
        };

        // Allocate rings on heap to reduce stack usage
        let cmd_ring = Box::new(Ring::new(&*host, CMD_RING_SIZE)?);
        let event_ring = Box::new(EventRing::new(&*host, EVENT_RING_SIZE)?);

        let mut ctrl = Self {
            mmio,
            mmio_size,
            cap_length,
            op_base,
            rt_base,
            db_offset,
            max_slots,
            max_ports,
            dcbaa,
            scratchpad,
            cmd_ring: Mutex::new(cmd_ring),
            event_ring: Mutex::new(event_ring),
            host,
        };

        ctrl.init()?;
        Ok(ctrl)
    }

    fn init(&mut self) -> Result<()> {
        // Stop controller if running
        let usbcmd = self.read_op::<u32>(reg::USBCMD);
        if (usbcmd & reg::USBCMD_RUN) != 0 {
            self.write_op(reg::USBCMD, usbcmd & !reg::USBCMD_RUN);
            let mut waited = 0usize;
            while (self.read_op::<u32>(reg::USBSTS) & reg::USBSTS_HCH) == 0 {
                waited = waited.saturating_add(1);
                if waited >= STOP_WAIT_SPINS {
                    return Err(UsbError::Timeout);
                }
                spin_loop();
            }
        }

        // Reset controller
        self.write_op(reg::USBCMD, reg::USBCMD_HCRST);
        let mut waited = 0usize;
        while (self.read_op::<u32>(reg::USBCMD) & reg::USBCMD_HCRST) != 0 {
            waited = waited.saturating_add(1);
            if waited >= RESET_WAIT_SPINS {
                return Err(UsbError::Timeout);
            }
            spin_loop();
        }
        waited = 0;
        while (self.read_op::<u32>(reg::USBSTS) & reg::USBSTS_CNR) != 0 {
            waited = waited.saturating_add(1);
            if waited >= RESET_WAIT_SPINS {
                return Err(UsbError::Timeout);
            }
            spin_loop();
        }

        // Configure controller
        self.write_op(reg::CONFIG, self.max_slots as u32);
        self.write_op(reg::DCBAAP, self.dcbaa.phys(&*self.host));

        // Setup command ring
        let cmd_ring = self.cmd_ring.lock();
        let crcr = cmd_ring.phys(&*self.host) | 1; // RCS = 1
        self.write_op(reg::CRCR, crcr);
        drop(cmd_ring);

        // Setup event ring
        let event_ring = self.event_ring.lock();
        let int_base = reg::interrupter_base(self.rt_base as u32 - self.mmio as u32, 0);

        self.write_reg(int_base + reg::ERSTSZ, 1u32);
        self.write_reg(int_base + reg::ERSTBA, event_ring.erst_phys(&*self.host));
        self.write_reg(int_base + reg::ERDP, event_ring.ring_phys(&*self.host));
        drop(event_ring);

        // Enable interrupts and start controller
        self.write_op(reg::USBCMD, reg::USBCMD_RUN | reg::USBCMD_INTE);

        // Wait for controller to be ready
        waited = 0;
        while (self.read_op::<u32>(reg::USBSTS) & reg::USBSTS_HCH) != 0 {
            waited = waited.saturating_add(1);
            if waited >= READY_WAIT_SPINS {
                return Err(UsbError::Timeout);
            }
            spin_loop();
        }

        Ok(())
    }

    fn read_reg<T: Copy>(&self, offset: usize) -> T {
        unsafe { ((self.mmio + offset) as *const T).read_volatile() }
    }

    fn write_reg<T: Copy>(&self, offset: usize, val: T) {
        unsafe {
            ((self.mmio + offset) as *mut T).write_volatile(val);
        }
    }

    fn read_op<T: Copy>(&self, offset: usize) -> T {
        self.read_reg(self.op_base - self.mmio + offset)
    }

    fn write_op<T: Copy>(&self, offset: usize, val: T) {
        self.write_reg(self.op_base - self.mmio + offset, val)
    }

    /// Ring the command doorbell
    fn ring_cmd_doorbell(&self) {
        let db = reg::doorbell(self.db_offset, 0);
        self.write_reg(db, 0u32);
    }

    /// Ring device doorbell
    pub fn ring_doorbell(&self, slot: u8, target: u8) {
        let db = reg::doorbell(self.db_offset, slot);
        self.write_reg(db, target as u32);
    }

    /// Update event ring dequeue pointer
    fn update_erdp(&self) {
        let event_ring = self.event_ring.lock();
        let int_base = reg::interrupter_base(self.rt_base as u32 - self.mmio as u32, 0);
        self.write_reg(
            int_base + reg::ERDP,
            event_ring.dequeue_ptr(&*self.host) | 0x8,
        );
    }

    /// Wait for command completion
    pub fn wait_command(&self) -> Result<Trb> {
        let mut waited = 0usize;
        loop {
            let trb = {
                let mut event_ring = self.event_ring.lock();
                event_ring.try_dequeue()
            };

            if let Some(trb) = trb {
                self.update_erdp();

                if trb.trb_type() == trb_type::COMMAND_COMPLETION as u8 {
                    let code = trb.completion_code();
                    if code != completion::SUCCESS {
                        return Err(UsbError::CmdFail(code));
                    }
                    return Ok(trb);
                }
            }

            waited = waited.saturating_add(1);
            if waited >= COMMAND_WAIT_SPINS {
                return Err(UsbError::Timeout);
            }
            spin_loop();
        }
    }

    /// Poll for transfer events (non-blocking)
    pub fn poll_event(&self) -> Option<Trb> {
        let mut event_ring = self.event_ring.lock();
        let trb = event_ring.try_dequeue();
        drop(event_ring);
        if trb.is_some() {
            self.update_erdp();
        }
        trb
    }

    /// Submit a command TRB
    pub fn submit_command(&self, trb: Trb) -> Result<Trb> {
        let mut cmd_ring = self.cmd_ring.lock();
        cmd_ring.enqueue(&*self.host, trb);
        drop(cmd_ring);
        self.ring_cmd_doorbell();
        self.wait_command()
    }

    /// Enable a device slot
    pub fn enable_slot(&self) -> Result<u8> {
        let trb = Trb {
            param: 0,
            status: 0,
            control: trb_type::ENABLE_SLOT << 10,
        };
        let evt = self.submit_command(trb)?;
        Ok(evt.slot_id())
    }

    /// Disable a device slot
    pub fn disable_slot(&self, slot_id: u8) -> Result<()> {
        let trb = Trb {
            param: 0,
            status: 0,
            control: (trb_type::DISABLE_SLOT << 10) | ((slot_id as u32) << 24),
        };
        self.submit_command(trb)?;
        Ok(())
    }

    /// Read port status
    pub fn port_status(&self, port: u8) -> u32 {
        let offset = reg::port_reg_base(self.cap_length, port);
        self.read_reg(offset)
    }

    /// Write port status (for clearing change bits, reset, etc.)
    pub fn write_port_status(&self, port: u8, val: u32) {
        let offset = reg::port_reg_base(self.cap_length, port);
        self.write_reg(offset, val);
    }

    /// Reset a port
    pub fn reset_port(&self, port: u8) -> Result<()> {
        let offset = reg::port_reg_base(self.cap_length, port);
        let portsc: u32 = self.read_reg(offset);

        // Set port reset, preserve PP, clear change bits
        let val = (portsc & reg::PORTSC_PP) | reg::PORTSC_PR;
        self.write_reg(offset, val);

        // Wait for reset to complete
        let mut waited = 0usize;
        loop {
            let portsc: u32 = self.read_reg(offset);
            if (portsc & reg::PORTSC_PR) == 0 {
                break;
            }
            waited = waited.saturating_add(1);
            if waited >= PORT_RESET_WAIT_SPINS {
                return Err(UsbError::Timeout);
            }
            spin_loop();
        }

        // Clear Port Reset Change
        let portsc: u32 = self.read_reg(offset);
        self.write_reg(offset, portsc | reg::PORTSC_PRC);

        Ok(())
    }

    /// Get port speed (after device is connected and port is enabled)
    pub fn port_speed(&self, port: u8) -> u8 {
        let portsc = self.port_status(port);
        ((portsc >> 10) & 0xf) as u8
    }

    /// Check if device is connected on port
    pub fn port_connected(&self, port: u8) -> bool {
        (self.port_status(port) & reg::PORTSC_CCS) != 0
    }

    /// Set device context in DCBAA
    pub fn set_device_context(&self, slot: u8, phys: u64) {
        unsafe {
            self.dcbaa
                .as_ptr::<u64>()
                .add(slot as usize)
                .write_volatile(phys);
        }
    }

    /// Get host reference
    pub fn host(&self) -> &H {
        &self.host
    }

    /// Get max slots
    pub fn max_slots(&self) -> u8 {
        self.max_slots
    }

    /// Get max ports
    pub fn max_ports(&self) -> u8 {
        self.max_ports
    }
}

impl<H: Dma> Drop for XhciCtrl<H> {
    fn drop(&mut self) {
        // Stop controller
        let usbcmd = self.read_op::<u32>(reg::USBCMD);
        self.write_op(reg::USBCMD, usbcmd & !reg::USBCMD_RUN);

        // Wait for halt
        let mut waited = 0usize;
        while (self.read_op::<u32>(reg::USBSTS) & reg::USBSTS_HCH) == 0 {
            waited = waited.saturating_add(1);
            if waited >= DROP_HALT_WAIT_SPINS {
                break;
            }
            spin_loop();
        }

        // Unmap MMIO
        unsafe {
            self.host.unmap_mmio(self.mmio, self.mmio_size);
        }
    }
}
