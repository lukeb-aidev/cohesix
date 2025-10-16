// Author: Lukas Bower
//! Minimal PL011 UART driver for seL4 targets.

#![cfg(feature = "kernel")]
#![allow(unsafe_code)]

use core::ptr::{read_volatile, write_volatile, NonNull};

use embedded_io::Io;
use nb::Error as NbError;

use super::{SerialDriver, SerialError};

const DR_OFFSET: usize = 0x00;
const FR_OFFSET: usize = 0x18;

const FR_TXFF: u32 = 1 << 5;
const FR_RXFE: u32 = 1 << 4;

/// MMIO-backed PL011 serial driver.
pub struct Pl011 {
    base: NonNull<u8>,
}

impl Pl011 {
    /// Create a driver from the provided MMIO base pointer.
    #[must_use]
    pub fn new(base: NonNull<u8>) -> Self {
        Self { base }
    }

    #[inline(always)]
    fn reg_dr(&self) -> *mut u32 {
        unsafe { self.base.as_ptr().cast::<u32>().add(DR_OFFSET / 4) }
    }

    #[inline(always)]
    fn reg_fr(&self) -> *mut u32 {
        unsafe { self.base.as_ptr().cast::<u32>().add(FR_OFFSET / 4) }
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
