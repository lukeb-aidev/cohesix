// CLASSIFICATION: COMMUNITY
// Filename: sys.rs v0.1
// Author: Lukas Bower
// Date Modified: 2027-12-23

use core::ffi::c_char;

extern "C" {
    fn open(path: *const c_char, flags: i32, mode: i32) -> i32;
    fn read(fd: i32, buf: *mut u8, len: usize) -> isize;
    fn close(fd: i32) -> i32;
    fn write(fd: i32, buf: *const u8, len: usize) -> isize;
    fn execv(path: *const c_char, argv: *const *const c_char) -> i32;
    fn getenv(name: *const c_char) -> *const c_char;
    fn setenv(name: *const c_char, val: *const c_char, overwrite: i32) -> i32;
    fn bind(name: *const c_char, old: *const c_char, flags: i32) -> i32;
}

pub unsafe fn coh_open(path: *const c_char, flags: i32, mode: i32) -> i32 {
    open(path, flags, mode)
}

pub unsafe fn coh_read(fd: i32, buf: *mut u8, len: usize) -> isize {
    read(fd, buf, len)
}

pub unsafe fn coh_close(fd: i32) -> i32 {
    close(fd)
}

pub unsafe fn coh_write(fd: i32, buf: *const u8, len: usize) -> isize {
    write(fd, buf, len)
}

pub unsafe fn coh_exec(path: *const c_char, argv: *const *const c_char) -> i32 {
    execv(path, argv)
}

pub unsafe fn coh_getenv(name: *const c_char) -> *const c_char {
    getenv(name)
}

pub unsafe fn coh_setenv(name: *const c_char, val: *const c_char) -> i32 {
    setenv(name, val, 1)
}

pub unsafe fn coh_bind(name: *const c_char, old: *const c_char, flags: i32) -> i32 {
    bind(name, old, flags)
}

pub fn coh_log(msg: &str) {
    crate::putstr(msg);
}
