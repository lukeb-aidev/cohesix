// Author: Lukas Bower

use core::cmp;
use core::fmt::{self, Arguments, Write};
use sel4_sys::{seL4_CPtr, seL4_Error};

use crate::sel4::debug_put_char;

/// [`Write`] implementation that forwards characters to [`seL4_DebugPutChar`].
pub struct DebugPutc;

impl Write for DebugPutc {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for &byte in s.as_bytes() {
            debug_put_char(byte as i32);
        }
        Ok(())
    }
}

#[inline]
pub(crate) fn println_args(args: Arguments<'_>) {
    let mut writer = DebugPutc;
    let _ = writer.write_fmt(args);
    let _ = writer.write_char('\n');
}

macro_rules! trace_println {
    ($($arg:tt)*) => {{
        $crate::trace::println_args(core::format_args!($($arg)*));
    }};
}

pub(crate) use trace_println as println;

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
pub fn hex_dump_slice(label: &str, buf: &[u8], max: usize) {
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

    debug_put_char(b'[' as i32);
    debug_put_char(b'e' as i32);
    debug_put_char(b'p' as i32);
    debug_put_char(b'=' as i32);
    let width = core::mem::size_of::<seL4_CPtr>() * 2;
    for nibble in (0..width).rev() {
        let shift = nibble * 4;
        let value = ((ep as usize) >> shift) & 0xF;
        debug_put_char(HEX[value] as i32);
    }
    debug_put_char(b']' as i32);
    debug_put_char(b'\n' as i32);
}

/// Emits a debug trace describing a bootstrap failure tagged with the provided label.
pub fn trace_fail(tag: &[u8], error: seL4_Error) {
    for &byte in b"[fail:" {
        debug_put_char(byte as i32);
    }
    for &byte in tag {
        debug_put_char(byte as i32);
    }
    for &byte in b"] err=" {
        debug_put_char(byte as i32);
    }

    let mut code = error as i32;
    if code < 0 {
        debug_put_char(b'-' as i32);
        code = -code;
    }

    let mut digits = [0u8; 10];
    let mut index = digits.len();
    let mut value = code as u32;
    if value == 0 {
        debug_put_char(b'0' as i32);
    } else {
        while value > 0 {
            index -= 1;
            digits[index] = b'0' + (value % 10) as u8;
            value /= 10;
        }
        for &digit in &digits[index..] {
            debug_put_char(digit as i32);
        }
    }

    debug_put_char(b'\n' as i32);
}

#[cfg(test)]
mod tests {
    use super::hex_dump_slice;

    #[test]
    fn hex_dump_slice_signature_accepts_slice() {
        fn assert_signature(_func: fn(&str, &[u8], usize)) {}
        assert_signature(hex_dump_slice);
    }
}
