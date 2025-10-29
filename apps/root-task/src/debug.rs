// Author: Lukas Bower
#![allow(dead_code)]

use sel4_sys::{seL4_CPtr, seL4_Word};

/// Identifies the object type stored in the supplied capability slot.
#[inline(always)]
pub fn identify_cap(slot: seL4_CPtr) -> seL4_Word {
    crate::sel4::debug_cap_identify(slot)
}

/// Human-readable name for kernel object types reported by [`identify_cap`].
#[inline(always)]
#[must_use]
pub fn name_of_type(object_type: seL4_Word) -> &'static str {
    match object_type {
        0 => "Null",
        1 => "Untyped",
        2 => "Tcb",
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
