// Author: Lukas Bower
#![cfg(feature = "kernel")]

//! Runtime guards for validating dynamic dispatch targets inside the root task.

use core::ops::Range;
use core::sync::atomic::{AtomicUsize, Ordering};

/// Trait implemented for plain function pointers so their raw addresses can be
/// retrieved in a generic fashion. This avoids relying on `as` casts that are
/// only permitted for primitive types.
pub trait FunctionPointer: Copy {
    /// Return the address of the function pointer as a usize.
    fn addr(self) -> usize;
}

impl<R> FunctionPointer for fn() -> R {
    #[inline(always)]
    fn addr(self) -> usize {
        self as usize
    }
}

impl<A: 'static, R> FunctionPointer for fn(A) -> R {
    #[inline(always)]
    fn addr(self) -> usize {
        self as usize
    }
}

impl<A, B, R> FunctionPointer for fn(A, B) -> R {
    #[inline(always)]
    fn addr(self) -> usize {
        self as usize
    }
}

impl<A, B, C, R> FunctionPointer for fn(A, B, C) -> R {
    #[inline(always)]
    fn addr(self) -> usize {
        self as usize
    }
}

impl<A, B, C, D, R> FunctionPointer for fn(A, B, C, D) -> R {
    #[inline(always)]
    fn addr(self) -> usize {
        self as usize
    }
}

impl<T, R> FunctionPointer for for<'a> fn(&'a [T]) -> R {
    #[inline(always)]
    fn addr(self) -> usize {
        self as usize
    }
}

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
    T: FunctionPointer,
    F: FnOnce(T) -> R,
{
    let addr = func.addr();
    if is_text_ptr(addr) {
        return call(func);
    }

    let bounds = text_bounds();
    log::error!(
        "[guard] rejected call target=0x{addr:x} text=[0x{lo:x}..0x{hi:x})",
        addr = addr,
        lo = bounds.start,
        hi = bounds.end
    );
    crate::sel4::debug_halt();
    panic!(
        "rejected indirect call to 0x{addr:x}; expected text range [0x{lo:x}..0x{hi:x})",
        addr = addr,
        lo = bounds.start,
        hi = bounds.end
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_checked_allows_text_target() {
        fn allowed() {}

        let addr = allowed as usize;
        init_text_bounds(addr, addr + core::mem::size_of::<usize>());

        let mut invoked = false;
        call_checked(allowed as fn(), |func| {
            invoked = true;
            assert_eq!(func.addr(), addr);
        });
        assert!(invoked);

        init_text_bounds(0, 0);
    }

    #[test]
    fn call_checked_rejects_non_text_target() {
        fn target() {}

        init_text_bounds(0, 0);

        let result = std::panic::catch_unwind(|| {
            call_checked(target as fn(), |func| {
                let _ = func.addr();
            });
        });

        assert!(result.is_err());
    }

    #[test]
    fn call_checked_supports_slice_arguments() {
        fn handler(_: &[usize]) {}

        let addr = handler as usize;
        init_text_bounds(addr, addr + core::mem::size_of::<usize>());

        let mut invoked = false;
        call_checked(handler as for<'a> fn(&'a [usize]), |func| {
            invoked = true;
            let scratch = [0usize; 0];
            func(&scratch);
        });
        assert!(invoked);

        init_text_bounds(0, 0);
    }
}
