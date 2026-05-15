// Author: Lukas Bower
// Purpose: Vendored usb-oxide source with Cohesix-specific timeout hardening for Pi4 local-seat initialization.
// Copyright 2026 Lukas Bower
//! TRB ring buffer structures for xHCI.

use crate::{Dma, Result, UsbError};

use core::{
    marker::PhantomData,
    sync::atomic::{compiler_fence, Ordering},
};

const XHCI_RING_ALIGNMENT: usize = 64;
const EVENT_RING_ERST_SEGMENTS: usize = 1;

/// Transfer Request Block (TRB) - 16 bytes aligned.
///
/// The fundamental data structure used for communication between
/// software and the xHCI controller.
#[repr(C, align(16))]
#[derive(Clone, Copy, Default)]
pub struct Trb {
    /// Parameter field (address or immediate data)
    pub param: u64,
    /// Status field (transfer length, completion code)
    pub status: u32,
    /// Control field (TRB type, flags)
    pub control: u32,
}

impl Trb {
    /// Creates a new zeroed TRB.
    pub const fn new() -> Self {
        Self {
            param: 0,
            status: 0,
            control: 0,
        }
    }

    /// Sets the cycle bit.
    pub fn set_cycle(&mut self, cycle: bool) {
        if cycle {
            self.control |= 1;
        } else {
            self.control &= !1;
        }
    }

    /// Returns the cycle bit.
    pub fn cycle(&self) -> bool {
        self.control & 1 != 0
    }

    /// Returns the TRB type.
    pub fn trb_type(&self) -> u8 {
        ((self.control >> 10) & 0x3f) as u8
    }

    /// Returns the completion code.
    pub fn completion_code(&self) -> u8 {
        ((self.status >> 24) & 0xff) as u8
    }

    /// Returns the slot ID.
    pub fn slot_id(&self) -> u8 {
        ((self.control >> 24) & 0xff) as u8
    }

    /// Returns the endpoint ID.
    pub fn endpoint_id(&self) -> u8 {
        ((self.control >> 16) & 0x1f) as u8
    }

    /// Returns the transfer length.
    pub fn transfer_length(&self) -> u32 {
        self.status & 0x1ffff
    }

    /// Returns the four 32-bit TRB fields in U-Boot queue_trb() order.
    fn uboot_dword_fields(self) -> [u32; 4] {
        [
            (self.param as u32).to_le(),
            ((self.param >> 32) as u32).to_le(),
            self.status.to_le(),
            self.control.to_le(),
        ]
    }

    /// Publishes a producer-owned TRB with the control/cycle dword last.
    ///
    /// xHCI treats the cycle bit in the control dword as the ownership handoff.
    /// U-Boot writes the four 32-bit fields in order and flushes the 16-byte
    /// record before ringing DB0; keep the same observable order here.
    unsafe fn write_producer_ordered(self, dst: *mut Trb) {
        let fields = self.uboot_dword_fields();
        let dst = dst.cast::<u32>();
        // SAFETY: the caller provides a valid 16-byte TRB slot. The first three
        // volatile dword stores publish parameter/status before ownership.
        unsafe {
            dst.add(0).write_volatile(fields[0]);
            dst.add(1).write_volatile(fields[1]);
            dst.add(2).write_volatile(fields[2]);
        }
        compiler_fence(Ordering::Release);
        // SAFETY: same bounded TRB slot; this final volatile store publishes
        // the control dword and cycle bit as the ownership handoff.
        unsafe {
            dst.add(3).write_volatile(fields[3]);
        }
        compiler_fence(Ordering::Release);
    }
}

/// TRB type codes as defined in the xHCI specification.
pub mod trb_type {
    // Transfer TRBs (used on Transfer Rings)
    /// Normal TRB - Used for bulk and interrupt transfers
    pub const NORMAL: u32 = 1;
    /// Setup Stage TRB - Control transfer setup stage
    pub const SETUP: u32 = 2;
    /// Data Stage TRB - Control transfer data stage
    pub const DATA: u32 = 3;
    /// Status Stage TRB - Control transfer status stage
    pub const STATUS: u32 = 4;
    /// Isoch TRB - Isochronous transfer
    pub const ISOCH: u32 = 5;
    /// Link TRB - Links to another segment
    pub const LINK: u32 = 6;
    /// Event Data TRB - Generate event with immediate data
    pub const EVENT_DATA: u32 = 7;
    /// No Op TRB (Transfer) - No operation on transfer ring
    pub const NO_OP: u32 = 8;

