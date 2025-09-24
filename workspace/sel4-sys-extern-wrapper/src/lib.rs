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
pub use bindings::{
    seL4_CallWithMRs, seL4_DebugHalt, seL4_DebugPutChar, seL4_GetBootInfo, seL4_InitBootInfo,
};

#[cfg(target_arch = "aarch64")]
#[no_mangle]
pub unsafe extern "C" fn seL4_Yield() {
    const SYSCALL: usize = (-7i32) as usize;
    core::arch::asm!("mov x7, {syscall}", "svc 0", syscall = in(reg) SYSCALL, options(nostack, preserves_flags));
}

use core::panic::PanicInfo;

#[cfg(feature = "panic-handler")]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
