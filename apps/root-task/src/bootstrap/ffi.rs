// Author: Lukas Bower
#![allow(non_camel_case_types)]
#![allow(unsafe_code)]

use sel4_sys as sys;

// One-shot wrappers to lock argument order/width on aarch64
pub fn cnode_mint_allrights(
    dest_root: sys::seL4_CNode,
    dest_index: sys::seL4_CPtr,
    dest_depth_bits: u8,
    src_root: sys::seL4_CNode,
    src_index: sys::seL4_CPtr,
    src_depth_bits: u8,
) -> sys::seL4_Error {
    unsafe {
        sys::seL4_CNode_Mint(
            dest_root,
            dest_index,
            dest_depth_bits,
            src_root,
            src_index,
            src_depth_bits,
            sys::seL4_CapRights_All,
            0,
        )
    }
}

pub fn cnode_delete(
    root: sys::seL4_CNode,
    index: sys::seL4_CPtr,
    depth_bits: u8,
) -> sys::seL4_Error {
    unsafe { sys::seL4_CNode_Delete(root, index, depth_bits) }
}

pub fn untyped_retype_one(
    untyped: sys::seL4_Untyped,
    obj_type: sys::seL4_ObjectType,
    obj_bits: u8,
    dest_root: sys::seL4_CNode,
    dest_index: sys::seL4_CPtr,
    dest_depth_bits: u8,
) -> sys::seL4_Error {
    // SAFETY: The wrapper fixes the argument ordering to match the seL4 C API and supplies
    // exactly one object with zero offset. The kernel contract for these arguments is upheld
    // by the callers in the bootstrap sequence.
    unsafe {
        sys::seL4_Untyped_Retype(
            untyped,
            obj_type as sys::seL4_Word,
            obj_bits as sys::seL4_Word,
            dest_root,
            dest_index,
            dest_depth_bits as sys::seL4_Word,
            0,
            1,
        )
    }
}
