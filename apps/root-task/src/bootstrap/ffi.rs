// Author: Lukas Bower
#![allow(dead_code)]
#![allow(non_camel_case_types)]
#![allow(unsafe_code)]

use core::fmt::Write;

use crate::bootstrap::cspace::CSpaceCtx;
use crate::sel4::{self, debug_put_char};
use heapless::String;
use sel4_sys as sys;

use super::cspace_sys;

const MAX_DIAGNOSTIC_LEN: usize = 224;

pub fn cnode_mint_to_slot(
    ctx: &CSpaceCtx,
    dst_slot: sys::seL4_CPtr,
    src_slot: sys::seL4_CPtr,
    rights: sys::seL4_CapRights,
    badge: sys::seL4_Word,
) -> sys::seL4_Error {
    let dest_root = ctx.root_cnode_cap;
    let dest_depth = 0u8;
    let src_depth = 0u8;
    let err =
        cspace_sys::cnode_mint_invocation(dest_root, dst_slot, dest_root, src_slot, rights, badge);
    if err != sys::seL4_NoError {
        log_cnode_mint_failure(
            err, dst_slot, dest_depth, dst_slot, src_slot, src_depth, rights, badge,
        );
    }
    err
}

pub fn untyped_retype_to_slot(
    ctx: &CSpaceCtx,
    untyped_cap: sys::seL4_CPtr,
    obj_type: sys::seL4_Word,
    size_bits: sys::seL4_Word,
    dst_slot: sys::seL4_CPtr,
) -> sys::seL4_Error {
    let dest_root = ctx.root_cnode_cap;
    let dest_depth = 0u8;
    let err = cspace_sys::untyped_retype_invocation(
        untyped_cap,
        obj_type,
        size_bits,
        dest_root,
        dst_slot,
    );
    if err != sys::seL4_NoError {
        log_untyped_retype_failure(
            err,
            untyped_cap,
            obj_type,
            size_bits,
            dst_slot,
            dest_root,
            dest_depth,
        );
    }
    err
}

fn log_cnode_mint_failure(
    err: sys::seL4_Error,
    dest_index: sys::seL4_CPtr,
    dest_depth: u8,
    dest_offset: sys::seL4_CPtr,
    src_index: sys::seL4_CPtr,
    src_depth: u8,
    rights: sys::seL4_CapRights,
    badge: sys::seL4_Word,
) {
    let mut line = String::<MAX_DIAGNOSTIC_LEN>::new();
    let _ = write!(
        &mut line,
        "[cnode] op=Mint err={code} dest_index={dest_index} dest_depth={dest_depth} dest_offset=0x{dest_offset:04x} src_index=0x{src:04x} src_depth={src_depth} rights=0x{rights:08x} badge=0x{badge:08x}",
        code = err,
        dest_index = dest_index,
        dest_depth = usize::from(dest_depth),
        dest_offset = dest_offset,
        src = src_index,
        src_depth = usize::from(src_depth),
        rights = rights.raw(),
        badge = badge,
    );
    for byte in line.as_bytes() {
        debug_put_char(*byte as i32);
    }
    debug_put_char(b'\n' as i32);
}

fn log_untyped_retype_failure(
    err: sys::seL4_Error,
    untyped: sys::seL4_CPtr,
    obj_type: sys::seL4_Word,
    obj_bits: sys::seL4_Word,
    dest_slot: sys::seL4_CPtr,
    _dest_root: sys::seL4_CNode,
    guard_depth: u8,
) {
    let mut line = String::<MAX_DIAGNOSTIC_LEN>::new();
    let _ = write!(
        &mut line,
        "[cnode] op=Retype err={code} dest_index=0x{dest_slot:04x} dest_depth={guard_depth} dest_offset=0x{dest_slot:04x} src_untyped=0x{untyped:08x} obj_type=0x{obj_type:08x} obj_bits={obj_bits}",
        code = err,
        guard_depth = guard_depth,
        dest_slot = dest_slot,
        untyped = untyped,
        obj_type = obj_type,
        obj_bits = obj_bits,
    );
    for byte in line.as_bytes() {
        sel4::debug_put_char(*byte as i32);
    }
    sel4::debug_put_char(b'\n' as i32);
}
