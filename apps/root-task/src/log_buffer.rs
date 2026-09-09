// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Bounded log ring backing /log/queen.log after the console handoff.
// Author: Lukas Bower

#![cfg(feature = "kernel")]

use core::fmt::{self, Write};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use heapless::{Deque, String as HeaplessString, Vec as HeaplessVec};
use spin::Mutex;

use crate::serial::DEFAULT_LINE_CAPACITY;

pub const LOG_RETENTION_LINES: usize = 2048;
pub const LOG_SNAPSHOT_LINES: usize = 64;
pub const LOG_EXPORT_BATCH_LINES: usize = LOG_SNAPSHOT_LINES;
const USER_RING_CAPACITY: usize = 16;
pub const LOG_USER_SNAPSHOT_LINES: usize = 16;
const WORKER_RECORD_BYTES: usize = 1024;
const WORKER_FRAGMENT_BYTES: usize = 176;

#[derive(Clone)]
struct LogEntry {
    seq: u64,
    line: HeaplessString<DEFAULT_LINE_CAPACITY>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogCursor {
    next_seq: u64,
    end_seq: u64,
    bytes: u64,
}

impl LogCursor {
    pub fn is_exhausted(&self) -> bool {
        self.next_seq >= self.end_seq
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }
}

struct LogRing {
    lines: Deque<LogEntry, LOG_RETENTION_LINES>,
    next_seq: u64,
    evicted: u64,
}

impl LogRing {
    const fn new() -> Self {
        Self {
            lines: Deque::new(),
            next_seq: 0,
            evicted: 0,
        }
    }

    fn push_line(&mut self, line: &str) {
        if line.is_empty() {
            return;
        }
        let mut entry: HeaplessString<DEFAULT_LINE_CAPACITY> = HeaplessString::new();
        let _ = entry.push_str(line);
        if self.lines.is_full() {
            let _ = self.lines.pop_front();
            self.evicted = self.evicted.saturating_add(1);
        }
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        let _ = self.lines.push_back(LogEntry { seq, line: entry });
    }

    fn append_bytes(&mut self, payload: &[u8]) {
        let Ok(text) = core::str::from_utf8(payload) else {
            return;
        };
        for line in text.lines() {
            self.push_line(line);
        }
    }

    /// Keep every fragment of one bounded Worker proof adjacent in the ring.
    /// The first sequence identifies the record across overlapping LOG exports.
    fn append_worker_record(&mut self, record: &str) {
        let id = self.next_seq;
        let mut remaining = record;
        let mut part = 0u8;
        while !remaining.is_empty() {
            let mut end = remaining.len().min(WORKER_FRAGMENT_BYTES);
            while !remaining.is_char_boundary(end) {
                end -= 1;
            }
            let mut line: HeaplessString<DEFAULT_LINE_CAPACITY> = HeaplessString::new();
            // 176 payload bytes plus the maximum decimal u64/u8 envelope fits
            // the existing 256-byte log line. No global log bound is enlarged.
            let _ = write!(
                line,
                "WORKER_LOG id={} part={} last={} data={}",
                id,
                part,
                u8::from(end == remaining.len()),
                &remaining[..end]
            );
            self.push_line(line.as_str());
            remaining = &remaining[end..];
            part += 1;
        }
    }

    fn snapshot<const LINE: usize, const LIMIT: usize>(
        &self,
    ) -> HeaplessVec<HeaplessString<LINE>, LIMIT> {
        let mut out = HeaplessVec::new();
        self.snapshot_into(&mut out);
        out
    }

    fn snapshot_into<const LINE: usize, const LIMIT: usize>(
        &self,
        output: &mut HeaplessVec<HeaplessString<LINE>, LIMIT>,
    ) {
        output.clear();
        for line in self.lines.iter().rev() {
            if output.is_full() {
                break;
            }
            let mut entry: HeaplessString<LINE> = HeaplessString::new();
            let _ = entry.push_str(line.line.as_str());
            let _ = output.push(entry);
        }
        let slice = output.as_mut_slice();
        let mut head = 0usize;
        let mut tail = slice.len().saturating_sub(1);
        while head < tail {
            slice.swap(head, tail);
            head = head.saturating_add(1);
            tail = tail.saturating_sub(1);
        }
    }

    fn cursor(&self) -> LogCursor {
        let next_seq = self
            .lines
            .iter()
            .next()
            .map_or(self.next_seq, |entry| entry.seq);
        let bytes = self.lines.iter().map(|entry| entry.line.len() as u64).sum();
        LogCursor {
            next_seq,
            end_seq: self.next_seq,
            bytes,
        }
    }

