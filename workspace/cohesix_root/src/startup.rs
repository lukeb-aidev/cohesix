// CLASSIFICATION: COMMUNITY
// Filename: startup.rs v0.1
// Author: Lukas Bower
// Date Modified: 2028-01-20

use core::arch::global_asm;

// Minimal exception vector table placed in its own section so the kernel can
// map it at a fixed address defined by `link.ld`.
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
    unsafe {
        // Set up the exception vector base for SVC handling.
        extern "C" {
            static VECTORS: [u32; 64];
        }
        let vectors_ptr = &VECTORS as *const _ as usize;
        core::arch::asm!(
            "msr VBAR_EL1, {0}",
            "isb", // ensure the new vector base is used before continuing
            in(reg) vectors_ptr,
            options(nostack, preserves_flags)
        );
        main();
    }
    loop {
        core::hint::spin_loop();
    }
}
