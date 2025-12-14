// Author: Lukas Bower
//! Global heap allocator initialised for kernel builds before dynamic memory is required.

#![cfg(feature = "kernel")]
#![allow(unsafe_code)]

use core::sync::atomic::{AtomicBool, Ordering};

use linked_list_allocator::LockedHeap;

const HEAP_BYTES: usize = 512 * 1024;

extern "C" {
    static __heap_start: u8;
    static __heap_end: u8;
}
static HEAP_INITIALISED: AtomicBool = AtomicBool::new(false);

#[global_allocator]
static GLOBAL_ALLOCATOR: LockedHeap = LockedHeap::empty();

/// Installs the global allocator over the statically reserved heap region.
///
/// The allocator is intentionally simple: a fixed 512 KiB window backed by a
/// `LockedHeap` from `linked_list_allocator`. The heap size mirrors the upper
/// bound of anticipated dynamic allocations during bootstrap (temporary `Vec`
/// instances used when parsing bootinfo metadata or capturing UART traces).
/// Additional memory hungry services are expected to provision dedicated pools
/// backed by untyped retypes rather than drawing from this bootstrap heap.
pub fn init_heap() {
    if HEAP_INITIALISED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        let start = unsafe { core::ptr::addr_of!(__heap_start) as usize };
        let end = unsafe { core::ptr::addr_of!(__heap_end) as usize };
        let len = end.saturating_sub(start);

        debug_assert_eq!(
            len, HEAP_BYTES,
            "linker heap span ({len:#x}) diverges from allocator expectation ({HEAP_BYTES:#x})"
        );

        unsafe {
            GLOBAL_ALLOCATOR
                .lock()
                .init(start as *mut u8, len.min(HEAP_BYTES));
        }
    }
}
