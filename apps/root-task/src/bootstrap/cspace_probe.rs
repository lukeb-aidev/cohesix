// Author: Lukas Bower
#![allow(dead_code)]

use crate::sel4::debug_put_char;
use sel4_sys as sys;

/// Copy the init thread's root CNode capability into `slot` and delete it again to prove
/// that the location is writable from our bootstrap CSpace view.
pub fn probe_slot_writable(
    root: sys::seL4_CPtr,
    depth_bits: u8,
    slot: u32,
) -> Result<(), sys::seL4_Error> {
    let dest_index = slot as sys::seL4_CPtr;
    let src_index = sys::seL4_CapInitThreadCNode;
    let rights = sys::seL4_CapRights::new(0, 0, 1, 1);

    let copy_result = unsafe {
        sys::seL4_CNode_Copy(
            root, dest_index, depth_bits, root, src_index, depth_bits, rights,
        )
    };
    if copy_result != sys::seL4_NoError {
        return Err(copy_result);
    }

    let delete_result = unsafe { sys::seL4_CNode_Delete(root, dest_index, depth_bits) };
    if delete_result != sys::seL4_NoError {
        return Err(delete_result);
    }

    debug_put_char(b'P' as i32);

    Ok(())
}
