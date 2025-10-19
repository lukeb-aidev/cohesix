// Author: Lukas Bower
#![allow(unsafe_code)]

use sel4_sys as sys;

/// Issues `seL4_CNode_Copy` using invocation addressing on both source and destination.
#[inline(always)]
pub fn cnode_copy_invocation(
    dst_slot: sys::seL4_CPtr,
    src_slot: sys::seL4_CPtr,
    rights: sys::seL4_CapRights,
) -> sys::seL4_Error {
    unsafe {
        sys::seL4_CNode_Copy(
            sys::seL4_CapInitThreadCNode,
            0,
            0u8,
            sys::seL4_CapInitThreadCNode,
            src_slot,
            0u8,
            rights,
            dst_slot,
        )
    }
}

/// Issues `seL4_CNode_Mint` using invocation addressing on both source and destination.
#[inline(always)]
pub fn cnode_mint_invocation(
    dst_slot: sys::seL4_CPtr,
    src_slot: sys::seL4_CPtr,
    rights: sys::seL4_CapRights,
    badge: sys::seL4_Word,
) -> sys::seL4_Error {
    unsafe {
        sys::seL4_CNode_Mint(
            sys::seL4_CapInitThreadCNode,
            0,
            0u8,
            sys::seL4_CapInitThreadCNode,
            src_slot,
            0u8,
            rights,
            badge,
            dst_slot,
        )
    }
}

/// Issues `seL4_Untyped_Retype` targeting the init thread CSpace via invocation addressing.
#[inline(always)]
pub fn untyped_retype_invocation(
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
            0,
            0u8,
            dst_slot,
            1,
        )
    }
}
