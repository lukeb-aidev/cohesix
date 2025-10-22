// Author: Lukas Bower
#![allow(unsafe_code)]

use core::fmt::{self, Write};
use sel4_sys::seL4_DebugPutChar;

/// [`Write`] implementation that forwards characters to [`seL4_DebugPutChar`].
pub struct DebugPutc;

impl Write for DebugPutc {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for &byte in s.as_bytes() {
            unsafe {
                seL4_DebugPutChar(byte);
            }
        }
        Ok(())
    }
}

/// Formats the provided [`u64`] value as a fixed-width hexadecimal literal.
#[inline]
pub fn hex_u64(mut writer: impl Write, value: u64) {
    let _ = writer.write_str("0x");
    for index in (0..16).rev() {
        let nibble = ((value >> (index * 4)) & 0xF) as u8;
        let digit = if nibble < 10 {
            b'0' + nibble
        } else {
            b'a' + (nibble - 10)
        };
        let _ = writer.write_char(char::from(digit));
    }
}

/// Writes the decimal representation of the provided [`u32`] without allocations.
#[inline]
pub fn dec_u32(mut writer: impl Write, mut value: u32) {
    if value == 0 {
        let _ = writer.write_str("0");
        return;
    }

    let mut buffer = [0u8; 10];
    let mut position = buffer.len();
    while value > 0 {
        position -= 1;
        buffer[position] = b'0' + (value % 10) as u8;
        value /= 10;
    }

    for &digit in &buffer[position..] {
        let _ = writer.write_char(char::from(digit));
    }
}
