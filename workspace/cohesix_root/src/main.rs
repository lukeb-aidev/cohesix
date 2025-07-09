// CLASSIFICATION: COMMUNITY
// Filename: main.rs v0.27
// Author: Lukas Bower
// Date Modified: 2027-11-08
#![no_std]
#![no_main]
#![feature(alloc_error_handler, asm_experimental_arch, lang_items)]

extern crate alloc;

mod allocator;
mod lang_items;

use core::arch::global_asm;
global_asm!(include_str!("entry.S"));

use alloc::vec::Vec;
use core::ffi::{c_char, CStr};
use core::ptr;

extern "C" {
    static __heap_start: u8;
    static __heap_end: u8;
    static __stack_start: u8;
    static __stack_end: u8;
    static mut __bss_start: u8;
    static mut __bss_end: u8;
}

#[no_mangle]
static mut VALIDATOR_HANDLE: *const u8 = core::ptr::null();
#[no_mangle]
static mut SRV_HANDLE: *const u8 = core::ptr::null();
#[no_mangle]
static mut CUDA_HANDLE: *const u8 = core::ptr::null();

static mut WATCHED_PTRS: [usize; 64] = [0; 64];
static mut WATCHED_IDX: usize = 0;

fn putchar(c: u8) {
    unsafe { seL4_DebugPutChar(c) };
}

fn put_hex(val: usize) {
    let hex = b"0123456789abcdef";
    let mut buf = [0u8; 18];
    buf[0] = b'0';
    buf[1] = b'x';
    for i in 0..16 {
        buf[17 - i] = hex[(val >> (i * 4)) & 0xf];
    }
    for c in &buf {
        putchar(*c);
    }
    putchar(b'\n');
}

fn dump_stack(sp: usize) {
    putstr("stack_dump_start");
    for i in 0..4 {
        let addr = sp.wrapping_add(i * core::mem::size_of::<usize>());
        put_hex(addr);
        let val = unsafe { (addr as *const usize).read_volatile() };
        put_hex(val);
    }
    putstr("stack_dump_end");
}

#[no_mangle]
pub extern "C" fn boot_log_hex(val: usize) {
    put_hex(val);
}

fn log_regs() {
    let mut sp: usize;
    let mut fp: usize;
    unsafe {
        core::arch::asm!("mov {0}, sp", out(reg) sp);
        core::arch::asm!("mov {0}, x29", out(reg) fp);
    }
    putstr("log_sp");
    put_hex(sp);
    putstr("log_fp");
    put_hex(fp);
}

fn log_heap_bounds(start: usize, end: usize) {
    putstr("heap_start");
    put_hex(start);
    putstr("heap_end");
    put_hex(end);
}

fn log_stack_bounds(start: usize, end: usize) {
    putstr("stack_start");
    put_hex(start);
    putstr("stack_end");
    put_hex(end);
}

fn log_global_ptrs() {
    unsafe {
        putstr("validator_ptr");
        put_hex(VALIDATOR_HANDLE as usize);
        putstr("srv_ptr");
        put_hex(SRV_HANDLE as usize);
        putstr("cuda_ptr");
        put_hex(CUDA_HANDLE as usize);
    }
}

fn watch_ptr(ptr: usize) {
    unsafe {
        if WATCHED_IDX < WATCHED_PTRS.len() {
            WATCHED_PTRS[WATCHED_IDX] = ptr;
            WATCHED_IDX += 1;
        }
    }
}

fn ptr_hash() -> usize {
    unsafe {
        let mut h = 0usize;
        for p in &WATCHED_PTRS[..WATCHED_IDX] {
            h ^= *p;
        }
        h
    }
}

pub fn check_heap_ptr(ptr: usize) {
    log_regs();
    putstr("check_heap_ptr");
    put_hex(ptr);
    let start = unsafe { &__heap_start as *const u8 as usize };
    let end = unsafe { &__heap_end as *const u8 as usize };
    if ptr < start || ptr >= end {
        putstr("HEAP POINTER OUT OF RANGE");
        put_hex(ptr);
        abort("heap pointer out of range");
    }
}

pub fn check_rodata_ptr(ptr: usize) {
    let text_start: usize = 0x400000;
    let ro_end = unsafe { &__bss_start as *const u8 as usize };
    if ptr < text_start || ptr >= ro_end {
        putstr("RODATA POINTER OUT OF RANGE");
        put_hex(ptr);
        abort("rodata pointer out of range");
    }
}