    // Command TRBs (used on Command Ring)
    /// Enable Slot Command
    pub const ENABLE_SLOT: u32 = 9;
    /// Disable Slot Command
    pub const DISABLE_SLOT: u32 = 10;
    /// Address Device Command
    pub const ADDRESS_DEVICE: u32 = 11;
    /// Configure Endpoint Command
    pub const CONFIGURE_ENDPOINT: u32 = 12;
    /// Evaluate Context Command
    pub const EVALUATE_CONTEXT: u32 = 13;
    /// Reset Endpoint Command
    pub const RESET_ENDPOINT: u32 = 14;
    /// Stop Endpoint Command
    pub const STOP_ENDPOINT: u32 = 15;
    /// Set TR Dequeue Pointer Command
    pub const SET_TR_DEQUEUE: u32 = 16;
    /// Reset Device Command
    pub const RESET_DEVICE: u32 = 17;
    /// Force Event Command (optional)
    pub const FORCE_EVENT: u32 = 18;
    /// Negotiate Bandwidth Command (optional)
    pub const NEGOTIATE_BANDWIDTH: u32 = 19;
    /// Set Latency Tolerance Value Command (optional)
    pub const SET_LATENCY_TOLERANCE: u32 = 20;
    /// Get Port Bandwidth Command (optional)
    pub const GET_PORT_BANDWIDTH: u32 = 21;
    /// Force Header Command
    pub const FORCE_HEADER: u32 = 22;
    /// No Op Command - No operation on command ring
    pub const NO_OP_CMD: u32 = 23;
    /// Get Extended Property Command (optional)
    pub const GET_EXTENDED_PROPERTY: u32 = 24;
    /// Set Extended Property Command (optional)
    pub const SET_EXTENDED_PROPERTY: u32 = 25;

    // Event TRBs (generated by xHC on Event Ring)
    /// Transfer Event - Completion of a transfer
    pub const TRANSFER_EVENT: u32 = 32;
    /// Command Completion Event
    pub const COMMAND_COMPLETION: u32 = 33;
    /// Port Status Change Event
    pub const PORT_STATUS_CHANGE: u32 = 34;
    /// Bandwidth Request Event (optional)
    pub const BANDWIDTH_REQUEST: u32 = 35;
    /// Doorbell Event (optional)
    pub const DOORBELL_EVENT: u32 = 36;
    /// Dma Controller Event
    pub const HOST_CONTROLLER_EVENT: u32 = 37;
    /// Device Notification Event
    pub const DEVICE_NOTIFICATION: u32 = 38;
    /// MFINDEX Wrap Event
    pub const MFINDEX_WRAP: u32 = 39;

    // Vendor-defined TRB types
    /// Vendor Defined Command
    pub const VENDOR_DEFINED_CMD: u32 = 48;
    /// Vendor Defined Event
    pub const VENDOR_DEFINED_EVENT: u32 = 49;
}

/// TRB completion codes as defined in the xHCI specification.
pub mod completion {
    /// Invalid - Not a valid completion code
    pub const INVALID: u8 = 0;
    /// Success - TRB completed without error
    pub const SUCCESS: u8 = 1;
    /// Data Buffer Error - Data buffer error
    pub const DATA_BUFFER_ERROR: u8 = 2;
    /// Babble Detected Error - Babble detected
    pub const BABBLE_DETECTED: u8 = 3;
    /// USB Transaction Error - USB transaction error
    pub const USB_TRANSACTION_ERROR: u8 = 4;
    /// TRB Error - TRB error
    pub const TRB_ERROR: u8 = 5;
    /// Stall Error - Endpoint stall
    pub const STALL_ERROR: u8 = 6;
    /// Resource Error - Inadequate xHC resources
    pub const RESOURCE_ERROR: u8 = 7;
    /// Bandwidth Error - Inadequate bandwidth
    pub const BANDWIDTH_ERROR: u8 = 8;
    /// No Slots Available Error - No device slots available
    pub const NO_SLOTS_AVAILABLE: u8 = 9;
    /// Invalid Stream Type Error - Invalid stream type
    pub const INVALID_STREAM_TYPE: u8 = 10;
    /// Slot Not Enabled Error - Slot not enabled
    pub const SLOT_NOT_ENABLED: u8 = 11;
    /// Endpoint Not Enabled Error - Endpoint not enabled
    pub const ENDPOINT_NOT_ENABLED: u8 = 12;
    /// Short Packet - Transfer completed with short packet
    pub const SHORT_PACKET: u8 = 13;
    /// Ring Underrun - Isoch transfer ring underrun
    pub const RING_UNDERRUN: u8 = 14;
    /// Ring Overrun - Isoch transfer ring overrun
    pub const RING_OVERRUN: u8 = 15;
    /// VF Event Ring Full Error - Virtual function event ring full
    pub const VF_EVENT_RING_FULL: u8 = 16;
    /// Parameter Error - Context parameter error
    pub const PARAMETER_ERROR: u8 = 17;
    /// Bandwidth Overrun Error - Isoch bandwidth overrun
    pub const BANDWIDTH_OVERRUN: u8 = 18;
    /// Context State Error - Context state error
    pub const CONTEXT_STATE_ERROR: u8 = 19;
    /// No Ping Response Error - No ping response
    pub const NO_PING_RESPONSE: u8 = 20;
    /// Event Ring Full Error - Event ring full
    pub const EVENT_RING_FULL: u8 = 21;
    /// Incompatible Device Error - Incompatible device
    pub const INCOMPATIBLE_DEVICE: u8 = 22;
    /// Missed Service Error - Missed service window
    pub const MISSED_SERVICE: u8 = 23;
    /// Command Ring Stopped - Command ring stopped
    pub const COMMAND_RING_STOPPED: u8 = 24;
    /// Command Aborted - Command aborted
    pub const COMMAND_ABORTED: u8 = 25;
    /// Stopped - Endpoint stopped
    pub const STOPPED: u8 = 26;
    /// Stopped - Length Invalid - Endpoint stopped with invalid length
    pub const STOPPED_LENGTH_INVALID: u8 = 27;
    /// Stopped - Short Packet - Endpoint stopped on short packet
    pub const STOPPED_SHORT_PACKET: u8 = 28;
    /// Max Exit Latency Too Large Error
    pub const MAX_EXIT_LATENCY_TOO_LARGE: u8 = 29;
    /// Isoch Buffer Overrun - Isoch buffer overrun
    pub const ISOCH_BUFFER_OVERRUN: u8 = 31;
    /// Event Lost Error - Event lost due to overflow
    pub const EVENT_LOST: u8 = 32;
    /// Undefined Error - Undefined error
    pub const UNDEFINED_ERROR: u8 = 33;
    /// Invalid Stream ID Error - Invalid stream ID
    pub const INVALID_STREAM_ID: u8 = 34;
    /// Secondary Bandwidth Error - Secondary bandwidth error
    pub const SECONDARY_BANDWIDTH_ERROR: u8 = 35;
    /// Split Transaction Error - Split transaction error
    pub const SPLIT_TRANSACTION_ERROR: u8 = 36;

