// Author: Lukas Bower
// Purpose: Vendored usb-oxide source with Cohesix-specific timeout hardening for Pi4 local-seat initialization.
// Copyright 2026 Lukas Bower
use crate::{
    Dma, Result, UsbError, reg,
    ring::{EventRing, PhysMem, Ring, Trb, completion, trb_type},
};

use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::{
    hint::spin_loop,
    sync::atomic::{Ordering, compiler_fence},
};
use spin::Mutex;

const MMIO_INIT_SIZE: usize = 0x1000;
const MMIO_MAX_SIZE: usize = 0x20_0000;
const CMD_RING_SIZE: usize = 256;
const EVENT_RING_SIZE: usize = 256;
const STOP_WAIT_SPINS: usize = 10_000_000;
const RESET_WAIT_SPINS: usize = 10_000_000;
const READY_WAIT_SPINS: usize = 10_000_000;
const COMMAND_WAIT_SPINS: usize = 20_000_000;
const PORT_RESET_WAIT_SPINS: usize = 10_000_000;
const PORT_ENABLE_WAIT_SPINS: usize = 10_000_000;
const PORT_SETTLE_SPINS: usize = 100_000;
const DROP_HALT_WAIT_SPINS: usize = 1_000_000;
const USBSTS_CLEAR_MASK: u32 = reg::USBSTS_EINT | reg::USBSTS_PCD | reg::USBSTS_HSE | reg::USBSTS_HCE;
const USBLEGACY_BIOS_OWNED: u32 = 1 << 16;
const USBLEGACY_OS_OWNED: u32 = 1 << 24;
const EXT_CAP_SCAN_LIMIT: usize = 64;
const MAX_REASONABLE_SLOTS: u8 = 255;
const MAX_REASONABLE_PORTS: u8 = 255;
// Pi4/VL805 uses a small scratchpad count; very large values are usually
// bogus capability reads from an incorrect MMIO candidate.
const MAX_REASONABLE_SCRATCHPAD: u16 = 256;
const PORT_CHANGE_BITS: u32 = reg::PORTSC_CSC
    | reg::PORTSC_PEC
    | reg::PORTSC_WRC
    | reg::PORTSC_OCC
    | reg::PORTSC_PRC
    | reg::PORTSC_PLC
    | reg::PORTSC_CEC;
const PORTSC_NEUTRAL_MASK: u32 = reg::PORTSC_CCS
    | reg::PORTSC_PED
    | reg::PORTSC_OCA
    | reg::PORTSC_PLS_MASK
    | reg::PORTSC_PP
    | reg::PORTSC_SPEED_MASK
    | reg::PORTSC_PIC_MASK
    | reg::PORTSC_CAS
    | reg::PORTSC_WCE
    | reg::PORTSC_WDE
    | reg::PORTSC_WOE
    | reg::PORTSC_DR;

#[inline(always)]
fn ring_write_barrier() {
    compiler_fence(Ordering::Release);
    #[cfg(target_arch = "aarch64")]
    unsafe {
        core::arch::asm!("dmb oshst", options(nostack, preserves_flags));
    }
    #[cfg(not(target_arch = "aarch64"))]
    core::sync::atomic::fence(Ordering::Release);
}

#[inline]
const fn port_state_neutral(portsc: u32) -> u32 {
    portsc & PORTSC_NEUTRAL_MASK
}

