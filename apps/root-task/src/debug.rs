// Author: Lukas Bower
#![allow(dead_code)]

use crate::sel4;
use sel4_sys::{seL4_CPtr, seL4_Word};

/// Identify the kernel object type stored in the supplied capability pointer.
#[inline(always)]
#[must_use]
pub fn identify_cap(cap: seL4_CPtr) -> seL4_Word {
    sel4::debug_cap_identify(cap)
}

/// Human-readable kernel object type names for debug logging.
#[inline(always)]
#[must_use]
pub fn name_of_type(object_type: seL4_Word) -> &'static str {
    match object_type {
        0 => "Null",
        1 => "Untyped",
        2 => "TCB",
        3 => "Endpoint",
        4 => "Notification",
        5 => "CNode",
        6 => "Frame",
        7 => "PageTable",
        8 => "PageDirectory",
        9 => "ASIDPool",
        10 => "ASIDControl",
        _ => "Unknown",
    }
}
