// Author: Lukas Bower
#![allow(unsafe_code)]

use sel4_sys as sys;

/// Issues a `seL4_CNode_Mint` syscall using direct addressing semantics.
#[inline(always)]
pub fn cnode_mint_direct(
    dest_root: sys::seL4_CNode,
    dest_index: sys::seL4_CPtr,
    dest_depth_bits: u8,
    src_root: sys::seL4_CNode,
    src_index: sys::seL4_CPtr,
    src_depth_bits: u8,
    rights: sys::seL4_CapRights,
    badge: sys::seL4_Word,
    dest_offset: sys::seL4_Word,
) -> sys::seL4_Error {
    // SAFETY: The caller guarantees that all parameters describe valid capabilities and slot
    // indices. The invocation mirrors the layout used by `sel4_sys::seL4_CNode_Mint` but extends
    // the message to carry the direct `dest_offset` parameter in MR6.
    unsafe {
        let mut mr0 = dest_index;
        let mut mr1 = sys::seL4_Word::from(dest_depth_bits);
        let mut mr2 = src_index;
        let mut mr3 = sys::seL4_Word::from(src_depth_bits);

        sys::seL4_SetCap(0, src_root);
        sys::seL4_SetMR(4, rights.raw());
        sys::seL4_SetMR(5, badge);
        sys::seL4_SetMR(6, dest_offset);

        let msg = sys::seL4_MessageInfo::new(7, 0, 1, 7);
        let info = sys::seL4_CallWithMRs(dest_root, msg, &mut mr0, &mut mr1, &mut mr2, &mut mr3);

        info.label() as sys::seL4_Error
    }
}

/// Issues a `seL4_Untyped_Retype` syscall using direct addressing semantics.
#[inline(always)]
pub fn untyped_retype_direct(
    untyped: sys::seL4_Untyped,
    obj_type: sys::seL4_Word,
    size_bits: sys::seL4_Word,
    dest_root: sys::seL4_CNode,
    dest_index: sys::seL4_Word,
    dest_depth_bits: u8,
    dest_offset: sys::seL4_Word,
) -> sys::seL4_Error {
    // SAFETY: The caller provides a writable CNode capability and slot offset derived from the
    // bootinfo empty window, ensuring the kernel may materialise the requested object directly.
    unsafe {
        let mut mr0 = obj_type;
        let mut mr1 = size_bits;
        let mut mr2 = dest_index;
        let mut mr3 = sys::seL4_Word::from(dest_depth_bits);

        sys::seL4_SetCap(0, dest_root);
        sys::seL4_SetMR(4, dest_offset);
        sys::seL4_SetMR(5, 1);

        let msg = sys::seL4_MessageInfo::new(1, 0, 1, 6);
        let info = sys::seL4_CallWithMRs(untyped, msg, &mut mr0, &mut mr1, &mut mr2, &mut mr3);

        info.label() as sys::seL4_Error
    }
}
