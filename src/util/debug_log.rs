// CLASSIFICATION: COMMUNITY
// Filename: debug_log.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-12-31

#![no_std]

use core::fmt::{self, Write};

extern "C" {
    fn seL4_DebugPutChar(c: u8);
}

static mut LOG_BUF: [u8; 1024] = [0; 1024];
static mut LOG_IDX: usize = 0;

fn log_bytes(bytes: &[u8]) {
    for &b in bytes {
        unsafe {
            seL4_DebugPutChar(b);
            if LOG_IDX < LOG_BUF.len() {
                LOG_BUF[LOG_IDX] = b;
                LOG_IDX += 1;
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

pub fn buffer() -> &'static [u8] {
    unsafe { &LOG_BUF[..LOG_IDX.min(LOG_BUF.len())] }
}

#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        $crate::util::debug_log::log_fmt(format_args!($($arg)*));
    };
}
