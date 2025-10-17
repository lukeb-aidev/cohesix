// Author: Lukas Bower
#![allow(dead_code)]

use sel4_sys as sys;

use super::cspace::CSpace;
use super::cspace_probe::probe_slot_writable;
use super::ffi::untyped_retype_one;

fn debug_put_unsigned(mut value: u64) {
    if value == 0 {
        crate::sel4::debug_put_char(b'0' as i32);
        return;
    }
    let mut buf = [0u8; 20];
    let mut index = buf.len();
    while value > 0 {
        index -= 1;
        buf[index] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    for &digit in &buf[index..] {
        crate::sel4::debug_put_char(digit as i32);
    }
}

fn debug_put_signed(value: isize) {
    if value < 0 {
        crate::sel4::debug_put_char(b'-' as i32);
        debug_put_unsigned(value.wrapping_abs() as u64);
    } else {
        debug_put_unsigned(value as u64);
    }
}

fn emit_retype_error(slot: u32, depth_bits: u8, error: sys::seL4_Error) {
    crate::sel4::debug_put_char(b'R' as i32);
    crate::sel4::debug_put_char(b'(' as i32);
    debug_put_unsigned(slot.into());
    crate::sel4::debug_put_char(b',' as i32);
    debug_put_unsigned(depth_bits.into());
    crate::sel4::debug_put_char(b',' as i32);
    debug_put_signed(error as isize);
    crate::sel4::debug_put_char(b')' as i32);
}

/// Retype a single object from `untyped_cap` into the init CSpace at a freshly allocated slot.
pub fn retype_one(
    untyped_cap: sys::seL4_CPtr,
    obj_type: sys::seL4_ObjectType,
    obj_size_bits: u8,
    cs: &mut CSpace,
) -> Result<sys::seL4_CPtr, sys::seL4_Error> {
    let Some(slot) = cs.alloc_slot() else {
        return Err(sys::seL4_NotEnoughMemory);
    };

    let (_, end) = cs.empty_bounds();
    if slot >= end {
        return Err(sys::seL4_RangeError);
    }

    probe_slot_writable(cs.root(), cs.depth_bits(), slot)?;

    let result = untyped_retype_one(
        untyped_cap,
        obj_type,
        obj_size_bits,
        cs.root(),
        slot as sys::seL4_Word,
        cs.depth_bits(),
    );

    if result != sys::seL4_NoError {
        emit_retype_error(slot, cs.depth_bits(), result);
        return Err(result);
    }

    Ok(slot as sys::seL4_CPtr)
}
