// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the cspace_view module for root-task.
// Author: Lukas Bower

use sel4_sys::seL4_CPtr;

#[inline(always)]
pub const fn slot_index_as_cptr(slot: seL4_CPtr) -> seL4_CPtr {
    slot
}
