// CLASSIFICATION: COMMUNITY
// Filename: main.rs v0.44
// Author: Lukas Bower
// Date Modified: 2027-12-31
#![no_std]
#![no_main]
#![feature(alloc_error_handler, asm_experimental_arch, lang_items)]

extern crate alloc;

mod allocator;
mod lang_items;
mod sys;
mod bootinfo;
mod dt;

use core::arch::global_asm;
global_asm!(include_str!("entry.S"));

use core::fmt::{self, Write};
use core::sync::atomic::{compiler_fence, Ordering};

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

use crate::sys::seL4_DebugPutChar;

#[no_mangle]
#[link_section = ".bss"]
static mut VALIDATOR_HANDLE: *const u8 = core::ptr::null();
#[no_mangle]
#[link_section = ".bss"]
static mut SRV_HANDLE: *const u8 = core::ptr::null();
#[no_mangle]
#[link_section = ".bss"]
static mut CUDA_HANDLE: *const u8 = core::ptr::null();

#[link_section = ".bss"]
static mut WATCHED_PTRS: [usize; 64] = [0; 64];
#[link_section = ".bss"]
static mut WATCHED_IDX: usize = 0;
#[link_section = ".bss"]
static mut ALLOC_CHECK: u64 = 0;

#[link_section = ".rodata"]
#[used]
static ROOTSERVER_ONLINE: &[u8] = b"ROOTSERVER ONLINE";

fn putchar(c: u8) {
    unsafe { seL4_DebugPutChar(c) };
}

fn uart_flush() {
    compiler_fence(Ordering::SeqCst);
}

pub struct CohLogger;

impl Write for CohLogger {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for &b in s.as_bytes() {
            putchar(b);
        }
        putchar(b'\n');
        uart_flush();
        Ok(())
    }
}

#[macro_export]
macro_rules! coherr {
    ($($arg:tt)*) => {{
        use core::fmt::Write;
        let _ = write!(&mut $crate::CohLogger, $($arg)*);
        $crate::uart_flush();
    }};
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
        putstr("offset_ptr");
        put_hex(crate::allocator::offset_addr());
    }
}

fn log_mem_map() {
    coherr!(
        "mem_map bss_start={:#x} bss_end={:#x} heap_start={:#x} heap_end={:#x} stack_start={:#x} stack_end={:#x}",
        unsafe { &__bss_start as *const u8 as usize },
        unsafe { &__bss_end as *const u8 as usize },
        unsafe { &__heap_start as *const u8 as usize },
        unsafe { &__heap_end as *const u8 as usize },
        unsafe { &__stack_start as *const u8 as usize },
        unsafe { &__stack_end as *const u8 as usize },
    );
}

fn image_end() -> usize {
    unsafe { &__stack_end as *const u8 as usize }
}

fn check_bss_zero() {
    let start = unsafe { &__bss_start as *const u8 };
    let end = unsafe { &__bss_end as *const u8 };
    let mut ptr = start;
    let mut first_nonzero: usize = 0;
    let mut count = 0usize;
    while ptr < end {
        unsafe {
            let val = ptr.read();
            if val != 0 {
                if count == 0 {
                    first_nonzero = ptr as usize;
                }
                count += 1;
            }
        }
        ptr = unsafe { ptr.add(1) };
    }
    if count > 0 {
        coherr!("bss_not_zero first={:#x} count={}", first_nonzero, count);
        panic!("bss not zero");
    } else {
        coherr!("bss_zero_ok");
    }
}

fn check_heap_bounds() {
    let start = unsafe { &__heap_start as *const u8 as usize };
    let end = unsafe { &__heap_end as *const u8 as usize };
    if start >= end {
        coherr!("heap_invalid start={:#x} end={:#x}", start, end);
        panic!("heap bounds invalid");
    }
}

