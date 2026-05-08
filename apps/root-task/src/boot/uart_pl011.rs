// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Publish the UART frame slot selected by the HAL-backed bootstrap mapper.
// Author: Lukas Bower
//! Bootstrap helpers for publishing the HAL-mapped UART console frame slot.

use core::sync::atomic::{AtomicUsize, Ordering};

use sel4_sys::{self, seL4_CPtr};

static UART_FRAME_SLOT: AtomicUsize = AtomicUsize::new(sel4_sys::seL4_CapNull as usize);

/// Publish the capability slot holding the PL011 frame mapping.
pub fn publish_uart_slot(slot: seL4_CPtr) {
    UART_FRAME_SLOT.store(slot as usize, Ordering::Release);
}

/// Retrieve the published PL011 frame slot, if it has been mapped.
pub fn uart_slot() -> Option<seL4_CPtr> {
    let slot = UART_FRAME_SLOT.load(Ordering::Acquire) as seL4_CPtr;
    if slot == sel4_sys::seL4_CapNull {
        None
    } else {
        Some(slot)
    }
}
