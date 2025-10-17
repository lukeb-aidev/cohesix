// Author: Lukas Bower
#![allow(non_camel_case_types)]

use sel4_sys as sys;

// One-shot wrappers to lock argument order/width on aarch64
pub unsafe fn cnode_mint_allrights(
    dest_root: sys::seL4_CPtr,
    dest_index: u64,
    dest_depth_bits: u32,
    src_root: sys::seL4_CPtr,
    src_index: u64,
    src_depth_bits: u32,
) -> sys::seL4_Error {
    let rights = sys::seL4_AllRights; // constant in sel4_sys (bitmask)
    sys::seL4_CNode_Mint(
        dest_root,
        dest_index,
        dest_depth_bits,
        src_root,
        src_index,
        src_depth_bits,
        rights,
        0, // badge
    )
}

pub unsafe fn cnode_delete(root: sys::seL4_CPtr, index: u64, depth_bits: u32) -> sys::seL4_Error {
    sys::seL4_CNode_Delete(root, index, depth_bits)
}

pub unsafe fn untyped_retype_one(
    untyped: sys::seL4_CPtr,
    obj_type: sys::seL4_Word,
    obj_bits: u8,
    dest_root: sys::seL4_CPtr,
    dest_index: u64,
    dest_depth_bits: u32,
) -> sys::seL4_Error {
    sys::seL4_Untyped_Retype(
        untyped,
        obj_type,
        obj_bits as sys::seL4_Word,
        dest_root,
        dest_index,
        dest_depth_bits as sys::seL4_Word,
        1,
    )
}
