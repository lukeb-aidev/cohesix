// CLASSIFICATION: COMMUNITY
// Filename: sys.rs v0.3
// Author: Lukas Bower
// Date Modified: 2027-12-26

use core::ffi::c_char;
use core::sync::atomic::{compiler_fence, Ordering};

#[cfg(target_arch = "aarch64")]
#[no_mangle]
pub extern "C" fn seL4_DebugPutChar(c: u8) {
    const UART: *mut u8 = 0x0900_0000 as *mut u8;
    unsafe { core::ptr::write_volatile(UART, c) };
}

#[cfg(not(target_arch = "aarch64"))]
#[no_mangle]
pub extern "C" fn seL4_DebugPutChar(_c: u8) {}

#[no_mangle]
pub unsafe extern "C" fn coh_open(_path: *const c_char, _flags: i32, _mode: i32) -> i32 {
    -1
}

#[no_mangle]
pub unsafe extern "C" fn coh_read(_fd: i32, _buf: *mut u8, _len: usize) -> isize {
    -1
}

#[no_mangle]
pub unsafe extern "C" fn coh_close(_fd: i32) -> i32 {
    -1
}

#[no_mangle]
pub unsafe extern "C" fn coh_write(_fd: i32, _buf: *const u8, _len: usize) -> isize {
    -1
}

#[no_mangle]
pub unsafe extern "C" fn coh_exec(_path: *const c_char, _argv: *const *const c_char) -> i32 {
    -1
}

#[no_mangle]
pub unsafe extern "C" fn coh_getenv(_name: *const c_char) -> *const c_char {
    core::ptr::null()
}

#[no_mangle]
pub unsafe extern "C" fn coh_setenv(_name: *const c_char, _val: *const c_char, _overwrite: i32) -> i32 {
    -1
}

#[no_mangle]
pub unsafe extern "C" fn coh_bind(_name: *const c_char, _old: *const c_char, _flags: i32) -> i32 {
    -1
}

pub fn coh_log(msg: &str) {
    for &b in msg.as_bytes() {
        unsafe { seL4_DebugPutChar(b) };
    }
    unsafe { seL4_DebugPutChar(b'\n') };
    compiler_fence(Ordering::SeqCst);
}
