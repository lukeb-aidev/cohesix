// Author: Lukas Bower
#![allow(unsafe_code)]

use crate::sel4 as sys;

pub const CANONICAL_CNODE_DEPTH_BITS: u8 = (core::mem::size_of::<sys::seL4_Word>() * 8) as u8;
#[inline]
pub fn caprights_rw_grant() -> sys::SeL4CapRights {
    #[cfg(target_os = "none")]
    {
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

pub fn cnode_copy_invoc(
    init_cnode_bits: u8,
    dst_slot: sys::seL4_CPtr,
    src_slot: sys::seL4_CPtr,
) -> sys::seL4_Error {
    debug_assert!(
        init_cnode_bits <= CANONICAL_CNODE_DEPTH_BITS,
        "init_cnode_bits {} exceeds canonical depth {}",
        init_cnode_bits,
        CANONICAL_CNODE_DEPTH_BITS
    );
    let rights = caprights_rw_grant();
    let depth = init_cnode_bits as sys::seL4_Word;

    #[cfg(target_os = "none")]
    unsafe {
        sys::seL4_CNode_Copy(
            sys::seL4_CapInitThreadCNode,
            dst_slot,
            depth,
            sys::seL4_CapInitThreadCNode,
            src_slot,
            depth,
            rights,
        )
    }

    #[cfg(not(target_os = "none"))]
    {
        let _ = (init_cnode_bits, dst_slot, src_slot, rights, depth);
        sys::seL4_NoError
    }
}

pub fn cnode_mint_invoc(
    init_cnode_bits: u8,
    dst_slot: sys::seL4_CPtr,
    src_slot: sys::seL4_CPtr,
    badge: sys::seL4_Word,
) -> sys::seL4_Error {
    debug_assert!(
        init_cnode_bits <= CANONICAL_CNODE_DEPTH_BITS,
        "init_cnode_bits {} exceeds canonical depth {}",
        init_cnode_bits,
        CANONICAL_CNODE_DEPTH_BITS
    );
    let rights = caprights_rw_grant();
    let depth = init_cnode_bits as sys::seL4_Word;

    #[cfg(target_os = "none")]
    unsafe {
        sys::seL4_CNode_Mint(
            sys::seL4_CapInitThreadCNode,
            dst_slot,
            depth,
            sys::seL4_CapInitThreadCNode,
            src_slot,
            depth,
            rights,
            badge,
        )
    }

    #[cfg(not(target_os = "none"))]
    {
        let _ = (init_cnode_bits, dst_slot, src_slot, badge, rights, depth);
        sys::seL4_NoError
    }
}

pub fn cnode_delete_invoc(slot: sys::seL4_CPtr) -> sys::seL4_Error {
    #[cfg(target_os = "none")]
    unsafe {
        sys::seL4_CNode_Delete(
            sys::seL4_CapInitThreadCNode,
            slot,
            CANONICAL_CNODE_DEPTH_BITS,
        )
    }

    #[cfg(not(target_os = "none"))]
    {
        let _ = slot;
        sys::seL4_NoError
    }
}

pub fn untyped_retype_invoc(
    init_cnode_bits: u8,
    untyped_slot: sys::seL4_CPtr,
    obj_type: sys::seL4_Word,
    size_bits: sys::seL4_Word,
    dst_slot: sys::seL4_CPtr,
) -> sys::seL4_Error {
    debug_assert!(
        init_cnode_bits <= CANONICAL_CNODE_DEPTH_BITS,
        "init_cnode_bits {} exceeds canonical depth {}",
        init_cnode_bits,
        CANONICAL_CNODE_DEPTH_BITS
    );
    let depth_word = init_cnode_bits as sys::seL4_Word;

    #[cfg(target_os = "none")]
    unsafe {
        sys::seL4_Untyped_Retype(
            untyped_slot,
            obj_type,
            size_bits,
            sys::seL4_CapInitThreadCNode,
            dst_slot,
            depth_word,
            0,
            1,
        )
    }

    #[cfg(not(target_os = "none"))]
    {
        let _ = (
            init_cnode_bits,
            untyped_slot,
            obj_type,
            size_bits,
            dst_slot,
            depth_word,
        );
        sys::seL4_NoError
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::sys;

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
            )
        }

        #[cfg(not(target_os = "none"))]
        {
            let _ = (init_cnode_bits, dst_slot, src_slot, rights, badge);
            sys::seL4_IllegalOperation
        }
    }

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
                init_cnode_bits as sys::seL4_Word,
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
