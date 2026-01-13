// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the console/io module for root-task.
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

    /// Blocking read of a single byte from the UART RX FIFO.
    #[must_use]
    pub fn read_byte_blocking(&mut self) -> u8 {
        self.uart.getc_blocking()
    }

    /// Read a line of input into `buf`, returning the number of bytes stored.
    ///
    /// Characters are echoed as they are received, CR/LF are normalised, and
    /// destructive backspace handling is performed when space remains in the
    /// buffer.
    pub fn read_line(&mut self, buf: &mut [u8]) -> usize {
        let mut len = 0usize;

        while len + 1 < buf.len() {
            let byte = self.read_byte_blocking();

            match byte {
                b'\r' | b'\n' => {
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
                    self.uart.putc(byte);
                }
            }
        }

        if len < buf.len() {
            buf[len] = 0;
        }

        len
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
