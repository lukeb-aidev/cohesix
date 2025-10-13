// Author: Lukas Bower
//! seL4 resource management helpers for the root task.
#![cfg(target_os = "none")]
#![allow(dead_code)]
#![allow(clippy::missing_panics_doc)]
#![allow(unsafe_code)]

use core::ptr::NonNull;

use heapless::Vec;
use sel4_sys::{
    seL4_ARM_PageTableObject, seL4_ARM_PageTable_Map, seL4_ARM_Page_Map, seL4_ARM_Page_Uncached,
    seL4_ARM_SmallPageObject, seL4_BootInfo, seL4_CNode, seL4_CPtr, seL4_CapRights_ReadWrite,
    seL4_FailedLookup, seL4_NoError, seL4_NotEnoughMemory, seL4_SlotRegion, seL4_Untyped,
    seL4_Untyped_Retype, seL4_Word, UntypedDesc, MAX_BOOTINFO_UNTYPEDS,
};

pub use sel4_sys::{seL4_CapInitThreadCNode, seL4_CapInitThreadVSpace, seL4_Error};

const PAGE_BITS: usize = 12;
const PAGE_TABLE_BITS: usize = 12;
const PAGE_SIZE: usize = 1 << PAGE_BITS;
const PAGE_TABLE_ALIGN: usize = 1 << 21;
const DEVICE_VADDR_BASE: usize = 0xA000_0000;
const DMA_VADDR_BASE: usize = 0xB000_0000;
const MAX_PAGE_TABLES: usize = 64;

/// Simple bump allocator for CSpace slots rooted at the initial thread's CNode.
pub struct SlotAllocator {
    cnode: seL4_CNode,
    start: seL4_CPtr,
    next: seL4_CPtr,
    end: seL4_CPtr,
    cnode_size_bits: seL4_Word,
}

impl SlotAllocator {
    /// Creates a new allocator spanning the provided bootinfo slot region.
    pub fn new(region: seL4_SlotRegion, cnode_size_bits: seL4_Word) -> Self {
        let capacity = 1usize
            .checked_shl(cnode_size_bits as u32)
            .unwrap_or(usize::MAX);
        debug_assert!(
            (region.end as usize) <= capacity,
            "bootinfo empty region exceeds root cnode capacity",
        );
        Self {
            cnode: seL4_CapInitThreadCNode,
            start: region.start,
            next: region.start,
            end: region.end,
            cnode_size_bits,
        }
    }

    /// Returns the number of free slots remaining in the allocator.
    #[must_use]
    pub fn remaining(&self) -> usize {
        (self.end - self.next) as usize
    }

    /// Returns the total capacity of the allocator in slots.
    #[must_use]
    pub fn capacity(&self) -> usize {
        (self.end - self.start) as usize
    }

    /// Returns the number of slots that have already been handed out.
    #[must_use]
    pub fn used(&self) -> usize {
        self.capacity().saturating_sub(self.remaining())
    }

    fn alloc(&mut self) -> Option<seL4_CPtr> {
        if self.next >= self.end {
            return None;
        }
        let slot = self.next;
        self.next += 1;
        let capacity = 1usize
            .checked_shl(self.cnode_size_bits as u32)
            .unwrap_or(usize::MAX);
        debug_assert!(
            (slot as usize) < capacity,
            "allocated cspace slot exceeds root cnode capacity",
        );
        Some(slot)
    }

    /// Returns the root CNode capability backing allocations.
    pub fn root(&self) -> seL4_CNode {
        self.cnode
    }

    /// Returns the depth of the root CNode in bits.
    #[inline(always)]
    pub fn depth(&self) -> seL4_Word {
        self.cnode_size_bits
    }

    /// Computes the slot offset within the root CNode for the provided capability pointer.
    #[inline(always)]
    pub fn slot_offset(&self, slot: seL4_CPtr) -> seL4_Word {
        let limit = 1usize
            .checked_shl(self.cnode_size_bits as u32)
            .expect("cnode size bits overflow while computing slot offset");
        let index = slot as usize;
        debug_assert!(
            index < limit,
            "cspace slot index {index} exceeds root cnode capacity {limit}",
        );
        index as seL4_Word
    }
}

