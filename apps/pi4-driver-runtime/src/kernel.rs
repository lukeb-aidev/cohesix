// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Define no_std Pi 4 driver runtime trap entrypoints.
// Author: Lukas Bower

#![allow(dead_code)]

use core::panic::PanicInfo;

/// Final linked-image entry symbol named by the generated root-task manifest.
#[allow(unsafe_code)]
#[no_mangle]
pub extern "C" fn cohesix_pi4_driver_runtime_entry(task_key: usize) -> ! {
    pi4_driver_runtime::runtime_main(task_key)
}

/// Minimal seL4 binary entrypoint for standalone driver image artifacts.
#[allow(unsafe_code)]
#[no_mangle]
pub extern "C" fn _start(task_key: usize) -> ! {
    cohesix_pi4_driver_runtime_entry(task_key)
}

/// Panic handler that enters the generated standard-fault containment lane.
#[allow(unsafe_code)]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    // SAFETY: `brk` deliberately raises an AArch64 user exception without
    // reading or writing memory. The generated standard-fault cap routes this
    // TCB to root-fault, which suspends it and hands exact-generation
    // containment to the driver supervisor.
    unsafe {
        core::arch::asm!("brk #0", options(noreturn, nostack, nomem));
    }
}
