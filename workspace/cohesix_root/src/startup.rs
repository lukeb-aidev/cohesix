// CLASSIFICATION: COMMUNITY
// Filename: startup.rs v0.2
// Author: Lukas Bower
// Date Modified: 2028-01-21

use core::arch::global_asm;

// Exception vector base is provided by `vec.S` and linked into `.vectors`.
// The kernel maps this region at the address configured in `link.ld`.

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
        // Set up the exception vector base for SVC handling. The address is
        // defined by `vectors_start` in `vec.S`.
        use core::ptr::addr_of;
        extern "C" {
            static vectors_start: u8;
        }
        let vectors_ptr = addr_of!(vectors_start) as usize;
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
