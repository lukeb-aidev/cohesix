// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the uart/pl011 module for root-task.
// Author: Lukas Bower
//! Minimal PL011 UART driver for bootstrap diagnostics and console I/O.
#![allow(unsafe_code)]

use core::ptr::NonNull;
use core::sync::atomic::{AtomicUsize, Ordering};

use sel4_sys::seL4_DebugPutChar;

const DR: usize = 0x00;
const FR: usize = 0x18;
const FR_TXFF: u32 = 1 << 5;
const FR_RXFE: u32 = 1 << 4;

static UART_BASE: AtomicUsize = AtomicUsize::new(0);

/// Registers a mapped PL011 UART base for later console use.
pub fn register_console_base(vaddr: usize) {
    UART_BASE.store(vaddr, Ordering::Release);
}

/// Returns the registered PL011 console base, if installed.
#[must_use]
pub fn console_base() -> Option<NonNull<u8>> {
    let base = UART_BASE.load(Ordering::Acquire);
    NonNull::new(base as *mut u8)
}

#[inline(always)]
fn base_ptr() -> *mut u8 {
    let base = UART_BASE.load(Ordering::Acquire);
    if base == 0 {
        panic!("PL011 console base not installed");
    }
    base as *mut u8
}

#[inline(always)]
unsafe fn read_reg(offset: usize) -> u32 {
    // SAFETY: `base_ptr` is installed only after the UART page has been mapped
    // by HAL. Offsets are constants for 32-bit PL011 registers in the mapped
    // page, and volatile access is required for MMIO.
    unsafe {
        let ptr = base_ptr().add(offset) as *const u32;
        core::ptr::read_volatile(ptr)
    }
}

#[inline(always)]
unsafe fn write_reg(offset: usize, value: u32) {
    // SAFETY: `base_ptr` is installed only after the UART page has been mapped
    // by HAL. Offsets are constants for 32-bit PL011 registers in the mapped
    // page, and volatile access is required for MMIO.
    unsafe {
        let ptr = base_ptr().add(offset) as *mut u32;
        core::ptr::write_volatile(ptr, value);
    }
}

fn wait_tx_ready() {
    // SAFETY: Polls the PL011 flag register through the HAL-installed MMIO
    // base; the register offset is within the mapped page.
    unsafe {
        while read_reg(FR) & FR_TXFF != 0 {
            core::hint::spin_loop();
        }
    }
}

fn putc(byte: u8) {
    wait_tx_ready();
    // SAFETY: Writes one byte to the PL011 data register after confirming the
    // transmit FIFO is not full.
    unsafe {
        write_reg(DR, byte as u32);
    }
}

/// Write a single byte to the PL011 UART.
pub fn write_byte(byte: u8) {
    putc(byte);
}

/// Poll for a pending byte without blocking.
pub fn poll_byte() -> Option<u8> {
    // SAFETY: Reads the PL011 flag/data registers through the HAL-installed
    // MMIO base and only consumes data when RXFE is clear.
    unsafe {
        if read_reg(FR) & FR_RXFE != 0 {
            None
        } else {
            Some(read_reg(DR) as u8)
        }
    }
}

fn puts(line: &str) {
    if line.trim().is_empty() {
        return;
    }
    for &byte in line.as_bytes() {
        if byte == b'\n' {
            putc(b'\r');
        }
        putc(byte);
    }
}

/// Write a full string to the UART, translating newlines to CRLF.
pub fn write_str(line: &str) {
    puts(line);
}

/// Emits a heartbeat byte to the seL4 debug console for diagnostics.
pub fn heartbeat(byte: u8) {
    #[cfg(target_os = "none")]
    // SAFETY: `seL4_DebugPutChar` is a byte-oriented kernel debug syscall and
    // does not dereference user memory.
    unsafe {
        seL4_DebugPutChar(byte);
    }
    #[cfg(not(target_os = "none"))]
    seL4_DebugPutChar(byte);
}