    /// Returns a human-readable name for the completion code.
    pub const fn name(code: u8) -> &'static str {
        match code {
            SUCCESS => "Success",
            DATA_BUFFER_ERROR => "Data Buffer Error",
            BABBLE_DETECTED => "Babble Detected",
            USB_TRANSACTION_ERROR => "USB Transaction Error",
            TRB_ERROR => "TRB Error",
            STALL_ERROR => "Stall Error",
            RESOURCE_ERROR => "Resource Error",
            BANDWIDTH_ERROR => "Bandwidth Error",
            NO_SLOTS_AVAILABLE => "No Slots Available",
            INVALID_STREAM_TYPE => "Invalid Stream Type",
            SLOT_NOT_ENABLED => "Slot Not Enabled",
            ENDPOINT_NOT_ENABLED => "Endpoint Not Enabled",
            SHORT_PACKET => "Short Packet",
            RING_UNDERRUN => "Ring Underrun",
            RING_OVERRUN => "Ring Overrun",
            VF_EVENT_RING_FULL => "VF Event Ring Full",
            PARAMETER_ERROR => "Parameter Error",
            BANDWIDTH_OVERRUN => "Bandwidth Overrun",
            CONTEXT_STATE_ERROR => "Context State Error",
            NO_PING_RESPONSE => "No Ping Response",
            EVENT_RING_FULL => "Event Ring Full",
            INCOMPATIBLE_DEVICE => "Incompatible Device",
            MISSED_SERVICE => "Missed Service",
            COMMAND_RING_STOPPED => "Command Ring Stopped",
            COMMAND_ABORTED => "Command Aborted",
            STOPPED => "Stopped",
            STOPPED_LENGTH_INVALID => "Stopped - Length Invalid",
            STOPPED_SHORT_PACKET => "Stopped - Short Packet",
            MAX_EXIT_LATENCY_TOO_LARGE => "Max Exit Latency Too Large",
            ISOCH_BUFFER_OVERRUN => "Isoch Buffer Overrun",
            EVENT_LOST => "Event Lost",
            UNDEFINED_ERROR => "Undefined Error",
            INVALID_STREAM_ID => "Invalid Stream ID",
            SECONDARY_BANDWIDTH_ERROR => "Secondary Bandwidth Error",
            SPLIT_TRANSACTION_ERROR => "Split Transaction Error",
            _ => "Unknown",
        }
    }
}

/// TRB control field flags.
pub mod trb_flags {
    /// Cycle bit
    pub const CYCLE: u32 = 1 << 0;
    /// Evaluate Next TRB (ENT)
    pub const ENT: u32 = 1 << 1;
    /// Interrupt on Short Packet (ISP)
    pub const ISP: u32 = 1 << 2;
    /// No Snoop (NS)
    pub const NO_SNOOP: u32 = 1 << 3;
    /// Chain bit
    pub const CHAIN: u32 = 1 << 4;
    /// Interrupt on Completion (IOC)
    pub const IOC: u32 = 1 << 5;
    /// Immediate Data (IDT)
    pub const IDT: u32 = 1 << 6;
    /// Toggle Cycle (for Link TRB)
    pub const TOGGLE_CYCLE: u32 = 1 << 1;
    /// Block Event Interrupt (BEI)
    pub const BEI: u32 = 1 << 9;
}

/// Represents a DMA-capable physical memory region.
pub struct PhysMem<H: Dma> {
    addr: usize,
    size: usize,
    align: usize,
    _host: PhantomData<H>,
}

impl<H: Dma> PhysMem<H> {
    fn empty_for_drop_replacement() -> Self {
        Self {
            addr: 0,
            size: 0,
            align: 1,
            _host: PhantomData,
        }
    }

