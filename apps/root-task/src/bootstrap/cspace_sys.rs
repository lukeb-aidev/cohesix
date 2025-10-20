// Author: Lukas Bower
#![allow(unsafe_code)]

use crate::sel4 as sys;

pub const CANONICAL_CNODE_DEPTH_BITS: u8 = (core::mem::size_of::<sys::seL4_Word>() * 8) as u8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CNodeDepth {
    bits_u8: u8,
    bits_word: sys::seL4_Word,
}

impl CNodeDepth {
    #[inline(always)]
    fn new(init_cnode_bits: u8) -> Self {
        debug_assert!(init_cnode_bits <= CANONICAL_CNODE_DEPTH_BITS);
        Self {
            bits_u8: init_cnode_bits,
            bits_word: init_cnode_bits as sys::seL4_Word,
        }
    }

    #[inline(always)]
    fn as_u8(self) -> u8 {
        self.bits_u8
    }

    #[inline(always)]
    fn as_word(self) -> sys::seL4_Word {
        self.bits_word
    }
}

#[inline(always)]
fn resolve_cnode_depth(init_cnode_bits: u8) -> CNodeDepth {
    // The initial thread's CNode is configured with guard bits so that
    // canonical capability pointers (the raw `seL4_CPtr` values) select the
    // correct slots. However, seL4's CNode invocations still expect the
    // *actual* radix width of the CNode for the depth arguments, not the
    // architectural word size. Passing the canonical depth causes the kernel to
    // overrun the radix and reject valid slots (manifesting as `Invalid source
    // slot`). Use the bootinfo-declared CNode size bits for both the integer and
    // word representations so that capability lookups align with the guard
    // configuration on every architecture.
    CNodeDepth::new(init_cnode_bits)
}
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
    let rights = caprights_rw_grant();
    #[cfg(target_os = "none")]
    {
        let depth = resolve_cnode_depth(init_cnode_bits);
        unsafe {
            sys::seL4_CNode_Copy(
                sys::seL4_CapInitThreadCNode,
                dst_slot,
                depth.as_u8(),
                sys::seL4_CapInitThreadCNode,
                src_slot,
                depth.as_u8(),
                rights,
            )
        }
    }

    #[cfg(not(target_os = "none"))]
    {
        let depth = resolve_cnode_depth(init_cnode_bits);
        let _ = (
            init_cnode_bits,
            dst_slot,
            src_slot,
            rights,
            depth.as_u8(),
            depth.as_word(),
        );
        sys::seL4_NoError
    }
}

pub fn cnode_mint_invoc(
    init_cnode_bits: u8,
    dst_slot: sys::seL4_CPtr,
    src_slot: sys::seL4_CPtr,
    badge: sys::seL4_Word,
) -> sys::seL4_Error {
    let rights = caprights_rw_grant();
    #[cfg(target_os = "none")]
    {
        let depth = resolve_cnode_depth(init_cnode_bits);
        unsafe {
            sys::seL4_CNode_Mint(
                sys::seL4_CapInitThreadCNode,
                dst_slot,
                depth.as_u8(),
                sys::seL4_CapInitThreadCNode,
                src_slot,
                depth.as_u8(),
                rights,
                badge,
            )
        }
    }

    #[cfg(not(target_os = "none"))]
    {
        let depth = resolve_cnode_depth(init_cnode_bits);
        let _ = (
            init_cnode_bits,
            dst_slot,
            src_slot,
            badge,
            rights,
            depth.as_u8(),
            depth.as_word(),
        );
        sys::seL4_NoError
    }
}

pub fn cnode_delete_invoc(init_cnode_bits: u8, slot: sys::seL4_CPtr) -> sys::seL4_Error {
    #[cfg(target_os = "none")]
    {
        let depth = resolve_cnode_depth(init_cnode_bits);
        unsafe { sys::seL4_CNode_Delete(sys::seL4_CapInitThreadCNode, slot, depth.as_u8()) }
    }

    #[cfg(not(target_os = "none"))]
    {
        let depth = resolve_cnode_depth(init_cnode_bits);
        let _ = (init_cnode_bits, slot, depth.as_u8(), depth.as_word());
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
    #[cfg(target_os = "none")]
    {
        let depth = resolve_cnode_depth(init_cnode_bits);
        unsafe {
            sys::seL4_Untyped_Retype(
                untyped_slot,
                obj_type,
                size_bits,
                sys::seL4_CapInitThreadCNode,
                dst_slot,
                depth.as_word(),
                0,
                1,
            )
        }
    }

    #[cfg(not(target_os = "none"))]
    {
        let depth = resolve_cnode_depth(init_cnode_bits);
        let _ = (
            init_cnode_bits,
            untyped_slot,
            obj_type,
            size_bits,
            dst_slot,
            depth.as_u8(),
            depth.as_word(),
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

#[cfg(test)]
mod tests {
    use super::{resolve_cnode_depth, CANONICAL_CNODE_DEPTH_BITS};

    #[test]
    fn resolve_cnode_depth_matches_requested_bits() {
        for bits in [1u8, 5, 13, CANONICAL_CNODE_DEPTH_BITS] {
            let depth = resolve_cnode_depth(bits);
            assert_eq!(depth.as_u8(), bits);
            assert_eq!(depth.as_word(), bits as super::sys::seL4_Word);

            // The helper still enforces the provided bounds to catch invalid inputs.
            assert!(bits <= CANONICAL_CNODE_DEPTH_BITS);
        }
    }
}
