// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines tests for root-task slot_alloc.
// Author: Lukas Bower

#![cfg(feature = "kernel")]

use root_task::sel4::{self, is_boot_reserved_slot, SlotAllocator};

#[test]
fn slot_allocator_skips_reserved_entries() {
    let region = sel4::seL4_SlotRegion { start: 0, end: 32 };
    let mut allocator = SlotAllocator::new(sel4::seL4_CapInitThreadCNode, region, 6);

    for _ in region.start..region.end {
        let Some(slot) = allocator.try_alloc() else {
            break;
        };
        assert!(
            !is_boot_reserved_slot(slot),
            "allocator returned reserved slot {slot:#x}"
        );
    }
}

#[test]
fn slot_allocator_exhaustion_reports_none() {
    let region = sel4::seL4_SlotRegion { start: 64, end: 68 };
    let mut allocator = SlotAllocator::new(sel4::seL4_CapInitThreadCNode, region, 8);

    for _ in 0..(region.end - region.start) {
        assert!(allocator.try_alloc().is_some());
    }
    assert!(allocator.try_alloc().is_none());
}
