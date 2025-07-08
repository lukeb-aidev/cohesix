// CLASSIFICATION: COMMUNITY
// Filename: main.rs v0.10
// Author: Lukas Bower
// Date Modified: 2027-10-04
#![no_std]
#![no_main]
#![feature(alloc_error_handler, asm_experimental_arch)]

extern crate alloc;

use alloc::vec::Vec;
use core::alloc::{GlobalAlloc, Layout};
use core::ffi::{c_char, CStr};
use core::panic::PanicInfo;
use core::ptr;

extern "C" {
    static __heap_start: u8;
    static __heap_end: u8;
    static __stack_end: u8;
}

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

fn print_heap_bounds(start: usize, end: usize) {
    putstr("heap_start");
    put_hex(start);
    putstr("heap_end");
    put_hex(end);
}

fn print_stack_bounds(start: usize, end: usize) {
    putstr("stack_start");
    put_hex(start);
    putstr("stack_end");
    put_hex(end);
}

#[no_mangle]
unsafe extern "C" fn seL4_DebugPutChar(_c: u8) {}
#[no_mangle]
unsafe extern "C" fn open(_path: *const c_char, _flags: i32, _mode: i32) -> i32 {
    -1
}
#[no_mangle]
unsafe extern "C" fn read(_fd: i32, _buf: *mut u8, _len: usize) -> isize {
    0
}
#[no_mangle]
unsafe extern "C" fn close(_fd: i32) -> i32 {
    0
}
#[no_mangle]
unsafe extern "C" fn write(_fd: i32, _buf: *const u8, _len: usize) -> isize {
    0
}
#[no_mangle]
unsafe extern "C" fn execv(_path: *const c_char, _argv: *const *const c_char) -> i32 {
    0
}
#[no_mangle]
unsafe extern "C" fn getenv(_name: *const c_char) -> *const c_char {
    core::ptr::null()
}
#[no_mangle]
unsafe extern "C" fn setenv(_name: *const c_char, _val: *const c_char, _overwrite: i32) -> i32 {
    0
}

struct BumpAllocator;
static mut OFFSET: usize = 0;

unsafe impl GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let align_mask = layout.align() - 1;
        let heap_start = &__heap_start as *const u8 as usize;
        let heap_end = &__heap_end as *const u8 as usize;
        let off = (OFFSET + align_mask) & !align_mask;
        let end_ptr = heap_start + off + layout.size();
        if end_ptr > heap_end {
            putstr("Allocation past heap end!");
            put_hex(end_ptr);
            loop {
                core::hint::spin_loop();
            }
        }
        let ptr = (heap_start + off) as *mut u8;
        OFFSET = off + layout.size();
        putstr("alloc ptr:");
        put_hex(ptr as usize);
        ptr
    }
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
static GLOBAL_ALLOC: BumpAllocator = BumpAllocator;

#[no_mangle]
pub unsafe extern "C" fn _start(_bootinfo: usize) -> ! {
    core::arch::asm!("mov sp, {}", in(reg) &__stack_end);
    main();
    loop {
        core::hint::spin_loop();
    }
}

fn putstr(s: &str) {
    for &b in s.as_bytes() {
        putchar(b);
    }
    putchar(b'\n');
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
        for token in text.split_whitespace() {
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

fn main() {
    putstr("COHESIX_BOOT_OK");
    let heap_start = unsafe { &__heap_start as *const u8 as usize };
    let heap_end = unsafe { &__heap_end as *const u8 as usize };
    print_heap_bounds(heap_start, heap_end);
    let stack_start = unsafe { &__stack_start as *const u8 as usize };
    let stack_end = unsafe { &__stack_end as *const u8 as usize };
    print_stack_bounds(stack_start, stack_end);
    assert!(heap_start >= 0xffffff8040000000 && heap_start < 0xffffff8040633000, "Heap start out of range");
    load_bootargs();
    let role_cstr = env_var("COHROLE");
    let role = role_cstr
        .map(|c| c.to_str().unwrap())
        .unwrap_or("DroneWorker");
    write_role(role);
    putstr("[root] launching userland...");
    exec_init(role);
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    putstr("[root] panic");
    loop {
        core::hint::spin_loop();
    }
}

#[alloc_error_handler]
fn alloc_error(_layout: core::alloc::Layout) -> ! {
    putstr("[root] alloc_error");
    loop {
        core::hint::spin_loop();
    }
}
