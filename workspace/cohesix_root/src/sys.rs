// CLASSIFICATION: COMMUNITY
// Filename: sys.rs v0.15
// Author: Lukas Bower
// Date Modified: 2030-03-09
#![allow(static_mut_refs)]

use crate::{debug_putchar, monotonic_ticks, rootfs, trace};
use core::ffi::c_char;
use core::sync::atomic::{compiler_fence, Ordering};

/// Initialize UART support.
///
/// The seL4 kernel already maps the debug console.  We simply ensure the
/// symbol is referenced so the linker keeps the `.uart` section but we avoid
/// touching the MMIO frame directly which caused an early fault when the
/// region was not yet mapped.
pub fn init_uart() {
    crate::drivers::uart::init();
}

/// Set the TLS pointer for seL4 syscalls.
#[cfg(target_arch = "aarch64")]
#[no_mangle]
pub unsafe extern "C" fn sel4_set_tls(ptr: *const u8) {
    core::arch::asm!(
        "msr tpidr_el0, {0}",
        in(reg) ptr,
        options(nostack, preserves_flags)
    );
}

#[cfg_attr(not(test), allow(dead_code))]
const SYS_DEBUG_PUTCHAR: i64 = -9;
#[cfg_attr(not(test), allow(dead_code))]
const SYS_SEND: i64 = -3;
#[cfg_attr(not(test), allow(dead_code))]
const SYS_RECV: i64 = -5;
#[cfg_attr(not(test), allow(dead_code))]
const SYS_YIELD: i64 = -7;
#[cfg_attr(not(test), allow(dead_code))]
const SYS_DEBUG_HALT: i64 = -11;

#[cfg(test)]
pub const fn debug_putchar_const() -> i64 {
    SYS_DEBUG_PUTCHAR
}

const ENOENT: i32 = -2;
const EBADF: i32 = -9;
const EINVAL: i32 = -22;
const ENOSYS: i32 = -38;

