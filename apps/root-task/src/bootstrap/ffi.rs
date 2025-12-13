// Author: Lukas Bower
#![allow(dead_code)]
#![allow(non_camel_case_types)]
#![allow(unsafe_code)]

use crate::bootstrap::cspace::CSpaceCtx;
use crate::bootstrap::log::force_uart_line;
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
    if ut_cap == sys::seL4_CapNull || dest_root == sys::seL4_CapNull {
        ::log::error!(
            "[retype] seL4_Untyped_Retype null cap ut=0x{ut:04x} root=0x{root:04x}; returning error",
            ut = ut_cap,
            root = dest_root,
        );
        force_uart_line("boot: invalid cap arg (null) in retype; returning error");
        return sys::seL4_InvalidCapability;
    }
    if node_index == 0 || node_offset == 0 {
        ::log::error!(
            "[retype] seL4_Untyped_Retype attempted to use slot 0 index=0x{idx:04x} offset=0x{off:04x}; returning error",
            idx = node_index,
            off = node_offset,
        );
        force_uart_line("boot: invalid slot (0) in retype; returning error");
        return sys::seL4_InvalidArgument;
    }
    let word_bits = seL4_WordBits as usize;
    let hex_width = (word_bits + 3) / 4;
    let mut line = heapless::String::<128>::new();
    let _ = core::fmt::write(
        &mut line,
        format_args!(
            "[retype] ut=0x{ut:0width$x} root=0x{root:04x} depth={depth} index=0x{index:0width$x} offset=0x{offset:0width$x} n={num}",
            ut = ut_cap,
            root = dest_root,
            depth = node_depth,
            index = node_index,
            offset = node_offset,
            num = num_objects,
            width = hex_width,
        ),
    );
    force_uart_line(line.as_str());
    let err = unsafe {
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
    };
    if err != sys::seL4_NoError {
        force_uart_line("[retype] seL4_Untyped_Retype failed; returning error");
        ::log::error!("[retype] seL4_Untyped_Retype err={err}");
    }
    err
}

#[cfg(target_os = "none")]
/// Returns a validated [`BootInfoView`] captured from the running kernel.
pub fn bootinfo_view() -> Result<BootInfoView, BootInfoError> {
    let bootinfo_ptr = unsafe { sys::seL4_GetBootInfo() };
    unsafe { BootInfoView::from_ptr(bootinfo_ptr) }
}
