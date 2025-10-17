// Author: Lukas Bower
#![allow(dead_code)]

use core::fmt::Write;

use heapless::String;
use sel4_sys as sys;

use crate::bootstrap::cspace::CSpace;

#[cfg(feature = "bootstrap-trace")]
fn emit_trace(line: &str) {
    for &byte in line.as_bytes() {
        crate::sel4::debug_put_char(byte as i32);
    }
}

/// Retype a single object from `untyped_cap` into the init CSpace at a freshly allocated slot.
pub fn retype_one(
    untyped_cap: sys::seL4_CPtr,
    obj_type: sys::seL4_ObjectType,
    obj_size_bits: u8,
    cs: &mut CSpace,
) -> Result<sys::seL4_CPtr, sys::seL4_Error> {
    let Some(slot) = cs.alloc_slot() else {
        return Err(sys::seL4_Error::seL4_NotEnoughMemory);
    };

    let root = cs.root();
    let depth = sys::seL4_Word::from(cs.depth_bits());
    let slot_word = sys::seL4_Word::from(slot);

    #[cfg(feature = "bootstrap-trace")]
    {
        let mut line = String::<128>::new();
        let _ = write!(
            line,
            "[retype u=0x{untyped:04x} type=0x{ty:02x} size={size} root=0x{root:04x} slot=0x{slot:04x} depth={depth}]\r\n",
            untyped = untyped_cap,
            ty = obj_type as sys::seL4_Word,
            size = obj_size_bits,
            root = root,
            slot = slot_word,
            depth = depth,
        );
        emit_trace(line.as_str());
    }

    let result = unsafe {
        sys::seL4_Untyped_Retype(
            untyped_cap,
            obj_type as sys::seL4_Word,
            sys::seL4_Word::from(obj_size_bits),
            root,
            slot_word,
            depth,
            slot_word,
            1,
        )
    };

    if result == sys::seL4_Error::seL4_NoError {
        Ok(slot_word as sys::seL4_CPtr)
    } else {
        Err(result)
    }
}