    /// Allocates a new physical memory region with the specified alignment.
    pub fn alloc(host: &H, size: usize, align: usize) -> Result<Self> {
        // SAFETY: `Dma::alloc` is the HAL-owned allocator for coherent USB DMA
        // memory. The returned address is accepted only when non-null and is
        // tracked with the same size/alignment for later `free`.
        let addr = unsafe { host.alloc(size, align) }.ok_or(UsbError::OoRam)?;

        // SAFETY: `addr` names the `size` bytes just returned by `Dma::alloc`,
        // so zeroing the whole allocation cannot alias other live Rust objects.
        unsafe {
            core::ptr::write_bytes(addr as *mut u8, 0, size);
        }
        Ok(Self {
            addr,
            size,
            align,
            _host: PhantomData,
        })
    }

    /// Returns the virtual address.
    pub fn virt(&self) -> usize {
        self.addr
    }

    /// Returns the physical address.
    pub fn phys(&self, host: &H) -> u64 {
        host.try_virt_to_phys(self.addr).unwrap_or(0) as u64
    }

    /// Returns the device-visible address or a DMA publication error.
    pub fn try_phys(&self, host: &H) -> Result<u64> {
        host.try_virt_to_phys(self.addr)
            .map(|addr| addr as u64)
            .ok_or(UsbError::DmaSync)
    }

    /// Prepares the region for device access and returns its bus address.
    pub fn share_for_device(&self, host: &H, label: &'static str) -> Result<u64> {
        host.share_for_device(self.addr, self.size, label)
            .map_err(|_| UsbError::DmaSync)?;
        host.try_virt_to_phys(self.addr)
            .map(|addr| addr as u64)
            .ok_or(UsbError::DmaSync)
    }

    /// Prepares a subrange for device access and returns its bus address.
    pub fn share_range_for_device(
        &self,
        host: &H,
        offset: usize,
        len: usize,
        label: &'static str,
    ) -> Result<u64> {
        let end = offset.checked_add(len).ok_or(UsbError::DmaSync)?;
        if len == 0 || end > self.size {
            return Err(UsbError::DmaSync);
        }
        let vaddr = self.addr.checked_add(offset).ok_or(UsbError::DmaSync)?;
        host.share_for_device(vaddr, len, label)
            .map_err(|_| UsbError::DmaSync)?;
        host.try_virt_to_phys(vaddr)
            .map(|addr| addr as u64)
            .ok_or(UsbError::DmaSync)
    }

    /// Makes a device-written subrange visible before CPU reads.
    pub fn sync_for_cpu_range(
        &self,
        host: &H,
        offset: usize,
        len: usize,
        label: &'static str,
    ) -> Result<()> {
        let end = offset.checked_add(len).ok_or(UsbError::DmaSync)?;
        if len == 0 || end > self.size {
            return Err(UsbError::DmaSync);
        }
        let vaddr = self.addr.checked_add(offset).ok_or(UsbError::DmaSync)?;
        host.sync_for_cpu(vaddr, len, label)
            .map_err(|_| UsbError::DmaSync)?;
        compiler_fence(Ordering::Acquire);
        Ok(())
    }

    /// Returns a pointer to the memory.
    pub fn as_ptr<T>(&self) -> *mut T {
        self.addr as *mut T
    }

    /// Returns the size in bytes.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Returns the alignment in bytes.
    pub fn align(&self) -> usize {
        self.align
    }

    /// Frees the memory region.
    pub fn free(self, host: &H) {
        // SAFETY: `self.addr/size/align` are the exact allocation tuple
        // returned by `Dma::alloc` for this `PhysMem`, and `self` is consumed so
        // the region cannot be freed twice.
        unsafe {
            host.free(self.addr, self.size, self.align);
        }
        core::mem::forget(self);
    }
}

pub(crate) struct Ring<H: Dma> {
    mem: PhysMem<H>,
    enqueue: usize,
    cycle: bool,
    size: usize,
}

impl<H: Dma> Ring<H> {
    /// Allocates a TRB ring and initializes its link TRB.
    pub fn new(host: &H, trb_count: usize) -> Result<Self> {
        if trb_count < 2 {
            return Err(UsbError::NotSupported);
        }
        let mem = PhysMem::alloc(
            host,
            trb_count * core::mem::size_of::<Trb>(),
            XHCI_RING_ALIGNMENT,
        )?;
        let mut ring = Self {
            mem,
            enqueue: 0,
            cycle: true,
            size: trb_count,
        };
        ring.init_link_trb(host)?;
        Ok(ring)
    }

    pub(crate) fn empty_for_drop_replacement() -> Self {
        Self {
            mem: PhysMem::empty_for_drop_replacement(),
            enqueue: 0,
            cycle: true,
            size: 0,
        }
    }

    /// Returns the device-visible physical address for the ring.
    pub fn phys(&self, host: &H) -> u64 {
        self.mem.phys(host)
    }

    /// Shares the current ring contents with the device.
    pub fn share_for_device(&self, host: &H, label: &'static str) -> Result<u64> {
        self.mem.share_for_device(host, label)
    }

