// Author: Lukas Bower

use crate::sel4::BootInfo;
use sel4_sys as sys;

/// Returns the first RAM-backed untyped capability satisfying the requested size.
pub fn pick_untyped(bi: &'static BootInfo, min_bits: u8) -> sys::seL4_CPtr {
    let total = (bi.untyped.end - bi.untyped.start) as usize;
    let entries = &bi.untypedList[..total];

    for (offset, ut) in entries.iter().enumerate() {
        if ut.isDevice == 0 && (ut.sizeBits as u8) >= min_bits {
            return bi.untyped.start + offset as sys::seL4_CPtr;
        }
    }

    entries
        .iter()
        .enumerate()
        .find(|(_, ut)| ut.isDevice == 0)
        .map(|(offset, _)| bi.untyped.start + offset as sys::seL4_CPtr)
        .expect("bootinfo must provide at least one RAM-backed untyped capability")
}
