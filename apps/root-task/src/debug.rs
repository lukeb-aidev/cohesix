// Author: Lukas Bower
#![allow(dead_code)]
#![allow(unsafe_code)]

use sel4_sys::{seL4_CPtr, seL4_Word};

extern "C" {
    fn seL4_DebugCapIdentify(cap: seL4_CPtr) -> seL4_Word;
}

#[inline(always)]
pub fn debug_identify(cap: seL4_CPtr) -> seL4_Word {
    unsafe { seL4_DebugCapIdentify(cap) }
}
