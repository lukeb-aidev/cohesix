// Author: Lukas Bower

pub mod tuples;

use core::convert::TryFrom;

use crate::sel4::{self, BootInfoExt};
use sel4_sys::{seL4_BootInfo, seL4_CPtr, seL4_Error, seL4_Word};

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
        let depth = u8::try_from(sel4_sys::seL4_WordBits).expect("seL4_WordBits must fit in u8");
        let encoded_dst = crate::bootstrap::cspace_sys::encode_slot(dst_slot, self.bits);
        let encoded_src = crate::bootstrap::cspace_sys::encode_slot(src_slot, self.bits);
        let word_bits = sel4_sys::seL4_WordBits as usize;
        let hex_width = (word_bits + 3) / 4;
        log::info!(
            "[cnode] Copy dst=0x{dst:04x} enc=0x{enc:0width$x} depth=WordBits({bits})",
            dst = dst_slot,
            enc = encoded_dst as u64,
            width = hex_width,
            bits = sel4_sys::seL4_WordBits,
        );
        log::info!(
            "[cnode] Copy src=0x{src:04x} enc=0x{enc:0width$x} depth=WordBits({bits})",
            src = src_slot,
            enc = encoded_src as u64,
            width = hex_width,
            bits = sel4_sys::seL4_WordBits,
        );
        sel4::cnode_copy_depth(
            self.root,
            encoded_dst,
            depth,
            self.root,
            encoded_src,
            depth,
            rights,
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
        let depth = u8::try_from(sel4_sys::seL4_WordBits).expect("seL4_WordBits must fit in u8");
        let encoded_dst = crate::bootstrap::cspace_sys::encode_slot(dst_slot, self.bits);
        let encoded_src = crate::bootstrap::cspace_sys::encode_slot(src_slot, self.bits);
        let word_bits = sel4_sys::seL4_WordBits as usize;
        let hex_width = (word_bits + 3) / 4;
        log::info!(
            "[cnode] Mint dst=0x{dst:04x} enc=0x{enc:0width$x} depth=WordBits({bits})",
            dst = dst_slot,
            enc = encoded_dst as u64,
            width = hex_width,
            bits = sel4_sys::seL4_WordBits,
        );
        log::info!(
            "[cnode] Mint src=0x{src:04x} enc=0x{enc:0width$x} depth=WordBits({bits})",
            src = src_slot,
            enc = encoded_src as u64,
            width = hex_width,
            bits = sel4_sys::seL4_WordBits,
        );
        sel4::cnode_mint_depth(
            self.root,
            encoded_dst,
            depth,
            self.root,
            encoded_src,
            depth,
            rights,
            badge,
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
