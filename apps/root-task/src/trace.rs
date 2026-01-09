// Author: Lukas Bower
//! Trace formatting and UART emission helpers shared across bootstrap diagnostics.

use core::cmp;
#[cfg(feature = "kernel")]
use core::fmt::Arguments;
use core::fmt::{self, Write};
#[cfg(feature = "kernel")]
use sel4_sys::{seL4_CPtr, seL4_Error};

#[cfg(all(feature = "bootstrap-trace", feature = "kernel"))]
pub mod bootstrap;

#[cfg(feature = "kernel")]
use crate::{
    sel4::{self, debug_put_char},
    serial,
};

pub use trace_model::TraceLevel;

pub(crate) trait RateLimitKey: Copy {
    const COUNT: usize;
    fn index(self) -> usize;
}

pub(crate) struct RateLimiter<const N: usize> {
    interval_ticks: u64,
    next_allowed: [u64; N],
    suppressed: [u32; N],
}

impl<const N: usize> RateLimiter<N> {
    pub const fn new(interval_ticks: u64) -> Self {
        Self {
            interval_ticks,
            next_allowed: [0; N],
            suppressed: [0; N],
        }
    }

    pub fn check<K: RateLimitKey>(&mut self, key: K, now_tick: u64) -> Option<u32> {
        debug_assert_eq!(K::COUNT, N, "rate limiter length mismatch");
        let idx = key.index();
        debug_assert!(idx < N, "rate limiter key out of range");
        if now_tick >= self.next_allowed[idx] {
            let suppressed = self.suppressed[idx];
            self.suppressed[idx] = 0;
            self.next_allowed[idx] = now_tick.saturating_add(self.interval_ticks);
            Some(suppressed)
        } else {
            self.suppressed[idx] = self.suppressed[idx].saturating_add(1);
            None
        }
    }
}

/// Trace sinks supported by the root task.
#[cfg(feature = "kernel")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceSink {
    /// Emit trace output directly to the UART debug console.
    Uart,
    /// Route trace output through the IPC endpoint when available.
    Ipc,
}

/// [`Write`] implementation that forwards characters to [`seL4_DebugPutChar`].
#[cfg(feature = "kernel")]
pub struct DebugPutc;

#[cfg(feature = "kernel")]
impl Write for DebugPutc {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for &byte in s.as_bytes() {
            debug_put_char(byte as i32);
        }
        Ok(())
    }
}

#[cfg(feature = "kernel")]
fn emit_line(args: Arguments<'_>) {
    let mut writer = DebugPutc;
    let _ = writer.write_fmt(args);
    let _ = writer.write_char('\n');
}

#[cfg(feature = "kernel")]
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

#[cfg(feature = "kernel")]
#[inline]
pub(crate) fn println_args(args: Arguments<'_>) {
    emit_with_sink(TraceSink::Uart, args);
}

/// Emit a trace line to the requested sink, falling back to UART if IPC is unavailable.
#[cfg(feature = "kernel")]
#[inline]
pub fn println_args_with_sink(sink: TraceSink, args: Arguments<'_>) {
    emit_with_sink(sink, args);
}

#[cfg(feature = "kernel")]
macro_rules! trace_println {
    ($($arg:tt)*) => {{
        $crate::trace::println_args(core::format_args!($($arg)*));
    }};
}

#[cfg(feature = "kernel")]
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

#[cfg(feature = "kernel")]
struct TagDisplay<'a>(&'a [u8]);

#[cfg(feature = "kernel")]
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
#[cfg(feature = "kernel")]
pub fn trace_ep(ep: seL4_CPtr) {
    let mut writer = DebugPutc;
    let width = core::mem::size_of::<seL4_CPtr>() * 2;
    if writeln!(writer, "[ep=0x{value:0width$x}]", value = ep, width = width).is_err() {
        // UART trace is best-effort.
    }
}

/// Emits a debug trace describing a bootstrap failure tagged with the provided label.
#[cfg(feature = "kernel")]
pub fn trace_fail(tag: &[u8], error: seL4_Error) {
    let mut writer = DebugPutc;
    if writeln!(writer, "[fail:{}] err={}", TagDisplay(tag), error as i32).is_err() {
        // UART trace is best-effort.
    }
}

#[cfg(feature = "bootstrap-trace")]
mod facade {
    use super::*;
    use core::fmt::Arguments;
    use core::sync::atomic::{AtomicU64, Ordering};
    use heapless::{Deque, String as HeaplessString, Vec as HeaplessVec};
    use spin::Mutex;
    use trace_model::{JsonLine, TraceEvent, TraceLevel, MESSAGE_CAPACITY};

    /// Number of events retained in the in-memory trace ring.
    pub const TRACE_RING_CAPACITY: usize = 128;

    #[derive(Debug)]
    struct TraceRing {
        events: Deque<TraceEvent, TRACE_RING_CAPACITY>,
    }

    impl TraceRing {
        const fn new() -> Self {
            Self {
                events: Deque::new(),
            }
        }

        fn push(&mut self, event: TraceEvent) {
            if self.events.is_full() {
                let _ = self.events.pop_front();
            }
            let _ = self.events.push_back(event);
        }
    }

    /// Global trace recorder retaining recent events for root-task diagnostics.
    pub struct Trace {
        sequence: AtomicU64,
        timestamp: AtomicU64,
        ring: Mutex<TraceRing>,
    }

