// Author: Lukas Bower
#![allow(dead_code)]

use core::convert::TryInto;

use crate::sel4::BootInfo;
use sel4_sys as sys;

/// Minimal allocator exposing the init thread's writable CSpace root.
#[derive(Copy, Clone)]
pub struct CSpace {
    /// Root CNode capability used when addressing slots during bootstrap.
    pub root: sys::seL4_CPtr,
    /// Guard depth (in bits) required when invoking init CSpace operations.
    pub depth_bits: u8,
    empty_start: u32,
    empty_end: u32,
    next: u32,
}

impl CSpace {
    /// Builds a [`CSpace`] view from seL4 boot information.
    #[must_use]
    pub fn from_bootinfo(bi: &'static BootInfo) -> Self {
        let root = sys::seL4_CapInitThreadCNode;

        let depth_bits: u8 = bi
            .initThreadCNodeSizeBits
            .try_into()
            .expect("initThreadCNodeSizeBits must fit in u8");

        let empty_start: u32 = bi
            .empty
            .start
            .try_into()
            .expect("bootinfo.empty.start must fit");
        let empty_end: u32 = bi
            .empty
            .end
            .try_into()
            .expect("bootinfo.empty.end must fit");

        debug_assert!(
            empty_start <= empty_end,
            "bootinfo empty range [{empty_start}, {empty_end}) is invalid"
        );

        Self {
            root,
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

    /// Returns the bootinfo-declared empty range bounds as a tuple `(start, end)`.
    #[must_use]
    pub fn empty_bounds(&self) -> (u32, u32) {
        (self.empty_start, self.empty_end)
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
