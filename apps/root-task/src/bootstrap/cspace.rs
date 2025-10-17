// Author: Lukas Bower

use crate::sel4::BootInfo;
use sel4_sys as sys;

#[derive(Copy, Clone)]
pub struct CSpace {
    pub root: sys::seL4_CPtr,
    pub depth_bits: u8,
    empty_start: u32,
    empty_end: u32,
    next: u32,
}

impl CSpace {
    pub fn from_bootinfo(bi: &'static BootInfo) -> Self {
        let root = sys::seL4_CapInitThreadCNode;
        let depth_bits = bi.initThreadCNodeSizeBits as u8;
        let empty_start = bi.empty.start as u32;
        let empty_end = bi.empty.end as u32;
        Self {
            root,
            depth_bits,
            empty_start,
            empty_end,
            next: empty_start,
        }
    }
    pub fn alloc_slot(&mut self) -> Option<u32> {
        if self.next >= self.empty_end {
            return None;
        }
        let s = self.next;
        self.next += 1;
        Some(s)
    }
    pub fn bounds(&self) -> (u32, u32) {
        (self.empty_start, self.empty_end)
    }
}
