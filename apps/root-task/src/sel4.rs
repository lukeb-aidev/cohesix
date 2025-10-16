// Author: Lukas Bower
//! seL4 resource management helpers for the root task.
#![cfg(any(test, target_os = "none"))]
#![allow(dead_code)]
#![allow(clippy::missing_panics_doc)]
#![allow(unsafe_code)]

use core::{mem, ptr::NonNull};

use heapless::Vec;
use sel4_sys::{
    seL4_ARM_PageTableObject, seL4_ARM_PageTable_Map, seL4_ARM_Page_Default, seL4_ARM_Page_Map,
    seL4_ARM_Page_Uncached, seL4_BootInfo, seL4_CNode, seL4_CNode_Delete, seL4_CPtr,
    seL4_CapRights_ReadWrite, seL4_NoError, seL4_NotEnoughMemory, seL4_ObjectType, seL4_SlotRegion,
    seL4_Untyped, seL4_Untyped_Retype, seL4_Word, UntypedDesc, MAX_BOOTINFO_UNTYPEDS,
};

fn objtype_name(t: seL4_Word) -> &'static str {
    use sel4_sys::seL4_ObjectType::*;
    if t == seL4_ARM_Page as seL4_Word {
        "seL4_ARM_Page"
    } else if t == seL4_ARM_PageTableObject as seL4_Word {
        "seL4_ARM_PageTableObject"
    } else {
        "<?>"
    }
}

#[cfg(all(target_os = "none", not(target_arch = "aarch64")))]
compile_error!("This path currently expects AArch64; wire correct ARM object types for your arch.");

const _: () = {
    let _check: [u8; core::mem::size_of::<seL4_Word>()] = [0; core::mem::size_of::<usize>()];
};

pub use sel4_sys::{seL4_CapInitThreadCNode, seL4_CapInitThreadVSpace, seL4_Error};

/// Extension trait exposing bootinfo fields and derived values used by the root task.
pub trait BootInfoExt {
    /// Returns the writable init thread CNode capability exposed via the initial CSpace root slot.
    fn init_cnode_cap(&self) -> seL4_CPtr;

    /// Returns the number of bits describing the capacity of the init thread's CSpace root.
    fn init_cnode_bits(&self) -> usize;

    /// Returns the first slot index within the bootinfo-declared empty slot window.
    fn empty_first_slot(&self) -> usize;

    /// Returns the exclusive upper bound of the bootinfo-declared empty slot window.
    fn empty_last_slot_excl(&self) -> usize;
}

impl BootInfoExt for seL4_BootInfo {
    #[inline(always)]
    fn init_cnode_cap(&self) -> seL4_CPtr {
        seL4_CapInitThreadCNode
    }

    #[inline(always)]
    fn init_cnode_bits(&self) -> usize {
        self.initThreadCNodeSizeBits as usize
    }

    #[inline(always)]
    fn empty_first_slot(&self) -> usize {
        self.empty.start as usize
    }

    #[inline(always)]
    fn empty_last_slot_excl(&self) -> usize {
        self.empty.end as usize
    }
}

/// Emits a concise dump of raw bootinfo parameters to aid debugging early boot wiring mistakes.
pub fn bootinfo_debug_dump(bi: &seL4_BootInfo) {
    let init_bits = bi.init_cnode_bits();
    log::info!(
        "[cohesix:root-task] bootinfo.raw: initCNode=0x{:x} initBits={} empty=[0x{:04x}..0x{:04x})",
        bi.init_cnode_cap(),
        init_bits,
        bi.empty_first_slot(),
        bi.empty_last_slot_excl()
    );
    assert!(
        init_bits > 0,
        "BootInfo.initThreadCNodeSizeBits is 0 — capacity invalid"
    );
}

const PAGE_BITS: usize = 12;
const PAGE_TABLE_BITS: usize = 12;
const PAGE_SIZE: usize = 1 << PAGE_BITS;
const PAGE_TABLE_ALIGN: usize = 1 << 21;
const PAGE_DIRECTORY_ALIGN: usize = 1 << 30;
const PAGE_UPPER_DIRECTORY_ALIGN: usize = 1 << 39;
const DEVICE_VADDR_BASE: usize = 0xA000_0000;
const DMA_VADDR_BASE: usize = 0xB000_0000;
const MAX_PAGE_TABLES: usize = 64;
const MAX_PAGE_DIRECTORIES: usize = 32;
const MAX_PAGE_UPPER_DIRECTORIES: usize = 8;
const WORD_BITS: seL4_Word = (mem::size_of::<seL4_Word>() * 8) as seL4_Word;

