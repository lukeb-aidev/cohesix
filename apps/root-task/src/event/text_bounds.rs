// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the event/text_bounds module for root-task.
// Author: Lukas Bower
use core::ptr;

#[cfg(all(feature = "kernel", debug_assertions))]
extern "C" {
    #[link_name = "_text"]
    static __text_start: u8;
    #[link_name = "_end"]
    static __text_end: u8;
}

#[cfg(all(feature = "kernel", debug_assertions))]
/// Return the inclusive-exclusive address range of the executable text segment.
pub(super) fn text_region_bounds() -> (usize, usize) {
    let start = ptr::addr_of!(__text_start) as usize;
    let end = ptr::addr_of!(__text_end) as usize;
    (start, end)
}
