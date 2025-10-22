// Author: Lukas Bower

//! Blocking console adaptor backed by the PL011 UART.

use core::fmt::{Result as FmtResult, Write};

use crate::serial::pl011::Pl011;

/// Simple console wrapper that exposes blocking read/write helpers.
pub struct Console {
    uart: Pl011,
}

impl Console {
    /// Create a new console bound to the provided UART driver.
    #[must_use]
    pub fn new(uart: Pl011) -> Self {
        Self { uart }
    }

    /// Read a line of input into `buf`, returning the number of bytes stored.
    pub fn read_line(&mut self, buf: &mut [u8]) -> usize {
        self.uart.read_line(buf)
    }

    /// Flush any pending UART output.
    pub fn flush(&mut self) {
        self.uart.flush();
    }

    /// Emit a single byte to the console.
    pub fn putc(&mut self, byte: u8) {
        self.uart.putc(byte);
    }

    /// Recover the underlying UART driver.
    #[must_use]
    pub fn into_inner(self) -> Pl011 {
        self.uart
    }
}

impl Write for Console {
    fn write_str(&mut self, s: &str) -> FmtResult {
        self.uart.write_str(s);
        Ok(())
    }
}