/// Simple bump allocator for CSpace slots rooted at the initial thread's CNode.
pub struct SlotAllocator {
    cnode: seL4_CNode,
    start: seL4_CPtr,
    next: seL4_CPtr,
    end: seL4_CPtr,
    cnode_size_bits: seL4_Word,
}

impl SlotAllocator {
    /// Creates a new allocator spanning the provided bootinfo slot region for the supplied root
    /// CNode capability.
    pub fn new(cnode: seL4_CNode, region: seL4_SlotRegion, cnode_size_bits: seL4_Word) -> Self {
        let capacity = 1usize
            .checked_shl(cnode_size_bits as u32)
            .unwrap_or(usize::MAX);
        debug_assert!(
            (region.end as usize) <= capacity,
            "bootinfo empty region exceeds root cnode capacity (end={:#x}, capacity={:#x}, bits={})",
            region.end,
            capacity,
            cnode_size_bits
        );
        Self {
            cnode,
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

    /// Returns the guard depth (in bits) of the root CNode capability.
    ///
    /// For the init thread's single-level CSpace this equals
    /// `bootinfo.initThreadCNodeSizeBits` and ensures capability paths resolve slots directly.
    #[inline(always)]
    pub fn depth(&self) -> seL4_Word {
        self.cnode_size_bits
    }

    /// Returns the number of bits describing the capacity of the root CNode.
    ///
    /// This mirrors `bootinfo.initThreadCNodeSizeBits` and reflects how many slots are
    /// addressable within the initial CSpace root.
    #[inline(always)]
    pub fn capacity_bits(&self) -> seL4_Word {
        self.cnode_size_bits
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
    page_tables: PageTableBookkeeper<MAX_PAGE_TABLES>,
    page_directories: PageDirectoryBookkeeper<MAX_PAGE_DIRECTORIES>,
    page_upper_directories: PageUpperDirectoryBookkeeper<MAX_PAGE_UPPER_DIRECTORIES>,
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
    /// Number of level-3 page tables currently mapped into the VSpace.
    pub page_tables_mapped: usize,
    /// Number of level-2 page directories currently mapped into the VSpace.
    pub page_directories_mapped: usize,
    /// Number of level-1 page upper directories currently mapped into the VSpace.
    pub page_upper_directories_mapped: usize,
    /// Summary of untyped catalogue utilisation.
    pub untyped: UntypedStats,
    /// Last observed retype attempt emitted by the environment.
    pub last_retype: Option<RetypeLog>,
}

/// Classification of the object that was being created during a retype attempt.
#[derive(Copy, Clone, Debug)]
pub enum RetypeKind {
    /// Device-mapped frame for MMIO peripherals.
    DevicePage {
        /// Physical base address of the targeted MMIO frame.
        paddr: usize,
    },
    /// DMA-capable RAM frame allocated for drivers.
    DmaPage {
        /// Physical base address of the RAM frame being retyped.
        paddr: usize,
    },
    /// Page table backing a virtual mapping.
    PageTable {
        /// Virtual base address of the page table's mapping range.
        vaddr: usize,
    },
    /// Page directory covering a 1 GiB region in the VSpace.
    PageDirectory {
        /// Virtual base address of the page directory's mapping range.
        vaddr: usize,
    },
    /// Page upper directory covering a 512 GiB region in the VSpace.
    PageUpperDirectory {
        /// Virtual base address of the page upper directory's mapping range.
        vaddr: usize,
    },
}

/// Detailed snapshot of the parameters used for a `seL4_Untyped_Retype` call.
///
/// The destination root **must** be the writable init thread CNode capability resident in slot
/// `seL4_CapInitThreadCNode`. Do not use allocator handles or read-only aliases. The init CSpace is
/// single-level, so retypes always traverse `(root=InitCNode, nodeIndex=InitCNode, nodeDepth=WORD_BITS)`
/// and select the final slot via `dest_offset`.
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
    /// Slot index within the selected CNode (root CNode policy: equals `dest_slot`).
    pub dest_offset: seL4_Word,
    /// `nodeDepth` argument supplied to `seL4_Untyped_Retype` while resolving the destination CNode.
    /// Root CNode policy: supply `WORD_BITS` so the kernel consumes the full guard width when
    /// selecting the writable root slot.
    pub cnode_depth: seL4_Word,
    /// `nodeIndex` argument supplied to `seL4_Untyped_Retype` when selecting a sub-CNode below
    /// `cnode_root`. Root CNode policy: MUST equal `seL4_CapInitThreadCNode`; legacy traces may pass
    /// 0 and are promoted automatically.
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
        let root_cnode_bits = bootinfo.init_cnode_bits();
        assert!(
            root_cnode_bits > 0,
            "BootInfo.initThreadCNodeSizeBits is 0 — capacity invalid"
        );
        let capacity = 1usize
            .checked_shl(root_cnode_bits as u32)
            .unwrap_or_else(|| {
                panic!(
                    "initThreadCNodeSizeBits {} exceeds host word size",
                    root_cnode_bits
                )
            });
        let empty_start = bootinfo.empty_first_slot();
        let empty_end = bootinfo.empty_last_slot_excl();
        let span = empty_end.saturating_sub(empty_start);
        log::info!(
            "[cohesix:root-task] bootinfo.empty slots [0x{start:04x}..0x{end:04x}) span={span} root_cnode_bits={bits}",
            start = empty_start,
            end = empty_end,
            span = span,
            bits = root_cnode_bits
        );
        assert!(
            empty_end <= capacity,
            "bootinfo empty region exceeds root cnode capacity (end={:#x}, capacity={:#x}, bits={})",
            empty_end,
            capacity,
            root_cnode_bits
        );

        let slots = SlotAllocator::new(
            bootinfo.init_cnode_cap(),
            bootinfo.empty,
            root_cnode_bits as seL4_Word,
        );
        let untyped = UntypedCatalog::new(bootinfo);
        Self {
            bootinfo,
            slots,
            untyped,
            page_tables: PageTableBookkeeper::new(),
            page_directories: PageDirectoryBookkeeper::new(),
            page_upper_directories: PageUpperDirectoryBookkeeper::new(),
            device_cursor: DEVICE_VADDR_BASE,
            dma_cursor: DMA_VADDR_BASE,
            last_retype: None,
        }
    }

    /// Returns the bootinfo pointer passed to the root task.
    pub fn bootinfo(&self) -> &'a seL4_BootInfo {
        self.bootinfo
    }

