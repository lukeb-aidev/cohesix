// Author: Lukas Bower
//! Minimal PL011 UART driver for seL4 targets.

#![cfg(feature = "kernel")]
#![allow(unsafe_code)]

use core::ptr::{addr_of_mut, read_volatile, write_volatile, NonNull};

use embedded_io::ErrorType;
use nb::Error as NbError;

use super::{SerialDriver, SerialError};
use sel4_sys::seL4_CPtr;

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

/// MMIO mapping metadata for the PL011 UART.
#[derive(Clone, Copy, Debug)]
pub struct Pl011Mmio {
    paddr: usize,
    vaddr: NonNull<u8>,
    cap: Option<seL4_CPtr>,
}

impl Pl011Mmio {
    /// Construct a mapping descriptor using the supplied physical address, capability, and base pointer.
    #[must_use]
    pub fn new(paddr: usize, cap: Option<seL4_CPtr>, vaddr: NonNull<u8>) -> Self {
        Self { paddr, vaddr, cap }
    }

    /// Construct a mapping descriptor from a required device-frame capability.
    #[must_use]
    pub fn mapped(paddr: usize, cap: seL4_CPtr, vaddr: NonNull<u8>) -> Self {
        Self::new(paddr, Some(cap), vaddr)
    }

    /// Physical address backing the mapping.
    #[must_use]
    pub fn paddr(&self) -> usize {
        self.paddr
    }

    /// Virtual address backing the mapping.
    #[must_use]
    pub fn vaddr(&self) -> NonNull<u8> {
        self.vaddr
    }

    /// Capability slot used to map the UART, if available.
    #[must_use]
    pub fn cap(&self) -> Option<seL4_CPtr> {
        self.cap
    }

    /// Whether the UART mapping is live.
    #[must_use]
    pub fn is_mapped(&self) -> bool {
        self.cap.is_some()
    }

    /// Validate alignment and span coverage for the UART mapping.
    pub fn assert_page_coverage(&self, page_size: usize, required_offset: usize) {
        let base = self.vaddr.as_ptr() as usize;
        assert_eq!(
            base & (page_size - 1),
            0,
            "PL011 MMIO base must be page-aligned",
        );
        assert!(
            required_offset < page_size,
            "PL011 offset {} exceeds mapped page size {}",
            required_offset,
            page_size
        );
        let limit = base
            .checked_add(page_size)
            .expect("PL011 MMIO base overflowed while checking span");
        assert!(
            base + required_offset < limit,
            "PL011 MMIO mapping does not cover required offset 0x{required_offset:x}"
        );
    }
}

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
    rx_cached: Option<u8>,
}

impl Pl011 {
    /// Create a driver from the provided MMIO base pointer.
    #[must_use]
    pub fn new(base: NonNull<u8>) -> Self {
        Self {
            base: base.cast(),
            rx_cached: None,
        }
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
            write_volatile(addr_of_mut!(regs.imsc), 0);
            write_volatile(addr_of_mut!(regs.ibrd), 13);
            write_volatile(addr_of_mut!(regs.fbrd), 2);
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

    /// Convenience helper mirroring [`putc_blocking`] for API symmetry.
    pub fn putc(&mut self, byte: u8) {
        self.putc_blocking(byte);
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

    /// Blocking read of a single byte from the RX FIFO.
    pub fn getc_blocking(&mut self) -> u8 {
        loop {
            if let Some(byte) = self.try_getc() {
                return byte;
            }
            core::hint::spin_loop();
        }
    }

    /// Non-blocking attempt to read a byte from the RX FIFO.
    pub fn try_getc(&mut self) -> Option<u8> {
        if let Some(byte) = self.rx_cached.take() {
            return Some(byte);
        }
        let fr = unsafe { read_volatile(self.reg_fr()) };
        if (fr & FR_RXFE) != 0 {
            None
        } else {
            let data = unsafe { read_volatile(self.reg_dr()) };
            Some((data & 0xFF) as u8)
        }
    }

    /// Read a console line into the provided buffer, normalising CRLF.
    pub fn read_line(&mut self, buf: &mut [u8]) -> usize {
        let mut len = 0usize;

        while len + 1 < buf.len() {
            let byte = self.getc_blocking();

            match byte {
                b'\r' => {
                    if let Some(next) = self.try_getc() {
                        if next != b'\n' {
                            self.rx_cached = Some(next);
                        }
                    }
                    self.write_str("\r\n");
                    break;
                }
                b'\n' => {
                    self.write_str("\r\n");
                    break;
                }
                0x08 | 0x7f => {
                    if len > 0 {
                        len -= 1;
                        self.write_str("\x08 \x08");
                    }
                }
                byte => {
                    buf[len] = byte;
                    len += 1;
                    self.putc_blocking(byte);
                }
            }
        }

        if len < buf.len() {
            buf[len] = 0;
        }

        len
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

impl ErrorType for Pl011 {
    type Error = SerialError;
}

impl SerialDriver for Pl011 {
    fn read_byte(&mut self) -> nb::Result<u8, Self::Error> {
        if let Some(byte) = self.rx_cached.take() {
            return Ok(byte);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn zeroed_regs() -> Pl011Regs {
        Pl011Regs {
            dr: 0,
            _rsrv04: [0; 5],
            fr: 0,
            _rsrv1c: [0; 1],
            ibrd: 0,
            fbrd: 0,
            lcrh: 0,
            cr: 0,
            ifls: 0,
            imsc: 0,
            _rsrv3c: [0; 1],
            icr: 0,
        }
    }

    #[test]
    fn read_line_preserves_following_byte_after_cr() {
        let mut regs = zeroed_regs();
        let base = NonNull::from(&mut regs).cast::<u8>();
        let mut uart = Pl011::new(base);

        uart.rx_cached = Some(b'\r');
        regs.fr = 0;
        regs.dr = b'c' as u32;

        let mut buf = [0u8; 8];
        let len = uart.read_line(&mut buf);

        assert_eq!(len, 0);
        assert_eq!(buf[0], 0);
        assert_eq!(uart.rx_cached, Some(b'c'));
        assert_eq!(uart.getc_blocking(), b'c');
    }
}
