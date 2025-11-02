// Author: Lukas Bower

pub mod tuples;

use crate::sel4::{self, BootInfoExt};
use sel4_sys::{seL4_BootInfo, seL4_CPtr, seL4_Error, seL4_Word, seL4_WordBits};

/// Helper managing allocation within the init thread's capability space.
pub struct CSpace {
    root: seL4_CPtr,
    bits: u8,
    next_free: seL4_CPtr,
}

impl CSpace {
    /// Constructs a capability-space helper from kernel boot information.
    #[must_use]
    pub fn from_bootinfo(bi: &seL4_BootInfo) -> Self {
        Self {
            root: bi.init_cnode_cap(),
            bits: bi.initThreadCNodeSizeBits as u8,
            next_free: bi.empty.start,
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

    /// Allocates the next available slot from the init CSpace.
    pub fn alloc_slot(&mut self) -> Result<seL4_CPtr, seL4_Error> {
        let limit = 1u64 << self.bits;
        if (self.next_free as u64) >= limit {
            return Err(sel4_sys::seL4_NotEnoughMemory);
        }
        let slot = self.next_free;
        self.next_free = self.next_free.saturating_add(1);
        Ok(slot)
    }

    /// Issues a `seL4_CNode_Copy` within the init CSpace.
    pub fn copy_here(
        &mut self,
        dst_slot: seL4_CPtr,
        src_slot: seL4_CPtr,
        rights: sel4_sys::seL4_CapRights,
    ) -> seL4_Error {
        let word_depth = seL4_WordBits as sel4::seL4_Word;
        log::info!(
            "[cnode] Copy dest: root=initCNode index=0x{slot:04x} depth={depth} offset=0",
            slot = dst_slot,
            depth = word_depth,
        );
        sel4::cnode_copy_depth(
            self.root, dst_slot, self.bits, self.root, src_slot, self.bits, rights,
        )
    }

    /// Issues a `seL4_CNode_Mint` within the init CSpace.
    pub fn mint_here(
        &mut self,
        dst_slot: seL4_CPtr,
        src_slot: seL4_CPtr,
        rights: sel4_sys::seL4_CapRights,
        badge: seL4_Word,
    ) -> seL4_Error {
        let word_depth = seL4_WordBits as sel4::seL4_Word;
        log::info!(
            "[cnode] Mint dest: root=initCNode index=0x{slot:04x} depth={depth} offset=0",
            slot = dst_slot,
            depth = word_depth,
        );
        sel4::cnode_mint_depth(
            self.root, dst_slot, self.bits, self.root, src_slot, self.bits, rights, badge,
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