    impl Trace {
        /// Construct a new trace recorder with an empty ring buffer.
        pub const fn new() -> Self {
            Self {
                sequence: AtomicU64::new(1),
                timestamp: AtomicU64::new(0),
                ring: Mutex::new(TraceRing::new()),
            }
        }

        /// Borrow the global trace recorder instance.
        #[must_use]
        pub fn global() -> &'static Self {
            &TRACE
        }

        fn next_sequence(&self) -> u64 {
            self.sequence.fetch_add(1, Ordering::Relaxed)
        }

        fn next_timestamp(&self) -> u64 {
            self.timestamp.fetch_add(1, Ordering::Relaxed)
        }

        /// Record an event constructed from the provided formatting arguments.
        #[cfg_attr(not(test), allow(dead_code))]
        pub fn record_args(
            &self,
            level: TraceLevel,
            category: &str,
            task: Option<&str>,
            args: Arguments<'_>,
        ) {
            let mut message = HeaplessString::<MESSAGE_CAPACITY>::new();
            let _ = message.write_fmt(args);
            let event = TraceEvent::new(
                self.next_sequence(),
                self.next_timestamp(),
                level,
                category,
                task,
                message.as_str(),
            );
            let mut guard = self.ring.lock();
            guard.push(event);
        }

        /// Snapshot the trace ring as JSONL lines for testing.
        pub fn snapshot_json(&self) -> HeaplessVec<JsonLine, TRACE_RING_CAPACITY> {
            let guard = self.ring.lock();
            let mut out = HeaplessVec::new();
            for event in guard.events.iter() {
                if let Ok(line) = event.to_json_line() {
                    let _ = out.push(line);
                }
            }
            out
        }
    }

    static TRACE: Trace = Trace::new();

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn record_with_args(
        level: TraceLevel,
        category: &str,
        task: Option<&str>,
        args: Arguments<'_>,
    ) {
        Trace::global().record_args(level, category, task, args);
    }

    pub(crate) fn snapshot_json() -> HeaplessVec<JsonLine, TRACE_RING_CAPACITY> {
        Trace::global().snapshot_json()
    }
}

#[cfg(feature = "bootstrap-trace")]
pub use facade::TRACE_RING_CAPACITY;

#[cfg(feature = "bootstrap-trace")]
pub use facade::Trace;

#[cfg(feature = "bootstrap-trace")]
/// Capture the current trace ring contents as JSON lines for assertions.
pub fn trace_snapshot_json() -> heapless::Vec<trace_model::JsonLine, TRACE_RING_CAPACITY> {
    facade::snapshot_json()
}

#[cfg(feature = "bootstrap-trace")]
#[doc(hidden)]
pub fn record_with_task(
    level: TraceLevel,
    category: &str,
    task: Option<&str>,
    args: core::fmt::Arguments<'_>,
) {
    facade::record_with_args(level, category, task, args);
}

#[cfg(not(feature = "bootstrap-trace"))]
#[doc(hidden)]
pub fn record_with_task(
    level: TraceLevel,
    _category: &str,
    _task: Option<&str>,
    _args: core::fmt::Arguments<'_>,
) {
    let _ = level;
}

/// Record a trace event emitted by the root task.
#[macro_export]
macro_rules! trace {
    ($level:expr, $category:expr, $task:expr, $($arg:tt)*) => {{
        $crate::trace::record_with_task($level, $category, $task, core::format_args!($($arg)*));
    }};
    ($level:expr, $category:expr, $($arg:tt)*) => {{
        $crate::trace::record_with_task($level, $category, Option::<&str>::None, core::format_args!($($arg)*));
    }};
}

#[cfg(test)]
mod tests {
    use super::{hex_dump_slice, RateLimitKey, RateLimiter};

    #[test]
    fn hex_dump_slice_signature_accepts_slice() {
        fn assert_signature(_func: fn(&str, &[u8], usize)) {}
        assert_signature(hex_dump_slice);
    }

    #[repr(u8)]
    #[derive(Clone, Copy, Debug)]
    enum TestKind {
        Alpha = 0,
        Beta = 1,
    }

    const TEST_KINDS: usize = 2;

    impl RateLimitKey for TestKind {
        const COUNT: usize = TEST_KINDS;

        fn index(self) -> usize {
            self as usize
        }
    }

    #[test]
    fn rate_limiter_is_deterministic_for_ticks() {
        let mut limiter = RateLimiter::<TEST_KINDS>::new(3);
        let ticks = [0u64, 1, 2, 3, 4, 6, 7];
        let mut outputs = [None; 7];
        for (idx, tick) in ticks.iter().enumerate() {
            outputs[idx] = limiter.check(TestKind::Alpha, *tick);
        }
        assert_eq!(
            outputs,
            [
                Some(0),
                None,
                None,
                Some(2),
                None,
                Some(1),
                None
            ]
        );
    }

    #[test]
    fn rate_limiter_is_fixed_size() {
        let expected = core::mem::size_of::<u64>() * (1 + TEST_KINDS)
            + core::mem::size_of::<u32>() * TEST_KINDS;
        assert_eq!(core::mem::size_of::<RateLimiter<TEST_KINDS>>(), expected);
    }
}
