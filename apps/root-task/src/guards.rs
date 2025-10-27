// Author: Lukas Bower
#![cfg(feature = "kernel")]

//! Runtime guards for validating dynamic dispatch targets inside the root task.

use core::ops::Range;
use core::sync::atomic::{AtomicUsize, Ordering};

static TEXT_START: AtomicUsize = AtomicUsize::new(0);
static TEXT_END: AtomicUsize = AtomicUsize::new(0);

/// Register the start and end bounds of the executable text segment.
#[inline(always)]
pub fn init_text_bounds(start: usize, end: usize) {
    TEXT_START.store(start, Ordering::Release);
    TEXT_END.store(end, Ordering::Release);
}

/// Retrieve the currently configured text bounds.
#[inline(always)]
pub fn text_bounds() -> Range<usize> {
    let start = TEXT_START.load(Ordering::Acquire);
    let end = TEXT_END.load(Ordering::Acquire);
    start..end
}

/// Determine whether `ptr` resides within the configured text segment.
#[inline(always)]
pub fn is_text_ptr(ptr: usize) -> bool {
    let range = text_bounds();
    ptr >= range.start && ptr < range.end
}

/// Dispatch a function pointer only if it targets the loaded text segment.
#[inline(always)]
pub fn call_checked<T, F, R>(func: T, call: F) -> R
where
    T: Copy,
    F: FnOnce(T) -> R,
{
    let addr = func as *const () as usize;
    if !is_text_ptr(addr) {
        let bounds = text_bounds();
        log::error!(
            "[guard] rejected call target=0x{addr:x} text=[0x{lo:x}..0x{hi:x})",
            addr = addr,
            lo = bounds.start,
            hi = bounds.end
        );
        crate::sel4::debug_halt();
    }

    call(func)
}
