// Author: Lukas Bower

use crate::sel4::BootInfoExt;
use sel4_sys::{
    seL4_BootInfo, seL4_CNode, seL4_CPtr, seL4_Error, seL4_NoError, seL4_Untyped_Retype, seL4_Word,
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
            cnode: bi.init_cnode_cap(),
            cnode_bits: bi.init_cnode_depth(),
            next_free: bi.empty_first_slot() as seL4_CPtr,
            endpoint: sel4_sys::seL4_CapNull,
        }
    }

    /// Allocates the next free capability slot from the tracked init CNode window.
    pub fn alloc_slot(&mut self) -> Result<seL4_CPtr, seL4_Error> {
        let limit = 1usize << usize::from(self.cnode_bits);
        if self.next_free >= limit {
            return Err(sel4_sys::seL4_NotEnoughMemory);
        }
        let slot = self.next_free;
        self.next_free = self.next_free.saturating_add(1);
        Ok(slot)
    }
}

/// Retypes an untyped capability into an endpoint object at the destination slot.
pub fn retype_endpoint_into_slot(
    untyped: seL4_CPtr,
    dst_cnode: seL4_CPtr,
    dst_slot: seL4_CPtr,
) -> Result<(), seL4_Error> {
    let err = unsafe {
        seL4_Untyped_Retype(
            untyped,
            sel4_sys::seL4_ObjectType::seL4_EndpointObject as seL4_Word,
            0,
            dst_cnode,
            0,
            0,
            dst_slot,
            1,
        )
    };
    if err == seL4_NoError {
        Ok(())
    } else {
        Err(err)
    }
}
