// Author: Lukas Bower
#![allow(dead_code)]
#![allow(non_camel_case_types)]
#![allow(unsafe_code)]

use crate::bootstrap::cspace::CSpaceCtx;
use sel4_sys as sys;

use super::cspace_sys;

pub fn cnode_mint_to_slot(
    ctx: &CSpaceCtx,
    dst_slot: sys::seL4_CPtr,
    src_slot: sys::seL4_CPtr,
    rights: sys::seL4_CapRights,
    badge: sys::seL4_Word,
) -> sys::seL4_Error {
    let err = cspace_sys::cnode_mint_dest_direct_src_invoc(
        ctx.init_cnode_bits,
        dst_slot,
        src_slot,
        rights,
        badge,
    );
    ctx.log_cnode_mint("MintToSlot", err, dst_slot, src_slot, rights, badge);
    err
}

pub fn untyped_retype_to_slot(
    ctx: &CSpaceCtx,
    untyped_cap: sys::seL4_CPtr,
    obj_type: sys::seL4_Word,
    size_bits: sys::seL4_Word,
    dst_slot: sys::seL4_CPtr,
) -> sys::seL4_Error {
    let err = cspace_sys::untyped_retype_dest_direct(
        ctx.init_cnode_bits,
        untyped_cap,
        obj_type,
        size_bits,
        dst_slot,
    );
    ctx.log_retype(
        "RetypeToSlot",
        err,
        untyped_cap,
        obj_type,
        size_bits,
        dst_slot,
    );
    err
}
