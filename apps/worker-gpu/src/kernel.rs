// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the kernel module for worker-gpu.
// Author: Lukas Bower
#![allow(dead_code)]

use core::panic::PanicInfo;

/// Minimal entry point for seL4 GPU worker binaries.
#[allow(unsafe_code)]
#[no_mangle]
pub extern "C" fn _start() -> ! {
    loop {
        core::hint::spin_loop();
    }
}

/// Panic handler that spins forever so debuggers can attach.
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
