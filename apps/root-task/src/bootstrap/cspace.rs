// Author: Lukas Bower

use crate::sel4::{self, BootInfo};
use sel4_sys as sys;

/// Minimal capability-space allocator backed by the init thread's root CNode.
#[derive(Copy, Clone)]
pub struct CSpace {
    /// Capability pointer referencing the init thread's CSpace root CNode.
    pub root: sys::seL4_CPtr,
    /// Slot index holding the writable init thread CNode capability.
    root_slot: sys::seL4_CPtr,
    /// Slot index holding an all-rights copy of the init thread CNode capability.
    root_writable_slot: Option<sys::seL4_CPtr>,
    /// Power-of-two size of the root CNode expressed as the number of address bits.
    cnode_bits: u8,
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
        let root_slot = sys::seL4_CapInitThreadCNode;
        let cnode_bits = bi.initThreadCNodeSizeBits as u8;
        let empty_start = bi.empty.start as sys::seL4_CPtr;
        let empty_end = bi.empty.end as sys::seL4_CPtr;
        Self {
            root,
            root_slot,
            root_writable_slot: None,
            cnode_bits,
            empty_start,
            empty_end,
            next: empty_start,
        }
    }

    fn slot_in_bounds(&self, slot: sys::seL4_CPtr) -> bool {
        slot >= self.empty_start && slot < self.empty_end
    }

    /// Returns `true` when the provided slot index references a kernel-reserved
    /// capability.
    #[inline(always)]
    pub fn is_reserved_slot(slot: sys::seL4_CPtr) -> bool {
        matches!(
            slot,
            sys::seL4_CapNull
                | sys::seL4_CapInitThreadTCB
                | sys::seL4_CapInitThreadCNode
                | sys::seL4_CapInitThreadVSpace
                | sys::seL4_CapIRQControl
                | sys::seL4_CapASIDControl
                | sys::seL4_CapInitThreadASIDPool
                | sys::seL4_CapIOPortControl
                | sys::seL4_CapIOSpace
                | sys::seL4_CapBootInfoFrame
                | sys::seL4_CapInitThreadIPCBuffer
        )
    }

    /// Reserves the next capability slot within the bootinfo span.
    pub fn alloc_slot(&mut self) -> Option<sys::seL4_CPtr> {
        if self.next >= self.empty_end {
            return None;
        }
        let mut slot = self.next;
        while slot < self.empty_end {
            if Self::is_reserved_slot(slot) {
                slot += 1;
                continue;
            }
            break;
        }
        if slot >= self.empty_end {
            self.next = self.empty_end;
            return None;
        }
        debug_assert!(self.is_empty_slot(slot));
        self.next = slot + 1;
        Some(slot)
    }

    /// Returns the inclusive start and exclusive end of the managed slot window.
    pub fn bounds(&self) -> (sys::seL4_CPtr, sys::seL4_CPtr) {
        (self.empty_start, self.empty_end)
    }

    /// Returns true when the slot still carries the kernel's bootinfo null capability.
    #[inline(always)]
    pub fn is_empty_slot(&self, slot: sys::seL4_CPtr) -> bool {
        slot >= self.next && slot < self.empty_end
    }

    /// Returns the slot index referencing the init thread's root CNode capability.
    #[inline(always)]
    pub fn root_slot(&self) -> sys::seL4_CPtr {
        self.root_slot
    }

    /// Returns the slot index containing an all-rights copy of the init thread CNode.
    #[inline(always)]
    pub fn root_writable(&self) -> sys::seL4_CPtr {
        self.root_writable_slot
            .expect("writable init CNode capability must be minted before use")
    }

    /// Returns the guard depth (in bits) used when addressing the init CSpace root.
    #[inline(always)]
    pub fn guard_depth_bits(&self) -> u8 {
        0
    }

    /// Returns the number of address bits describing the root CNode's slot capacity.
    #[inline(always)]
    pub fn cnode_bits(&self) -> u8 {
        self.cnode_bits
    }

    /// Copies the init thread's root CNode capability with full write permissions.
    pub fn make_writable_root_copy(&mut self) -> Result<sys::seL4_CPtr, sys::seL4_Error> {
        if let Some(slot) = self.root_writable_slot {
            return Ok(slot);
        }

        let Some(slot) = self.alloc_slot() else {
            return Err(sys::seL4_NotEnoughMemory);
        };
        assert!(
            self.slot_in_bounds(slot),
            "writable root CNode slot is outside bootinfo.empty"
        );
        assert!(
            !Self::is_reserved_slot(slot),
            "attempted to reuse reserved capability slot for writable root copy"
        );

        let err = sel4::cnode_copy(
            sys::seL4_CapInitThreadCNode,
            slot,
            0,
            sys::seL4_CapInitThreadCNode,
            sys::seL4_CapInitThreadCNode,
            0,
            sys::seL4_CapRights_All,
        );
        if err != sys::seL4_NoError {
            return Err(err);
        }
        self.root_writable_slot = Some(slot);
        Ok(slot)
    }
}
