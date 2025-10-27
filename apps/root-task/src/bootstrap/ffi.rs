// Author: Lukas Bower
#![allow(dead_code)]
#![allow(non_camel_case_types)]
#![allow(unsafe_code)]

use crate::bootstrap::cspace::{CSpaceCtx, DestCNode};
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
    ctx: &CSpaceCtx,
    untyped_cap: sys::seL4_CPtr,
    obj_type: sys::seL4_Word,
    size_bits: sys::seL4_Word,
    dst_slot: sys::seL4_CPtr,
) -> sys::seL4_Error {
    let (err, root_cap, node_index, node_depth, node_offset, path_label) = match ctx.dest {
        DestCNode::Init => {
            let node_index = 0;
            let node_depth = super::cspace_sys::encode_cnode_depth(
                super::cspace_sys::INIT_CNODE_RETYPE_DEPTH_BITS,
            );
            let node_offset = dst_slot as sys::seL4_Word;
            let err = match super::cspace_sys::untyped_retype_into_init_root(
                untyped_cap,
                obj_type,
                size_bits,
                dst_slot,
            ) {
                Ok(()) => sys::seL4_NoError,
                Err(err) => err.into_sel4_error(),
            };
            (
                err,
                sys::seL4_CapInitThreadCNode,
                node_index,
                node_depth,
                node_offset,
                DestCNode::Init.label(),
            )
        }
        DestCNode::Other { cap, bits } => (
            super::cspace_sys::untyped_retype_into_cnode(
                cap,
                bits,
                untyped_cap,
                obj_type,
                size_bits,
                dst_slot,
            ),
            cap,
            dst_slot as sys::seL4_Word,
            super::cspace_sys::encode_cnode_depth(bits),
            0,
            DestCNode::Other { cap, bits }.label(),
        ),
    };
    ctx.log_retype(
        err,
        root_cap,
        untyped_cap,
        obj_type,
        size_bits,
        dst_slot,
        node_index,
        node_depth,
        node_offset,
        path_label,
    );
    err
}
