// Author: Lukas Bower
//! Minimal PL011 UART driver for bootstrap diagnostics and console I/O.
#![allow(unsafe_code)]

use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::cspace::tuples::RetypeTuple;
use sel4_sys::{
    seL4_ARM_Page_Map, seL4_ARM_Page_Uncached, seL4_ARM_SmallPageObject, seL4_CPtr,
    seL4_CapRights_ReadWrite, seL4_DebugPutChar, seL4_Error, seL4_Word,
};

pub const PL011_PADDR: u64 = 0x0900_0000;
/// Virtual address used for the bootstrap console mapping.
pub const PL011_VADDR: usize = 0xA000_0000;

const DR: usize = 0x00;
const FR: usize = 0x18;
const IBRD: usize = 0x24;
const FBRD: usize = 0x28;
const LCRH: usize = 0x2c;
const CR: usize = 0x30;
const ICR: usize = 0x44;

const FR_TXFF: u32 = 1 << 5;
const FR_RXFE: u32 = 1 << 4;

const CR_UARTEN: u32 = 1 << 0;
const CR_TXE: u32 = 1 << 8;
const CR_RXE: u32 = 1 << 9;

static UART_BASE: AtomicUsize = AtomicUsize::new(0);
static UART_READY: AtomicBool = AtomicBool::new(false);

/// Registers a mapped PL011 UART base for later console use.
pub fn register_console_base(vaddr: usize) {
    UART_BASE.store(vaddr, Ordering::Release);
    UART_READY.store(true, Ordering::Release);
}

/// Returns true when the PL011 base has been registered.
pub fn is_ready() -> bool {
    UART_READY.load(Ordering::Acquire)
}

#[inline(always)]
fn base_ptr() -> Option<*mut u8> {
    if !is_ready() {
        unsafe { seL4_DebugPutChar(b'!' as _) };
        return None;
    }

    let base = UART_BASE.load(Ordering::Acquire);
    if base == 0 {
        unsafe { seL4_DebugPutChar(b'!' as _) };
        return None;
    }

    Some(base as *mut u8)
}

#[inline(always)]
unsafe fn read_reg(offset: usize) -> Option<u32> {
    base_ptr().map(|base| unsafe {
        let ptr = base.add(offset) as *const u32;
        core::ptr::read_volatile(ptr)
    })
}

#[inline(always)]
unsafe fn write_reg(offset: usize, value: u32) -> bool {
    if let Some(base) = base_ptr() {
        unsafe {
            let ptr = base.add(offset) as *mut u32;
            core::ptr::write_volatile(ptr, value);
        }
        true
    } else {
        false
    }
}

fn wait_tx_ready() -> bool {
    unsafe {
        while let Some(flags) = read_reg(FR) {
            if flags & FR_TXFF == 0 {
                return true;
            }
            core::hint::spin_loop();
        }
    }

    false
}

fn wait_rx_ready() -> bool {
    unsafe {
        while let Some(flags) = read_reg(FR) {
            if flags & FR_RXFE == 0 {
                return true;
            }
            core::hint::spin_loop();
        }
    }

    false
}

/// Initialise the PL011 UART for 115200 8N1 polled operation.
pub fn init_pl011() {
    if !is_ready() {
        unsafe { seL4_DebugPutChar(b'!' as _) };
        return;
    }

    unsafe {
        if !write_reg(CR, 0) {
            return;
        }
        if !write_reg(ICR, 0x7ff) {
            return;
        }
        if !write_reg(IBRD, 13) {
            return;
        }
        if !write_reg(FBRD, 2) {
            return;
        }
        if !write_reg(LCRH, (3 << 5) | (0 << 4)) {
            return;
        }
        let _ = write_reg(CR, CR_UARTEN | CR_TXE | CR_RXE);
    }
}

fn putc(byte: u8) {
    if !wait_tx_ready() {
        return;
    }
    unsafe {
        let _ = write_reg(DR, byte as u32);
    }
}

fn getc_blocking() -> u8 {
    if !wait_rx_ready() {
        return 0;
    }
    unsafe { read_reg(DR).unwrap_or(0) as u8 }
}

/// Write a single byte to the PL011 UART.
pub fn write_byte(byte: u8) {
    putc(byte);
}

/// Poll for a pending byte without blocking.
pub fn poll_byte() -> Option<u8> {
    unsafe {
        match read_reg(FR) {
            Some(flags) if flags & FR_RXFE == 0 => read_reg(DR).map(|value| value as u8),
            _ => None,
        }
    }
}

fn puts(line: &str) {
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

fn read_line_blocking(buffer: &mut [u8]) -> usize {
    let mut written = 0usize;
    while written + 1 < buffer.len() {
        let byte = getc_blocking();
        match byte {
            b'\r' => {
                putc(b'\r');
                putc(b'\n');
                break;
            }
            b'\n' => {
                putc(b'\r');
                putc(b'\n');
                break;
            }
            0x08 | 0x7f => {
                if written > 0 {
                    written -= 1;
                    putc(0x08);
                    putc(b' ');
                    putc(0x08);
                }
            }
            _ => {
                putc(byte);
                buffer[written] = byte;
                written += 1;
            }
        }
    }
    buffer[written] = 0;
    written
}

/// Maps the PL011 UART MMIO page into the root VSpace with uncached attributes.
pub fn map_pl011_smallpage(
    dev_ut: seL4_CPtr,
    page_slot: seL4_Word,
    cnode: &RetypeTuple,
    vspace: seL4_CPtr,
) -> seL4_Error {
    let retype = unsafe {
        sel4_sys::seL4_Untyped_Retype(
            dev_ut,
            seL4_ARM_SmallPageObject as seL4_Word,
            12,
            cnode.node_root,
            cnode.node_index,
            u64::from(cnode.node_depth),
            page_slot as seL4_CPtr,
            1,
        )
    };
    log::info!(
        "[pl011] retype -> slot=0x{slot:04x} err={err}",
        slot = page_slot,
        err = retype,
    );
    if retype != sel4_sys::seL4_NoError {
        return retype;
    }

    let map_err = unsafe {
        let pl011_vaddr = sel4_sys::seL4_Word::try_from(PL011_VADDR)
            .expect("PL011 virtual address must fit in seL4_Word");
        seL4_ARM_Page_Map(
            page_slot as seL4_CPtr,
            vspace,
            pl011_vaddr,
            seL4_CapRights_ReadWrite,
            seL4_ARM_Page_Uncached,
        )
    };
    log::info!("[pl011] map  -> err={err}", err = map_err);
    map_err
}

/// Simple console loop servicing the bootstrap REPL.
pub fn console_main() -> ! {
    init_pl011();
    puts("console ready\n");
    let mut buffer = [0u8; 128];
    loop {
        puts("cohesix> ");
        let len = read_line_blocking(&mut buffer);
        if len == 0 {
            continue;
        }
        let line = core::str::from_utf8(&buffer[..len]).unwrap_or("").trim();
        match line {
            "help" => {
                puts("Commands: help, reboot\n");
            }
            "reboot" => {
                puts("(stub) reboot not implemented\n");
            }
            other if other.is_empty() => {}
            other => {
                puts("unknown command: ");
                puts(other);
                puts("\n");
            }
        }
    }
}

/// Returns the physical address targeted by the PL011 map helper.
#[must_use]
pub const fn pl011_paddr() -> u64 {
    PL011_PADDR
}

/// Emits a heartbeat byte to the seL4 debug console for diagnostics.
pub fn heartbeat(byte: u8) {
    unsafe {
        seL4_DebugPutChar(byte);
    }
}
