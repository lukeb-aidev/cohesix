// Author: Lukas Bower
#![allow(unsafe_code)]

use crate::sel4 as sys;

#[inline]
pub fn caprights_rw_grant() -> sys::SeL4CapRights {
    #[cfg(target_os = "none")]
    unsafe {
        sys::seL4_CapRights::new(0, 1, 1, 1)
    }

    #[cfg(not(target_os = "none"))]
    {
        let mut value = 0usize;
        value |= 1 << 0; // write
        value |= 1 << 1; // read
        value |= 1 << 2; // grant
        value as sys::SeL4CapRights
    }
}

/// Slot-range guard ensuring the root CNode index stays within `2^init_cnode_bits`.
#[inline]
pub fn check_slot_in_range(init_cnode_bits: u8, slot: sys::seL4_CPtr) {
    let limit = 1u64 << (init_cnode_bits as u64);
    assert!(
        (slot as u64) < limit,
        "slot {} out of range for init_cnode_bits={} (limit={})",
        slot,
        init_cnode_bits,
        limit
    );
}

/// COPY: invocation addressing only, slots passed via `*_index` with `*_depth = 0`.
pub fn cnode_copy_invoc(dst_slot: sys::seL4_CPtr, src_slot: sys::seL4_CPtr) -> sys::seL4_Error {
    let rights = caprights_rw_grant();

    #[cfg(target_os = "none")]
    unsafe {
        sys::seL4_CNode_Copy(
            sys::seL4_CapInitThreadCNode,
            dst_slot,
            0u8,
            sys::seL4_CapInitThreadCNode,
            src_slot,
            0u8,
            rights,
            0,
        )
    }

    #[cfg(not(target_os = "none"))]
    {
        let _ = (dst_slot, src_slot, rights);
        sys::seL4_NoError
    }
}

/// MINT: invocation addressing on both operands with zero offset.
pub fn cnode_mint_invoc(
    dst_slot: sys::seL4_CPtr,
    src_slot: sys::seL4_CPtr,
    badge: sys::seL4_Word,
) -> sys::seL4_Error {
    let rights = caprights_rw_grant();

    #[cfg(target_os = "none")]
    unsafe {
        sys::seL4_CNode_Mint(
            sys::seL4_CapInitThreadCNode,
            dst_slot,
            0u8,
            sys::seL4_CapInitThreadCNode,
            src_slot,
            0u8,
            rights,
            badge,
            0,
        )
    }

    #[cfg(not(target_os = "none"))]
    {
        let _ = (dst_slot, src_slot, badge, rights);
        sys::seL4_NoError
    }
}

/// DELETE: invocation addressing helper for boot-time cleanup.
pub fn cnode_delete_invoc(slot: sys::seL4_CPtr) -> sys::seL4_Error {
    #[cfg(target_os = "none")]
    unsafe {
        sys::seL4_CNode_Delete(sys::seL4_CapInitThreadCNode, slot, 0u8)
    }

    #[cfg(not(target_os = "none"))]
    {
        let _ = slot;
        sys::seL4_NoError
    }
}

/// RETYPE: destination slot addressed via invocation tuple `(index=slot, depth=0, offset=0)`.
pub fn untyped_retype_invoc(
    untyped_slot: sys::seL4_CPtr,
    obj_type: sys::seL4_Word,
    size_bits: sys::seL4_Word,
    dst_slot: sys::seL4_CPtr,
) -> sys::seL4_Error {
    #[cfg(target_os = "none")]
    unsafe {
        sys::seL4_Untyped_Retype(
            untyped_slot,
            obj_type,
            size_bits,
            sys::seL4_CapInitThreadCNode,
            dst_slot,
            0u8,
            0,
            1,
        )
    }

    #[cfg(not(target_os = "none"))]
    {
        let _ = (untyped_slot, obj_type, size_bits, dst_slot);
        sys::seL4_NoError
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::sys;

    /// Test-only helper that issues `seL4_CNode_Mint` using direct addressing.
    #[allow(dead_code)]
    pub fn cnode_mint_direct_dest(
        init_cnode_bits: u8,
        dst_slot: sys::seL4_CPtr,
        src_slot: sys::seL4_CPtr,
        rights: sys::SeL4CapRights,
        badge: sys::seL4_Word,
    ) -> sys::seL4_Error {
        #[cfg(target_os = "none")]
        unsafe {
            sys::seL4_CNode_Mint(
                sys::seL4_CapInitThreadCNode,
                dst_slot,
                init_cnode_bits,
                sys::seL4_CapInitThreadCNode,
                src_slot,
                0u8,
                rights,
                badge,
                0,
            )
        }

        #[cfg(not(target_os = "none"))]
        {
            let _ = (init_cnode_bits, dst_slot, src_slot, rights, badge);
            sys::seL4_IllegalOperation
        }
    }

    /// Test-only helper that issues `seL4_Untyped_Retype` using direct addressing.
    #[allow(dead_code)]
    pub fn untyped_retype_direct_dest(
        init_cnode_bits: u8,
        untyped: sys::seL4_CPtr,
        obj_type: sys::seL4_Word,
        size_bits: sys::seL4_Word,
        dst_slot: sys::seL4_CPtr,
    ) -> sys::seL4_Error {
        #[cfg(target_os = "none")]
        unsafe {
            sys::seL4_Untyped_Retype(
                untyped,
                obj_type,
                size_bits,
                sys::seL4_CapInitThreadCNode,
                dst_slot,
                init_cnode_bits,
                0,
                1,
            )
        }

        #[cfg(not(target_os = "none"))]
        {
            let _ = (init_cnode_bits, untyped, obj_type, size_bits, dst_slot);
            sys::seL4_IllegalOperation
        }
    }
}
