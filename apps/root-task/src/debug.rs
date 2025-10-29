// Author: Lukas Bower
#![allow(dead_code)]
#![allow(unsafe_code)]

use sel4_sys::{seL4_CPtr, seL4_Word};

extern "C" {
    fn seL4_DebugCapIdentify(cap: seL4_CPtr) -> seL4_Word;
}

#[inline(always)]
pub fn identify_cap(cap: seL4_CPtr) -> seL4_Word {
    unsafe { seL4_DebugCapIdentify(cap) }
}

#[inline(always)]
pub fn type_name(ty: seL4_Word) -> &'static str {
    match ty {
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
