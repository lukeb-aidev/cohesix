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

extern "C" {
    pub fn cohesix_seL4_Untyped_Retype(
        ut: seL4_Untyped,
        type_: seL4_Word,
        size_bits: seL4_Word,
        root: seL4_CPtr,
        node_index: seL4_Word,
        node_depth: seL4_Word,
        node_offset: seL4_Word,
        num_objects: seL4_Word,
    ) -> cty::c_int;

    pub fn cohesix_seL4_CNode_Mint(
        dest_root: seL4_CPtr,
        dest_index: seL4_CPtr,
        dest_depth: seL4_Word,
        src_root: seL4_CPtr,
        src_index: seL4_CPtr,
        src_depth: seL4_Word,
        rights: seL4_CapRights_t,
        data: seL4_CNode_CapData_t,
    ) -> cty::c_int;

    pub fn cohesix_seL4_CNode_Delete(
        dest_root: seL4_CPtr,
        dest_index: seL4_CPtr,
        dest_depth: seL4_Word,
    ) -> cty::c_int;

    pub fn cohesix_seL4_ARM_Page_Map(
        page: seL4_ARM_Page,
        vspace: seL4_CPtr,
        vaddr: seL4_Word,
        rights: seL4_CapRights_t,
        attr: seL4_ARM_VMAttributes,
    ) -> cty::c_int;

    pub fn cohesix_seL4_ARM_Page_Unmap(page: seL4_ARM_Page) -> cty::c_int;

    pub fn cohesix_seL4_CapRights_new(
        grant_reply: seL4_Uint64,
        grant: seL4_Uint64,
        read: seL4_Uint64,
        write: seL4_Uint64,
    ) -> seL4_CapRights_t;

    pub fn cohesix_seL4_CNode_CapData_new(
        guard: seL4_Uint64,
        guard_size: seL4_Uint64,
    ) -> seL4_CNode_CapData_t;
}

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