/// Handle to an untyped capability reserved from the bootinfo catalog.
pub struct ReservedUntyped {
    cap: seL4_Untyped,
    paddr: usize,
    size_bits: u8,
    index: usize,
}

impl ReservedUntyped {
    /// Returns the capability slot referencing the reserved untyped.
    #[must_use]
    pub fn cap(&self) -> seL4_Untyped {
        self.cap
    }

    /// Returns the physical address backing the untyped capability.
    #[must_use]
    pub fn paddr(&self) -> usize {
        self.paddr
    }

    /// Returns the size of the reserved region in bits.
    #[must_use]
    pub fn size_bits(&self) -> u8 {
        self.size_bits
    }

    /// Returns the index within the bootinfo untyped list.
    #[must_use]
    pub fn index(&self) -> usize {
        self.index
    }
}

/// Summary of untyped capability utilisation available to the root task.
#[derive(Copy, Clone, Debug)]
pub struct UntypedStats {
    /// Total number of untyped capabilities exported by the kernel.
    pub total: usize,
    /// Number of untyped capabilities that have been reserved so far.
    pub used: usize,
    /// Number of device-tagged untyped capabilities.
    pub device_total: usize,
    /// Number of device-tagged untyped capabilities that have been consumed.
    pub device_used: usize,
}

/// Diagnostic view describing a device untyped region that covers a physical range.
#[derive(Copy, Clone, Debug)]
pub struct DeviceCoverage {
    /// Physical base address of the underlying untyped region.
    pub base: usize,
    /// Exclusive upper bound of the untyped region.
    pub limit: usize,
    /// Size of the untyped region in bits.
    pub size_bits: u8,
    /// Index of the region within the bootinfo untyped list.
    pub index: usize,
    /// Indicates whether the region has already been reserved.
    pub used: bool,
}

/// Index of bootinfo-provided untyped capabilities available to the root task.
pub struct UntypedCatalog<'a> {
    bootinfo: &'a seL4_BootInfo,
    entries: &'a [UntypedDesc],
    used: Vec<usize, MAX_BOOTINFO_UNTYPEDS>,
}

impl<'a> UntypedCatalog<'a> {
    /// Creates a catalog view over the untyped list exported by seL4.
    pub fn new(bootinfo: &'a seL4_BootInfo) -> Self {
        let count = bootinfo.untyped.end - bootinfo.untyped.start;
        let entries = &bootinfo.untypedList[..count as usize];
        Self {
            bootinfo,
            entries,
            used: Vec::new(),
        }
    }

    fn is_used(&self, index: usize) -> bool {
        self.used.iter().any(|&value| value == index)
    }

    fn reserve_index(&mut self, index: usize) -> Option<ReservedUntyped> {
        if self.is_used(index) {
            return None;
        }
        self.used.push(index).ok()?;
        let desc = &self.entries[index];
        Some(ReservedUntyped {
            cap: self.bootinfo.untyped.start + index as seL4_CPtr,
            paddr: desc.paddr as usize,
            size_bits: desc.sizeBits,
            index,
        })
    }

    /// Reserves an untyped covering the supplied device physical address range.
    pub fn reserve_device(&mut self, paddr: usize, size_bits: usize) -> Option<ReservedUntyped> {
        let end = paddr.saturating_add(1usize << size_bits);
        for (index, desc) in self.entries.iter().enumerate() {
            if desc.isDevice == 0 || self.is_used(index) {
                continue;
            }
            let base = desc.paddr as usize;
            let limit = base.saturating_add(1usize << desc.sizeBits);
            if base <= paddr && end <= limit {
                return self.reserve_index(index);
            }
        }
        None
    }

    /// Reserves the first RAM untyped meeting the requested size.
    pub fn reserve_ram(&mut self, min_size_bits: u8) -> Option<ReservedUntyped> {
        for (index, desc) in self.entries.iter().enumerate() {
            if desc.isDevice != 0 || desc.sizeBits < min_size_bits || self.is_used(index) {
                continue;
            }
            return self.reserve_index(index);
        }
        None
    }

