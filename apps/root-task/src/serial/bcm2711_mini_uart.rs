// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: BCM2711 mini-UART serial driver for Raspberry Pi 4 kernel builds.
// Author: Lukas Bower
//! Minimal BCM2711 mini-UART driver for seL4 targets.

#![cfg(feature = "kernel")]
#![allow(unsafe_code)]

use core::ptr::{read_volatile, write_volatile, NonNull};

use embedded_io::ErrorType;
use nb::Error as NbError;
use sel4_sys::seL4_CPtr;

use super::{SerialDriver, SerialError};

/// Offset (in bytes) to the AUX enable register.
pub const AUX_ENABLES_OFFSET: usize = 0x04;
/// Offset (in bytes) to the mini-UART data register.
pub const MU_IO_OFFSET: usize = 0x40;
/// Offset (in bytes) to the mini-UART interrupt enable register.
pub const MU_IER_OFFSET: usize = 0x44;
/// Offset (in bytes) to the mini-UART interrupt identify register.
pub const MU_IIR_OFFSET: usize = 0x48;
/// Offset (in bytes) to the mini-UART line control register.
pub const MU_LCR_OFFSET: usize = 0x4c;
/// Offset (in bytes) to the mini-UART modem control register.
pub const MU_MCR_OFFSET: usize = 0x50;
/// Offset (in bytes) to the mini-UART line status register.
pub const MU_LSR_OFFSET: usize = 0x54;
/// Offset (in bytes) to the mini-UART control register.
pub const MU_CNTL_OFFSET: usize = 0x60;

const LSR_DATA_READY: u32 = 1 << 0;
const LSR_TX_EMPTY: u32 = 1 << 5;
const LSR_TX_IDLE: u32 = 1 << 6;
const TX_SPIN_LIMIT: usize = 1_000_000;

/// MMIO mapping metadata for the BCM2711 mini-UART.
#[derive(Clone, Copy, Debug)]
pub struct Bcm2711MiniUartMmio {
    paddr: usize,
    vaddr: NonNull<u8>,
    cap: Option<seL4_CPtr>,
}

impl Bcm2711MiniUartMmio {
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
            "mini-UART MMIO base must be page-aligned",
        );
        assert!(
            required_offset < page_size,
            "mini-UART offset {} exceeds mapped page size {}",
            required_offset,
            page_size
        );
        let limit = base
            .checked_add(page_size)
            .expect("mini-UART MMIO base overflowed while checking span");
        assert!(
            base + required_offset < limit,
            "mini-UART MMIO mapping does not cover required offset 0x{required_offset:x}"
        );
    }
}

/// MMIO-backed BCM2711 mini-UART serial driver.
pub struct Bcm2711MiniUart {
    base: NonNull<u8>,
    rx_cached: Option<u8>,
}

impl Bcm2711MiniUart {
    /// Create a driver from the provided MMIO base pointer.
    #[must_use]
    pub fn new(base: NonNull<u8>) -> Self {
        Self {
            base,
            rx_cached: None,
        }
    }

    /// Returns the virtual address backing the UART registers.
    #[must_use]
    pub fn vaddr(&self) -> usize {
        self.base.as_ptr() as usize
    }

    #[inline(always)]
    fn reg_ptr(&self, offset: usize) -> *mut u32 {
        unsafe { self.base.as_ptr().add(offset).cast::<u32>() }
    }

    #[inline(always)]
    fn read_reg(&self, offset: usize) -> u32 {
        unsafe { read_volatile(self.reg_ptr(offset)) }
    }

    #[inline(always)]
    fn write_reg(&self, offset: usize, value: u32) {
        unsafe {
            write_volatile(self.reg_ptr(offset), value);
        }
    }

    /// Initialise mini-UART with a conservative 8-bit, polled configuration.
    ///
    /// The baud-rate divisor is left untouched so firmware-selected clocking
    /// remains authoritative in UEFI bring-up environments.
    pub fn init(&mut self) {
        let enables = self.read_reg(AUX_ENABLES_OFFSET);
        self.write_reg(AUX_ENABLES_OFFSET, enables | 0x1);
        self.write_reg(MU_IER_OFFSET, 0);
        self.write_reg(MU_CNTL_OFFSET, 0);
        self.write_reg(MU_LCR_OFFSET, 0x3);
        self.write_reg(MU_MCR_OFFSET, 0);
        self.write_reg(MU_IIR_OFFSET, 0xC6);
        self.write_reg(MU_CNTL_OFFSET, 0x3);
    }

    /// Emit a single byte, blocking until the transmitter can accept data.
    pub fn putc_blocking(&mut self, byte: u8) {
        let mut spins = 0usize;
        while (self.read_reg(MU_LSR_OFFSET) & LSR_TX_EMPTY) == 0 {
            spins = spins.saturating_add(1);
            if spins >= TX_SPIN_LIMIT {
                return;
            }
            core::hint::spin_loop();
        }
        self.write_reg(MU_IO_OFFSET, u32::from(byte));
    }

    /// Convenience helper mirroring [`putc_blocking`] for API symmetry.
    pub fn putc(&mut self, byte: u8) {
        self.putc_blocking(byte);
    }

    /// Flush pending characters until the transmitter is idle.
    pub fn flush(&mut self) {
        let mut spins = 0usize;
        while (self.read_reg(MU_LSR_OFFSET) & LSR_TX_IDLE) == 0 {
            spins = spins.saturating_add(1);
            if spins >= TX_SPIN_LIMIT {
                return;
            }
            core::hint::spin_loop();
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
        if (self.read_reg(MU_LSR_OFFSET) & LSR_DATA_READY) == 0 {
            return None;
        }
        Some((self.read_reg(MU_IO_OFFSET) & 0xff) as u8)
    }
}

impl ErrorType for Bcm2711MiniUart {
    type Error = SerialError;
}

impl SerialDriver for Bcm2711MiniUart {
    fn read_byte(&mut self) -> nb::Result<u8, Self::Error> {
        if let Some(byte) = self.rx_cached.take() {
            return Ok(byte);
        }
        if (self.read_reg(MU_LSR_OFFSET) & LSR_DATA_READY) == 0 {
            return Err(NbError::WouldBlock);
        }
        Ok((self.read_reg(MU_IO_OFFSET) & 0xff) as u8)
    }

    fn write_byte(&mut self, byte: u8) -> nb::Result<(), Self::Error> {
        if (self.read_reg(MU_LSR_OFFSET) & LSR_TX_EMPTY) == 0 {
            return Err(NbError::WouldBlock);
        }
        self.write_reg(MU_IO_OFFSET, u32::from(byte));
        Ok(())
    }
}
