// CLASSIFICATION: COMMUNITY
// Filename: logdemo.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-08-27
// SPDX-License-Identifier: MIT

#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
use cohesix::kernel::user_api::{SYS_EXIT, SYS_LOG};

fn main() {
    unsafe {
        if let Some(log) = SYS_LOG {
            let msg1 = b"logdemo: line one\n";
            log(msg1.as_ptr(), msg1.len());
            let msg2 = b"logdemo: line two\n";
            log(msg2.as_ptr(), msg2.len());
        }
        if let Some(exit_fn) = SYS_EXIT {
            exit_fn(42);
        }
    }
}
