// Author: Lukas Bower
#![allow(dead_code)]

use core::convert::TryInto;

use crate::sel4::{BootInfo, BootInfoExt};
use sel4_sys as sys;

/// Minimal allocator exposing the init thread's writable CSpace root.
#[derive(Copy, Clone)]
pub struct CSpace {
    root: sys::seL4_CPtr,
    depth_bits: u8,
    empty_start: u32,
    empty_end: u32,
    next: u32,
}

impl CSpace {
    /// Builds a [`CSpace`] view from seL4 boot information.
    #[must_use]
    pub fn from_bootinfo(bi: &'static BootInfo) -> Self {
        let depth_bits: u8 = bi
            .init_cnode_bits()
            .try_into()
            .expect("initThreadCNodeSizeBits must fit in u8");
        let empty_start: u32 = bi
            .empty_first_slot()
            .try_into()
            .expect("bootinfo.empty.start must fit in u32");
        let empty_end: u32 = bi
            .empty_last_slot_excl()
            .try_into()
            .expect("bootinfo.empty.end must fit in u32");
        Self {
            root: bi.init_cnode_cap(),
            depth_bits,
            empty_start,
            empty_end,
            next: empty_start,
        }
    }

    /// Returns the writable root CNode capability.
    #[must_use]
    pub fn root(&self) -> sys::seL4_CPtr {
        self.root
    }

    /// Returns the number of guard bits describing the root CNode capacity.
    #[must_use]
    pub fn depth_bits(&self) -> u8 {
        self.depth_bits
    }

    /// Allocates the next empty slot within the init CSpace.
    pub fn alloc_slot(&mut self) -> Option<u32> {
        if self.next >= self.empty_end {
            return None;
        }
        let slot = self.next;
        self.next = self.next.saturating_add(1);
        Some(slot)
    }

    /// Returns the number of slots handed out so far.
    #[must_use]
    pub fn consumed(&self) -> u32 {
        self.next.saturating_sub(self.empty_start)
    }

    /// Returns the number of empty slots remaining.
    #[must_use]
    pub fn remaining(&self) -> u32 {
        self.empty_end.saturating_sub(self.next)
    }
}
