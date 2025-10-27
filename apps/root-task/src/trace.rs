// Author: Lukas Bower
//! Trace formatting and UART emission helpers shared across bootstrap diagnostics.

use core::cmp;
use core::fmt::{self, Arguments, Write};
use sel4_sys::{seL4_CPtr, seL4_Error};

#[cfg(feature = "bootstrap-trace")]
pub mod bootstrap;

use crate::{
    sel4::{self, debug_put_char},
    serial,
};

/// Trace sinks supported by the root task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceSink {
    /// Emit trace output directly to the UART debug console.
    Uart,
    /// Route trace output through the IPC endpoint when available.
    Ipc,
}

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

fn emit_line(args: Arguments<'_>) {
    let mut writer = DebugPutc;
    let _ = writer.write_fmt(args);
    let _ = writer.write_char('\n');
}

fn emit_with_sink(sink: TraceSink, args: Arguments<'_>) {
    match sink {
        TraceSink::Uart => emit_line(args),
        TraceSink::Ipc => {
            if !sel4::ep_ready() {
                serial::puts_once("[trace] EP not ready; falling back to UART\n");
                emit_line(args);
            } else {
                // IPC logging path not yet implemented; prefer UART until
                // the dispatcher is wired.
                emit_line(args);
            }
        }
    }
}

#[inline]
pub(crate) fn println_args(args: Arguments<'_>) {
    emit_with_sink(TraceSink::Uart, args);
}

/// Emit a trace line to the requested sink, falling back to UART if IPC is unavailable.
#[inline]
pub fn println_args_with_sink(sink: TraceSink, args: Arguments<'_>) {
    emit_with_sink(sink, args);
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
    let _ = write!(writer, "{value:#018x}");
}

/// Writes the decimal representation of the provided [`u32`] without allocations.
#[inline]
pub fn dec_u32(mut writer: impl Write, value: u32) {
    let _ = write!(writer, "{value}");
}

struct HexChunk<'a>(&'a [u8]);

impl fmt::Display for HexChunk<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{:02x} ", byte)?;
        }
        Ok(())
    }
}

struct TagDisplay<'a>(&'a [u8]);

impl fmt::Display for TagDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for &byte in self.0 {
            f.write_char(char::from(byte))?;
        }
        Ok(())
    }
}

/// Emits a bounded hexadecimal dump of the provided buffer.
pub fn hex_dump_slice(label: &str, buf: &[u8], max: usize) {
    let limit = cmp::min(buf.len(), max);
    log::trace!("[dump] {label} len={limit}");
    for (index, chunk) in buf[..limit].chunks(16).enumerate() {
        let offset = index * 16;
        log::trace!("  {offset:04x}: {}", HexChunk(chunk));
    }
}

/// Emits a diagnostic dump of machine words using structured logging.
pub fn dump_words(label: &str, words: &[usize]) {
    log::trace!("{label} len={}", words.len());
    for (i, word) in words.iter().enumerate() {
        log::trace!("  {i:04}: {word:#018x}");
    }
}

/// Emits a trace describing the endpoint capability slot in hexadecimal form.
pub fn trace_ep(ep: seL4_CPtr) {
    let mut writer = DebugPutc;
    let width = core::mem::size_of::<seL4_CPtr>() * 2;
    if writeln!(writer, "[ep=0x{value:0width$x}]", value = ep, width = width).is_err() {
        // UART trace is best-effort.
    }
}

/// Emits a debug trace describing a bootstrap failure tagged with the provided label.
pub fn trace_fail(tag: &[u8], error: seL4_Error) {
    let mut writer = DebugPutc;
    if writeln!(writer, "[fail:{}] err={}", TagDisplay(tag), error as i32).is_err() {
        // UART trace is best-effort.
    }
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
