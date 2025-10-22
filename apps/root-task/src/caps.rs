// Author: Lukas Bower

use sel4_sys::{
    seL4_BootInfo, seL4_CNode, seL4_CPtr, seL4_Error, seL4_NoError, seL4_Untyped,
    seL4_Untyped_Retype, seL4_Word,
};

/// Root-task view over kernel capabilities required during bootstrap.
pub struct RootCaps {
    /// Capability pointer for the init thread CNode.
    pub cnode: seL4_CNode,
    /// Radix width of the init thread CNode.
    pub cnode_bits: u8,
    /// Next free slot index tracked by the root task.
    pub next_free: seL4_CPtr,
    /// Slot index storing the bootstrap endpoint capability.
    pub endpoint: seL4_CPtr,
}

impl RootCaps {
    /// Constructs a [`RootCaps`] projection from kernel boot information.
    #[must_use]
    pub fn from_bootinfo(bi: &seL4_BootInfo) -> Self {
        Self {
            cnode: bi.initThreadCNode,
            cnode_bits: bi.initThreadCNodeSizeBits as u8,
            next_free: bi.first_free_slot,
            endpoint: sel4_sys::seL4_CapNull,
        }
    }

    /// Allocates the next free capability slot from the tracked init CNode window.
    pub fn alloc_slot(&mut self) -> Result<seL4_CPtr, seL4_Error> {
        let limit = 1u64 << self.cnode_bits;
        if (self.next_free as u64) >= limit {
            return Err(seL4_Error::seL4_NotEnoughMemory);
        }
        let slot = self.next_free;
        self.next_free = self.next_free.saturating_add(1);
        Ok(slot)
    }
}

/// Retypes an untyped capability into an endpoint object at the destination slot.
pub fn retype_endpoint(
    untyped: seL4_Untyped,
    dst_cnode: seL4_CNode,
    dst_slot: seL4_CPtr,
    dst_depth_bits: u8,
) -> Result<(), seL4_Error> {
    let depth = seL4_Word::from(dst_depth_bits);
    let err = unsafe {
        seL4_Untyped_Retype(
            untyped,
            sel4_sys::seL4_ObjectType::seL4_EndpointObject as seL4_Word,
            0,
            dst_cnode,
            dst_slot,
            depth,
            0,
            1,
        )
    };
    if err == seL4_NoError {
        Ok(())
    } else {
        Err(err)
    }
}