fn check_globals_zero() {
    unsafe {
        if !VALIDATOR_HANDLE.is_null()
            || !SRV_HANDLE.is_null()
            || !CUDA_HANDLE.is_null()
        {
            coherr!(
                "globals_nonzero v={:#x} s={:#x} c={:#x}",
                VALIDATOR_HANDLE as usize,
                SRV_HANDLE as usize,
                CUDA_HANDLE as usize
            );
            panic!("globals not zero");
        }
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
    let img_end = image_end();
    if ptr < start || ptr >= end || ptr >= img_end || ptr < 0x400000 {
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
    let max = image_end();
    if ptr == 0 || ptr < BASE || ptr >= max {
        putstr("PTR OUT OF RANGE");
        put_hex(ptr);
        abort("invalid pointer");
    }
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
const PATH_PLAN9_NS: &[u8] = b"/etc/plan9.ns\0";
const INIT_BIN: &[u8] = b"/bin/init\0";
// DEBUG: rodata audit string
static RODATA_CHECK: &str = "RODATA_OK";

fn load_bootargs() {
    unsafe {
        let fd = sys::coh_open(cstr(PATH_BOOTARGS), 0, 0);
        if fd < 0 {
            return;
        }
        let mut buf = [0u8; 256];
        let n = sys::coh_read(fd, buf.as_mut_ptr(), buf.len()) as usize;
        sys::coh_close(fd);
        let text = match core::str::from_utf8(&buf[..n]) {
            Ok(t) => t,
            Err(_) => return,
        };
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
            let token = match core::str::from_utf8(&bytes[start..i]) {
                Ok(t) => t,
                Err(_) => { continue; }
            };
            if let Some(eq) = token.find('=') {
                let (k, v) = token.split_at(eq);
                let v = &v[1..];
                let mut kb = Vec::from(k.as_bytes());
                kb.push(0);
                let mut vb = Vec::from(v.as_bytes());
                vb.push(0);
                sys::coh_setenv(
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
        let ptr = sys::coh_getenv(nb.as_ptr() as *const c_char);
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
        let fd = sys::coh_open(cstr(PATH_COHROLE), 0x601, 0o644);
        if fd >= 0 {
            let _ = sys::coh_write(fd, rb.as_ptr(), role.len());
            sys::coh_close(fd);
        }
    }
}

const MAFTER: i32 = 2;

fn read_file(path: &[u8]) -> Option<Vec<u8>> {
    unsafe {
        let fd = sys::coh_open(cstr(path), 0, 0);
        if fd < 0 {
            coherr!("open_failed {}", core::str::from_utf8_unchecked(path));
            return None;
        }
        let mut data = Vec::new();
        let mut buf = [0u8; 256];
        loop {
            let n = sys::coh_read(fd, buf.as_mut_ptr(), buf.len()) as isize;
            if n <= 0 {
                break;
            }
            data.extend_from_slice(&buf[..n as usize]);
        }
        sys::coh_close(fd);
        Some(data)
    }
}

fn apply_namespace() {
    if let Some(data) = read_file(PATH_PLAN9_NS) {
        if let Ok(text) = core::str::from_utf8(&data) {
            for line in text.lines() {
                let line = line.split('#').next().unwrap_or("").trim();
                if line.is_empty() {
                    continue;
                }
                let parts: Vec<&str> = line.split_whitespace().collect();
                match parts.as_slice() {
                    ["bind", "-a", src, dst] => unsafe {
                        coherr!("bind -a {} {}", src, dst);
                        let mut sb = Vec::from(src.as_bytes());
                        sb.push(0);
                        let mut db = Vec::from(dst.as_bytes());
                        db.push(0);
                        sys::coh_bind(sb.as_ptr() as *const c_char, db.as_ptr() as *const c_char, MAFTER);
                    },
                    ["bind", src, dst] => unsafe {
                        coherr!("bind {} {}", src, dst);
                        let mut sb = Vec::from(src.as_bytes());
                        sb.push(0);
                        let mut db = Vec::from(dst.as_bytes());
                        db.push(0);
                        sys::coh_bind(sb.as_ptr() as *const c_char, db.as_ptr() as *const c_char, 0);
                    },
                    _ => coherr!("ignore_line {}", line),
                }
            }
        }
    }
}

fn check_init_exists() -> bool {
    unsafe {
        let fd = sys::coh_open(cstr(INIT_BIN), 0, 0);
        if fd < 0 {
            coherr!("missing_init_bin");
            false
        } else {
            sys::coh_close(fd);
            true
        }
    }
}

fn exec_init() -> ! {
    let argv = [INIT_BIN.as_ptr() as *const c_char, ptr::null()];
    unsafe {
        if sys::coh_exec(INIT_BIN.as_ptr() as *const c_char, argv.as_ptr()) != 0 {
            sys::coh_log("fatal: exec failed");
        }
    }
    loop {
        sys::coh_log("fatal: exec failed");
        core::hint::spin_loop();
    }
}

#[no_mangle]
pub extern "C" fn main() {
    sys::init_uart();
    sys::coh_log("ROOTSERVER ONLINE");
    unsafe {
        ALLOC_CHECK = 0xdeadbeefdeadbeef;
        if ALLOC_CHECK != 0xdeadbeefdeadbeef {
            coherr!("BSS corruption detected.");
            panic!("Fence failed.");
        }
    }
    check_bss_zero();
    log_mem_map();
    coherr!(
        "heap_state start={:#x} end={:#x} offset_ptr={:#x}",
        unsafe { &__heap_start as *const u8 as usize },
        unsafe { &__heap_end as *const u8 as usize },
        crate::allocator::offset_addr(),
    );
    check_heap_bounds();
    check_globals_zero();
    coherr!("boot_ok: bss, heap, globals validated");
    crate::allocator::allocator_init_log();
    unsafe { bootinfo::dump_bootinfo(); }
    coherr!("main_start bss_start={:#x} bss_end={:#x} heap_start={:#x} heap_ptr={:#x} heap_end={:#x} img_end={:#x}",
        unsafe { &__bss_start as *const u8 as usize },
        unsafe { &__bss_end as *const u8 as usize },
        unsafe { &__heap_start as *const u8 as usize },
        crate::allocator::current_heap_ptr(),
        unsafe { &__heap_end as *const u8 as usize },
        image_end());
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
    assert!(bss_start < bss_end, "bss range invalid");
    putstr("bss_start");
    put_hex(bss_start);
    putstr("bss_end");
    put_hex(bss_end);
    putstr("bootargs_ptr");
    put_hex(PATH_BOOTARGS.as_ptr() as usize);
    putstr("cohrole_ptr");
    put_hex(PATH_COHROLE.as_ptr() as usize);
    putstr("init_bin_ptr");
    put_hex(INIT_BIN.as_ptr() as usize);
    let local = 0u8;
    putstr("local");
    put_hex(&local as *const _ as usize);
    let heap_start = unsafe { &__heap_start as *const u8 as usize };
    let heap_end = unsafe { &__heap_end as *const u8 as usize };
    log_heap_bounds(heap_start, heap_end);
    let heap_ptr = crate::allocator::current_heap_ptr();
    assert!(heap_ptr < image_end(), "heap ptr beyond image end");
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
    if env_var("INIT_SH_DEBUG").is_some() {
        coherr!("bootarg_init_debug");
    }
    if env_var("INIT_SKIP_CUDA").is_some() {
        coherr!("bootarg_skip_cuda");
    }
    let role_cstr = env_var("COHROLE");
    let role = role_cstr
        .and_then(|c| c.to_str().ok())
        .unwrap_or("DroneWorker");
    write_role(role);
    apply_namespace();
    if !check_init_exists() {
        coherr!("fatal_missing_init");
    }
    putstr("[root] launching userland...");
    exec_init();
    putstr("✅ rootserver main loop entered");
    main_loop();
    unsafe { sys::seL4_DebugHalt(); }
}

fn main_loop() -> ! {
    loop {
        unsafe { sys::seL4_Yield(); }
    }
}

