// Author: Lukas Bower
//! Global heap allocator initialised for kernel builds before dynamic memory is required.

#![cfg(feature = "kernel")]
#![allow(unsafe_code)]

use core::alloc::{GlobalAlloc, Layout};
use core::ops::Range;
use core::sync::atomic::{AtomicBool, Ordering};

use linked_list_allocator::LockedHeap;

use crate::bootstrap::{log as boot_log, no_alloc};

/// Statically reserved heap span used during bootstrap.
pub const HEAP_BYTES: usize = 512 * 1024;

static HEAP_INITIALISED: AtomicBool = AtomicBool::new(false);

struct GuardedAllocator {
    inner: LockedHeap,
}

impl GuardedAllocator {
    const fn new() -> Self {
        Self {
            inner: LockedHeap::empty(),
        }
    }

    unsafe fn init(&self, span: Range<usize>) {
        self.inner
            .lock()
            .init(span.start as *mut u8, span.end.saturating_sub(span.start));
    }
}

unsafe impl GlobalAlloc for GuardedAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !no_alloc::alloc_ready() {
            no_alloc::assert_no_alloc("alloc");
        }

        self.inner.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if !no_alloc::alloc_ready() {
            no_alloc::assert_no_alloc("dealloc");
        }

        self.inner.dealloc(ptr, layout)
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if !no_alloc::alloc_ready() {
            no_alloc::assert_no_alloc("alloc_zeroed");
        }

        self.inner.alloc_zeroed(layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if !no_alloc::alloc_ready() {
            no_alloc::assert_no_alloc("realloc");
        }

        self.inner.realloc(ptr, layout, new_size)
    }
}

#[global_allocator]
static GLOBAL_ALLOCATOR: GuardedAllocator = GuardedAllocator::new();

fn report_heap_error(tag: &str, detail: &str) -> ! {
    let mut line = heapless::String::<96>::new();
    let _ = core::fmt::write(&mut line, format_args!("[alloc:init] {tag}: {detail}"));
    boot_log::force_uart_line(line.as_str());
    panic!("{tag}: {detail}");
}

/// Installs the global allocator over the supplied heap span once all layout checks pass.
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
