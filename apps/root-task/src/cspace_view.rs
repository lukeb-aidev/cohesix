// Author: Lukas Bower

use sel4_sys::seL4_CPtr;

/// Convert an init CSpace slot index into a capability pointer usable by syscalls.
#[inline(always)]
#[must_use]
pub const fn slot_as_cptr(slot_index: seL4_CPtr) -> seL4_CPtr {
    slot_index
}
