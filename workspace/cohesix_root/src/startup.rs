// CLASSIFICATION: COMMUNITY
// Filename: startup.rs v0.1
// Author: Lukas Bower
// Date Modified: 2027-12-31

use core::arch::global_asm;

// Minimal exception vector table placed in its own section so the kernel can
// map it at a fixed address defined by `kernel.lds`.
#[link_section = ".vectors"]
#[no_mangle]
pub static VECTORS: [u32; 64] = [0; 64];

global_asm!(
    r#"
    .section .text.startup
    .global rust_entry
rust_entry:
    bl rust_start
    b .
"#);

/// Rust entry point invoked from `entry.S`.
#[no_mangle]
pub extern "C" fn rust_start() -> ! {
    extern "C" {
        fn main();
    }
    unsafe { main(); }
    loop {
        core::hint::spin_loop();
    }
}
