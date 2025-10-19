// Author: Lukas Bower
#![allow(unsafe_code)]

use sel4_sys as sys;

/// Mint: DEST = direct addressing (depth = init_cnode_bits), SRC = invocation (depth = 0).
#[inline(always)]
pub fn cnode_mint_dest_direct_src_invoc(
    init_cnode_bits: u8,
    dst_slot: sys::seL4_CPtr,
    src_slot: sys::seL4_CPtr,
    rights: sys::seL4_CapRights,
    badge: sys::seL4_Word,
) -> sys::seL4_Error {
    unsafe {
        sys::seL4_CNode_Mint(
            sys::seL4_CapInitThreadCNode,
            sys::seL4_CapInitThreadCNode,
            init_cnode_bits,
            sys::seL4_CapInitThreadCNode,
            src_slot,
            0u8,
            rights,
            badge,
            dst_slot,
        )
    }
}

/// Retype: DEST = direct addressing.
#[inline(always)]
pub fn untyped_retype_dest_direct(
    init_cnode_bits: u8,
    untyped: sys::seL4_CPtr,
    obj_type: sys::seL4_Word,
    size_bits: sys::seL4_Word,
    dst_slot: sys::seL4_CPtr,
) -> sys::seL4_Error {
    unsafe {
        sys::seL4_Untyped_Retype(
            untyped,
            obj_type,
            size_bits,
            sys::seL4_CapInitThreadCNode,
            sys::seL4_CapInitThreadCNode,
            init_cnode_bits.into(),
            dst_slot,
            1,
        )
    }
}

/// Delete: DEST = direct addressing.
#[inline(always)]
pub fn cnode_delete_dest_direct(init_cnode_bits: u8, dst_slot: sys::seL4_CPtr) -> sys::seL4_Error {
    unsafe {
        sys::seL4_CNode_Delete(
            sys::seL4_CapInitThreadCNode,
            dst_slot,
            init_cnode_bits.into(),
        )
    }
}