    fn release_index(&mut self, index: usize) {
        if let Some(position) = self.used.iter().position(|&value| value == index) {
            let _ = self.used.swap_remove(position);
        }
    }

    /// Releases a previously reserved untyped so it may be reused.
    pub fn release(&mut self, reserved: &ReservedUntyped) {
        self.release_index(reserved.index);
    }

    /// Returns diagnostic statistics describing untyped catalogue utilisation.
    #[must_use]
    pub fn stats(&self) -> UntypedStats {
        let total = self.entries.len();
        let used = self.used.len();
        let device_total = self
            .entries
            .iter()
            .filter(|desc| desc.isDevice != 0)
            .count();
        let device_used = self
            .used
            .iter()
            .filter(|&&index| {
                self.entries
                    .get(index)
                    .map_or(false, |desc| desc.isDevice != 0)
            })
            .count();
        UntypedStats {
            total,
            used,
            device_total,
            device_used,
        }
    }

    /// Locates the device untyped covering the requested physical range, if available.
    #[must_use]
    pub fn device_coverage(&self, paddr: usize, size_bits: usize) -> Option<DeviceCoverage> {
        let end = paddr.saturating_add(1usize << size_bits);
        self.entries.iter().enumerate().find_map(|(index, desc)| {
            if desc.isDevice == 0 {
                return None;
            }
            let base = desc.paddr as usize;
            let limit = base.saturating_add(1usize << desc.sizeBits);
            if base <= paddr && end <= limit {
                Some(DeviceCoverage {
                    base,
                    limit,
                    size_bits: desc.sizeBits,
                    index,
                    used: self.is_used(index),
                })
            } else {
                None
            }
        })
    }
}

/// Virtual mapping of a physical device frame.
#[derive(Clone)]
pub struct DeviceFrame {
    cap: seL4_CPtr,
    paddr: usize,
    ptr: NonNull<u8>,
}

impl DeviceFrame {
    /// Returns the capability referencing this frame.
    #[must_use]
    pub fn cap(&self) -> seL4_CPtr {
        self.cap
    }

    /// Returns the virtual pointer to the mapped frame.
    #[must_use]
    pub fn ptr(&self) -> NonNull<u8> {
        self.ptr
    }

    /// Returns the physical address backing the device frame.
    #[must_use]
    pub fn paddr(&self) -> usize {
        self.paddr
    }
}

/// Virtual mapping of DMA-capable RAM used for driver buffers.
#[derive(Clone)]
pub struct RamFrame {
    cap: seL4_CPtr,
    paddr: usize,
    ptr: NonNull<u8>,
}

impl RamFrame {
    /// Returns the virtual pointer to the mapped RAM.
    #[must_use]
    pub fn ptr(&self) -> NonNull<u8> {
        self.ptr
    }

    /// Returns the physical address for DMA.
    #[must_use]
    pub fn paddr(&self) -> usize {
        self.paddr
    }

    /// Returns the capability referencing this RAM frame.
    #[must_use]
    pub fn cap(&self) -> seL4_CPtr {
        self.cap
    }

    /// Returns the frame as a mutable byte slice covering one page.
    #[must_use]
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.ptr.as_ptr(), PAGE_SIZE) }
    }

    /// Returns the frame as an immutable byte slice covering one page.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.ptr.as_ptr(), PAGE_SIZE) }
    }
}

/// Aggregates bootinfo-derived allocators and helpers for the root task.
pub struct KernelEnv<'a> {
    bootinfo: &'a seL4_BootInfo,
    slots: SlotAllocator,
    untyped: UntypedCatalog<'a>,
    mapped_pts: Vec<usize, MAX_PAGE_TABLES>,
    device_cursor: usize,
    dma_cursor: usize,
    last_retype: Option<RetypeLog>,
}