    /// Publishes host writes made after the initial ring share to the device.
    pub fn sync_for_device(&self, host: &H, label: &'static str) -> Result<()> {
        compiler_fence(Ordering::Release);
        self.mem.share_for_device(host, label)?;
        Ok(())
    }

    /// Enqueues one TRB and returns its device-visible address.
    pub fn try_enqueue(&mut self, host: &H, mut trb: Trb) -> Result<u64> {
        trb.set_cycle(self.cycle);
        let ring_phys = self.mem.try_phys(host)?;
        let addr = ring_phys + (self.enqueue * 16) as u64;
        let slot = self.enqueue;
        // SAFETY: `slot` is the current producer index within a ring allocated
        // for exactly `self.size` TRBs; only the owning producer mutates it.
        unsafe {
            trb.write_producer_ordered(self.mem.as_ptr::<Trb>().add(slot));
        }
        self.enqueue += 1;

        if self.enqueue >= self.size - 1 {
            let mut link = Trb::new();
            link.param = ring_phys;
            link.control = (trb_type::LINK << 10) | 2;
            link.set_cycle(self.cycle);
            let link_slot = self.enqueue;
            // SAFETY: `link_slot` is the reserved final link TRB in the
            // allocated ring and is mutated only by the owning producer.
            unsafe {
                link.write_producer_ordered(self.mem.as_ptr::<Trb>().add(link_slot));
            }
            self.enqueue = 0;
            self.cycle = !self.cycle;
        }

        Ok(addr)
    }

    /// Enqueues one TRB, then publishes the updated ring contents to the device.
    pub fn enqueue_and_sync(&mut self, host: &H, trb: Trb, label: &'static str) -> Result<u64> {
        let addr = self.try_enqueue(host, trb)?;
        self.sync_for_device(host, label)?;
        Ok(addr)
    }

    /// Returns `(enqueue_index, producer_cycle)` for diagnostics.
    pub fn debug_state(&self) -> (usize, bool) {
        (self.enqueue, self.cycle)
    }

    pub fn debug_trb_at(&self, index: usize) -> Option<Trb> {
        if index >= self.size {
            return None;
        }
        // SAFETY: index is bounded by self.size, and Ring::new allocates ring
        // memory for exactly self.size TRBs.
        Some(unsafe { (self.mem.as_ptr::<Trb>()).add(index).read_volatile() })
    }

    fn init_link_trb(&mut self, host: &H) -> Result<()> {
        let last = self.size - 1;
        let mut link = Trb::new();
        link.param = self.mem.try_phys(host)?;
        link.control = (trb_type::LINK << 10) | 2; // Toggle cycle
        link.set_cycle(self.cycle);
        // SAFETY: `last` is the reserved final link TRB in the freshly
        // allocated ring, and no device can observe it before initial share.
        unsafe {
            link.write_producer_ordered(self.mem.as_ptr::<Trb>().add(last));
        }
        Ok(())
    }

    /// Frees the ring allocation.
    pub fn free(self, host: &H) {
        self.mem.free(host);
    }
}

#[repr(C, align(16))]
#[derive(Clone, Copy, Default)]
pub(crate) struct ErstEntry {
    pub base: u64,
    pub size: u16,
    _0: [u8; 6],
}

const ERST_TABLE_ALIGNMENT: usize = 64;

pub(crate) struct EventRing<H: Dma> {
    ring: PhysMem<H>,
    erst: PhysMem<H>,
    size: usize,
    erst_entries: usize,
    dequeue: usize,
    cycle: bool,
}

impl<H: Dma> EventRing<H> {
    pub fn new(host: &H, trb_count: usize) -> Result<Self> {
        let ring = PhysMem::alloc(
            host,
            trb_count * core::mem::size_of::<Trb>(),
            XHCI_RING_ALIGNMENT,
        )?;
        let erst = PhysMem::alloc(host, host.page_size(), ERST_TABLE_ALIGNMENT)?;

        let erst_entries = trb_count.min(EVENT_RING_ERST_SEGMENTS).max(1);
        let entry = erst.as_ptr::<ErstEntry>();
        let trbs_per_entry = trb_count / erst_entries;
        let extra_trbs = trb_count % erst_entries;
        let mut trb_offset = 0usize;
        // SAFETY: the ERST allocation is one page, which holds far more than
        // EVENT_RING_ERST_SEGMENTS entries. Each written entry is within that
        // bounded prefix, and each segment points into the allocated ring.
        unsafe {
            for index in 0..erst_entries {
                let segment_trbs = trbs_per_entry + usize::from(index < extra_trbs);
                (*entry.add(index)).base =
                    ring.try_phys(host)? + (trb_offset * core::mem::size_of::<Trb>()) as u64;
                (*entry.add(index)).size = segment_trbs as u16;
                trb_offset += segment_trbs;
            }
        }

        Ok(Self {
            ring,
            erst,
            size: trb_count,
            erst_entries,
            dequeue: 0,
            cycle: true,
        })
    }

    pub fn ring_phys(&self, host: &H) -> u64 {
        self.ring.phys(host)
    }

