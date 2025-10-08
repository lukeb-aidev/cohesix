// CLASSIFICATION: COMMUNITY
// Filename: uart.rs v0.2
// Author: Lukas Bower
// Date Modified: 2029-10-07

use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::dt;
use sel4_sys_extern_wrapper::seL4_DebugPutChar;

const UART_DR: usize = 0x00;
const UART_FR: usize = 0x18;
const UART_IBRD: usize = 0x24;
const UART_FBRD: usize = 0x28;
const UART_LCRH: usize = 0x2c;
const UART_CR: usize = 0x30;
const UART_IMSC: usize = 0x38;
const UART_ICR: usize = 0x44;
const FR_TXFF: u32 = 1 << 5;

static INITIALISED: AtomicBool = AtomicBool::new(false);
static MMIO_ENABLED: AtomicBool = AtomicBool::new(false);
static WARNED_MMIO_DISABLED: AtomicBool = AtomicBool::new(false);

fn base_ptr(offset: usize) -> *mut u32 {
    (dt::UART_BASE + offset) as *mut u32
}

#[inline(always)]
fn mmio_enabled() -> bool {
    MMIO_ENABLED.load(Ordering::Relaxed)
}

#[cfg(test)]
pub fn test_mmio_enabled() -> bool {
    mmio_enabled()
}

#[cfg(test)]
pub fn test_reset_mmio_state() {
    MMIO_ENABLED.store(false, Ordering::SeqCst);
    INITIALISED.store(false, Ordering::SeqCst);
    WARNED_MMIO_DISABLED.store(false, Ordering::SeqCst);
}

fn log_mmio_disabled_once() {
    if !crate::bootinfo::CONFIG_PRINTING {
        return;
    }
    if WARNED_MMIO_DISABLED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
        .is_err()
    {
        return;
    }
    for &b in b"uart:mmio_unmapped" {
        seL4_DebugPutChar(b as i32);
    }
    seL4_DebugPutChar(b'\n' as i32);
}

/// Enable MMIO access once the UART frame has been mapped into the address space.
pub fn enable_mmio() {
    MMIO_ENABLED.store(true, Ordering::SeqCst);
    INITIALISED.store(false, Ordering::SeqCst);
}

pub fn init() {
    if !mmio_enabled() {
        INITIALISED.store(true, Ordering::Release);
        log_mmio_disabled_once();
        return;
    }
    if INITIALISED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    unsafe {
        // Disable UART while configuring
        ptr::write_volatile(base_ptr(UART_CR), 0);
        // Mask and clear interrupts
        ptr::write_volatile(base_ptr(UART_IMSC), 0);
        ptr::write_volatile(base_ptr(UART_ICR), 0x7ff);
        // Baud rate divisors for 24 MHz clock -> 115200 baud (IBRD=13, FBRD=2)
        ptr::write_volatile(base_ptr(UART_IBRD), 13);
        ptr::write_volatile(base_ptr(UART_FBRD), 2);
        // 8-bit words, FIFO disabled, no parity
        ptr::write_volatile(base_ptr(UART_LCRH), 0b11 << 5);
        // Enable UART, TX and RX
        ptr::write_volatile(base_ptr(UART_CR), (1 << 0) | (1 << 8) | (1 << 9));
    }
}

#[inline(always)]
pub fn write_byte(byte: u8) {
    if !mmio_enabled() {
        return;
    }
    if !INITIALISED.load(Ordering::Acquire) {
        init();
    }
    if !INITIALISED.load(Ordering::Acquire) {
        return;
    }
    unsafe {
        let fr = base_ptr(UART_FR) as *const u32;
        let dr = base_ptr(UART_DR);
        // Wait for space in the TX FIFO
        let mut spins = 0;
        while ptr::read_volatile(fr) & FR_TXFF != 0 {
            spins += 1;
            if spins > 1_000_000 {
                break;
            }
        }
        ptr::write_volatile(dr, byte as u32);
    }
}

pub fn write_char(c: u8) {
    if c == b'\n' {
        write_byte(b'\r');
    }
    write_byte(c);
}