/// Diagnostic snapshot capturing resource utilisation within the [`KernelEnv`].
#[derive(Copy, Clone, Debug)]
pub struct KernelEnvSnapshot {
    /// Virtual base of the device-mapping window.
    pub device_base: usize,
    /// Virtual cursor indicating the next free device mapping address.
    pub device_cursor: usize,
    /// Virtual base of the DMA window.
    pub dma_base: usize,
    /// Virtual cursor indicating the next free DMA mapping address.
    pub dma_cursor: usize,
    /// Capability designating the root CNode supplied to retype operations.
    pub cspace_root: seL4_CNode,
    /// Guard depth (in bits) associated with the root CNode capability.
    pub cspace_root_depth: seL4_Word,
    /// Total number of CSpace slots managed by the allocator.
    pub cspace_capacity: usize,
    /// Number of CSpace slots handed out so far.
    pub cspace_used: usize,
    /// Number of CSpace slots remaining for future allocations.
    pub cspace_remaining: usize,
    /// Summary of untyped catalogue utilisation.
    pub untyped: UntypedStats,
    /// Last observed retype attempt emitted by the environment.
    pub last_retype: Option<RetypeLog>,
}

/// Classification of the object that was being created during a retype attempt.
#[derive(Copy, Clone, Debug)]
pub enum RetypeKind {
    /// Device-mapped frame for MMIO peripherals.
    DevicePage { paddr: usize },
    /// DMA-capable RAM frame allocated for drivers.
    DmaPage { paddr: usize },
    /// Page table backing a virtual mapping.
    PageTable { vaddr: usize },
}

/// Detailed snapshot of the parameters used for a `seL4_Untyped_Retype` call.
#[derive(Copy, Clone, Debug)]
pub struct RetypeTrace {
    /// Capability designating the source untyped region.
    pub untyped_cap: seL4_Untyped,
    /// Physical base address advertised by the untyped descriptor.
    pub untyped_paddr: usize,
    /// Size (in bits) of the backing untyped region.
    pub untyped_size_bits: u8,
    /// Capability designating the root CNode supplied to the kernel.
    pub cnode_root: seL4_CNode,
    /// Destination slot selected for the newly created object.
    pub dest_slot: seL4_CPtr,
    /// Offset within the root CNode calculated for the destination slot.
    pub dest_offset: seL4_Word,
    /// Depth of the root CNode used for the allocation.
    pub cnode_depth: seL4_Word,
    /// Index supplied to the kernel when resolving the destination CNode.
    pub node_index: seL4_Word,
    /// Object type requested from the kernel.
    pub object_type: seL4_Word,
    /// Object size (in bits) supplied to the kernel.
    pub object_size_bits: seL4_Word,
    /// High-level description of the object being materialised.
    pub kind: RetypeKind,
}

/// Result marker describing whether the most recent retype succeeded.
#[derive(Copy, Clone, Debug)]
pub enum RetypeStatus {
    /// A retype call has not yet completed.
    Pending,
    /// The retype call completed successfully.
    Ok,
    /// The retype call failed with the captured error code.
    Err(seL4_Error),
}

/// Log entry capturing the trace and outcome for the latest retype attempt.
#[derive(Copy, Clone, Debug)]
pub struct RetypeLog {
    /// Parameters passed to the kernel.
    pub trace: RetypeTrace,
    /// Outcome returned by the kernel.
    pub status: RetypeStatus,
}

impl<'a> KernelEnv<'a> {
    /// Builds a new environment from the seL4 bootinfo struct.
    pub fn new(bootinfo: &'a seL4_BootInfo) -> Self {
        let slots = SlotAllocator::new(bootinfo.empty, bootinfo.initThreadCNodeSizeBits);
        let untyped = UntypedCatalog::new(bootinfo);
        Self {
            bootinfo,
            slots,
            untyped,
            mapped_pts: Vec::new(),
            device_cursor: DEVICE_VADDR_BASE,
            dma_cursor: DMA_VADDR_BASE,
            last_retype: None,
        }
    }

