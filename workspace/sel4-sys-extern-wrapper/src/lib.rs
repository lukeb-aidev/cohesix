// CLASSIFICATION: COMMUNITY
// Filename: lib.rs v0.1
// Author: Lukas Bower
// Date Modified: 2028-11-21

#![no_std]
#![cfg_attr(not(test), no_main)]
extern crate cty;

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

use core::panic::PanicInfo;

#[cfg(feature = "panic-handler")]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
