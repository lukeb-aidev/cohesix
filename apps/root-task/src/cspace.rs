// Author: Lukas Bower

pub mod tuples;

use core::sync::atomic::{AtomicBool, Ordering};

use crate::sel4::{self, BootInfoExt, CapTag};
use sel4_sys::{seL4_BootInfo, seL4_CPtr, seL4_Error, seL4_Word};

fn is_cnode_cap(raw_ty: seL4_Word) -> bool {
    matches!(CapTag::from_raw(raw_ty), Some(CapTag::CNode))
}

static DEST_ROOT_LOGGED: AtomicBool = AtomicBool::new(false);

/// Helper managing allocation within the init thread's capability space.
pub struct CSpace {
    root: seL4_CPtr,
    bits: u8,
    next_free: seL4_CPtr,
    empty_start: seL4_CPtr,
    empty_end: seL4_CPtr,
    reserved_floor: seL4_CPtr,
    highest_next_free: seL4_CPtr,
}

impl CSpace {
    /// Constructs a capability-space helper from kernel boot information.
    #[must_use]
    pub fn from_bootinfo(bi: &seL4_BootInfo) -> Self {
        Self {
            root: bi.init_cnode_cap(),
            bits: bi.init_cnode_bits() as u8,
            next_free: bi.empty.start,
            empty_start: bi.empty.start,
            empty_end: bi.empty.end,
            reserved_floor: bi.empty.start,
            highest_next_free: bi.empty.start,
        }
    }

    /// Returns the radix width (in bits) of the init CNode.
    #[must_use]
    pub fn depth(&self) -> u8 {
        self.bits
    }

    /// Returns the root capability pointer for the init thread CNode.
    #[must_use]
    pub fn root(&self) -> seL4_CPtr {
        self.root
    }

    /// Returns the next free slot index that will be handed out by [`alloc_slot`].
    #[must_use]
    pub fn next_free_slot(&self) -> seL4_CPtr {
        self.next_free
    }

    #[inline(always)]
    fn cnode_invocation_depth(&self) -> u8 {
        self.bits
    }

    fn assert_invariants(&self) {
        assert!(
            self.next_free >= self.empty_start,
            "first_free moved below empty window start: next_free=0x{next:04x} start=0x{start:04x}",
            next = self.next_free,
            start = self.empty_start,
        );
        assert!(
            self.next_free <= self.empty_end,
            "first_free exceeded empty window end: next_free=0x{next:04x} end=0x{end:04x}",
            next = self.next_free,
            end = self.empty_end,
        );
        assert!(
            self.next_free >= self.reserved_floor,
            "first_free overlapped reserved range: next_free=0x{next:04x} reserved_floor=0x{floor:04x}",
            next = self.next_free,
            floor = self.reserved_floor,
        );
    }

    /// Allocates the next available slot from the init CSpace.
    pub fn alloc_slot(&mut self) -> Result<seL4_CPtr, seL4_Error> {
        let limit = 1u64 << self.bits;
        if (self.next_free as u64) >= limit || self.next_free >= self.empty_end {
            return Err(sel4_sys::seL4_NotEnoughMemory);
        }
        self.assert_invariants();
        let slot = self.next_free;
        self.next_free = self.next_free.saturating_add(1);
        self.highest_next_free = core::cmp::max(self.highest_next_free, self.next_free);
        self.assert_invariants();
        Ok(slot)
    }

    /// Releases a slot previously returned by [`alloc_slot`], allowing it to be reused.
    pub fn release_slot(&mut self, slot: seL4_CPtr) {
        if slot + 1 == self.next_free && slot + 1 > self.reserved_floor {
            self.next_free = slot;
        }
        self.assert_invariants();
    }

    /// Reserve a capability slot so the allocator will never hand it out again.
    pub fn reserve_slot(&mut self, slot: seL4_CPtr) {
        assert!(
            slot >= self.empty_start && slot < self.empty_end,
            "reserved slot 0x{slot:04x} outside empty window [0x{start:04x}..0x{end:04x})",
            slot = slot,
            start = self.empty_start,
            end = self.empty_end,
        );
        self.reserved_floor = core::cmp::max(self.reserved_floor, slot.saturating_add(1));
        if self.next_free < self.reserved_floor {
            self.next_free = self.reserved_floor;
        }
        self.highest_next_free = core::cmp::max(self.highest_next_free, self.next_free);
        self.assert_invariants();
    }