    fn tail_cursor(&self, limit: usize) -> LogCursor {
        if limit == 0 {
            return LogCursor {
                next_seq: self.next_seq,
                end_seq: self.next_seq,
                bytes: 0,
            };
        }

        let mut selected = 0usize;
        let mut bytes = 0u64;
        let mut next_seq = self.next_seq;
        for entry in self.lines.iter().rev() {
            if selected >= limit {
                break;
            }
            next_seq = entry.seq;
            bytes = bytes.saturating_add(entry.line.len() as u64);
            selected = selected.saturating_add(1);
        }

        LogCursor {
            next_seq,
            end_seq: self.next_seq,
            bytes,
        }
    }

    fn read_cursor_into<const LINE: usize, const LIMIT: usize>(
        &self,
        cursor: &mut LogCursor,
        output: &mut HeaplessVec<HeaplessString<LINE>, LIMIT>,
    ) -> bool {
        output.clear();
        if cursor.is_exhausted() {
            return true;
        }

        if let Some(first) = self.lines.iter().next() {
            if cursor.next_seq < first.seq {
                cursor.next_seq = first.seq;
            }
        } else {
            cursor.next_seq = cursor.end_seq;
            return true;
        }

        let Some(first) = self.lines.iter().next() else {
            cursor.next_seq = cursor.end_seq;
            return true;
        };
        let mut offset = cursor.next_seq.saturating_sub(first.seq) as usize;
        let total = self.lines.len();
        let (head, tail) = self.lines.as_slices();
        while offset < total {
            if output.is_full() {
                break;
            }
            let line = if offset < head.len() {
                &head[offset]
            } else {
                &tail[offset.saturating_sub(head.len())]
            };
            if line.seq < cursor.next_seq {
                offset = offset.saturating_add(1);
                continue;
            }
            if line.seq >= cursor.end_seq {
                cursor.next_seq = cursor.end_seq;
                break;
            }
            let mut entry: HeaplessString<LINE> = HeaplessString::new();
            let _ = entry.push_str(line.line.as_str());
            let _ = output.push(entry);
            cursor.next_seq = line.seq.saturating_add(1);
            offset = offset.saturating_add(1);
        }

        if output.is_empty() && cursor.next_seq < cursor.end_seq {
            cursor.next_seq = self.next_seq.min(cursor.end_seq);
        }
        cursor.is_exhausted()
    }

    fn evicted(&self) -> u64 {
        self.evicted
    }

    #[cfg(test)]
    fn clear_for_test(&mut self) {
        self.lines.clear();
        self.next_seq = 0;
        self.evicted = 0;
    }
}

struct UserRing {
    lines: Deque<HeaplessString<DEFAULT_LINE_CAPACITY>, USER_RING_CAPACITY>,
}

impl UserRing {
    const fn new() -> Self {
        Self {
            lines: Deque::new(),
        }
    }

    fn push_line(&mut self, line: &str) {
        if line.is_empty() {
            return;
        }
        let mut entry: HeaplessString<DEFAULT_LINE_CAPACITY> = HeaplessString::new();
        let _ = entry.push_str(line);
        if self.lines.is_full() {
            let _ = self.lines.pop_front();
        }
        let _ = self.lines.push_back(entry);
    }

    fn snapshot<const LINE: usize, const LIMIT: usize>(
        &self,
    ) -> HeaplessVec<HeaplessString<LINE>, LIMIT> {
        let mut out = HeaplessVec::new();
        self.snapshot_into(&mut out);
        out
    }