    pub fn erst_phys(&self, host: &H) -> u64 {
        self.erst.phys(host)
    }

    pub fn erst_entries(&self) -> u32 {
        self.erst_entries as u32
    }

    pub fn share_for_device(&self, host: &H) -> Result<(u64, u64)> {
        let ring = self.ring.share_for_device(host, "xhci-event-ring")?;
        let erst = self.erst.share_for_device(host, "xhci-erst")?;
        Ok((ring, erst))
    }

    pub fn sync_current_for_cpu(&self, host: &H, label: &'static str) -> Result<()> {
        self.ring.sync_for_cpu_range(
            host,
            self.dequeue * core::mem::size_of::<Trb>(),
            core::mem::size_of::<Trb>(),
            label,
        )
    }

    pub fn sync_prefix_for_cpu(
        &self,
        host: &H,
        trb_count: usize,
        label: &'static str,
    ) -> Result<()> {
        let bounded_count = trb_count.min(self.size);
        if bounded_count == 0 {
            return Err(UsbError::DmaSync);
        }
        self.ring
            .sync_for_cpu_range(host, 0, bounded_count * core::mem::size_of::<Trb>(), label)
    }

    pub fn debug_state(&self) -> (usize, bool) {
        (self.dequeue, self.cycle)
    }

    pub fn debug_trb_at(&self, index: usize) -> Option<Trb> {
        if index >= self.size {
            return None;
        }
        // SAFETY: index is bounded by self.size, and EventRing::new allocates
        // ring memory for exactly self.size TRBs.
        Some(unsafe { (self.ring.as_ptr::<Trb>()).add(index).read_volatile() })
    }

    pub fn try_dequeue(&mut self) -> Option<Trb> {
        // Read the candidate TRB twice before consuming it. On some firmware /
        // controller combinations, software can observe an event entry while
        // the xHC is still writing fields; consuming such a transient entry can
        // surface as a spurious "Invalid completion code" command failure.
        // SAFETY: `self.dequeue` is maintained modulo `self.size`; `ring`
        // points at DMA memory for exactly `self.size` TRBs.
        let first = unsafe {
            (self.ring.as_ptr::<Trb>())
                .add(self.dequeue)
                .read_volatile()
        };
        if first.cycle() != self.cycle {
            return None;
        }

        // SAFETY: Same bounded ring slot as the first read; this second volatile
        // read checks that the controller has stopped mutating the event entry.
        let second = unsafe {
            (self.ring.as_ptr::<Trb>())
                .add(self.dequeue)
                .read_volatile()
        };
        if second.cycle() != self.cycle {
            return None;
        }

        if first.param != second.param
            || first.status != second.status
            || first.control != second.control
        {
            return None;
        }

        // Completion-style events should not present `INVALID` completion code.
        // Treat it as an unstable read and retry.
        let event_type = second.trb_type() as u32;
        if (event_type == trb_type::COMMAND_COMPLETION || event_type == trb_type::TRANSFER_EVENT)
            && second.completion_code() == completion::INVALID
        {
            return None;
        }

        self.dequeue += 1;
        if self.dequeue >= self.size {
            self.dequeue = 0;
            self.cycle = !self.cycle;
        }
        Some(second)
    }

