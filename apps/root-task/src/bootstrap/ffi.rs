// Author: Lukas Bower
#![allow(dead_code)]
#![allow(non_camel_case_types)]
#![allow(unsafe_code)]

use crate::bootstrap::cspace::CSpaceCtx;
use crate::sel4 as sys;

/// Helper that logs and forwards a `seL4_CNode_Mint` request through [`CSpaceCtx`].
pub fn cnode_mint_to_slot(
    ctx: &mut CSpaceCtx,
    dst_slot: sys::seL4_CPtr,
    src_slot: sys::seL4_CPtr,
    badge: sys::seL4_Word,
) -> sys::seL4_Error {
    let rights = crate::cspace::cap_rights_read_write_grant();
    let err = ctx.cspace.mint_here(dst_slot, src_slot, rights, badge);
    ctx.log_cnode_mint(err, dst_slot, src_slot, badge);
    err
}

/// Helper that logs and forwards a `seL4_Untyped_Retype` request through [`CSpaceCtx`].
pub fn untyped_retype_to_slot(
    ctx: &mut CSpaceCtx,
    untyped_cap: sys::seL4_CPtr,
    obj_type: sys::seL4_Word,
    size_bits: sys::seL4_Word,
    dst_slot: sys::seL4_CPtr,
) -> sys::seL4_Error {
    ctx.retype_to_slot(untyped_cap, obj_type, size_bits, dst_slot)
}

/// Thin wrapper around [`sys::seL4_Untyped_Retype`] that centralises the
/// unavoidable unsafe block required by the seL4 syscall binding.
pub fn raw_untyped_retype(
    ut_cap: sys::seL4_Word,
    obj_type: sys::seL4_Word,
    size_bits: sys::seL4_Word,
    dest_root: sys::seL4_CPtr,
    node_index: sys::seL4_CPtr,
    node_depth: sys::seL4_Word,
    node_offset: sys::seL4_Word,
    num_objects: sys::seL4_Word,
) -> sys::seL4_Error {
    unsafe {
        sys::seL4_Untyped_Retype(
            ut_cap,
            obj_type,
            size_bits,
            dest_root,
            node_index,
            node_depth,
            node_offset,
            num_objects,
        )
    }
}
