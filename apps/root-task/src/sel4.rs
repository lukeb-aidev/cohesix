// Author: Lukas Bower
#![allow(dead_code)]
#![allow(clippy::missing_panics_doc)]

use core::ptr;

use sel4_sys::{
    seL4_ARM_Page, seL4_ARM_Page_Map, seL4_ARM_Page_Uncached, seL4_ARM_Page_Unmap,
    seL4_ARM_PageTable, seL4_ARM_PageTable_Map, seL4_BootInfo, seL4_CNode,
    seL4_CapInitThreadCNode, seL4_CapRights_ReadWrite, seL4_CPtr, seL4_Error, seL4_SlotRegion,
    seL4_Untyped, seL4_Untyped_Retype, seL4_Word, UntypedDesc, seL4_ARM_SmallPageObject,
    seL4_NoError,
};

const PAGE_BITS: usize = 12;
const PAGE_SIZE: usize = 1 << PAGE_BITS;

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
    entries: &'a [UntypedDesc],
}

impl<'a> UntypedCatalog<'a> {
    pub fn new(bootinfo: &'a seL4_BootInfo) -> Self {
        let count = bootinfo.untyped.end - bootinfo.untyped.start;
        let entries = &bootinfo.untypedList[..count as usize];
        Self { entries }
    }

    pub fn find_for_paddr(&self, paddr: usize, size_bits: usize) -> Option<&'a UntypedDesc> {
        let length = 1usize << size_bits;
        let end = paddr.saturating_add(length);
        self.entries.iter().find(|desc| {
            if desc.isDevice == 0 {
                return false;
            }
            let base = desc.paddr as usize;
            let limit = base.saturating_add(1usize << desc.sizeBits);
            base <= paddr && end <= limit
        })
    }

    pub fn index_of(&self, target: &'a UntypedDesc) -> Option<usize> {
        self.entries
            .iter()
            .position(|entry| ptr::eq(*entry, target))
    }
}

#[derive(Debug)]
pub struct DeviceFrame {
    pub cap: seL4_CPtr,
    pub vaddr: usize,
}

pub struct KernelEnv<'a> {
    bootinfo: &'a seL4_BootInfo,
    slots: SlotAllocator,
    untyped: UntypedCatalog<'a>,
}

impl<'a> KernelEnv<'a> {
    pub fn new(bootinfo: &'a seL4_BootInfo) -> Self {
        let slots = SlotAllocator::new(bootinfo.empty, bootinfo.initThreadCNodeSizeBits);
        let untyped = UntypedCatalog::new(bootinfo);
        Self {
            bootinfo,
            slots,
            untyped,
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
        size_bits: usize,
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

    pub fn map_device(
        &mut self,
        paddr: usize,
        desired_vaddr: usize,
    ) -> Result<DeviceFrame, seL4_Error> {
        let desc_ref = self
            .untyped
            .find_for_paddr(paddr, PAGE_BITS)
            .ok_or(-1)?;
        let offset = self.untyped.index_of(desc_ref).ok_or(-1)?;
        let untyped_cap = self.bootinfo.untyped.start + offset as seL4_CPtr;
        let frame_slot = self.allocate_slot();
        self.retype_page(untyped_cap, frame_slot, PAGE_BITS)?;
        self.map_frame(frame_slot, desired_vaddr)?;
        Ok(DeviceFrame {
            cap: frame_slot,
            vaddr: desired_vaddr,
        })
    }

    fn map_frame(&mut self, frame_cap: seL4_CPtr, vaddr: usize) -> Result<(), seL4_Error> {
        let vspace = sel4_sys::seL4_CapInitThreadVSpace;
        unsafe {
            // Assume second-level page table already populated; attempt mapping.
            let res = seL4_ARM_Page_Map(
                frame_cap,
                vspace,
                vaddr,
                seL4_CapRights_ReadWrite,
                seL4_ARM_Page_Uncached,
            );
            if res != seL4_NoError {
                return Err(res);
            }
        }
        Ok(())
    }
}
