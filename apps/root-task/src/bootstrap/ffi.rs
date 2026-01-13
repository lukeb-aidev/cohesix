// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the bootstrap/ffi module for root-task.
// Author: Lukas Bower
#![allow(dead_code)]
#![allow(non_camel_case_types)]
#![allow(unsafe_code)]

use crate::bootstrap::cspace::CSpaceCtx;
use crate::sel4 as sys;
#[cfg(target_os = "none")]
use crate::sel4::{BootInfoError, BootInfoView};
use sel4_sys::seL4_WordBits;

/// Helper that logs and forwards a `seL4_CNode_Mint` request through [`CSpaceCtx`].
pub fn cnode_mint_to_slot(
    ctx: &mut CSpaceCtx,
    dst_slot: sys::seL4_CPtr,
    src_slot: sys::seL4_CPtr,
    badge: sys::seL4_Word,
) -> sys::seL4_Error {
    let rights = crate::cspace::cap_rights_read_write_grant();
    ctx.mint_raw_from_root(dst_slot, src_slot, rights, badge)
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
    node_depth: u8,
    node_offset: sys::seL4_Word,
    num_objects: sys::seL4_Word,
) -> sys::seL4_Error {
    let word_bits = seL4_WordBits as usize;
    let hex_width = (word_bits + 3) / 4;
    ::log::info!(
        "[retype] ut=0x{ut:0width$x} root=0x{root:04x} depth={depth} index=0x{index:0width$x} offset=0x{offset:0width$x} n={num}",
        ut = ut_cap,
        root = dest_root,
        depth = node_depth,
        index = node_index,
        offset = node_offset,
        num = num_objects,
        width = hex_width,
    );
    unsafe {
        sys::seL4_Untyped_Retype(
            ut_cap,
            obj_type,
            size_bits,
            dest_root,
            node_index,
            u64::from(node_depth),
            node_offset,
            num_objects,
        )
    }
}

#[cfg(target_os = "none")]
/// Returns a validated [`BootInfoView`] captured from the running kernel.
pub fn bootinfo_view() -> Result<BootInfoView, BootInfoError> {
    let bootinfo_ptr = unsafe { sys::seL4_GetBootInfo() };
    unsafe { BootInfoView::from_ptr(bootinfo_ptr) }
}