fn parse_controller_params(
    cap_length: u8,
    hcs1: u32,
    hcs2: u32,
    db_offset: u32,
    rts_offset: u32,
) -> Option<(u8, u8, u16, usize)> {
    if cap_length < 0x20 || (cap_length as usize) >= MMIO_INIT_SIZE || (cap_length & 0x3) != 0 {
        return None;
    }

    let max_slots = (hcs1 & 0xff) as u8;
    let max_ports = ((hcs1 >> 24) & 0xff) as u8;
    let max_scratchpad = (((hcs2 >> 27) & 0x1f) | (((hcs2 >> 21) & 0x1f) << 5)) as u16;
    if max_slots == 0 || max_slots > MAX_REASONABLE_SLOTS {
        return None;
    }
    if max_ports == 0 || max_ports > MAX_REASONABLE_PORTS {
        return None;
    }
    if max_scratchpad > MAX_REASONABLE_SCRATCHPAD {
        return None;
    }

    if (db_offset & 0x3) != 0 || (rts_offset & 0x1f) != 0 {
        return None;
    }
    if db_offset < cap_length as u32 || rts_offset < cap_length as u32 {
        return None;
    }

    let mmio_size = (rts_offset as usize + 0x20 + 0x20)
        .max(db_offset as usize + (max_slots as usize + 1) * 4)
        .max(0x10000);
    if !(MMIO_INIT_SIZE..=MMIO_MAX_SIZE).contains(&mmio_size) {
        return None;
    }

    Some((max_slots, max_ports, max_scratchpad, mmio_size))
}

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
    scratchpad: Option<ScratchpadSet<H>>,
    cmd_ring: Mutex<Box<Ring<H>>>,
    event_ring: Mutex<Box<EventRing<H>>>,
    host: Arc<H>,
}

/// Snapshot of xHCI command/event ring state for timeout diagnostics.
#[derive(Clone, Copy, Debug)]
pub struct XhciCommandDiag {
    /// Current value of `USBCMD`.
    pub usbcmd: u32,
    /// Current value of `USBSTS`.
    pub usbsts: u32,
    /// Current value of `CRCR`.
    pub crcr: u64,
    /// Current value of `DCBAAP`.
    pub dcbaap: u64,
    /// Current value of interrupter 0 `IMAN`.
    pub iman: u32,
    /// Current value of interrupter 0 `ERDP`.
    pub erdp: u64,
    /// Current value of interrupter 0 `ERSTBA`.
    pub erstba: u64,
    /// Current value of the selected port `PORTSC`.
    pub portsc: u32,
}

struct ScratchpadSet<H: Dma> {
    array: PhysMem<H>,
    buffers: Vec<PhysMem<H>>,
}

impl<H: Dma> ScratchpadSet<H> {
    fn build(host: &H, count: usize) -> Result<Self> {
        // xHCI spec requires 64-byte alignment for the scratchpad pointer array.
        let array = PhysMem::alloc(host, count * core::mem::size_of::<u64>(), 64)?;
        let mut buffers = Vec::new();
        buffers
            .try_reserve_exact(count)
            .map_err(|_| UsbError::OoRam)?;

        let array_ptr = array.as_ptr::<u64>();
        for index in 0..count {
            // Scratchpad buffers are page-sized and page-aligned.
            let page = PhysMem::alloc(host, host.page_size(), host.page_size())?;
            let phys = page.phys(host);
            unsafe {
                array_ptr.add(index).write_volatile(phys);
            }
            buffers.push(page);
        }

        Ok(Self { array, buffers })
    }
}

