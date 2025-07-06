// CLASSIFICATION: COMMUNITY
// Filename: main.rs v0.5
// Author: Lukas Bower
// Date Modified: 2027-08-15
#![no_std]
#![no_main]

extern crate alloc;

use core::panic::PanicInfo;
use alloc::{string::String, vec::Vec};
use libm::sqrtf;

use cohesix::{println, sh_loop::run};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    main();
    loop {
        core::hint::spin_loop();
    }
}

fn main() {
    println!("COHESIX_BOOT_OK");
    println!("[root] booting...");
    let roots: Vec<String> = Vec::new();
    println!("[root] {} mounts", roots.len());
    println!("[root] sqrt(2) = {}", sqrtf(2.0));
    println!("[root] launching shell");
    run();
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    coherr!("[root] panic");
    loop {
        core::hint::spin_loop();
    }
}