pub fn validate_ptr(ptr: usize) {
    const BASE: usize = 0x400000;
    const MAX: usize = BASE + 0x800000; // limit to first 8MB
    if ptr == 0 || ptr < BASE || ptr >= MAX {
        putstr("PTR OUT OF RANGE");
        put_hex(ptr);
        abort("invalid pointer");
    }
}

#[no_mangle]
pub unsafe extern "C" fn seL4_DebugPutChar(_c: u8) {}
#[no_mangle]
unsafe extern "C" fn open(path: *const c_char, _flags: i32, _mode: i32) -> i32 {
    putstr("open ptr");
    put_hex(path as usize);
    validate_ptr(path as usize);
    watch_ptr(path as usize);
    -1
}
#[no_mangle]
unsafe extern "C" fn read(_fd: i32, buf: *mut u8, _len: usize) -> isize {
    putstr("read buf");
    put_hex(buf as usize);
    validate_ptr(buf as usize);
    watch_ptr(buf as usize);
    check_heap_ptr(buf as usize);
    0
}
#[no_mangle]
unsafe extern "C" fn close(_fd: i32) -> i32 {
    0
}
#[no_mangle]
unsafe extern "C" fn write(_fd: i32, buf: *const u8, _len: usize) -> isize {
    putstr("write buf");
    put_hex(buf as usize);
    validate_ptr(buf as usize);
    watch_ptr(buf as usize);
    check_heap_ptr(buf as usize);
    0
}
#[no_mangle]
unsafe extern "C" fn execv(path: *const c_char, _argv: *const *const c_char) -> i32 {
    putstr("execv path");
    put_hex(path as usize);
    validate_ptr(path as usize);
    watch_ptr(path as usize);
    -1
}
#[no_mangle]
unsafe extern "C" fn getenv(name: *const c_char) -> *const c_char {
    putstr("getenv name");
    put_hex(name as usize);
    validate_ptr(name as usize);
    watch_ptr(name as usize);
    core::ptr::null()
}
#[no_mangle]
unsafe extern "C" fn setenv(name: *const c_char, val: *const c_char, _overwrite: i32) -> i32 {
    putstr("setenv name");
    put_hex(name as usize);
    validate_ptr(name as usize);
    watch_ptr(name as usize);
    putstr("setenv val");
    put_hex(val as usize);
    validate_ptr(val as usize);
    watch_ptr(val as usize);
    0
}



pub fn putstr(s: &str) {
    for &b in s.as_bytes() {
        putchar(b);
    }
    putchar(b'\n');
}

pub fn abort(msg: &str) -> ! {
    putstr(msg);
    loop {
        core::hint::spin_loop();
    }
}

fn cstr(bytes: &[u8]) -> *const c_char {
    bytes.as_ptr() as *const c_char
}

const PATH_BOOTARGS: &[u8] = b"/boot/bootargs.txt\0";
const PATH_COHROLE: &[u8] = b"/srv/cohrole\0";
const BIN_RC: &[u8] = b"/bin/rc\0";
const SCRIPT_WORKER: &[u8] = b"/init/worker.rc\0";
const SCRIPT_KIOSK: &[u8] = b"/init/kiosk.rc\0";
const SCRIPT_SENSOR: &[u8] = b"/init/sensor.rc\0";
const SCRIPT_SIMTEST: &[u8] = b"/init/simtest.rc\0";
const SCRIPT_QUEEN: &[u8] = b"/init/queen.rc\0";
// DEBUG: rodata audit string
static RODATA_CHECK: &str = "RODATA_OK";

fn load_bootargs() {
    unsafe {
        let fd = open(cstr(PATH_BOOTARGS), 0, 0);
        if fd < 0 {
            return;
        }
        let mut buf = [0u8; 256];
        let n = read(fd, buf.as_mut_ptr(), buf.len()) as usize;
        close(fd);
        let text = core::str::from_utf8_unchecked(&buf[..n]);
        let bytes = text.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i >= bytes.len() {
                break;
            }
            let start = i;
            while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            let token = core::str::from_utf8_unchecked(&bytes[start..i]);
            if let Some(eq) = token.find('=') {
                let (k, v) = token.split_at(eq);
                let v = &v[1..];
                let mut kb = Vec::from(k.as_bytes());
                kb.push(0);
                let mut vb = Vec::from(v.as_bytes());
                vb.push(0);
                setenv(
                    kb.as_ptr() as *const c_char,
                    vb.as_ptr() as *const c_char,
                    1,
                );
            }
        }
    }
}

