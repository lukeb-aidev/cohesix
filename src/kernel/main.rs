// CLASSIFICATION: COMMUNITY
// Filename: main.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-08-01

#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    loop {}
}
