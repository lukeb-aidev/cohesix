// CLASSIFICATION: COMMUNITY
// Filename: allocator.rs v0.7
// Author: Lukas Bower
// Date Modified: 2027-11-20

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
#[link_section = ".bss"]
static mut OFFSET: usize = 0;

/// Return the current heap offset pointer for auditing
pub fn offset_addr() -> usize {
    unsafe { &OFFSET as *const usize as usize }
}

/// Return the current heap pointer (heap_start + OFFSET)
pub fn current_heap_ptr() -> usize {
    unsafe { (&__heap_start as *const u8 as usize).wrapping_add(OFFSET) }
}

/// Log allocator state during initialization
pub fn allocator_init_log() {
    coherr!(
        "allocator_init heap_start={:#x} heap_ptr={:#x} heap_end={:#x} img_end={:#x}",
        unsafe { &__heap_start as *const u8 as usize },
        current_heap_ptr(),
        unsafe { &__heap_end as *const u8 as usize },
        crate::image_end()
    );
}

unsafe impl GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let align_mask = layout.align() - 1;
        let heap_start = &__heap_start as *const u8 as usize;
        let heap_end = &__heap_end as *const u8 as usize;
        log_regs();
        putstr("alloc offset");
        put_hex(OFFSET);
        let off = OFFSET
            .checked_add(align_mask)
            .unwrap_or_else(|| {
                putstr("alloc off ovf");
                crate::abort("heap overflow");
            })
            & !align_mask;
        putstr("alloc aligned");
        put_hex(off);
        let end_ptr = heap_start
            .checked_add(off)
            .and_then(|v| v.checked_add(layout.size()))
            .unwrap_or_else(|| {
                putstr("alloc end ovf");
                crate::abort("heap overflow");
            });
        putstr("alloc endptr");
        put_hex(end_ptr);
        if end_ptr > heap_end {
            putstr("alloc overflow");
            put_hex(end_ptr);
            crate::abort("heap overflow");
        }
        let img_end = crate::image_end();
        if end_ptr > img_end {
            putstr("alloc past image_end");
            put_hex(end_ptr);
            put_hex(img_end);
            crate::abort("heap overflow");
        }
        let ptr = (heap_start + off) as *mut u8;
        OFFSET = off + layout.size();
        putstr("alloc ptr");
        put_hex(ptr as usize);
        check_heap_ptr(end_ptr - 1);
        check_heap_ptr(ptr as usize);
        crate::validate_ptr(ptr as usize);
        ptr
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
pub static GLOBAL_ALLOC: BumpAllocator = BumpAllocator;
