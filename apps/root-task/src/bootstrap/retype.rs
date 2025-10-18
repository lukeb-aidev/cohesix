// Author: Lukas Bower

use core::fmt::Write;

use crate::sel4;
use heapless::String;
use sel4_sys as sys;

use super::cspace::CSpaceCtx;

const MAX_DIAGNOSTIC_LEN: usize = 256;

/// Retypes a single kernel object from an untyped capability into the init CSpace.
pub fn retype_one(
    ctx: &mut CSpaceCtx,
    untyped: sys::seL4_CPtr,
    obj_type: sys::seL4_ObjectType,
    obj_bits: u8,
) -> Result<sys::seL4_CPtr, sys::seL4_Error> {
    let slot = match ctx.alloc_slot_checked() {
        Ok(slot) => slot,
        Err(err) => {
            ctx.log_slot_failure(err);
            return Err(sys::seL4_RangeError);
        }
    };

    let err = ctx.retype_to_slot(
        untyped,
        obj_type as sys::seL4_Word,
        obj_bits as sys::seL4_Word,
        slot,
    );
    if err != sys::seL4_NoError {
        let (lo, hi) = ctx.empty_bounds();
        log_retype_failure(err, untyped, obj_type, obj_bits, slot, 0, lo, hi);
        return Err(err);
    }

    Ok(slot)
}

fn log_retype_failure(
    err: sys::seL4_Error,
    untyped: sys::seL4_CPtr,
    obj_type: sys::seL4_ObjectType,
    obj_bits: u8,
    dest_slot: sys::seL4_CPtr,
    guard_depth: sys::seL4_Word,
    empty_lo: sys::seL4_CPtr,
    empty_hi: sys::seL4_CPtr,
) {
    let mut line = String::<MAX_DIAGNOSTIC_LEN>::new();
    let _ = write!(
        &mut line,
        "Untyped_Retype err={code} dest_index=0x{dest_slot:04x} dest_depth={guard_depth} dest_offset=0x{dest_slot:04x} \\n         src_untyped=0x{untyped:08x} obj_type={obj_type:?} obj_bits={obj_bits} boot.empty.lo=0x{empty_lo:08x} boot.empty.hi=0x{empty_hi:08x}",
        code = err,
        guard_depth = guard_depth,
        dest_slot = dest_slot,
        untyped = untyped,
        obj_type = obj_type,
        obj_bits = obj_bits,
        empty_lo = empty_lo,
        empty_hi = empty_hi,
    );
    for byte in line.as_bytes() {
        sel4::debug_put_char(*byte as i32);
    }
    sel4::debug_put_char(b'\n' as i32);
}
