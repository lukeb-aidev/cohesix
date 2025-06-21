// CLASSIFICATION: COMMUNITY
// Filename: sel4_entry.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-12-31

#![no_std]
#![no_main]

use core::panic::PanicInfo;
use cohesix::debug;

#[no_mangle]
pub extern "C" fn _sel4_start() -> ! {
    debug!("COHESIX_BOOT_OK\n");
    cohesix::sh_loop::run();
    debug!("ENTRY SETUP OK\n");
    loop {}
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

