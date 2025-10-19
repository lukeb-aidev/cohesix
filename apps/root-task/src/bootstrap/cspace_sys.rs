// Author: Lukas Bower
#![allow(unsafe_code)]

use crate::sel4 as sys;

/// COPY with invocation addressing (both sides): slots go in the `*_index` fields and
/// `dest_offset` must remain zero.
#[inline(always)]
pub fn cnode_copy_invoc(
    dst_slot: sys::seL4_CPtr,
    src_slot: sys::seL4_CPtr,
    rights: sys::seL4_CapRights,
) -> sys::seL4_Error {
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

/// MINT with invocation addressing for both the destination and source slots.
#[inline(always)]
pub fn cnode_mint_invoc(
    dst_slot: sys::seL4_CPtr,
    src_slot: sys::seL4_CPtr,
    rights: sys::seL4_CapRights,
    badge: sys::seL4_Word,
) -> sys::seL4_Error {
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
        let _ = (dst_slot, src_slot, rights, badge);
        sys::seL4_NoError
    }
}

/// RETYPE with invocation addressing for the destination slot.
#[inline(always)]
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
            0,
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
        rights: sys::seL4_CapRights,
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
                0,
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
                init_cnode_bits.into(),
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
