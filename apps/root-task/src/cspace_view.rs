// Author: Lukas Bower

use sel4_sys::seL4_CPtr;

#[inline(always)]
pub const fn slot_index_as_cptr(slot: seL4_CPtr) -> seL4_CPtr {
    slot
}
