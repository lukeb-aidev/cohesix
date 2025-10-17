// Author: Lukas Bower

use crate::sel4::BootInfo;
use sel4_sys as sys;

/// Minimal capability-space allocator backed by the init thread's root CNode.
#[derive(Copy, Clone)]
pub struct CSpace {
    /// Capability pointer referencing the init thread's CSpace root CNode.
    pub root: sys::seL4_CPtr,
    /// Power-of-two size of the root CNode expressed as the number of address bits.
    pub depth_bits: u8,
    /// Inclusive start of the bootinfo-declared free slot window.
    empty_start: sys::seL4_CPtr,
    /// Exclusive end of the bootinfo-declared free slot window.
    empty_end: sys::seL4_CPtr,
    /// Next slot candidate handed out by the bump allocator.
    next: sys::seL4_CPtr,
}

impl CSpace {
    /// Constructs a bump allocator spanning the bootinfo-advertised empty slot window.
    pub fn from_bootinfo(bi: &'static BootInfo) -> Self {
        let root = sys::seL4_CapInitThreadCNode;
        let depth_bits = bi.initThreadCNodeSizeBits as u8;
        let empty_start = bi.empty.start as sys::seL4_CPtr;
        let empty_end = bi.empty.end as sys::seL4_CPtr;
        Self {
            root,
            depth_bits,
            empty_start,
            empty_end,
            next: empty_start,
        }
    }

    /// Reserves the next capability slot within the bootinfo span.
    pub fn alloc_slot(&mut self) -> Option<sys::seL4_CPtr> {
        if self.next >= self.empty_end {
            return None;
        }
        let slot = self.next;
        self.next += 1;
        Some(slot)
    }

    /// Returns the inclusive start and exclusive end of the managed slot window.
    pub fn bounds(&self) -> (sys::seL4_CPtr, sys::seL4_CPtr) {
        (self.empty_start, self.empty_end)
    }
}