    fn snapshot_into<const LINE: usize, const LIMIT: usize>(
        &self,
        output: &mut HeaplessVec<HeaplessString<LINE>, LIMIT>,
    ) {
        output.clear();
        for line in self.lines.iter().rev() {
            if output.is_full() {
                break;
            }
            let mut entry: HeaplessString<LINE> = HeaplessString::new();
            let _ = entry.push_str(line.as_str());
            let _ = output.push(entry);
        }
        let slice = output.as_mut_slice();
        let mut head = 0usize;
        let mut tail = slice.len().saturating_sub(1);
        while head < tail {
            slice.swap(head, tail);
            head = head.saturating_add(1);
            tail = tail.saturating_sub(1);
        }
    }
}

static LOG_RING: Mutex<LogRing> = Mutex::new(LogRing::new());
static USER_RING: Mutex<UserRing> = Mutex::new(UserRing::new());
static LOG_CHANNEL_ACTIVE: AtomicBool = AtomicBool::new(false);
static LOG_CONTENTION_DROPPED_WRITES: AtomicU64 = AtomicU64::new(0);

pub fn log_channel_active() -> bool {
    LOG_CHANNEL_ACTIVE.load(Ordering::Acquire)
}

pub fn enable_log_channel() -> bool {
    LOG_CHANNEL_ACTIVE
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
}

fn try_append_log_bytes_to(
    ring: &Mutex<LogRing>,
    dropped_writes: &AtomicU64,
    payload: &[u8],
) -> bool {
    let Some(mut ring) = ring.try_lock() else {
        dropped_writes.fetch_add(1, Ordering::Relaxed);
        return false;
    };
    ring.append_bytes(payload);
    true
}

fn try_append_log_line_to(ring: &Mutex<LogRing>, dropped_writes: &AtomicU64, line: &str) -> bool {
    let Some(mut ring) = ring.try_lock() else {
        dropped_writes.fetch_add(1, Ordering::Relaxed);
        return false;
    };
    ring.push_line(line);
    true
}

fn try_append_retained_log_line_to(ring: &Mutex<LogRing>, line: &str) -> bool {
    let Some(mut ring) = ring.try_lock() else {
        return false;
    };
    ring.push_line(line);
    true
}

pub fn append_log_bytes(payload: &[u8]) {
    let _ = try_append_log_bytes_to(&LOG_RING, &LOG_CONTENTION_DROPPED_WRITES, payload);
}

/// Retain a complete Worker observation without UART I/O or waiting on a lock.
/// Overflow is explicit; collectors must never interpret a truncated proof.
pub(crate) fn append_worker_record(args: fmt::Arguments<'_>) {
    let mut record: HeaplessString<WORKER_RECORD_BYTES> = HeaplessString::new();
    if record.write_fmt(args).is_err() || record.contains(['\r', '\n']) {
        append_log_line("WORKER_LOG_ERROR reason=invalid-record");
        return;
    }
    let Some(mut ring) = LOG_RING.try_lock() else {
        LOG_CONTENTION_DROPPED_WRITES.fetch_add(1, Ordering::Relaxed);
        return;
    };
    ring.append_worker_record(record.as_str());
}

/// Attempt one complete qlog line without waiting behind a preempted owner.
///
/// Mandatory callers retain their own bounded record until this returns true;
/// ordinary best-effort logging continues to use [`append_log_line`].
#[must_use]
pub(crate) fn try_append_retained_log_line(line: &str) -> bool {
    try_append_retained_log_line_to(&LOG_RING, line)
}

pub fn append_log_line(line: &str) {
    let _ = try_append_log_line_to(&LOG_RING, &LOG_CONTENTION_DROPPED_WRITES, line);
}

/// Number of complete diagnostic writes dropped instead of spinning behind a
/// preempted ring owner. This is monotonic for the root-task lifetime.
pub fn contention_dropped_writes() -> u64 {
    LOG_CONTENTION_DROPPED_WRITES.load(Ordering::Relaxed)
}

pub fn append_user_line(line: &str) {
    USER_RING.lock().push_line(line);
}

pub fn snapshot_lines<const LINE: usize, const LIMIT: usize>(
) -> HeaplessVec<HeaplessString<LINE>, LIMIT> {
    LOG_RING.lock().snapshot::<LINE, LIMIT>()
}

pub fn snapshot_lines_into<const LINE: usize, const LIMIT: usize>(
    output: &mut HeaplessVec<HeaplessString<LINE>, LIMIT>,
) {
    LOG_RING.lock().snapshot_into(output);
}

pub fn export_cursor() -> LogCursor {
    LOG_RING.lock().cursor()
}

pub fn tail_cursor(lines: usize) -> LogCursor {
    LOG_RING.lock().tail_cursor(lines)
}

pub fn read_cursor_lines_into<const LINE: usize, const LIMIT: usize>(
    cursor: &mut LogCursor,
    output: &mut HeaplessVec<HeaplessString<LINE>, LIMIT>,
) -> bool {
    LOG_RING.lock().read_cursor_into(cursor, output)
}

pub fn evicted_lines() -> u64 {
    LOG_RING.lock().evicted()
}

pub fn snapshot_user_lines<const LINE: usize, const LIMIT: usize>(
) -> HeaplessVec<HeaplessString<LINE>, LIMIT> {
    USER_RING.lock().snapshot::<LINE, LIMIT>()
}

pub fn snapshot_user_lines_into<const LINE: usize, const LIMIT: usize>(
    output: &mut HeaplessVec<HeaplessString<LINE>, LIMIT>,
) {
    USER_RING.lock().snapshot_into(output);
}

#[cfg(test)]
pub fn clear_for_test() {
    LOG_RING.lock().clear_for_test();
    USER_RING.lock().lines.clear();
    LOG_CHANNEL_ACTIVE.store(false, Ordering::Release);
    LOG_CONTENTION_DROPPED_WRITES.store(0, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::fmt::Write;

    static TEST_RING: Mutex<LogRing> = Mutex::new(LogRing::new());
    static TEST_DROPPED_WRITES: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn worker_fragments_preserve_utf8_and_record_identity() {
        let mut guard = TEST_RING.lock();
        guard.clear_for_test();
        let record = "é".repeat(200);
        guard.append_worker_record(&record);
        let lines: std::vec::Vec<_> = guard.lines.iter().map(|row| row.line.clone()).collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].starts_with("WORKER_LOG id=0 part=0 last=0 data="));
        assert!(lines[2].starts_with("WORKER_LOG id=0 part=2 last=1 data="));
        let rebuilt: std::string::String = lines
            .iter()
            .map(|line| line.split_once(" data=").unwrap().1)
            .collect();
        assert_eq!(rebuilt, record);
        guard.append_worker_record("WORKER_TASK_READY role=worker-heartbeat");
        assert!(guard
            .lines
            .back()
            .unwrap()
            .line
            .starts_with("WORKER_LOG id=3 part=0 last=1 data="));
    }