#[derive(Clone, Copy)]
enum FileData {
    Static(&'static [u8]),
    BootLog,
}

#[derive(Clone, Copy)]
struct File {
    data: FileData,
    pos: usize,
    in_use: bool,
}

impl File {
    const fn unused() -> Self {
        File {
            data: FileData::Static(b""),
            pos: 0,
            in_use: false,
        }
    }
}

const MAX_FILES: usize = 64;
static mut FILES: [File; MAX_FILES] = [File::unused(); MAX_FILES];

fn allocate_file(data: FileData) -> i32 {
    unsafe {
        for (idx, file) in FILES.iter_mut().enumerate() {
            if !file.in_use {
                file.data = data;
                file.pos = match file.data {
                    FileData::BootLog => crate::bootlog::base_offset(),
                    FileData::Static(_) => 0,
                };
                file.in_use = true;
                return idx as i32;
            }
        }
    }
    EBADF
}

#[no_mangle]
pub unsafe extern "C" fn coh_open(path: *const c_char, _flags: i32, _mode: i32) -> i32 {
    if path.is_null() {
        return EINVAL;
    }
    let p = core::ffi::CStr::from_ptr(path);
    let path_bytes = p.to_bytes();
    if path_bytes == b"/log/boot_ring" {
        return allocate_file(FileData::BootLog);
    }
    if let Some(data) = rootfs::lookup(path_bytes) {
        return allocate_file(FileData::Static(data));
    }
    ENOENT
}

#[no_mangle]
pub unsafe extern "C" fn coh_read(fd: i32, buf: *mut u8, len: usize) -> isize {
    if fd < 0 || (fd as usize) >= FILES.len() {
        return EBADF as isize;
    }
    let f = &mut FILES[fd as usize];
    if !f.in_use {
        return EBADF as isize;
    }
    match f.data {
        FileData::Static(data) => {
            let remain = &data[f.pos..];
            let n = core::cmp::min(len, remain.len());
            if n == 0 {
                return 0;
            }
            core::ptr::copy_nonoverlapping(remain.as_ptr(), buf, n);
            f.pos += n;
            n as isize
        }
        FileData::BootLog => {
            let slice = core::slice::from_raw_parts_mut(buf, len);
            let read = crate::bootlog::read_from(f.pos, slice);
            f.pos = f.pos.saturating_add(read);
            read as isize
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn coh_close(fd: i32) -> i32 {
    if fd < 0 || (fd as usize) >= FILES.len() {
        EBADF
    } else {
        let file = &mut FILES[fd as usize];
        if !file.in_use {
            return EBADF;
        }
        file.pos = match file.data {
            FileData::Static(_) => 0,
            FileData::BootLog => crate::bootlog::base_offset(),
        };
        file.in_use = false;
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn coh_write(_fd: i32, _buf: *const u8, len: usize) -> isize {
    len as isize
}

#[no_mangle]
pub unsafe extern "C" fn coh_exec(path: *const c_char, _argv: *const *const c_char) -> i32 {
    if path.is_null() {
        return EINVAL;
    }
    let p = core::ffi::CStr::from_ptr(path);
    if p.to_bytes() == b"/bin/init" {
        coh_log("COHESIX_USERLAND_BOOT_OK");
        0
    } else {
        ENOENT
    }
}

#[no_mangle]
pub unsafe extern "C" fn coh_getenv(_name: *const c_char) -> *const c_char {
    core::ptr::null()
}

#[no_mangle]
pub unsafe extern "C" fn coh_setenv(
    _name: *const c_char,
    _val: *const c_char,
    _overwrite: i32,
) -> i32 {
    ENOSYS
}

#[no_mangle]
pub unsafe extern "C" fn coh_bind(_name: *const c_char, _old: *const c_char, _flags: i32) -> i32 {
    0
}

#[no_mangle]
pub unsafe extern "C" fn coh_mount(srv: *const c_char, dst: *const c_char, _flags: i32) -> i32 {
    if srv.is_null() || dst.is_null() {
        return EINVAL;
    }
    trace::record("ns:mount_call", monotonic_ticks());
    0
}

#[no_mangle]
pub unsafe extern "C" fn coh_srv(path: *const c_char) -> i32 {
    if path.is_null() {
        return EINVAL;
    }
    trace::record("ns:srv_call", monotonic_ticks());
    0
}

pub fn coh_log(msg: &str) {
    for &b in msg.as_bytes() {
        debug_putchar(b);
    }
    debug_putchar(b'\n');
    compiler_fence(Ordering::SeqCst);
}

// Manual mem* shims to avoid LLVM lowering to self-recursive symbols in no_std builds.
#[no_mangle]
pub unsafe extern "C" fn memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    let mut i = 0;
    while i < n {
        *dest.add(i) = *src.add(i);
        i += 1;
    }
    dest
}

#[no_mangle]
pub unsafe extern "C" fn memmove(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    if n == 0 || dest as *const u8 == src {
        return dest;
    }
    let dest_addr = dest as usize;
    let src_addr = src as usize;
    if dest_addr < src_addr || dest_addr >= src_addr.wrapping_add(n) {
        let mut i = 0;
        while i < n {
            *dest.add(i) = *src.add(i);
            i += 1;
        }
    } else {
        let mut i = n;
        while i != 0 {
            i -= 1;
            *dest.add(i) = *src.add(i);
        }
    }
    dest
}

#[no_mangle]
pub unsafe extern "C" fn memset(dest: *mut u8, c: i32, n: usize) -> *mut u8 {
    let byte = c as u8;
    let mut i = 0;
    while i < n {
        *dest.add(i) = byte;
        i += 1;
    }
    dest
}

#[no_mangle]
pub unsafe extern "C" fn memcmp(a: *const u8, b: *const u8, n: usize) -> i32 {
    for i in 0..n {
        let ca = core::ptr::read(a.add(i));
        let cb = core::ptr::read(b.add(i));
        if ca != cb {
            return ca as i32 - cb as i32;
        }
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn strlen(mut s: *const u8) -> usize {
    let mut len = 0;
    while core::ptr::read(s) != 0 {
        len += 1;
        s = s.add(1);
    }
    len
}
