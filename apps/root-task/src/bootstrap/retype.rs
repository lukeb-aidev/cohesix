// Author: Lukas Bower

use crate::sel4;
use sel4_sys as sys;

use super::cspace::CSpace;
use super::cspace_probe::probe_slot_writable;

use super::ffi::untyped_retype_one;

/// Retypes a single capability-sized object into the init CSpace using the provided allocator.
pub fn retype_one(
    untyped: sys::seL4_CPtr,
    obj_type: sys::seL4_ObjectType,
    obj_bits: u8,
    cs: &mut CSpace,
) -> Result<sys::seL4_CPtr, sys::seL4_Error> {
    let (lo, hi) = cs.bounds();
    let Some(slot) = cs.alloc_slot() else {
        return Err(sys::seL4_NotEnoughMemory);
    };
    if !(slot >= lo && slot < hi) {
        return Err(sys::seL4_RangeError);
    }

    // Mint/Delete probe — if THIS fails, we print 'M' and return its error
    if let Err(e) = probe_slot_writable(cs.root, cs.depth_bits, slot) {
        sel4::debug_put_char(b'M' as i32);
        return Err(e);
    }

    // Retype — if THIS fails, we print 'R' and dump params
    let err = untyped_retype_one(untyped, obj_type, obj_bits, cs.root, slot, cs.depth_bits);
    if err != sys::seL4_NoError {
        sel4::debug_put_char(b'R' as i32);
        // 1-char crumbs: depth (0..) and last hex nibble of slot
        let d = b'0' + (cs.depth_bits & 0x0F);
        let n = b'0' + ((slot as u8) & 0x0F);
        sel4::debug_put_char(d as i32);
        sel4::debug_put_char(n as i32);
        return Err(err);
    }
    Ok(slot)
}
