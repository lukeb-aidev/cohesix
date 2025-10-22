// Author: Lukas Bower
//! Minimal PL011 UART driver for seL4 targets.

#![cfg(feature = "kernel")]
#![allow(unsafe_code)]

use core::ptr::{addr_of_mut, read_volatile, write_volatile, NonNull};

use embedded_io::Io;
use nb::Error as NbError;

use super::{SerialDriver, SerialError};

/// Offset (in bytes) to the data register within the PL011 MMIO window.
pub const DR_OFFSET: usize = 0x00;
/// Offset (in bytes) to the flag register within the PL011 MMIO window.
pub const FR_OFFSET: usize = 0x18;
/// Offset (in bytes) to the integer baud rate divisor register.
pub const IBRD_OFFSET: usize = 0x24;
/// Offset (in bytes) to the fractional baud rate divisor register.
pub const FBRD_OFFSET: usize = 0x28;
/// Offset (in bytes) to the line control register.
pub const LCRH_OFFSET: usize = 0x2C;
/// Offset (in bytes) to the control register.
pub const CR_OFFSET: usize = 0x30;
/// Offset (in bytes) to the interrupt mask set/clear register.
pub const IMSC_OFFSET: usize = 0x38;
/// Offset (in bytes) to the interrupt clear register.
pub const ICR_OFFSET: usize = 0x44;

const FR_TXFF: u32 = 1 << 5;
const FR_RXFE: u32 = 1 << 4;
const FR_BUSY: u32 = 1 << 3;
const CR_UARTEN: u32 = 1 << 0;
const CR_TXE: u32 = 1 << 8;
const CR_RXE: u32 = 1 << 9;
const LCRH_FEN: u32 = 1 << 4;
const LCRH_WLEN_8: u32 = 0b11 << 5;

#[repr(C)]
struct Pl011Regs {
    dr: u32,
    _rsrv04: [u32; 5],
    fr: u32,
    _rsrv1c: [u32; 1],
    ibrd: u32,
    fbrd: u32,
    lcrh: u32,
    cr: u32,
    ifls: u32,
    imsc: u32,
    _rsrv3c: [u32; 1],
    icr: u32,
}

/// MMIO-backed PL011 serial driver.
pub struct Pl011 {
    base: NonNull<Pl011Regs>,
}

impl Pl011 {
    /// Create a driver from the provided MMIO base pointer.
    #[must_use]
    pub fn new(base: NonNull<u8>) -> Self {
        Self { base: base.cast() }
    }

    #[inline(always)]
    fn regs(&self) -> &Pl011Regs {
        unsafe { self.base.as_ref() }
    }

    #[inline(always)]
    fn regs_mut(&mut self) -> &mut Pl011Regs {
        unsafe { self.base.as_mut() }
    }

    #[inline(always)]
    fn regs_ptr(&self) -> *mut Pl011Regs {
        self.base.as_ptr()
    }

    /// Returns the virtual address backing the UART registers.
    #[must_use]
    pub fn vaddr(&self) -> usize {
        self.base.as_ptr() as usize
    }

    /// Initialise the UART with a basic 8N1 configuration and enable FIFO.
    pub fn init(&mut self) {
        unsafe {
            let regs = self.regs_mut();
            write_volatile(addr_of_mut!(regs.cr), 0);
            write_volatile(addr_of_mut!(regs.icr), 0x7FF);
            write_volatile(addr_of_mut!(regs.ibrd), 1);
            write_volatile(addr_of_mut!(regs.fbrd), 40);
            write_volatile(addr_of_mut!(regs.lcrh), LCRH_WLEN_8 | LCRH_FEN);
            write_volatile(addr_of_mut!(regs.cr), CR_UARTEN | CR_TXE | CR_RXE);
        }
    }

    /// Emit a single byte, blocking until the FIFO can accept data.
    pub fn putc_blocking(&mut self, byte: u8) {
        unsafe {
            let regs = self.regs_ptr();
            while read_volatile(addr_of_mut!((*regs).fr)) & FR_TXFF != 0 {
                core::hint::spin_loop();
            }
            write_volatile(addr_of_mut!((*regs).dr), u32::from(byte));
        }
    }

    /// Flush pending characters until the UART is idle.
    pub fn flush(&mut self) {
        unsafe {
            let regs = self.regs_ptr();
            while read_volatile(addr_of_mut!((*regs).fr)) & FR_BUSY != 0 {
                core::hint::spin_loop();
            }
        }
    }

    /// Convenience helper to write a string, performing CRLF translation.
    pub fn write_str(&mut self, text: &str) {
        for byte in text.bytes() {
            if byte == b'\n' {
                self.putc_blocking(b'\r');
            }
            self.putc_blocking(byte);
        }
        self.flush();
    }

    #[inline(always)]
    fn reg_dr(&self) -> *mut u32 {
        unsafe { addr_of_mut!((*self.regs_ptr()).dr) }
    }

    #[inline(always)]
    fn reg_fr(&self) -> *mut u32 {
        unsafe { addr_of_mut!((*self.regs_ptr()).fr) }
    }
}

impl Io for Pl011 {
    type Error = SerialError;
}

impl SerialDriver for Pl011 {
    fn read_byte(&mut self) -> nb::Result<u8, Self::Error> {
        let fr = unsafe { read_volatile(self.reg_fr()) };
        if (fr & FR_RXFE) != 0 {
            return Err(NbError::WouldBlock);
        }
        let data = unsafe { read_volatile(self.reg_dr()) };
        Ok((data & 0xFF) as u8)
    }

    fn write_byte(&mut self, byte: u8) -> nb::Result<(), Self::Error> {
        let fr = unsafe { read_volatile(self.reg_fr()) };
        if (fr & FR_TXFF) != 0 {
            return Err(NbError::WouldBlock);
        }
        unsafe {
            write_volatile(self.reg_dr(), u32::from(byte));
        }
        Ok(())
    }
}
