// Author: Lukas Bower
#![allow(unsafe_code)]

use core::cmp;
use core::fmt::{self, Write};
use sel4_sys::{seL4_CPtr, seL4_DebugPutChar, seL4_Error};

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

/// Emits a bounded hexadecimal dump of the provided buffer.
pub fn hex_dump(label: &str, buf: &[u8], max: usize) {
    let mut writer = DebugPutc;
    let limit = cmp::min(buf.len(), max);
    let _ = write!(writer, "[dump:{} len={}]\n", label, limit);

    let mut offset = 0usize;
    while offset < limit {
        let line_end = cmp::min(offset + 16, limit);
        let _ = write!(writer, "{:08x}: ", offset);
        for byte in &buf[offset..line_end] {
            let _ = write!(writer, "{:02x} ", byte);
        }
        let _ = writer.write_str("\n");
        offset = line_end;
    }
}

/// Emits a trace describing the endpoint capability slot in hexadecimal form.
pub fn trace_ep(ep: seL4_CPtr) {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    unsafe {
        seL4_DebugPutChar(b'[');
        seL4_DebugPutChar(b'e');
        seL4_DebugPutChar(b'p');
        seL4_DebugPutChar(b'=');
        let width = core::mem::size_of::<seL4_CPtr>() * 2;
        for nibble in (0..width).rev() {
            let shift = nibble * 4;
            let value = ((ep as usize) >> shift) & 0xF;
            seL4_DebugPutChar(HEX[value]);
        }
        seL4_DebugPutChar(b']');
        seL4_DebugPutChar(b'\n');
    }
}

/// Emits a debug trace describing a bootstrap failure tagged with the provided label.
pub fn trace_fail(tag: &[u8], error: seL4_Error) {
    unsafe {
        for &byte in b"[fail:" {
            seL4_DebugPutChar(byte);
        }
        for &byte in tag {
            seL4_DebugPutChar(byte);
        }
        for &byte in b"] err=" {
            seL4_DebugPutChar(byte);
        }

        let mut code = error as i32;
        if code < 0 {
            seL4_DebugPutChar(b'-');
            code = -code;
        }

        let mut digits = [0u8; 10];
        let mut index = digits.len();
        let mut value = code as u32;
        if value == 0 {
            seL4_DebugPutChar(b'0');
        } else {
            while value > 0 {
                index -= 1;
                digits[index] = b'0' + (value % 10) as u8;
                value /= 10;
            }
            for &digit in &digits[index..] {
                seL4_DebugPutChar(digit);
            }
        }

        seL4_DebugPutChar(b'\n');
    }
}
