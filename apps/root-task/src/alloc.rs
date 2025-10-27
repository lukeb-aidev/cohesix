// Author: Lukas Bower
//! Global heap allocator initialised for kernel builds before dynamic memory is required.

#![cfg(feature = "kernel")]
#![allow(unsafe_code)]

use core::sync::atomic::{AtomicBool, Ordering};

use linked_list_allocator::LockedHeap;

const HEAP_BYTES: usize = 512 * 1024;

static mut HEAP: [u8; HEAP_BYTES] = [0; HEAP_BYTES];
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
        unsafe {
            let heap_ptr = core::ptr::addr_of_mut!(HEAP).cast::<u8>();
            GLOBAL_ALLOCATOR.lock().init(heap_ptr, HEAP_BYTES);
        }
    }
}