    pub fn try_dequeue_ptr(&self, host: &H) -> Result<u64> {
        Ok(self.ring.try_phys(host)? + (self.dequeue * 16) as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::{trb_type, PhysMem, Ring, Trb, XHCI_RING_ALIGNMENT};
    use crate::{Dma, DmaShareError};
    use core::sync::atomic::{AtomicUsize, Ordering};
    use std::alloc::{alloc_zeroed, dealloc, Layout};
    use std::sync::Mutex;
    use std::vec::Vec;

    #[derive(Default)]
    struct MockDma {
        allocations: Mutex<Vec<(usize, usize, usize)>>,
        share_calls: AtomicUsize,
        last_share_vaddr: AtomicUsize,
        last_share_len: AtomicUsize,
    }

    impl Dma for MockDma {
        unsafe fn alloc(&self, size: usize, align: usize) -> Option<usize> {
            let layout = Layout::from_size_align(size, align).ok()?;
            // SAFETY: `layout` was constructed by `Layout::from_size_align`.
            let ptr = unsafe { alloc_zeroed(layout) };
            if ptr.is_null() {
                return None;
            }
            self.allocations
                .lock()
                .expect("allocations mutex")
                .push((ptr as usize, size, align));
            Some(ptr as usize)
        }

        unsafe fn free(&self, addr: usize, size: usize, align: usize) {
            let mut allocations = self.allocations.lock().expect("allocations mutex");
            if let Some(index) =
                allocations
                    .iter()
                    .position(|&(base, stored_size, stored_align)| {
                        base == addr && stored_size == size && stored_align == align
                    })
            {
                let (base, stored_size, stored_align) = allocations.swap_remove(index);
                let layout =
                    Layout::from_size_align(stored_size, stored_align).expect("valid layout");
                // SAFETY: The allocation record proves `base`, `stored_size`,
                // and `stored_align` match the original allocation layout.
                unsafe {
                    dealloc(base as *mut u8, layout);
                }
            }
        }

        unsafe fn map_mmio(&self, _phys: usize, _size: usize) -> Option<usize> {
            None
        }

        unsafe fn unmap_mmio(&self, _virt: usize, _size: usize) {}

        fn virt_to_phys(&self, va: usize) -> usize {
            va + 0x1000
        }

        fn share_for_device(
            &self,
            vaddr: usize,
            len: usize,
            _label: &'static str,
        ) -> core::result::Result<(), DmaShareError> {
            self.share_calls.fetch_add(1, Ordering::Relaxed);
            self.last_share_vaddr.store(vaddr, Ordering::Relaxed);
            self.last_share_len.store(len, Ordering::Relaxed);
            Ok(())
        }

        fn sync_for_cpu(
            &self,
            vaddr: usize,
            len: usize,
            _label: &'static str,
        ) -> core::result::Result<(), DmaShareError> {
            self.share_calls.fetch_add(1, Ordering::Relaxed);
            self.last_share_vaddr.store(vaddr, Ordering::Relaxed);
            self.last_share_len.store(len, Ordering::Relaxed);
            Ok(())
        }
    }

    #[test]
    fn trb_uboot_dword_fields_keep_cycle_handoff_last() {
        let mut trb = Trb {
            param: 0x1122_3344_5566_7788,
            status: 0xaabb_ccdd,
            control: trb_type::ENABLE_SLOT << 10,
        };
        trb.set_cycle(true);

        let fields = trb.uboot_dword_fields();

        assert_eq!(u32::from_le(fields[0]), 0x5566_7788);
        assert_eq!(u32::from_le(fields[1]), 0x1122_3344);
        assert_eq!(u32::from_le(fields[2]), 0xaabb_ccdd);
        assert_eq!(u32::from_le(fields[3]), (trb_type::ENABLE_SLOT << 10) | 1);
    }

    #[test]
    fn trb_ordered_write_preserves_uboot_field_layout() {
        let mut slot = Trb::new();
        let mut trb = Trb {
            param: 0x0102_0304_0506_0708,
            status: 0x090a_0b0c,
            control: trb_type::NO_OP_CMD << 10,
        };
        trb.set_cycle(true);

        // SAFETY: the stack slot is a valid, aligned TRB destination for this
        // single producer-side publication test.
        unsafe {
            trb.write_producer_ordered(&mut slot);
        }

        assert_eq!(slot.param, trb.param);
        assert_eq!(slot.status, trb.status);
        assert_eq!(slot.control, trb.control);
    }

    #[test]
    fn physmem_share_for_device_calls_host_hook_before_returning_bus_address() {
        let host = MockDma::default();
        let mem = PhysMem::alloc(&host, 128, 64).expect("allocate physmem");

        let bus = mem
            .share_for_device(&host, "test-share")
            .expect("share for device");

        assert_eq!(host.share_calls.load(Ordering::Relaxed), 1);
        assert_eq!(host.last_share_vaddr.load(Ordering::Relaxed), mem.virt());
        assert_eq!(host.last_share_len.load(Ordering::Relaxed), 128);
        assert_eq!(bus, (mem.virt() + 0x1000) as u64);

        mem.free(&host);
    }

    #[test]
    fn ring_enqueue_and_sync_reshare_after_late_trb_write() {
        let host = MockDma::default();
        let mut ring = Ring::<MockDma>::new(&host, 8).expect("allocate ring");
        host.share_calls.store(0, Ordering::Relaxed);
        host.last_share_vaddr.store(0, Ordering::Relaxed);
        host.last_share_len.store(0, Ordering::Relaxed);

        let addr = ring
            .enqueue_and_sync(
                &host,
                Trb {
                    param: 0,
                    status: 0,
                    control: trb_type::NO_OP_CMD << 10,
                },
                "cmd-submit",
            )
            .expect("sync after enqueue");

        assert_eq!(addr, ring.phys(&host));
        assert_eq!(host.share_calls.load(Ordering::Relaxed), 1);
        assert_eq!(
            host.last_share_vaddr.load(Ordering::Relaxed),
            ring.mem.virt()
        );
        assert_eq!(
            host.last_share_len.load(Ordering::Relaxed),
            8 * core::mem::size_of::<Trb>()
        );
        let observed = ring.debug_trb_at(0).expect("debug trb");
        assert_eq!(observed.control, (trb_type::NO_OP_CMD << 10) | 1);
    }

    #[test]
    fn ring_enqueue_and_sync_publishes_full_ring_for_command_visibility() {
        let host = MockDma::default();
        let mut ring = Ring::<MockDma>::new(&host, 8).expect("allocate ring");
        host.share_calls.store(0, Ordering::Relaxed);
        host.last_share_vaddr.store(0, Ordering::Relaxed);
        host.last_share_len.store(0, Ordering::Relaxed);

        let addr = ring
            .enqueue_and_sync(
                &host,
                Trb {
                    param: 0,
                    status: 0,
                    control: trb_type::NO_OP_CMD << 10,
                },
                "cmd-submit",
            )
            .expect("sync submitted trb");

        assert_eq!(addr, ring.phys(&host));
        assert_eq!(host.share_calls.load(Ordering::Relaxed), 1);
        assert_eq!(
            host.last_share_vaddr.load(Ordering::Relaxed),
            ring.mem.virt()
        );
        assert_eq!(
            host.last_share_len.load(Ordering::Relaxed),
            8 * core::mem::size_of::<Trb>()
        );
        let observed = ring.debug_trb_at(0).expect("debug trb");
        assert_eq!(observed.control, (trb_type::NO_OP_CMD << 10) | 1);
    }

    #[test]
    fn command_ring_uses_xhci_required_alignment() {
        let host = MockDma::default();
        let ring = Ring::<MockDma>::new(&host, 64).expect("allocate command ring");

        assert_eq!(XHCI_RING_ALIGNMENT, 64);
        assert_eq!(ring.mem.align(), XHCI_RING_ALIGNMENT);
        assert_eq!(ring.phys(&host) & 0x3f, 0);
    }

    #[test]
    fn event_ring_sync_current_calls_host_before_cpu_read() {
        let host = MockDma::default();
        let event_ring = super::EventRing::<MockDma>::new(&host, 8).expect("allocate event ring");
        host.share_calls.store(0, Ordering::Relaxed);
        host.last_share_vaddr.store(0, Ordering::Relaxed);
        host.last_share_len.store(0, Ordering::Relaxed);

        event_ring
            .sync_current_for_cpu(&host, "event-sync")
            .expect("sync current event trb");

        assert_eq!(host.share_calls.load(Ordering::Relaxed), 1);
        assert_eq!(
            host.last_share_vaddr.load(Ordering::Relaxed),
            event_ring.ring.virt()
        );
        assert_eq!(
            host.last_share_len.load(Ordering::Relaxed),
            core::mem::size_of::<Trb>()
        );
    }

    #[test]
    fn event_ring_debug_prefix_syncs_and_reads_without_dequeueing() {
        let host = MockDma::default();
        let event_ring = super::EventRing::<MockDma>::new(&host, 8).expect("allocate event ring");
        let first = Trb {
            param: 0x1122,
            status: 0x3344,
            control: trb_type::COMMAND_COMPLETION << 10,
        };
        // SAFETY: the test writes the first allocated TRB in an 8-entry event
        // ring before asking the debug accessor to read that same entry.
        unsafe {
            (event_ring.ring.as_ptr::<Trb>()).write(first);
        }
        host.share_calls.store(0, Ordering::Relaxed);
        host.last_share_vaddr.store(0, Ordering::Relaxed);
        host.last_share_len.store(0, Ordering::Relaxed);

        event_ring
            .sync_prefix_for_cpu(&host, 4, "event-prefix")
            .expect("sync event prefix");
        let (dequeue, cycle) = event_ring.debug_state();
        let observed = event_ring.debug_trb_at(0).expect("debug trb");

        assert_eq!(host.share_calls.load(Ordering::Relaxed), 1);
        assert_eq!(
            host.last_share_vaddr.load(Ordering::Relaxed),
            event_ring.ring.virt()
        );
        assert_eq!(
            host.last_share_len.load(Ordering::Relaxed),
            4 * core::mem::size_of::<Trb>()
        );
        assert_eq!((dequeue, cycle), (0, true));
        assert_eq!(observed.param, first.param);
        assert_eq!(observed.status, first.status);
        assert_eq!(observed.control, first.control);
    }

    #[test]
    fn event_ring_erst_uses_uboot_shaped_single_segment() {
        let host = MockDma::default();
        let event_ring = super::EventRing::<MockDma>::new(&host, 64).expect("allocate event ring");
        let entry = event_ring.erst.as_ptr::<super::ErstEntry>();

        assert_eq!(core::mem::size_of::<super::ErstEntry>(), 16);
        assert_eq!(event_ring.ring.align(), XHCI_RING_ALIGNMENT);
        assert_eq!(event_ring.ring.phys(&host) & 0x3f, 0);
        assert_eq!(event_ring.erst.align(), 64);
        assert_eq!(event_ring.erst.phys(&host) & 0x3f, 0);
        assert_eq!(event_ring.erst_entries(), 1);
        // SAFETY: EventRing::new initialized the first ERST entry.
        let observed = unsafe { entry.read() };
        assert_eq!(observed.base, event_ring.ring.phys(&host));
        assert_eq!(observed.size, 64);
    }

    #[test]
    fn event_ring_erst_entries_are_hardware_strided() {
        let host = MockDma::default();
        let event_ring = super::EventRing::<MockDma>::new(&host, 64).expect("allocate event ring");
        let bytes = event_ring.erst.as_ptr::<u8>();

        // SAFETY: each xHCI ERST entry is a 16-byte hardware record. The table
        // is page-sized and EventRing::new initialized the first record.
        let observed = unsafe { (bytes as *const super::ErstEntry).read_unaligned() };
        assert_eq!(observed.base, event_ring.ring.phys(&host));
        assert_eq!(observed.size, 64);
    }
}
