// CLASSIFICATION: COMMUNITY
// Filename: sel4_entry.rs v0.3
// Author: Lukas Bower
// Date Modified: 2026-08-12

#![cfg(feature = "sel4_entry_bin")]
#![cfg_attr(not(feature = "std"), no_std)]
#![no_main]

#[cfg(feature = "std")]
extern crate std;

#[cfg(not(feature = "std"))]
use core::panic::PanicInfo;
use cohesix::debug;

#[no_mangle]
pub extern "C" fn _sel4_start() -> ! {
    debug!("COHESIX_BOOT_OK\n");
    cohesix::sh_loop::run();
    debug!("ENTRY SETUP OK\n");
    loop {}
}

#[cfg(not(feature = "std"))]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

