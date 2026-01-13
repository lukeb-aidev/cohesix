// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the bootstrap/no_alloc module for root-task.
// Author: Lukas Bower
//! Allocation guards for early bootstrap paths.

#![allow(dead_code)]

use core::fmt::Write;
use core::sync::atomic::{AtomicBool, Ordering};

use heapless::String;

use crate::bootstrap::log as boot_log;

const MAX_LINE: usize = 96;

static BOOT_ALLOC_READY: AtomicBool = AtomicBool::new(false);

/// Returns whether the allocator has been initialised and cleared for use.
#[must_use]
pub fn alloc_ready() -> bool {
    BOOT_ALLOC_READY.load(Ordering::Acquire)
}

/// Marks the allocator as ready exactly once.
pub fn mark_alloc_ready() {
    BOOT_ALLOC_READY.store(true, Ordering::Release);
}

/// Panics immediately if allocations are attempted before the allocator is ready.
pub fn assert_no_alloc(tag: &str) -> ! {
    let mut line = String::<MAX_LINE>::new();
    let _ = write!(line, "[alloc:block] {tag} before allocator ready");
    boot_log::force_uart_line(line.as_str());
    panic!("allocation attempted before allocator ready ({tag})");
}
