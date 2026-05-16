// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: seL4 runtime entry glue and bootstrap stack provisioning.
// Author: Lukas Bower
#![no_std]
#![allow(clippy::missing_safety_doc)]

use core::cell::UnsafeCell;
use core::ptr;

use sel4_sys::seL4_BootInfo;

#[cfg(target_arch = "aarch64")]
#[repr(align(16))]
struct TlsBaseCell;

#[cfg(target_arch = "aarch64")]
#[no_mangle]
#[used]
static mut __tls_base: TlsBaseCell = TlsBaseCell;

#[cfg(all(target_arch = "aarch64", target_os = "none"))]
extern "C" {
    static __stack_top: u8;
}

struct BootInfoCell {
    ptr: UnsafeCell<*mut seL4_BootInfo>,
    init: UnsafeCell<bool>,
}

// SAFETY: seL4 enters the root task on one bootstrap thread. `set_once` runs
// during that single-threaded entry path, and later readers only observe the
// immutable bootinfo pointer published by the kernel.
unsafe impl Sync for BootInfoCell {}

impl BootInfoCell {
    const fn new() -> Self {
        Self {
            ptr: UnsafeCell::new(ptr::null_mut()),
            init: UnsafeCell::new(false),
        }
    }

    /// Stores the bootinfo pointer on first invocation.
    unsafe fn set_once(&self, bootinfo: *mut seL4_BootInfo) {
        if !*self.init.get() {
            *self.ptr.get() = bootinfo;
            *self.init.get() = true;
        }
    }

    /// Returns the stored bootinfo pointer when initialised.
    fn get(&self) -> Option<*mut seL4_BootInfo> {
        let ptr = unsafe { *self.ptr.get() };
        if ptr.is_null() {
            None
        } else {
            Some(ptr)
        }
    }
}

static BOOTINFO: BootInfoCell = BootInfoCell::new();

#[cfg(all(target_arch = "aarch64", target_os = "none"))]
// seL4 kernel entry stub invoked after seL4 initialises the initial thread.
// Defined in global assembly to avoid unstable `#[naked]` functions while
// preserving the debug stack instrumentation.
core::arch::global_asm!(
    "
    .section .text._start,\"ax\"
    .globl _start
    .p2align 2
_start:
    adrp x1, {stack_top}
    add x1, x1, :lo12:{stack_top}
    mov sp, x1
    b {entry}
    ",
    stack_top = sym __stack_top,
    entry = sym __sel4_start_rust,
);

#[cfg(all(target_arch = "aarch64", target_os = "none"))]
#[inline(never)]
unsafe extern "C" fn __sel4_start_rust(bootinfo: *mut seL4_BootInfo) -> ! {
    __sel4_start_init_boot_info(bootinfo);
    extern "C" {
        fn sel4_start(bootinfo: *const seL4_BootInfo) -> !;
    }
    sel4_start(bootinfo)
}

#[cfg(all(
    not(all(target_arch = "aarch64", target_os = "none")),
    not(test),
    not(doc)
))]
#[no_mangle]
pub unsafe extern "C" fn _start(_bootinfo: *mut seL4_BootInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

/// C-compatible hook used by the seL4 start stubs to record bootinfo.
#[no_mangle]
pub unsafe extern "C" fn __sel4_start_init_boot_info(bootinfo: *mut seL4_BootInfo) {
    BOOTINFO.set_once(bootinfo);
    sel4_sys::seL4_InitBootInfo(bootinfo);
}

/// Returns the bootinfo pointer recorded during startup, if initialised.
pub fn bootinfo() -> Option<&'static mut seL4_BootInfo> {
    BOOTINFO
        .get()
        .map(|ptr| unsafe { &mut *ptr.cast::<seL4_BootInfo>() })
}

#[cfg(all(target_arch = "aarch64", target_os = "none"))]
extern "C" {
    pub fn _start(bootinfo: *mut seL4_BootInfo) -> !;
}

#[cfg(all(target_arch = "aarch64", target_os = "none"))]
#[used]
static START_PTR: unsafe extern "C" fn(*mut seL4_BootInfo) -> ! = _start;
