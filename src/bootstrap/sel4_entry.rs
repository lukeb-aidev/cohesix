// CLASSIFICATION: COMMUNITY
// Filename: sel4_entry.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-12-31

#![no_std]
#![no_main]

use core::panic::PanicInfo;

// Minimal FFI to seL4 debug output
extern "C" {
    fn seL4_DebugPutChar(c: u8);
}

fn debug_print(msg: &str) {
    for &b in msg.as_bytes() {
        unsafe { seL4_DebugPutChar(b); }
    }
}

#[no_mangle]
pub extern "C" fn _sel4_start() -> ! {
    debug_print("COHESIX_BOOT_OK\n");
    cohesix::sh_loop::run();
    debug_print("ENTRY SETUP OK\n");
    loop {}
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

