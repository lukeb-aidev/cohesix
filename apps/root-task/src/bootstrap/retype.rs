// Author: Lukas Bower

use core::fmt::Write;

use crate::sel4;
use heapless::String;
use sel4_sys as sys;

use super::cspace::CSpace;
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
    assert!(slot >= lo && slot < hi, "dest slot out of bootinfo.empty");
    assert!(
        !CSpace::is_reserved_slot(slot),
        "attempted to allocate into a reserved capability slot"
    );

    // Retype — if THIS fails, we print 'R' and dump params
    let dest_root = cs.root_writable();
    let node_index = 0;
    let guard_depth: sys::seL4_Word = 0;
    let dest_offset = slot;
    let err = untyped_retype_one(
        untyped,
        obj_type,
        obj_bits,
        dest_root,
        node_index,
        0,
        dest_offset,
    );
    if err != sys::seL4_NoError {
        sel4::debug_put_char(b'R' as i32);
        // 1-char crumbs: guard depth (0..) and last hex nibble of slot
        let d = b'0' + ((guard_depth as u8) & 0x0F);
        let n = b'0' + ((slot as u8) & 0x0F);
        sel4::debug_put_char(d as i32);
        sel4::debug_put_char(n as i32);
        log_retype_failure(
            err,
            untyped,
            obj_type,
            obj_bits,
            dest_root,
            node_index,
            guard_depth,
            dest_offset,
            lo,
            hi,
        );
        return Err(err);
    }
    Ok(slot)
}

fn log_retype_failure(
    err: sys::seL4_Error,
    untyped: sys::seL4_CPtr,
    obj_type: sys::seL4_ObjectType,
    obj_bits: u8,
    dest_root: sys::seL4_CPtr,
    node_index: sys::seL4_CPtr,
    guard_depth: sys::seL4_Word,
    dest_offset: sys::seL4_CPtr,
    empty_lo: sys::seL4_CPtr,
    empty_hi: sys::seL4_CPtr,
) {
    let mut line = String::<256>::new();
    let _ = write!(
        &mut line,
        "retype status=err({code}:{name}) raw.untyped=0x{untyped:08x} raw.obj_type={obj_type:?} \
         raw.size_bits={obj_bits} raw.dest_root=0x{dest_root:08x} raw.node_index=0x{node_index:08x} \
         raw.guard_depth={guard_depth} raw.dest_offset=0x{dest_offset:08x} boot.empty.lo=0x{empty_lo:08x} \
         boot.empty.hi=0x{empty_hi:08x}",
        code = err,
        name = sel4::error_name(err),
    );
    for byte in line.as_bytes() {
        sel4::debug_put_char(*byte as i32);
    }
    sel4::debug_put_char(b'\n' as i32);
}
