// CLASSIFICATION: COMMUNITY
// Filename: lib.rs v0.1
// Author: Lukas Bower
// Date Modified: 2028-12-13

#![no_std]
#![cfg_attr(not(test), no_main)]
extern crate cty;

pub mod bindings {
    #![allow(non_camel_case_types, non_snake_case, non_upper_case_globals)]
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

pub use bindings::*;
pub use bindings::{seL4_CallWithMRs, seL4_GetBootInfo, seL4_InitBootInfo};

#[no_mangle]
pub extern "C" fn seL4_DebugPutChar(c: cty::c_int) {
    unsafe {
        const SYSCALL: usize = (-9i32) as usize;
        let ch = c as usize;
        core::arch::asm!(
            "mov x0, {arg}",
            "mov x7, {syscall}",
            "svc 0",
            arg = in(reg) ch,
            syscall = in(reg) SYSCALL,
            options(nostack, preserves_flags)
        );
    }
}

#[no_mangle]
pub extern "C" fn seL4_DebugHalt() {
    unsafe {
        const SYSCALL: usize = (-11i32) as usize;
        core::arch::asm!(
            "mov x7, {syscall}",
            "svc 0",
            syscall = in(reg) SYSCALL,
            options(nostack, preserves_flags)
        );
    }
}

#[cfg(target_arch = "aarch64")]
#[no_mangle]
pub unsafe extern "C" fn seL4_Yield() {
    const SYSCALL: usize = (-7i32) as usize;
    core::arch::asm!("mov x7, {syscall}", "svc 0", syscall = in(reg) SYSCALL, options(nostack, preserves_flags));
}

#[cfg(feature = "panic-handler")]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
