// Author: Lukas Bower
#![allow(dead_code)]
#![allow(non_camel_case_types)]

use sel4_sys as sys;

/// Safe wrapper ensuring the Rust call-site order matches the kernel's ABI expectation.
#[inline(always)]
pub fn untyped_retype_one(
    untyped: sys::seL4_CPtr,
    obj_type: sys::seL4_ObjectType,
    obj_size_bits: u8,
    dest_root: sys::seL4_CPtr,
    dest_index: sys::seL4_Word,
    dest_depth: u8,
) -> sys::seL4_Error {
    sys::seL4_untyped_retype(
        untyped,
        obj_type,
        obj_size_bits,
        dest_root,
        dest_index,
        dest_depth as sys::seL4_Word,
        0,
        1,
    )
}