fn env_var(name: &str) -> Option<&'static CStr> {
    let mut nb = Vec::from(name.as_bytes());
    nb.push(0);
    unsafe {
        let ptr = getenv(nb.as_ptr() as *const c_char);
        putstr("getenv return");
        put_hex(ptr as usize);
        if ptr.is_null() {
            None
        } else {
            Some(CStr::from_ptr(ptr))
        }
    }
}

fn write_role(role: &str) {
    unsafe {
        let mut rb = Vec::from(role.as_bytes());
        rb.push(0);
        let fd = open(cstr(PATH_COHROLE), 0x601, 0o644);
        if fd >= 0 {
            let _ = write(fd, rb.as_ptr(), role.len());
            close(fd);
        }
    }
}

fn role_script(role: &str) -> &'static [u8] {
    match role {
        "DroneWorker" => SCRIPT_WORKER,
        "KioskInteractive" | "InteractiveAiBooth" => SCRIPT_KIOSK,
        "SensorRelay" => SCRIPT_SENSOR,
        "SimulatorTest" => SCRIPT_SIMTEST,
        _ => SCRIPT_QUEEN,
    }
}

fn exec_init(role: &str) -> ! {
    let script = role_script(role);
    let argv = [
        BIN_RC.as_ptr() as *const c_char,
        script.as_ptr() as *const c_char,
        ptr::null(),
    ];
    unsafe {
        execv(BIN_RC.as_ptr() as *const c_char, argv.as_ptr());
    }
    loop {
        core::hint::spin_loop();
    }
}

#[no_mangle]
pub extern "C" fn main() {
    putstr("COHESIX_BOOT_OK");
    let mut sp: usize;
    let mut fp: usize;
    unsafe {
        core::arch::asm!("mov {0}, sp", out(reg) sp);
        core::arch::asm!("mov {0}, x29", out(reg) fp);
    }
    putstr("SP");
    put_hex(sp);
    putstr("FP");
    put_hex(fp);
    dump_stack(sp);
    log_global_ptrs();
    let ph = ptr_hash();
    putstr("ptr_hash");
    put_hex(ph);
    let bss_start = unsafe { &__bss_start as *const u8 as usize };
    let bss_end = unsafe { &__bss_end as *const u8 as usize };
    putstr("bss_start");
    put_hex(bss_start);
    putstr("bss_end");
    put_hex(bss_end);
    putstr("bootargs_ptr");
    put_hex(PATH_BOOTARGS.as_ptr() as usize);
    putstr("cohrole_ptr");
    put_hex(PATH_COHROLE.as_ptr() as usize);
    putstr("rc_bin_ptr");
    put_hex(BIN_RC.as_ptr() as usize);
    let local = 0u8;
    putstr("local");
    put_hex(&local as *const _ as usize);
    let heap_start = unsafe { &__heap_start as *const u8 as usize };
    let heap_end = unsafe { &__heap_end as *const u8 as usize };
    log_heap_bounds(heap_start, heap_end);
    let stack_start = unsafe { &__stack_start as *const u8 as usize };
    let stack_end = unsafe { &__stack_end as *const u8 as usize };
    log_stack_bounds(stack_start, stack_end);
    if sp < stack_start || sp > stack_end {
        putstr("STACK CORRUPTION");
        abort("sp out of range");
    }
    let ro_ptr = RODATA_CHECK.as_ptr() as usize;
    putstr("rodata_addr");
    put_hex(ro_ptr);
    check_rodata_ptr(ro_ptr);
    putstr("rodata_val");
    putstr(RODATA_CHECK);
    let local_addr = &local as *const _ as usize;
    if !(local_addr >= stack_start && local_addr <= stack_end) {
        abort("local var outside stack");
    }
    if !(heap_start >= 0x400000 && heap_start < 0xa33000) {
        abort("heap start out of range");
    }
    if !(stack_end > stack_start && stack_end - stack_start == 0x10000) {
        abort("stack bounds invalid");
    }
    load_bootargs();
    let role_cstr = env_var("COHROLE");
    let role = role_cstr
        .and_then(|c| c.to_str().ok())
        .unwrap_or("DroneWorker");
    write_role(role);
    putstr("[root] launching userland...");
    exec_init(role);
}