impl<H: Dma> XhciCtrl<H> {
    /// Create and initialize a new xHCI controller
    pub fn new(mmio_phys: usize, host: H) -> Result<Self> {
        let host = Arc::new(host);

        // Initial map to read capability registers
        let init_mmio =
            unsafe { host.map_mmio(mmio_phys, MMIO_INIT_SIZE) }.ok_or(UsbError::MapFail)?;

        let cap_length = unsafe { (init_mmio as *const u8).read_volatile() };
        let hcs1: u32 = unsafe { ((init_mmio + reg::HCSPARAMS1) as *const u32).read_volatile() };
        let hcs2: u32 = unsafe { ((init_mmio + reg::HCSPARAMS2) as *const u32).read_volatile() };
        let db_offset: u32 = unsafe { ((init_mmio + reg::DBOFF) as *const u32).read_volatile() };
        let rts_offset: u32 = unsafe { ((init_mmio + reg::RTSOFF) as *const u32).read_volatile() };

        let parsed = parse_controller_params(cap_length, hcs1, hcs2, db_offset, rts_offset);

        unsafe {
            host.unmap_mmio(init_mmio, MMIO_INIT_SIZE);
        }
        let Some((max_slots, max_ports, max_scratchpad, mmio_size)) = parsed else {
            return Err(UsbError::MapFail);
        };

        // Remap with full size
        let mmio = unsafe { host.map_mmio(mmio_phys, mmio_size) }.ok_or(UsbError::MapFail)?;

        let op_base = mmio + cap_length as usize;
        let rt_base = mmio + rts_offset as usize;

        // Allocate DCBAA (Device Context Base Address Array)
        // xHCI spec requires 64-byte alignment for DCBAA
        let dcbaa = PhysMem::alloc(&*host, (max_slots as usize + 1) * 8, 64)?;

        // Allocate scratchpad if needed
        let scratchpad = if max_scratchpad > 0 {
            let set = ScratchpadSet::build(&*host, max_scratchpad as usize)?;
            unsafe {
                dcbaa.as_ptr::<u64>().write_volatile(set.array.phys(&*host));
            }
            Some(set)
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
        // Some firmware/UEFI stacks leave xHCI under legacy ownership until
        // the OS-owned semaphore is asserted.
        self.claim_legacy_ownership()?;

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
        // Prime ERDP and clear Event Handler Busy before running the controller.
        self.write_reg(int_base + reg::ERDP, event_ring.ring_phys(&*self.host) | 0x8);
        // Keep moderation disabled; Cohesix local-seat uses polling and does
        // not install xHCI IRQ handlers during early boot.
        self.write_reg(int_base + reg::IMOD, 0u32);
        self.write_reg(int_base + reg::IMAN, reg::IMAN_IP);
        drop(event_ring);

        // Clear stale status before run so command completions are observable.
        self.write_op(reg::USBSTS, USBSTS_CLEAR_MASK);
        // Start controller in polling mode (interrupt delivery remains masked).
        self.write_op(reg::USBCMD, reg::USBCMD_RUN);

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

    fn claim_legacy_ownership(&self) -> Result<()> {
        let hccparams1 = self.read_reg::<u32>(reg::HCCPARAMS1);
        let mut ext_offset = (((hccparams1 >> 16) & 0xffff) as usize) * 4;
        if ext_offset == 0 || ext_offset >= self.mmio_size {
            return Ok(());
        }

        for _ in 0..EXT_CAP_SCAN_LIMIT {
            if ext_offset + core::mem::size_of::<u32>() > self.mmio_size {
                return Ok(());
            }
            let legacy = self.read_reg::<u32>(ext_offset);
            let cap_id = (legacy & 0xff) as u8;
            let next = ((legacy >> 8) & 0xff) as usize * 4;

            if cap_id == reg::ECAP_USB_LEGACY {
                if (legacy & USBLEGACY_BIOS_OWNED) == 0 {
                    return Ok(());
                }
                self.write_reg(ext_offset, legacy | USBLEGACY_OS_OWNED);
                let mut waited = 0usize;
                while (self.read_reg::<u32>(ext_offset) & USBLEGACY_BIOS_OWNED) != 0 {
                    waited = waited.saturating_add(1);
                    if waited >= RESET_WAIT_SPINS {
                        return Err(UsbError::Timeout);
                    }
                    spin_loop();
                }
                return Ok(());
            }

            if next == 0 {
                return Ok(());
            }
            ext_offset = ext_offset.saturating_add(next);
            if ext_offset >= self.mmio_size {
                return Ok(());
            }
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

    #[inline(always)]
    fn read_op_u64(&self, offset: usize) -> u64 {
        let lo = self.read_op::<u32>(offset) as u64;
        let hi = self.read_op::<u32>(offset + 4) as u64;
        (hi << 32) | lo
    }

    #[inline(always)]
    fn read_reg_u64(&self, offset: usize) -> u64 {
        let lo = self.read_reg::<u32>(offset) as u64;
        let hi = self.read_reg::<u32>(offset + 4) as u64;
        (hi << 32) | lo
    }

    /// Ring the command doorbell
    fn ring_cmd_doorbell(&self) {
        let db = reg::doorbell(self.db_offset, 0);
        ring_write_barrier();
        self.write_reg(db, 0u32);
        let _ = self.read_reg::<u32>(db);
    }

    /// Ring device doorbell
    pub fn ring_doorbell(&self, slot: u8, target: u8) {
        let db = reg::doorbell(self.db_offset, slot);
        ring_write_barrier();
        self.write_reg(db, target as u32);
        let _ = self.read_reg::<u32>(db);
    }

    /// Update event ring dequeue pointer
    fn update_erdp(&self) {
        let event_ring = self.event_ring.lock();
        let int_base = reg::interrupter_base(self.rt_base as u32 - self.mmio as u32, 0);
        self.write_reg(
            int_base + reg::ERDP,
            event_ring.dequeue_ptr(&*self.host) | 0x8,
        );
        // Keep interrupt delivery masked; acknowledge pending interrupt state.
        self.write_reg(int_base + reg::IMAN, reg::IMAN_IP);
        self.write_op(reg::USBSTS, reg::USBSTS_EINT);
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
        let evt = match self.submit_command(trb) {
            Err(UsbError::Timeout) => return Err(UsbError::EnableSlotTimeout),
            Err(err) => return Err(err),
            Ok(evt) => evt,
        };
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
        let mut portsc: u32 = self.read_reg(offset);
        if (portsc & reg::PORTSC_CCS) == 0 {
            return Err(UsbError::DeviceNotFound);
        }

        // Clear stale change bits before asserting reset.
        self.write_reg(offset, port_state_neutral(portsc) | PORT_CHANGE_BITS);
        portsc = self.read_reg(offset);

        // Keep power enabled while requesting reset.
        let reset = port_state_neutral(portsc) | reg::PORTSC_PP | reg::PORTSC_PR;
        self.write_reg(offset, reset);

        // Wait for reset to complete
        let mut waited = 0usize;
        loop {
            portsc = self.read_reg(offset);
            if (portsc & reg::PORTSC_PR) == 0 {
                break;
            }
            waited = waited.saturating_add(1);
            if waited >= PORT_RESET_WAIT_SPINS {
                return Err(UsbError::PortResetTimeout);
            }
            spin_loop();
        }

        // Wait for the link to settle and expose either PED or speed bits.
        waited = 0;
        loop {
            portsc = self.read_reg(offset);
            let connected = (portsc & reg::PORTSC_CCS) != 0;
            let enabled = (portsc & reg::PORTSC_PED) != 0;
            let speed = reg::portsc_speed(portsc);
            if connected && (enabled || speed != 0) {
                break;
            }
            waited = waited.saturating_add(1);
            if waited >= PORT_ENABLE_WAIT_SPINS {
                return Err(UsbError::PortEnableTimeout);
            }
            spin_loop();
        }

        // Acknowledge port status change bits after reset.
        self.write_reg(offset, port_state_neutral(portsc) | PORT_CHANGE_BITS);
        for _ in 0..PORT_SETTLE_SPINS {
            spin_loop();
        }

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

    /// Captures key command/event-ring registers for timeout debugging.
    pub fn command_diag_for_port(&self, port: u8) -> XhciCommandDiag {
        let int_base = reg::interrupter_base(self.rt_base as u32 - self.mmio as u32, 0);
        XhciCommandDiag {
            usbcmd: self.read_op::<u32>(reg::USBCMD),
            usbsts: self.read_op::<u32>(reg::USBSTS),
            crcr: self.read_op_u64(reg::CRCR),
            dcbaap: self.read_op_u64(reg::DCBAAP),
            iman: self.read_reg::<u32>(int_base + reg::IMAN),
            erdp: self.read_reg_u64(int_base + reg::ERDP),
            erstba: self.read_reg_u64(int_base + reg::ERSTBA),
            portsc: self.port_status(port),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_controller_params;

    #[test]
    fn parse_controller_params_rejects_all_ones() {
        assert!(
            parse_controller_params(0xff, 0xffff_ffff, 0xffff_ffff, 0xffff_ffff, 0xffff_ffff)
                .is_none()
        );
    }

    #[test]
    fn parse_controller_params_accepts_reasonable_window() {
        let hcs1 = 32u32 | (8u32 << 24);
        let parsed = parse_controller_params(0x40, hcs1, 0, 0x1000, 0x2000);
        assert!(parsed.is_some());
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