    /// Returns the writable init CNode capability published through bootinfo.
    #[inline(always)]
    pub fn init_cnode_cap(&self) -> seL4_CNode {
        self.bootinfo.init_cnode_cap()
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
            page_tables_mapped: self.page_tables.count(),
            page_directories_mapped: self.page_directories.count(),
            page_upper_directories_mapped: self.page_upper_directories.count(),
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
        #[cfg(target_arch = "aarch64")]
        let page_obj: seL4_Word = sel4_sys::seL4_ObjectType::seL4_ARM_Page as seL4_Word;
        #[cfg(target_arch = "aarch64")]
        let page_bits: seL4_Word = 12;

        #[cfg(not(target_arch = "aarch64"))]
        compile_error!("Wire correct page object type/size for non-AArch64 targets.");

        let dev_index = reserved.index();
        let dev_base_paddr = reserved.paddr();
        let dev_size_bits = reserved.size_bits();
        let dev_span = 1usize.checked_shl(dev_size_bits as u32).unwrap_or_else(|| {
            panic!(
                "device untyped size_bits {} exceeds host word size",
                dev_size_bits
            )
        });
        let dev_end_paddr = dev_base_paddr.saturating_add(dev_span);
        log::trace!(
            "device_untyped chosen: cap=0x{:x} idx={} covers=[0x{:08x}..0x{:08x}) size_bits={} target=0x{:08x}",
            reserved.cap(),
            dev_index,
            dev_base_paddr as u64,
            dev_end_paddr as u64,
            dev_size_bits,
            paddr as u64
        );

        let trace = self.prepare_retype_trace(
            &reserved,
            frame_slot,
            page_obj,
            page_bits,
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
            seL4_ObjectType::seL4_ARM_Page as seL4_Word,
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
        debug_assert!(
            matches!(
                trace.kind,
                RetypeKind::DevicePage { .. } | RetypeKind::DmaPage { .. }
            ),
            "retype_page expects a page-related trace"
        );
        debug_assert_eq!(
            trace.object_type,
            seL4_ObjectType::seL4_ARM_Page as seL4_Word,
            "ARM device/RAM frames must use seL4_ARM_Page",
        );
        debug_assert_eq!(
            trace.object_size_bits, PAGE_BITS as seL4_Word,
            "ARM device/RAM frames must have 4KiB size bits"
        );

        let (trace, _init_bits) = self.sanitise_retype_trace(*trace);
        log::trace!(
            "Retype → root=0x{:x} index={} depth={} offset=0x{:x} (objtype={}({}), size_bits={}, untyped_paddr=0x{:08x})",
            trace.cnode_root,
            trace.node_index,
            trace.cnode_depth,
            trace.dest_offset,
            trace.object_type,
            objtype_name(trace.object_type),
            trace.object_size_bits,
            trace.untyped_paddr,
        );

        #[cfg(target_arch = "aarch64")]
        if matches!(trace.kind, RetypeKind::DevicePage { .. }) {
            debug_assert_eq!(
                trace.object_type,
                sel4_sys::seL4_ObjectType::seL4_ARM_Page as seL4_Word,
                "Device page retype must use seL4_ARM_Page on AArch64"
            );
            debug_assert_eq!(
                trace.object_size_bits, 12,
                "AArch64 page size must be 12 bits (4 KiB)"
            );
        }

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
        let (trace, _init_bits) = self.sanitise_retype_trace(*trace);
        log::trace!(
            "Retype → root=0x{:x} index={} depth={} offset=0x{:x} (objtype={}({}), size_bits={}, untyped_paddr=0x{:08x})",
            trace.cnode_root,
            trace.node_index,
            trace.cnode_depth,
            trace.dest_offset,
            trace.object_type,
            objtype_name(trace.object_type),
            trace.object_size_bits,
            trace.untyped_paddr,
        );

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

    fn sanitise_retype_trace(&self, trace: RetypeTrace) -> (RetypeTrace, usize) {
        let init_bits = self.bootinfo.init_cnode_bits();
        let max_slots = 1usize.checked_shl(init_bits as u32).unwrap_or_else(|| {
            panic!(
                "initThreadCNodeSizeBits {} exceeds host word size",
                init_bits
            )
        });

        assert!(
            trace.node_index == 0,
            "Retype: node_index 0x{:x} must be zero when targeting the init CSpace root",
            trace.node_index
        );
        assert!(
            trace.cnode_depth == 0,
            "Retype: cnode_depth {} invalid for init CSpace root traversal (expected 0)",
            trace.cnode_depth
        );

        let mut sanitised = trace;
        sanitised.cnode_root = self.bootinfo.init_cnode_cap();
        sanitised.node_index = 0;
        sanitised.cnode_depth = 0;
        sanitised.dest_offset = sanitised.dest_slot as seL4_Word;

        assert_eq!(
            sanitised.cnode_root,
            self.bootinfo.init_cnode_cap(),
            "Retype: cnode_root must be the init CSpace root capability",
        );
        assert_eq!(
            sanitised.node_index, 0,
            "Retype: node_index must remain zero when inserting into the init CSpace root",
        );
        assert_eq!(
            sanitised.cnode_depth, 0,
            "Retype: cnode_depth must remain zero when inserting into the init CSpace root",
        );
        assert!(
            (sanitised.dest_offset as usize) < max_slots,
            "Retype: dest_offset 0x{:x} out of range (init_bits={}, max_slots={})",
            sanitised.dest_offset,
            init_bits,
            max_slots
        );

        (sanitised, init_bits)
    }

    fn map_frame(
        &mut self,
        frame_cap: seL4_CPtr,
        vaddr: usize,
        attr: sel4_sys::seL4_ARM_VMAttributes,
    ) -> Result<(), seL4_Error> {
        self.ensure_page_table(vaddr)?;
        let result = unsafe {
            seL4_ARM_Page_Map(
                frame_cap,
                seL4_CapInitThreadVSpace,
                vaddr,
                seL4_CapRights_ReadWrite,
                attr,
            )
        };

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

    fn ensure_page_table(&mut self, vaddr: usize) -> Result<(), seL4_Error> {
        self.ensure_page_directory(vaddr)?;
        let pt_base = PageTableBookkeeper::<MAX_PAGE_TABLES>::base_for(vaddr);
        if self.page_tables.contains_base(pt_base) {
            return Ok(());
        }

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
            seL4_ARM_PageTable_Map(
                pt_slot,
                seL4_CapInitThreadVSpace,
                pt_base,
                seL4_ARM_Page_Default,
            )
        };
        if map_res != seL4_NoError {
            self.record_retype(trace, RetypeStatus::Err(map_res));
            unsafe {
                let depth = self.bootinfo.init_cnode_bits() as seL4_Word;
                let _ = seL4_CNode_Delete(self.init_cnode_cap(), pt_slot, depth);
            }
            self.untyped.release(&reserved);
            return Err(map_res);
        }

        self.page_tables
            .remember_base(pt_base)
            .map_err(|_| seL4_NotEnoughMemory)?;
        Ok(())
    }

    fn ensure_page_directory(&mut self, vaddr: usize) -> Result<(), seL4_Error> {
        let pd_base = PageDirectoryBookkeeper::<MAX_PAGE_DIRECTORIES>::base_for(vaddr);
        if self.page_directories.contains_base(pd_base) {
            return Ok(());
        }

        self.ensure_page_upper_directory(vaddr)?;

        let reserved = self
            .untyped
            .reserve_ram(PAGE_TABLE_BITS as u8)
            .ok_or(seL4_NotEnoughMemory)?;
        let pd_slot = self.allocate_slot();
        let trace = self.prepare_retype_trace(
            &reserved,
            pd_slot,
            seL4_ARM_PageTableObject,
            PAGE_TABLE_BITS as seL4_Word,
            RetypeKind::PageDirectory { vaddr: pd_base },
        );
        self.record_retype(trace, RetypeStatus::Pending);
        if let Err(err) = self.retype_page_table(reserved.cap(), &trace) {
            self.record_retype(trace, RetypeStatus::Err(err));
            self.untyped.release(&reserved);
            return Err(err);
        }
        self.record_retype(trace, RetypeStatus::Ok);

        let map_res = unsafe {
            seL4_ARM_PageTable_Map(
                pd_slot,
                seL4_CapInitThreadVSpace,
                pd_base,
                seL4_ARM_Page_Default,
            )
        };
        if map_res != seL4_NoError {
            self.record_retype(trace, RetypeStatus::Err(map_res));
            unsafe {
                let depth = self.bootinfo.init_cnode_bits() as seL4_Word;
                let _ = seL4_CNode_Delete(self.init_cnode_cap(), pd_slot, depth);
            }
            self.untyped.release(&reserved);
            return Err(map_res);
        }

        self.page_directories
            .remember_base(pd_base)
            .map_err(|_| seL4_NotEnoughMemory)?;
        Ok(())
    }

    fn ensure_page_upper_directory(&mut self, vaddr: usize) -> Result<(), seL4_Error> {
        let pud_base = PageUpperDirectoryBookkeeper::<MAX_PAGE_UPPER_DIRECTORIES>::base_for(vaddr);
        if self.page_upper_directories.contains_base(pud_base) {
            return Ok(());
        }

        let reserved = self
            .untyped
            .reserve_ram(PAGE_TABLE_BITS as u8)
            .ok_or(seL4_NotEnoughMemory)?;
        let pud_slot = self.allocate_slot();
        let trace = self.prepare_retype_trace(
            &reserved,
            pud_slot,
            seL4_ARM_PageTableObject,
            PAGE_TABLE_BITS as seL4_Word,
            RetypeKind::PageUpperDirectory { vaddr: pud_base },
        );
        self.record_retype(trace, RetypeStatus::Pending);
        if let Err(err) = self.retype_page_table(reserved.cap(), &trace) {
            self.record_retype(trace, RetypeStatus::Err(err));
            self.untyped.release(&reserved);
            return Err(err);
        }
        self.record_retype(trace, RetypeStatus::Ok);

        let map_res = unsafe {
            seL4_ARM_PageTable_Map(
                pud_slot,
                seL4_CapInitThreadVSpace,
                pud_base,
                seL4_ARM_Page_Default,
            )
        };
        if map_res != seL4_NoError {
            self.record_retype(trace, RetypeStatus::Err(map_res));
            unsafe {
                let depth = self.bootinfo.init_cnode_bits() as seL4_Word;
                let _ = seL4_CNode_Delete(self.init_cnode_cap(), pud_slot, depth);
            }
            self.untyped.release(&reserved);
            return Err(map_res);
        }

        self.page_upper_directories
            .remember_base(pud_base)
            .map_err(|_| seL4_NotEnoughMemory)?;
        Ok(())
    }

    fn prepare_retype_trace(
        &mut self,
        reserved: &ReservedUntyped,
        slot: seL4_CPtr,
        object_type: seL4_Word,
        object_size_bits: seL4_Word,
        kind: RetypeKind,
    ) -> RetypeTrace {
        // Canonical: target the root CNode directly; put the destination slot in 'dest_offset'.
        // seL4 interprets `node_index`/`node_depth` as a traversal path beneath the supplied
        // `cnode_root`. Because we insert into the root CNode itself we must leave that path empty
        // (zero bits) so the kernel consumes the destination slot entirely via `dest_offset`.
        // Pointing `node_index` at the root slot triggers a redundant lookup that fails once the
        // guard bits are considered, yielding `seL4_FailedLookup`.
        let cnode_root = self.slots.root(); // seL4_CapInitThreadCNode
        let node_index = 0; // stay at the root CNode
        let cnode_depth = 0; // zero bits => no traversal beneath the root
        let dest_offset = slot as seL4_Word; // actual slot to fill
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

#[derive(Clone)]
struct TranslationBookkeeper<const N: usize, const ALIGN: usize> {
    entries: Vec<usize, N>,
}

impl<const N: usize, const ALIGN: usize> TranslationBookkeeper<N, ALIGN> {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    fn base_for(vaddr: usize) -> usize {
        debug_assert!(ALIGN.is_power_of_two());
        vaddr & !(ALIGN - 1)
    }

    fn contains_base(&self, base: usize) -> bool {
        self.entries.iter().any(|&value| value == base)
    }

    fn contains(&self, vaddr: usize) -> bool {
        let base = Self::base_for(vaddr);
        self.contains_base(base)
    }

    fn remember_base(&mut self, base: usize) -> Result<(), ()> {
        if self.contains_base(base) {
            return Ok(());
        }
        self.entries.push(base).map_err(|_| ())
    }

    fn forget_base(&mut self, base: usize) {
        if let Some(position) = self.entries.iter().position(|&value| value == base) {
            let _ = self.entries.swap_remove(position);
        }
    }

    fn count(&self) -> usize {
        self.entries.len()
    }
}

type PageTableBookkeeper<const N: usize> = TranslationBookkeeper<N, PAGE_TABLE_ALIGN>;
type PageDirectoryBookkeeper<const N: usize> = TranslationBookkeeper<N, PAGE_DIRECTORY_ALIGN>;
type PageUpperDirectoryBookkeeper<const N: usize> =
    TranslationBookkeeper<N, PAGE_UPPER_DIRECTORY_ALIGN>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_table_alignment_matches_two_meg_regions() {
        let base0 = PageTableBookkeeper::<4>::base_for(0xA000_1234);
        assert_eq!(base0, 0xA000_0000);
        let base1 = PageTableBookkeeper::<4>::base_for(0xA020_1000);
        assert_eq!(base1, 0xA020_0000);
    }

    #[test]
    fn page_directory_alignment_matches_one_gib_regions() {
        let base0 = PageDirectoryBookkeeper::<2>::base_for(0x4000_1000);
        assert_eq!(base0, 0x4000_0000);
        let base1 = PageDirectoryBookkeeper::<2>::base_for(0x7FFF_FFFF);
        assert_eq!(base1, 0x4000_0000);
    }

    #[test]
    fn page_upper_directory_alignment_matches_512_gib_regions() {
        let addr = 0x0002_0000_1000usize;
        let base = PageUpperDirectoryBookkeeper::<2>::base_for(addr);
        assert_eq!(base, 0x0002_0000_0000);
    }

    #[test]
    fn remember_base_deduplicates_entries() {
        let mut keeper: PageTableBookkeeper<2> = PageTableBookkeeper::new();
        let base = PageTableBookkeeper::<2>::base_for(0x1000);
        assert!(keeper.remember_base(base).is_ok());
        assert!(keeper.remember_base(base).is_ok());
        assert!(keeper.contains_base(base));
        assert_eq!(keeper.count(), 1);
    }

    #[test]
    fn remember_base_respects_capacity() {
        let mut keeper: PageTableBookkeeper<1> = PageTableBookkeeper::new();
        let base0 = PageTableBookkeeper::<1>::base_for(0x0);
        let base1 = PageTableBookkeeper::<1>::base_for(PAGE_TABLE_ALIGN);
        assert!(keeper.remember_base(base0).is_ok());
        assert!(keeper.remember_base(base1).is_err());
        assert!(keeper.contains_base(base0));
        assert_eq!(keeper.count(), 1);
    }

    #[test]
    fn contains_uses_alignment_when_tracking() {
        let mut keeper: PageTableBookkeeper<4> = PageTableBookkeeper::new();
        let base = PageTableBookkeeper::<4>::base_for(0xA000_0000);
        assert!(keeper.remember_base(base).is_ok());
        assert!(keeper.contains(0xA000_0ABC));
        assert!(keeper.contains(0xA001_FFFF));
        assert!(!keeper.contains(0xA002_0000));
    }

    #[test]
    fn retype_trace_targets_root_cnode_slot() {
        let mut bootinfo: seL4_BootInfo = unsafe { core::mem::zeroed() };
        bootinfo.empty = seL4_SlotRegion {
            start: 0,
            end: 1 << 13,
        };
        bootinfo.initThreadCNodeSizeBits = 13;
        let bootinfo_ref: &'static mut seL4_BootInfo = Box::leak(Box::new(bootinfo));
        let mut env = KernelEnv::new(bootinfo_ref);
        let reserved = ReservedUntyped {
            cap: 0x200,
            paddr: 0,
            size_bits: PAGE_BITS as u8,
            index: 0,
        };
        let slot: seL4_CPtr = 0x00c8;
        let trace = env.prepare_retype_trace(
            &reserved,
            slot,
            seL4_ObjectType::seL4_ARM_Page as seL4_Word,
            PAGE_BITS as seL4_Word,
            RetypeKind::DevicePage { paddr: 0 },
        );
        assert_eq!(trace.cnode_root, seL4_CapInitThreadCNode);
        assert_eq!(trace.node_index, 0);
        assert_eq!(trace.cnode_depth, 0);
        assert_eq!(trace.dest_offset, slot);
        assert_eq!(trace.dest_slot, slot);
    }

    #[test]
    fn bootinfo_capacity_bits_drive_cspace_math() {
        let mut bootinfo: seL4_BootInfo = unsafe { core::mem::zeroed() };
        bootinfo.initThreadCNodeSizeBits = 13;
        let init_bits = bootinfo.init_cnode_bits();
        assert_eq!(init_bits, 13);

        let capacity = 1usize << init_bits;
        assert_eq!(capacity, 8192);

        let empty_start = 0x00c8usize;
        let empty_end = 0x2000usize;
        assert!(empty_start < empty_end);
        assert!(empty_end <= capacity);
    }

    #[test]
    fn retype_bounds_use_bootinfo_bits_not_path_depth() {
        let mut bootinfo: seL4_BootInfo = unsafe { core::mem::zeroed() };
        bootinfo.empty = seL4_SlotRegion {
            start: 0,
            end: 1 << 13,
        };
        bootinfo.initThreadCNodeSizeBits = 13;
        let bootinfo_ref: &'static mut seL4_BootInfo = Box::leak(Box::new(bootinfo));
        let env = KernelEnv::new(bootinfo_ref);

        let slot: seL4_CPtr = 0x00c8;
        let trace = RetypeTrace {
            untyped_cap: 0x200,
            untyped_paddr: 0,
            untyped_size_bits: PAGE_BITS as u8,
            cnode_root: seL4_CapInitThreadCNode,
            dest_slot: slot,
            dest_offset: slot,
            cnode_depth: 0,
            node_index: seL4_CapInitThreadCNode,
            object_type: seL4_ObjectType::seL4_ARM_Page as seL4_Word,
            object_size_bits: PAGE_BITS as seL4_Word,
            kind: RetypeKind::DevicePage { paddr: 0 },
        };

        let (_, init_bits) = env.sanitise_retype_trace(trace);
        let max_slots = 1usize << init_bits;
        assert_eq!(init_bits, env.bootinfo().init_cnode_bits());
        assert!(slot as usize < max_slots);
    }

    #[test]
    fn retype_trace_is_root_slot() {
        let mut bootinfo: seL4_BootInfo = unsafe { core::mem::zeroed() };
        bootinfo.empty = seL4_SlotRegion {
            start: 0,
            end: 1 << 13,
        };
        bootinfo.initThreadCNodeSizeBits = 13;
        let bootinfo_ref: &'static mut seL4_BootInfo = Box::leak(Box::new(bootinfo));
        let env = KernelEnv::new(bootinfo_ref);

        let slot: seL4_CPtr = 0x0097;
        let trace = RetypeTrace {
            untyped_cap: 0x100,
            untyped_paddr: 0,
            untyped_size_bits: PAGE_BITS as u8,
            cnode_root: seL4_CapInitThreadCNode,
            dest_slot: slot,
            dest_offset: slot,
            cnode_depth: 13,
            node_index: 123,
            object_type: seL4_ObjectType::seL4_ARM_Page as seL4_Word,
            object_size_bits: PAGE_BITS as seL4_Word,
            kind: RetypeKind::DevicePage { paddr: 0 },
        };

        let (sanitised, init_bits) = env.sanitise_retype_trace(trace);
        assert_eq!(sanitised.node_index, 0);
        assert_eq!(sanitised.cnode_depth, 0);
        assert_eq!(sanitised.dest_offset, slot);
        assert_eq!(init_bits, 13);
    }

    #[test]
    fn sanitise_retype_trace_validates_offset_against_init_bits() {
        use std::panic::{self, AssertUnwindSafe};

        let mut bootinfo: seL4_BootInfo = unsafe { core::mem::zeroed() };
        bootinfo.empty = seL4_SlotRegion {
            start: 0,
            end: 1 << 13,
        };
        bootinfo.initThreadCNodeSizeBits = 13;
        let bootinfo_ref: &'static mut seL4_BootInfo = Box::leak(Box::new(bootinfo));
        let env = KernelEnv::new(bootinfo_ref);
        let valid_trace = RetypeTrace {
            untyped_cap: 0x100,
            untyped_paddr: 0,
            untyped_size_bits: PAGE_BITS as u8,
            cnode_root: seL4_CapInitThreadCNode,
            dest_slot: 0x1ff,
            dest_offset: 0x1ff,
            cnode_depth: 0,
            node_index: seL4_CapInitThreadCNode,
            object_type: seL4_ObjectType::seL4_ARM_Page as seL4_Word,
            object_size_bits: PAGE_BITS as seL4_Word,
            kind: RetypeKind::DmaPage { paddr: 0 },
        };

        let (_, init_bits) = env.sanitise_retype_trace(valid_trace);
        assert_eq!(init_bits, 13);

        let mut invalid_index = valid_trace;
        invalid_index.node_index = seL4_CapInitThreadCNode + 1;
        let index_check = panic::catch_unwind(AssertUnwindSafe(|| {
            env.sanitise_retype_trace(invalid_index);
        }));
        assert!(index_check.is_err());

        let mut invalid_depth = valid_trace;
        invalid_depth.cnode_depth = 1;
        let depth_check = panic::catch_unwind(AssertUnwindSafe(|| {
            env.sanitise_retype_trace(invalid_depth);
        }));
        assert!(depth_check.is_err());

        let mut invalid_offset = valid_trace;
        invalid_offset.dest_slot = 1 << 13;
        invalid_offset.dest_offset = 1 << 13;
        let offset_check = panic::catch_unwind(AssertUnwindSafe(|| {
            env.sanitise_retype_trace(invalid_offset);
        }));
        assert!(offset_check.is_err());
    }
}