    /// Issues a `seL4_CNode_Copy` within the init CSpace.
    pub fn copy_here(
        &mut self,
        dst_slot: seL4_CPtr,
        src_root: seL4_CPtr,
        src_slot: seL4_CPtr,
        rights: sel4_sys::seL4_CapRights,
    ) -> seL4_Error {
        let depth = self.cnode_invocation_depth();
        log::info!(
            "[cnode] Copy dst=0x{dst:04x} depth={depth}",
            dst = dst_slot,
            depth = depth,
        );
        log::info!(
            "[cnode] Copy src=0x{src:04x} depth={depth}",
            src = src_slot,
            depth = depth,
        );
        sel4::cnode_copy_depth(
            self.root, dst_slot, depth, src_root, src_slot, depth, rights,
        )
    }

    /// Issues a `seL4_CNode_Mint` within the init CSpace.
    pub fn mint_here(
        &mut self,
        dst_slot: seL4_CPtr,
        src_root: seL4_CPtr,
        src_slot: seL4_CPtr,
        rights: sel4_sys::seL4_CapRights,
        badge: seL4_Word,
    ) -> seL4_Error {
        let depth = self.cnode_invocation_depth();
        let limit = 1u64 << self.bits;
        assert!(
            (dst_slot as u64) < limit,
            "dest slot 0x{dst_slot:04x} exceeds cnode depth {depth}",
            dst_slot = dst_slot,
            depth = self.bits,
        );
        assert!(
            dst_slot >= self.empty_start && dst_slot < self.empty_end,
            "dest slot 0x{dst_slot:04x} outside empty window [0x{start:04x}..0x{end:04x})",
            dst_slot = dst_slot,
            start = self.empty_start,
            end = self.empty_end,
        );
        let ident = sel4::debug_cap_identify(self.root);
        if ident != 0 {
            assert!(
                is_cnode_cap(ident),
                "dest root 0x{root:04x} identify=0x{ident:08x}; expected CNode capability",
                root = self.root,
                ident = ident,
            );
        }
        debug_assert_eq!(
            self.root,
            sel4_sys::seL4_CapInitThreadCNode,
            "dest root expected to be init CNode",
        );
        if !DEST_ROOT_LOGGED.swap(true, Ordering::AcqRel) {
            log::info!(
                "[cspace] using CSpace root=0x{root:04x} (type=0x{ident:08x}) as destRoot for CNode ops",
                root = self.root,
                ident = ident,
            );
        }
        log::info!(
            "[cnode] Mint dst=0x{dst:04x} depth={depth}",
            dst = dst_slot,
            depth = depth,
        );
        log::info!(
            "[cnode] Mint src=0x{src:04x} depth={depth}",
            src = src_slot,
            depth = depth,
        );
        sel4::cnode_mint_depth(
            self.root, dst_slot, depth, src_root, src_slot, depth, rights, badge,
        )
    }
}

/// Constructs capability rights permitting read, write, and grant.
#[must_use]
pub fn cap_rights_read_write_grant() -> sel4_sys::seL4_CapRights {
    #[cfg(target_os = "none")]
    {
        sel4_sys::seL4_CapRights::new(0, 1, 1, 1)
    }

    #[cfg(not(target_os = "none"))]
    {
        sel4_sys::seL4_CapRights_All
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cnode_invocation_depth_tracks_init_bits() {
        let mut cspace = CSpace {
            root: 2,
            bits: 13,
            next_free: 0,
            empty_start: 0,
            empty_end: 1,
            reserved_floor: 0,
            highest_next_free: 0,
        };

        assert_eq!(cspace.cnode_invocation_depth(), 13);

        cspace.bits = 16;
        assert_eq!(cspace.cnode_invocation_depth(), 16);
    }
}
