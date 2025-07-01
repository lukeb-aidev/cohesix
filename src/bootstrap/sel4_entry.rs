// CLASSIFICATION: COMMUNITY
// Filename: sel4_entry.rs v0.4
// Author: Lukas Bower
// Date Modified: 2026-08-21

use crate::prelude::*;
#![no_main]
#![cfg(all(feature = "sel4", feature = "kernel_bin", feature = "minimal_uefi"))]

#[no_mangle]
pub extern "C" fn _start() -> ! {
    extern "C" {
        static _sel4_kernel_entry: u8;
    }

    unsafe {
        let entry = &_sel4_kernel_entry as *const u8;
        core::arch::asm!("br {}", in(reg) entry, options(noreturn));
    }
}
