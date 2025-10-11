// Author: Lukas Bower
#![allow(dead_code)]

use core::panic::PanicInfo;

/// Minimal entry point for seL4 heartbeat worker binaries.
#[allow(unsafe_code)]
#[no_mangle]
pub extern "C" fn _start() -> ! {
    loop {
        core::hint::spin_loop();
    }
}

/// Panic handler that traps execution in a spin loop until the debugger intervenes.
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
