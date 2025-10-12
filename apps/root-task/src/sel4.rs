// Author: Lukas Bower
#![cfg(target_os = "none")]
#![allow(dead_code)]
#![allow(clippy::missing_panics_doc)]

use core::ptr::NonNull;

use heapless::Vec;
use sel4_sys::{
    seL4_ARM_Page, seL4_ARM_PageTable, seL4_ARM_PageTable_Map, seL4_ARM_Page_Map,
    seL4_ARM_Page_Uncached, seL4_ARM_SmallPageObject, seL4_BootInfo, seL4_CNode, seL4_CPtr,
    seL4_CapInitThreadCNode, seL4_CapInitThreadVSpace, seL4_CapRights_ReadWrite, seL4_Error,
    seL4_FailedLookup, seL4_NoError, seL4_NotEnoughMemory, seL4_SlotRegion, seL4_Untyped,
    seL4_Untyped_Retype, seL4_Word, UntypedDesc,
};

const PAGE_BITS: usize = 12;
const MAX_UNTYPEDS: usize = sel4_sys::MAX_BOOTINFO_UNTYPEDS;
const PAGE_TABLE_ALIGN: usize = 1 << 21;
const MAX_PAGE_TABLES: usize = 64;

#[derive(Debug)]
pub struct SlotAllocator {
    cnode: seL4_CNode,
    next: seL4_CPtr,
    end: seL4_CPtr,
    depth: seL4_Word,
}

impl SlotAllocator {
    pub fn new(region: seL4_SlotRegion, cnode_size_bits: seL4_Word) -> Self {
        Self {
            cnode: seL4_CapInitThreadCNode,
            next: region.start,
            end: region.end,
            depth: cnode_size_bits,
        }
    }

    fn alloc(&mut self) -> Option<seL4_CPtr> {
        if self.next >= self.end {
            return None;
        }
        let slot = self.next;
        self.next += 1;
        Some(slot)
    }

    pub fn depth(&self) -> seL4_Word {
        self.depth
    }

    pub fn root(&self) -> seL4_CNode {
        self.cnode
    }
}

#[derive(Debug)]
pub struct UntypedCatalog<'a> {
    bootinfo: &'a seL4_BootInfo,
    entries: &'a [UntypedDesc],
    used: Vec<usize, MAX_UNTYPEDS>,
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

    fn reserve_index(&mut self, index: usize) -> Option<seL4_Untyped> {
        if self.is_used(index) {
            return None;
        }
        self.used.push(index).ok()?;
        Some(self.bootinfo.untyped.start + index as seL4_CPtr)
    }

    pub fn reserve_device(&mut self, paddr: usize, size_bits: usize) -> Option<seL4_Untyped> {
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

    pub fn reserve_ram(&mut self, min_size_bits: u8) -> Option<seL4_Untyped> {
        for (index, desc) in self.entries.iter().enumerate() {
            if desc.isDevice != 0 || desc.sizeBits < min_size_bits || self.is_used(index) {
                continue;
            }
            return self.reserve_index(index);
        }
        None
    }
}

#[derive(Debug)]
pub struct DeviceFrame {
    pub cap: seL4_CPtr,
    ptr: NonNull<u8>,
}

impl DeviceFrame {
    #[must_use]
    pub fn ptr(&self) -> NonNull<u8> {
        self.ptr
    }
}

pub struct KernelEnv<'a> {
    bootinfo: &'a seL4_BootInfo,
    slots: SlotAllocator,
    untyped: UntypedCatalog<'a>,
    mapped_pts: Vec<usize, MAX_PAGE_TABLES>,
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
        }
    }

    pub fn bootinfo(&self) -> &'a seL4_BootInfo {
        self.bootinfo
    }

    pub fn allocate_slot(&mut self) -> seL4_CPtr {
        self.slots
            .alloc()
            .expect("out of CSpace slots for root task")
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
                sel4_sys::seL4_ARM_PageTableObject,
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

    fn align_down(value: usize, align: usize) -> usize {
        debug_assert!(align.is_power_of_two());
        value & !(align - 1)
    }

    pub fn map_device(&mut self, paddr: usize) -> Result<DeviceFrame, seL4_Error> {
        let untyped_cap = self
            .untyped
            .reserve_device(paddr, PAGE_BITS)
            .ok_or(seL4_NotEnoughMemory)?;
        let frame_slot = self.allocate_slot();
        self.retype_page(untyped_cap, frame_slot)?;
        self.map_frame(frame_slot, paddr)?;
        Ok(DeviceFrame {
            cap: frame_slot,
            ptr: NonNull::new(paddr as *mut u8).expect("device mappings must be non-null"),
        })
    }

    fn map_frame(&mut self, frame_cap: seL4_CPtr, vaddr: usize) -> Result<(), seL4_Error> {
        let mut result = unsafe {
            seL4_ARM_Page_Map(
                frame_cap,
                seL4_CapInitThreadVSpace,
                vaddr,
                seL4_CapRights_ReadWrite,
                seL4_ARM_Page_Uncached,
            )
        };

        if result == seL4_FailedLookup {
            let pt_base = Self::align_down(vaddr, PAGE_TABLE_ALIGN);
            if !self.mapped_pts.iter().any(|&addr| addr == pt_base) {
                let pt_untyped = self
                    .untyped
                    .reserve_ram(PAGE_BITS as u8)
                    .ok_or(seL4_NotEnoughMemory)?;
                let pt_slot = self.allocate_slot();
                self.retype_page_table(pt_untyped, pt_slot)?;
                let map_res = unsafe {
                    seL4_ARM_PageTable_Map(
                        pt_slot,
                        seL4_CapInitThreadVSpace,
                        pt_base,
                        seL4_ARM_Page_Uncached,
                    )
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
                    seL4_ARM_Page_Uncached,
                )
            };
        }

        if result == seL4_NoError {
            Ok(())
        } else {
            Err(result)
        }
    }
}
