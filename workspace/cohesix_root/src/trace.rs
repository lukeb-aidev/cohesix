// CLASSIFICATION: COMMUNITY
// Filename: trace.rs v0.1
// Author: Lukas Bower
// Date Modified: 2029-01-15

#![allow(dead_code)]

use core::cmp;
use core::sync::atomic::{AtomicUsize, Ordering};

/// Maximum length of a trace label recorded during boot.
pub const TRACE_LABEL_LEN: usize = 32;
/// Maximum number of trace events captured during boot.
pub const TRACE_CAPACITY: usize = 32;

/// Single boot trace event.
#[derive(Copy, Clone)]
pub struct TraceEvent {
    ticks: u64,
    len: u8,
    label: [u8; TRACE_LABEL_LEN],
}

impl TraceEvent {
    /// Create an empty trace event.
    pub const fn empty() -> Self {
        Self {
            ticks: 0,
            len: 0,
            label: [0; TRACE_LABEL_LEN],
        }
    }

    /// Timestamp associated with the event.
    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    /// Event label as a UTF-8 string.
    pub fn label(&self) -> &str {
        let len = self.len as usize;
        match core::str::from_utf8(&self.label[..len]) {
            Ok(s) => s,
            Err(_) => "<invalid>",
        }
    }
}

static TRACE_INDEX: AtomicUsize = AtomicUsize::new(0);
static mut TRACE_EVENTS: [TraceEvent; TRACE_CAPACITY] = [TraceEvent::empty(); TRACE_CAPACITY];

/// Record a trace event.
pub fn record(label: &str, ticks: u64) {
    let idx = TRACE_INDEX.fetch_add(1, Ordering::AcqRel);
    if idx >= TRACE_CAPACITY {
        return;
    }
    let mut event = TraceEvent::empty();
    event.ticks = ticks;
    let bytes = label.as_bytes();
    let count = cmp::min(bytes.len(), TRACE_LABEL_LEN);
    let mut i = 0;
    while i < count {
        event.label[i] = bytes[i];
        i += 1;
    }
    event.len = count as u8;
    unsafe {
        TRACE_EVENTS[idx] = event;
    }
}

/// Return a snapshot slice of all recorded trace events.
pub fn events() -> &'static [TraceEvent] {
    let count = cmp::min(TRACE_INDEX.load(Ordering::Acquire), TRACE_CAPACITY);
    unsafe { &TRACE_EVENTS[..count] }
}

/// Reset the trace buffer. Used in unit tests.
#[cfg(test)]
pub fn reset() {
    TRACE_INDEX.store(0, Ordering::Release);
    unsafe {
        TRACE_EVENTS = [TraceEvent::empty(); TRACE_CAPACITY];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_events() {
        reset();
        record("boot", 1);
        record("ns", 2);
        let ev = events();
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0].label(), "boot");
        assert_eq!(ev[1].ticks(), 2);
    }
}
