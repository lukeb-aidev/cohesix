// CLASSIFICATION: COMMUNITY
// Filename: debug_log.rs v0.3
// Author: Lukas Bower
// Date Modified: 2026-08-06


use core::fmt;

#[cfg(not(feature = "std"))]
use core::cell::UnsafeCell;
#[cfg(not(feature = "std"))]
use core::sync::atomic::{AtomicUsize, Ordering};

#[cfg(feature = "std")]
use std::io::{self, Write as IoWrite};

#[cfg(not(feature = "std"))]
extern "C" {
    fn seL4_DebugPutChar(c: u8);
}

#[cfg(not(feature = "std"))]
const BUF_SIZE: usize = 1024;
#[cfg(not(feature = "std"))]
struct LogBuffer(UnsafeCell<[u8; BUF_SIZE]>);
#[cfg(not(feature = "std"))]
unsafe impl Sync for LogBuffer {}
#[cfg(not(feature = "std"))]
static LOG_BUF: LogBuffer = LogBuffer(UnsafeCell::new([0; BUF_SIZE]));
#[cfg(not(feature = "std"))]
static LOG_IDX: AtomicUsize = AtomicUsize::new(0);

#[cfg(feature = "std")]
fn log_bytes(bytes: &[u8]) {
    let _ = io::stderr().write_all(bytes);
}

#[cfg(not(feature = "std"))]
fn log_bytes(bytes: &[u8]) {
    for &b in bytes {
        unsafe { seL4_DebugPutChar(b) };
        let idx = LOG_IDX.fetch_add(1, Ordering::Relaxed);
        if idx < BUF_SIZE {
            unsafe {
                (*LOG_BUF.0.get())[idx] = b;
            }
        }
    }
}

pub fn log_fmt(args: fmt::Arguments) {
    struct Writer;
    impl fmt::Write for Writer {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            log_bytes(s.as_bytes());
            Ok(())
        }
    }
    let mut w = Writer;
    let _ = fmt::write(&mut w, args);
}

#[cfg(not(feature = "std"))]
pub fn buffer() -> &'static [u8] {
    let idx = LOG_IDX.load(Ordering::Relaxed);
    let buf = unsafe { &*LOG_BUF.0.get() };
    &buf[..core::cmp::min(idx, BUF_SIZE)]
}

#[cfg(feature = "std")]
pub fn buffer() -> &'static [u8] {
    &[]
}

#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        $crate::util::debug_log::log_fmt(format_args!($($arg)*));
    };
}
