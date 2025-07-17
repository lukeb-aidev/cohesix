// CLASSIFICATION: COMMUNITY
// Filename: sys.rs v0.8
// Author: Lukas Bower
// Date Modified: 2025-07-12

use core::ffi::c_char;
use core::sync::atomic::{compiler_fence, Ordering};
use crate::dt::UART_BASE;
use crate::coherr;

#[link_section = ".uart"]
#[used]
static mut UART_FRAME: [u8; 0x1000] = [0; 0x1000];

extern "C" {
    static __uart_start: u8;
    static __uart_end: u8;
}

/// Ensure the UART frame is mapped before use
pub fn init_uart() {
    unsafe {
        core::ptr::write_volatile(UART_BASE as *mut u8, 0);
    }
}

pub fn validate_uart_ptr() {
    let start = unsafe { &__uart_start as *const u8 as usize };
    let end = unsafe { &__uart_end as *const u8 as usize };
    if UART_BASE < start || UART_BASE >= end {
        coherr!("uart_base_out_of_range start={:#x} end={:#x} base={:#x}", start, end, UART_BASE);
    }
}

#[cfg(target_arch = "aarch64")]
#[no_mangle]
pub extern "C" fn seL4_DebugPutChar(c: u8) {
    unsafe { core::ptr::write_volatile(UART_BASE as *mut u8, c) };
}

#[cfg(not(target_arch = "aarch64"))]
#[no_mangle]
pub extern "C" fn seL4_DebugPutChar(_c: u8) {}

#[cfg(target_arch = "aarch64")]
#[no_mangle]
pub unsafe extern "C" fn seL4_Send(dest: u64, msg: *const u64) {
    core::arch::asm!(
        "mov x0, {0}",
        "mov x1, {1}",
        "mov x16, #1",
        "svc #0",
        in(reg) dest,
        in(reg) msg,
        options(nostack)
    );
}

#[cfg(target_arch = "aarch64")]
#[no_mangle]
pub unsafe extern "C" fn seL4_Recv(src: u64, msg: *mut u64) {
    core::arch::asm!(
        "mov x0, {0}",
        "mov x1, {1}",
        "mov x16, #8",
        "svc #0",
        in(reg) src,
        in(reg) msg,
        options(nostack)
    );
}

#[cfg(target_arch = "aarch64")]
#[no_mangle]
pub unsafe extern "C" fn seL4_Yield() {
    core::arch::asm!(
        "mov x16, #6",
        "svc #0",
        options(nostack)
    );
}

const ENOENT: i32 = -2;
const EBADF: i32 = -9;
const EINVAL: i32 = -22;
const ENOSYS: i32 = -38;

#[derive(Clone, Copy)]
struct File {
    data: &'static [u8],
    pos: usize,
}

static INIT_DATA: &[u8] = include_bytes!("../../../userland/miniroot/bin/init");
static NS_DATA: &[u8] = include_bytes!("../../../config/plan9.ns");
static BOOTARGS_DATA: &[u8] = b"COHROLE=DroneWorker\n";

static mut FILES: [File; 4] = [
    File { data: INIT_DATA, pos: 0 },     // 0 => /bin/init
    File { data: NS_DATA, pos: 0 },       // 1 => /etc/plan9.ns
    File { data: BOOTARGS_DATA, pos: 0 }, // 2 => /boot/bootargs.txt
    File { data: b"" as &[u8], pos: 0 },  // 3 => /srv/cohrole
];

#[no_mangle]
pub unsafe extern "C" fn coh_open(path: *const c_char, _flags: i32, _mode: i32) -> i32 {
    if path.is_null() {
        return EINVAL;
    }
    let p = core::ffi::CStr::from_ptr(path);
    match p.to_bytes() {
        b"/bin/init" => 0,
        b"/etc/plan9.ns" => 1,
        b"/boot/bootargs.txt" => 2,
        b"/srv/cohrole" => 3,
        _ => ENOENT,
    }
}

#[no_mangle]
pub unsafe extern "C" fn coh_read(fd: i32, buf: *mut u8, len: usize) -> isize {
    if fd < 0 || (fd as usize) >= FILES.len() {
        return EBADF as isize;
    }
    let f = &mut FILES[fd as usize];
    let remain = &f.data[f.pos..];
    let n = core::cmp::min(len, remain.len());
    if n == 0 {
        return 0;
    }
    core::ptr::copy_nonoverlapping(remain.as_ptr(), buf, n);
    f.pos += n;
    n as isize
}

#[no_mangle]
pub unsafe extern "C" fn coh_close(fd: i32) -> i32 {
    if fd < 0 || (fd as usize) >= FILES.len() {
        EBADF
    } else {
        FILES[fd as usize].pos = 0;
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
pub unsafe extern "C" fn coh_setenv(_name: *const c_char, _val: *const c_char, _overwrite: i32) -> i32 {
    ENOSYS
}

#[no_mangle]
pub unsafe extern "C" fn coh_bind(_name: *const c_char, _old: *const c_char, _flags: i32) -> i32 {
    0
}

pub fn coh_log(msg: &str) {
    for &b in msg.as_bytes() {
        unsafe { seL4_DebugPutChar(b) };
    }
    unsafe { seL4_DebugPutChar(b'\n') };
    compiler_fence(Ordering::SeqCst);
}

#[no_mangle]
pub unsafe extern "C" fn memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    core::ptr::copy_nonoverlapping(src, dest, n);
    dest
}

#[no_mangle]
pub unsafe extern "C" fn memmove(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    core::ptr::copy(src, dest, n);
    dest
}

#[no_mangle]
pub unsafe extern "C" fn memset(dest: *mut u8, c: i32, n: usize) -> *mut u8 {
    core::ptr::write_bytes(dest, c as u8, n);
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