    #[test]
    fn worker_fragment_envelope_fits_existing_line_bound_at_maximum_id() {
        let mut guard = TEST_RING.lock();
        guard.clear_for_test();
        guard.next_seq = u64::MAX - 6;
        guard.append_worker_record(&"x".repeat(WORKER_RECORD_BYTES));
        assert_eq!(guard.lines.len(), 6);
        assert!(guard
            .lines
            .iter()
            .all(|row| !row.line.is_empty() && row.line.len() <= DEFAULT_LINE_CAPACITY));
        assert!(guard.lines.back().unwrap().line.contains("part=5 last=1"));
    }

    #[test]
    fn contended_diagnostic_append_drops_without_waiting() {
        TEST_RING.lock().clear_for_test();
        TEST_DROPPED_WRITES.store(0, Ordering::Relaxed);
        let held = TEST_RING.lock();
        assert!(!try_append_log_line_to(
            &TEST_RING,
            &TEST_DROPPED_WRITES,
            "dropped"
        ));
        assert_eq!(TEST_DROPPED_WRITES.load(Ordering::Relaxed), 1);
        drop(held);

        assert!(try_append_log_line_to(
            &TEST_RING,
            &TEST_DROPPED_WRITES,
            "retained"
        ));
        assert_eq!(TEST_RING.lock().lines.len(), 1);
        assert_eq!(TEST_DROPPED_WRITES.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn retained_diagnostic_retry_does_not_count_a_drop() {
        TEST_RING.lock().clear_for_test();
        TEST_DROPPED_WRITES.store(0, Ordering::Relaxed);
        let held = TEST_RING.lock();
        assert!(!try_append_retained_log_line_to(&TEST_RING, "retry"));
        assert_eq!(TEST_DROPPED_WRITES.load(Ordering::Relaxed), 0);
        drop(held);
        assert!(try_append_retained_log_line_to(&TEST_RING, "retry"));
        assert_eq!(TEST_DROPPED_WRITES.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn cursor_reads_retained_lines_in_order_across_batches() {
        let mut ring_guard = TEST_RING.lock();
        let ring = &mut *ring_guard;
        ring.clear_for_test();
        for index in 0..LOG_RETENTION_LINES + 2 {
            let mut line: HeaplessString<DEFAULT_LINE_CAPACITY> = HeaplessString::new();
            write!(line, "line-{index:04}").unwrap();
            ring.push_line(line.as_str());
        }

        assert_eq!(ring.evicted(), 2);
        let mut cursor = ring.cursor();
        let mut seen = 0usize;
        loop {
            let mut batch: HeaplessVec<
                HeaplessString<DEFAULT_LINE_CAPACITY>,
                { LOG_EXPORT_BATCH_LINES },
            > = HeaplessVec::new();
            let exhausted = ring.read_cursor_into(&mut cursor, &mut batch);
            for line in batch.iter() {
                let mut expected: HeaplessString<DEFAULT_LINE_CAPACITY> = HeaplessString::new();
                write!(expected, "line-{:04}", seen + 2).unwrap();
                assert_eq!(line.as_str(), expected.as_str());
                seen = seen.saturating_add(1);
            }
            if exhausted {
                break;
            }
        }

        assert_eq!(seen, LOG_RETENTION_LINES);
        assert!(cursor.is_exhausted());
    }
}
