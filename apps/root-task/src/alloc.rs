// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the alloc module for root-task.
// Author: Lukas Bower
//! Global heap allocator initialised for kernel builds before dynamic memory is required.

#![cfg(feature = "kernel")]
#![allow(unsafe_code)]

use core::alloc::{GlobalAlloc, Layout};
use core::ops::Range;
#[cfg(target_os = "none")]
use core::sync::atomic::{AtomicBool, Ordering};

#[cfg(target_os = "none")]
use linked_list_allocator::LockedHeap;

use crate::bootstrap::{log as boot_log, no_alloc};

/// Statically reserved heap span used during bootstrap.
pub const HEAP_BYTES: usize = 2 * 1024 * 1024;

#[cfg(target_os = "none")]
static HEAP_INITIALISED: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "none")]
struct GuardedAllocator {
    inner: LockedHeap,
}

#[cfg(target_os = "none")]
impl GuardedAllocator {
    const fn new() -> Self {
        Self {
            inner: LockedHeap::empty(),
        }
    }

    unsafe fn init(&self, span: Range<usize>) {
        unsafe {
            self.inner
                .lock()
                .init(span.start as *mut u8, span.end.saturating_sub(span.start));
        }
    }

    #[inline(always)]
    fn zero_sized_dangling(layout: Layout) -> *mut u8 {
        layout.align() as *mut u8
    }
}

#[cfg(target_os = "none")]
unsafe impl GlobalAlloc for GuardedAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !no_alloc::alloc_ready() {
            no_alloc::assert_no_alloc("alloc");
        }

        if layout.size() == 0 {
            return Self::zero_sized_dangling(layout);
        }

        unsafe { self.inner.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if !no_alloc::alloc_ready() {
            no_alloc::assert_no_alloc("dealloc");
        }

        if layout.size() == 0 {
            return;
        }

        unsafe { self.inner.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if !no_alloc::alloc_ready() {
            no_alloc::assert_no_alloc("alloc_zeroed");
        }

        if layout.size() == 0 {
            return Self::zero_sized_dangling(layout);
        }

        unsafe { self.inner.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if !no_alloc::alloc_ready() {
            no_alloc::assert_no_alloc("realloc");
        }

        if new_size == 0 {
            if layout.size() != 0 {
                unsafe { self.inner.dealloc(ptr, layout) }
            }
            return Self::zero_sized_dangling(layout);
        }

        if layout.size() == 0 {
            let Ok(new_layout) = Layout::from_size_align(new_size, layout.align()) else {
                return core::ptr::null_mut();
            };
            return unsafe { self.inner.alloc(new_layout) };
        }

        unsafe { self.inner.realloc(ptr, layout, new_size) }
    }
}

#[cfg(target_os = "none")]
#[global_allocator]
static GLOBAL_ALLOCATOR: GuardedAllocator = GuardedAllocator::new();

#[cfg(not(target_os = "none"))]
#[global_allocator]
static GLOBAL_ALLOCATOR: std::alloc::System = std::alloc::System;

#[cfg(target_os = "none")]
fn report_heap_error(tag: &str, detail: &str) -> ! {
    let mut line = heapless::String::<96>::new();
    let _ = core::fmt::write(&mut line, format_args!("[alloc:init] {tag}: {detail}"));
    boot_log::force_uart_line(line.as_str());
    panic!("{tag}: {detail}");
}

/// Installs the global allocator over the supplied heap span once all layout checks pass.
#[cfg(target_os = "none")]
pub fn init_heap(span: Range<usize>) {
    if span.start >= span.end {
        report_heap_error("invalid-span", "heap start >= end");
    }

    if (span.start & ((1usize << sel4_sys::seL4_PageBits) - 1)) != 0 {
        report_heap_error("misaligned-span", "heap start not page aligned");
    }

    if HEAP_INITIALISED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    unsafe {
        GLOBAL_ALLOCATOR.init(span);
    }

    no_alloc::mark_alloc_ready();
    boot_log::force_uart_line("[boot] allocator ready");
}

/// Host-test allocator initialisation is a no-op because the system allocator is active.
#[cfg(not(target_os = "none"))]
pub fn init_heap(_span: Range<usize>) {
    no_alloc::mark_alloc_ready();
}
