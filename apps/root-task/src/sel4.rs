// Author: Lukas Bower
#![cfg(target_os = "none")]
#![allow(dead_code)]
#![allow(clippy::missing_panics_doc)]
#![allow(unsafe_code)]

use core::mem;
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
const PAGE_SIZE: usize = 1 << PAGE_BITS;
const PAGE_TABLE_ALIGN: usize = 1 << 21;
const DMA_VADDR_BASE: usize = 0xB000_0000;
const MAX_PAGE_TABLES: usize = 64;

pub struct SlotAllocator {
    cnode: seL4_CNode,
    next: seL4_CPtr,
    end: seL4_CPtr,
    cnode_size_bits: seL4_Word,
}

impl SlotAllocator {
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
            next: region.start,
            end: region.end,
            cnode_size_bits,
        }
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

    pub fn depth(&self) -> seL4_Word {
        self.cnode_size_bits
    }

    pub fn root(&self) -> seL4_CNode {
        self.cnode
    }
}

struct ReservedUntyped {
    cap: seL4_Untyped,
    paddr: usize,
    size_bits: u8,
}

pub struct UntypedCatalog<'a> {
    bootinfo: &'a seL4_BootInfo,
    entries: &'a [UntypedDesc],
    used: Vec<usize, MAX_BOOTINFO_UNTYPEDS>,
}

impl<'a> UntypedCatalog<'a> {
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
        })
    }

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

    pub fn reserve_ram(&mut self, min_size_bits: u8) -> Option<ReservedUntyped> {
        for (index, desc) in self.entries.iter().enumerate() {
            if desc.isDevice != 0 || desc.sizeBits < min_size_bits || self.is_used(index) {
                continue;
            }
            return self.reserve_index(index);
        }
        None
    }
}

#[derive(Clone)]
pub struct DeviceFrame {
    cap: seL4_CPtr,
    paddr: usize,
    ptr: NonNull<u8>,
}

impl DeviceFrame {
    #[must_use]
    pub fn ptr(&self) -> NonNull<u8> {
        self.ptr
    }

    #[must_use]
    pub fn paddr(&self) -> usize {
        self.paddr
    }
}

#[derive(Clone)]
pub struct RamFrame {
    cap: seL4_CPtr,
    paddr: usize,
    ptr: NonNull<u8>,
}

impl RamFrame {
    #[must_use]
    pub fn ptr(&self) -> NonNull<u8> {
        self.ptr
    }

    #[must_use]
    pub fn paddr(&self) -> usize {
        self.paddr
    }

    #[must_use]
    pub fn cap(&self) -> seL4_CPtr {
        self.cap
    }

    #[must_use]
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.ptr.as_ptr(), PAGE_SIZE) }
    }

    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.ptr.as_ptr(), PAGE_SIZE) }
    }
}

pub struct KernelEnv<'a> {
    bootinfo: &'a seL4_BootInfo,
    slots: SlotAllocator,
    untyped: UntypedCatalog<'a>,
    mapped_pts: Vec<usize, MAX_PAGE_TABLES>,
    dma_cursor: usize,
}

impl<'a> KernelEnv<'a> {
    pub fn new(bootinfo: &'a seL4_BootInfo) -> Self {
        let slots = SlotAllocator::new(bootinfo.empty, bootinfo.initThreadCNodeSizeBits);
        let untyped = UntypedCatalog::new(bootinfo);
        Self {
            bootinfo,
            slots,
            untyped,
            mapped_pts: Vec::new(),
            dma_cursor: DMA_VADDR_BASE,
        }
    }

    pub fn bootinfo(&self) -> &'a seL4_BootInfo {
        self.bootinfo
    }

    pub fn allocate_slot(&mut self) -> seL4_CPtr {
        self.slots
            .alloc()
            .expect("cspace exhausted while allocating seL4 objects")
    }

    pub fn map_device(&mut self, paddr: usize) -> Result<DeviceFrame, seL4_Error> {
        let reserved = self
            .untyped
            .reserve_device(paddr, PAGE_BITS)
            .ok_or(seL4_NotEnoughMemory)?;
        let frame_slot = self.allocate_slot();
        self.retype_page(reserved.cap, frame_slot)?;
        self.map_frame(frame_slot, paddr, seL4_ARM_Page_Uncached)?;
        Ok(DeviceFrame {
            cap: frame_slot,
            paddr,
            ptr: NonNull::new(paddr as *mut u8).expect("device mapping address must be non-null"),
        })
    }

    pub fn alloc_dma_frame(&mut self) -> Result<RamFrame, seL4_Error> {
        let reserved = self
            .untyped
            .reserve_ram(PAGE_BITS as u8)
            .ok_or(seL4_NotEnoughMemory)?;
        let frame_slot = self.allocate_slot();
        self.retype_page(reserved.cap, frame_slot)?;
        let vaddr = self
            .dma_cursor
            .checked_add(PAGE_SIZE)
            .expect("dma cursor overflow (address space exhausted)")
            - PAGE_SIZE;
        self.dma_cursor += PAGE_SIZE;
        self.map_frame(frame_slot, vaddr, seL4_ARM_Page_Uncached)?;
        Ok(RamFrame {
            cap: frame_slot,
            paddr: reserved.paddr,
            ptr: NonNull::new(vaddr as *mut u8).expect("DMA mapping address must be non-null"),
        })
    }

    fn retype_page(
        &mut self,
        untyped_cap: seL4_Untyped,
        slot: seL4_CPtr,
    ) -> Result<(), seL4_Error> {
        let res = unsafe {
            seL4_Untyped_Retype(
                untyped_cap,
                seL4_ARM_SmallPageObject,
                0,
                self.slots.root(),
                slot,
                self.slots.depth(),
                0,
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
        slot: seL4_CPtr,
    ) -> Result<(), seL4_Error> {
        let res = unsafe {
            seL4_Untyped_Retype(
                untyped_cap,
                seL4_ARM_PageTableObject,
                0,
                self.slots.root(),
                slot,
                self.slots.depth(),
                0,
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
                    .reserve_ram(PAGE_BITS as u8)
                    .ok_or(seL4_NotEnoughMemory)?;
                let pt_slot = self.allocate_slot();
                self.retype_page_table(reserved.cap, pt_slot)?;
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
}