    /// Returns the bootinfo pointer passed to the root task.
    pub fn bootinfo(&self) -> &'a seL4_BootInfo {
        self.bootinfo
    }

    /// Produces a diagnostic snapshot describing allocator state.
    #[must_use]
    pub fn snapshot(&self) -> KernelEnvSnapshot {
        let cspace_capacity = self.slots.capacity();
        let cspace_remaining = self.slots.remaining();
        KernelEnvSnapshot {
            device_base: DEVICE_VADDR_BASE,
            device_cursor: self.device_cursor,
            dma_base: DMA_VADDR_BASE,
            dma_cursor: self.dma_cursor,
            cspace_root: self.slots.root(),
            cspace_root_depth: self.slots.depth(),
            cspace_capacity,
            cspace_used: self.slots.used(),
            cspace_remaining,
            untyped: self.untyped.stats(),
            last_retype: self.last_retype,
        }
    }

    /// Returns the device untyped covering the supplied range, if any, without reserving it.
    #[must_use]
    pub fn device_coverage(&self, paddr: usize, size_bits: usize) -> Option<DeviceCoverage> {
        self.untyped.device_coverage(paddr, size_bits)
    }

    /// Allocates a new CSpace slot, panicking if the root CNode is exhausted.
    pub fn allocate_slot(&mut self) -> seL4_CPtr {
        self.slots
            .alloc()
            .expect("cspace exhausted while allocating seL4 objects")
    }

    /// Maps a physical device frame into the root task's device window.
    pub fn map_device(&mut self, paddr: usize) -> Result<DeviceFrame, seL4_Error> {
        let reserved = self
            .untyped
            .reserve_device(paddr, PAGE_BITS)
            .ok_or(seL4_NotEnoughMemory)?;
        let frame_slot = self.allocate_slot();
        let trace = self.prepare_retype_trace(
            &reserved,
            frame_slot,
            seL4_ARM_SmallPageObject,
            PAGE_BITS as seL4_Word,
            RetypeKind::DevicePage { paddr },
        );
        self.record_retype(trace, RetypeStatus::Pending);
        if let Err(err) = self.retype_page(reserved.cap(), &trace) {
            self.record_retype(trace, RetypeStatus::Err(err));
            self.untyped.release(&reserved);
            return Err(err);
        }
        self.record_retype(trace, RetypeStatus::Ok);
        let vaddr = self
            .device_cursor
            .checked_add(PAGE_SIZE)
            .expect("device cursor overflow (address space exhausted)")
            - PAGE_SIZE;
        self.device_cursor += PAGE_SIZE;
        self.map_frame(frame_slot, vaddr, seL4_ARM_Page_Uncached)?;
        Ok(DeviceFrame {
            cap: frame_slot,
            paddr,
            ptr: NonNull::new(vaddr as *mut u8).expect("device mapping address must be non-null"),
        })
    }

    /// Allocates a DMA-capable frame of RAM and maps it into the DMA window.
    pub fn alloc_dma_frame(&mut self) -> Result<RamFrame, seL4_Error> {
        let reserved = self
            .untyped
            .reserve_ram(PAGE_BITS as u8)
            .ok_or(seL4_NotEnoughMemory)?;
        let frame_slot = self.allocate_slot();
        let trace = self.prepare_retype_trace(
            &reserved,
            frame_slot,
            seL4_ARM_SmallPageObject,
            PAGE_BITS as seL4_Word,
            RetypeKind::DmaPage {
                paddr: reserved.paddr(),
            },
        );
        self.record_retype(trace, RetypeStatus::Pending);
        if let Err(err) = self.retype_page(reserved.cap(), &trace) {
            self.record_retype(trace, RetypeStatus::Err(err));
            self.untyped.release(&reserved);
            return Err(err);
        }
        self.record_retype(trace, RetypeStatus::Ok);
        let vaddr = self
            .dma_cursor
            .checked_add(PAGE_SIZE)
            .expect("dma cursor overflow (address space exhausted)")
            - PAGE_SIZE;
        self.dma_cursor += PAGE_SIZE;
        self.map_frame(frame_slot, vaddr, seL4_ARM_Page_Uncached)?;
        Ok(RamFrame {
            cap: frame_slot,
            paddr: reserved.paddr(),
            ptr: NonNull::new(vaddr as *mut u8).expect("DMA mapping address must be non-null"),
        })
    }

    fn retype_page(
        &mut self,
        untyped_cap: seL4_Untyped,
        trace: &RetypeTrace,
    ) -> Result<(), seL4_Error> {
        let res = unsafe {
            seL4_Untyped_Retype(
                untyped_cap,
                trace.object_type,
                trace.object_size_bits,
                trace.cnode_root,
                trace.node_index,
                trace.cnode_depth,
                trace.dest_offset,
                1,
            )
        };
        if res == seL4_NoError {
            Ok(())
        } else {
            Err(res)
        }
    }

    fn retype_page_table(
        &mut self,
        untyped_cap: seL4_Untyped,
        trace: &RetypeTrace,
    ) -> Result<(), seL4_Error> {
        let res = unsafe {
            seL4_Untyped_Retype(
                untyped_cap,
                trace.object_type,
                trace.object_size_bits,
                trace.cnode_root,
                trace.node_index,
                trace.cnode_depth,
                trace.dest_offset,
                1,
            )
        };
        if res == seL4_NoError {
            Ok(())
        } else {
            Err(res)
        }
    }

    fn map_frame(
        &mut self,
        frame_cap: seL4_CPtr,
        vaddr: usize,
        attr: sel4_sys::seL4_ARM_VMAttributes,
    ) -> Result<(), seL4_Error> {
        let mut result = unsafe {
            seL4_ARM_Page_Map(
                frame_cap,
                seL4_CapInitThreadVSpace,
                vaddr,
                seL4_CapRights_ReadWrite,
                attr,
            )
        };

        if result == seL4_FailedLookup {
            let pt_base = Self::align_down(vaddr, PAGE_TABLE_ALIGN);
            if !self.mapped_pts.iter().any(|&addr| addr == pt_base) {
                let reserved = self
                    .untyped
                    .reserve_ram(PAGE_TABLE_BITS as u8)
                    .ok_or(seL4_NotEnoughMemory)?;
                let pt_slot = self.allocate_slot();
                let trace = self.prepare_retype_trace(
                    &reserved,
                    pt_slot,
                    seL4_ARM_PageTableObject,
                    PAGE_TABLE_BITS as seL4_Word,
                    RetypeKind::PageTable { vaddr: pt_base },
                );
                self.record_retype(trace, RetypeStatus::Pending);
                if let Err(err) = self.retype_page_table(reserved.cap(), &trace) {
                    self.record_retype(trace, RetypeStatus::Err(err));
                    self.untyped.release(&reserved);
                    return Err(err);
                }
                self.record_retype(trace, RetypeStatus::Ok);
                let map_res = unsafe {
                    seL4_ARM_PageTable_Map(pt_slot, seL4_CapInitThreadVSpace, pt_base, attr)
                };
                if map_res != seL4_NoError {
                    return Err(map_res);
                }
                let _ = self.mapped_pts.push(pt_base);
            }
            result = unsafe {
                seL4_ARM_Page_Map(
                    frame_cap,
                    seL4_CapInitThreadVSpace,
                    vaddr,
                    seL4_CapRights_ReadWrite,
                    attr,
                )
            };
        }

        if result == seL4_NoError {
            Ok(())
        } else {
            Err(result)
        }
    }

    fn align_down(value: usize, align: usize) -> usize {
        debug_assert!(align.is_power_of_two());
        value & !(align - 1)
    }

    fn prepare_retype_trace(
        &mut self,
        reserved: &ReservedUntyped,
        slot: seL4_CPtr,
        object_type: seL4_Word,
        object_size_bits: seL4_Word,
        kind: RetypeKind,
    ) -> RetypeTrace {
        let dest_offset = self.slots.slot_offset(slot);
        let cnode_root = self.slots.root();
        let node_index = cnode_root as seL4_Word;
        let cnode_depth = self.slots.depth();
        RetypeTrace {
            untyped_cap: reserved.cap(),
            untyped_paddr: reserved.paddr(),
            untyped_size_bits: reserved.size_bits(),
            cnode_root,
            dest_slot: slot,
            dest_offset,
            cnode_depth,
            node_index,
            object_type,
            object_size_bits,
            kind,
        }
    }

    fn record_retype(&mut self, trace: RetypeTrace, status: RetypeStatus) {
        self.last_retype = Some(RetypeLog { trace, status });
    }
}
