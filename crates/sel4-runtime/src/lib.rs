// Author: Lukas Bower
#![no_std]

use core::cell::UnsafeCell;
use core::ptr;

use sel4_sys::seL4_BootInfo;

const STACK_BYTES: usize = 16 * 1024;

#[repr(align(16))]
struct BootStack([u8; STACK_BYTES]);

// seL4 linker scripts previously mapped `.bss.uninit` near `USER_TOP`, which
// inflated the PT_LOAD span when the root-task stack lived in that section.
// Pin the stack to a dedicated data segment so it stays adjacent to the rest
// of the root-task image and leaves the kernel window untouched.
#[link_section = ".data.boot_stack"]
static mut BOOT_STACK: BootStack = BootStack([0; STACK_BYTES]);

struct BootInfoCell {
    ptr: UnsafeCell<*mut seL4_BootInfo>,
    init: UnsafeCell<bool>,
}

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

#[inline(always)]
fn stack_top() -> *mut u8 {
    unsafe {
        let stack = ptr::addr_of_mut!(BOOT_STACK);
        (*stack).0.as_mut_ptr().add(STACK_BYTES)
    }
}

/// seL4 kernel entry stub invoked after seL4 initialises the initial thread.
#[cfg(target_arch = "aarch64")]
#[no_mangle]
pub unsafe extern "C" fn _start(bootinfo: *mut seL4_BootInfo) -> ! {
    core::arch::asm!(
        "mov sp, {stack}",
        stack = in(reg) stack_top(),
        options(nostack, preserves_flags),
    );
    __sel4_start_init_boot_info(bootinfo);
    extern "C" {
        fn sel4_start(bootinfo: *const seL4_BootInfo) -> !;
    }
    sel4_start(bootinfo)
}

#[cfg(not(target_arch = "aarch64"))]
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
}

/// Returns the bootinfo pointer recorded during startup, if initialised.
pub fn bootinfo() -> Option<&'static mut seL4_BootInfo> {
    BOOTINFO
        .get()
        .map(|ptr| unsafe { &mut *ptr.cast::<seL4_BootInfo>() })
}
