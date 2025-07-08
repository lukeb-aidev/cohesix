// CLASSIFICATION: COMMUNITY
// Filename: allocator.rs v0.4
// Author: Lukas Bower
// Date Modified: 2027-10-17

use core::alloc::{GlobalAlloc, Layout};
use crate::check_heap_ptr;

extern "C" {
    static __heap_start: u8;
    static __heap_end: u8;
}

fn putchar(c: u8) {
    unsafe { crate::seL4_DebugPutChar(c) };
}

fn putstr(s: &str) {
    for &b in s.as_bytes() {
        putchar(b);
    }
    putchar(b'\n');
}

fn put_hex(mut val: usize) {
    let hex = b"0123456789abcdef";
    let mut buf = [0u8; 18];
    buf[0] = b'0';
    buf[1] = b'x';
    for i in 0..16 {
        buf[17 - i] = hex[(val & 0xf) as usize];
        val >>= 4;
    }
    for c in &buf {
        putchar(*c);
    }
    putchar(b'\n');
}

fn log_regs() {
    let mut sp: usize;
    let mut fp: usize;
    unsafe {
        core::arch::asm!("mov {0}, sp", out(reg) sp);
        core::arch::asm!("mov {0}, x29", out(reg) fp);
    }
    putstr("reg_sp");
    put_hex(sp);
    putstr("reg_fp");
    put_hex(fp);
}

pub struct BumpAllocator;
static mut OFFSET: usize = 0;

unsafe impl GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let align_mask = layout.align() - 1;
        let heap_start = &__heap_start as *const u8 as usize;
        let heap_end = &__heap_end as *const u8 as usize;
        log_regs();
        putstr("alloc offset");
        put_hex(OFFSET);
        let off = (OFFSET + align_mask) & !align_mask;
        putstr("alloc aligned");
        put_hex(off);
        let end_ptr = heap_start + off + layout.size();
        putstr("alloc endptr");
        put_hex(end_ptr);
        if end_ptr > heap_end {
            putstr("alloc overflow");
            put_hex(end_ptr);
            crate::abort("heap overflow");
        }
        let ptr = (heap_start + off) as *mut u8;
        OFFSET = off + layout.size();
        putstr("alloc ptr");
        put_hex(ptr as usize);
        check_heap_ptr(end_ptr - 1);
        check_heap_ptr(ptr as usize);
        ptr
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
pub static GLOBAL_ALLOC: BumpAllocator = BumpAllocator;
