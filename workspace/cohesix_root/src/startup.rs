// CLASSIFICATION: COMMUNITY
// Filename: startup.rs v0.3
// Author: Lukas Bower
// Date Modified: 2025-10-06

use core::arch::global_asm;
use sel4_sys_extern_wrapper::seL4_DebugPutChar;

// Exception vector base is provided by `vec.S` and linked into `.vectors`.
// The kernel maps this region at the address configured in `link.ld`.

global_asm!(
    r#"
    .section .text.startup
    .global rust_entry
rust_entry:
    bl rust_start
    b .
"#
);

/// Rust entry point invoked from `entry.S`.
#[no_mangle]
pub extern "C" fn rust_start() -> ! {
    unsafe {
        seL4_DebugPutChar(b'C' as i32);
    }
    extern "C" {
        fn main();
    }
    unsafe {
        // The kernel installs the user exception vectors. Jump straight to `main`.
        main();
    }
    loop {
        core::hint::spin_loop();
    }
}
