// Author: Lukas Bower
#![allow(unsafe_code)]

use crate::sel4 as sys;

/// Issues a `seL4_CNode_Mint` syscall using direct addressing semantics.
#[inline(always)]
pub fn cnode_mint_direct(
    dest_root: sys::seL4_CPtr,
    dest_index: sys::seL4_CPtr,
    dest_depth_bits: u8,
    src_root: sys::seL4_CPtr,
    src_index: sys::seL4_CPtr,
    src_depth_bits: u8,
    rights: sys::seL4_CapRights,
    badge: sys::seL4_Word,
    dest_offset: sys::seL4_CPtr,
) -> sys::seL4_Error {
    unsafe {
        sys::seL4_CNode_Mint(
            dest_root,
            dest_index,
            dest_depth_bits,
            src_root,
            src_index,
            src_depth_bits,
            rights,
            badge,
            dest_offset,
        )
    }
}

/// Issues a `seL4_Untyped_Retype` syscall using direct addressing semantics.
#[inline(always)]
pub fn untyped_retype_direct(
    untyped: sys::seL4_CPtr,
    obj_type: sys::seL4_Word,
    size_bits: sys::seL4_Word,
    dest_root: sys::seL4_CPtr,
    dest_index: sys::seL4_CPtr,
    dest_depth_bits: u8,
    dest_offset: sys::seL4_CPtr,
) -> sys::seL4_Error {
    unsafe {
        sys::seL4_Untyped_Retype(
            untyped,
            obj_type,
            size_bits,
            dest_root,
            dest_index,
            sys::seL4_Word::from(dest_depth_bits),
            dest_offset,
            1,
        )
    }
}
