// Author: Lukas Bower
#![allow(dead_code)]

use crate::sel4::BootInfo;
use sel4_sys as sys;

/// Selects a RAM-backed untyped capability large enough to host the requested object size.
#[must_use]
pub fn pick_untyped(bi: &'static BootInfo, min_bits: u8) -> sys::seL4_CPtr {
    let total = (bi.untyped.end - bi.untyped.start) as usize;
    let entries = &bi.untypedList[..total];
    let mut fallback: Option<sys::seL4_CPtr> = None;

    for (index, desc) in entries.iter().enumerate() {
        if desc.isDevice != 0 {
            continue;
        }
        let cap = bi.untyped.start + index as sys::seL4_CPtr;
        if fallback.is_none() {
            fallback = Some(cap);
        }
        if desc.sizeBits >= min_bits {
            return cap;
        }
    }

    fallback.expect("bootinfo must provide at least one RAM-backed untyped capability")
}
