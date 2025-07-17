// CLASSIFICATION: COMMUNITY
// Filename: lib.rs v0.1
// Author: Lukas Bower
// Date Modified: 2027-12-31

#![no_std]
extern crate cty;

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
