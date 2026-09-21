// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Bounded log ring backing /log/queen.log after the console handoff.
// Author: Lukas Bower

#![cfg(feature = "kernel")]

use core::fmt::{self, Write};
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};

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
// Only explicit trusted boot emitters can use these slots. The final slot is
// reserved for a sticky capture-failure receipt; ordinary logs retain 2048 lines.
const BOOT_AUDIT_RETENTION_LINES: usize = 64;
const BOOT_AUDIT_CAPTURE_LINES: usize = BOOT_AUDIT_RETENTION_LINES - 1;
const BOOT_AUDIT_CONTENTION: u8 = 1;
const BOOT_AUDIT_INVALID_RECORD: u8 = 2;
const BOOT_AUDIT_FULL: u8 = 4;
const BOOT_AUDIT_SEQUENCE_EXHAUSTED: u8 = 8;

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
    boot_audit: Deque<LogEntry, BOOT_AUDIT_RETENTION_LINES>,
    boot_audit_failure_recorded: bool,
    next_seq: u64,
    evicted: u64,
}

impl LogRing {
    const fn new() -> Self {
        Self {
            lines: Deque::new(),
            boot_audit: Deque::new(),
            boot_audit_failure_recorded: false,
            next_seq: 0,
            evicted: 0,
        }
    }

    fn push_line(&mut self, line: &str) {
        if line.is_empty() {
            return;
        }
        if line.len() > DEFAULT_LINE_CAPACITY && is_driver_proof_line(line) {
            if line.len() > WORKER_RECORD_BYTES || line.contains(['\r', '\n']) {
                self.push_line("DRIVER_LOG_ERROR reason=invalid-record");
            } else {
                self.append_fragmented_record(line, "DRIVER_LOG");
            }
            return;
        }
        let mut entry: HeaplessString<DEFAULT_LINE_CAPACITY> = HeaplessString::new();
        let _ = entry.push_str(line);
        if self.lines.is_full() {
            if let Some(discarded) = self.lines.pop_front() {
                self.evicted = self.evicted.saturating_add(1);
                // Keep a driver's retained observation all-or-nothing. At
                // most five further fragments share this boot-local id.
                if let Some((identity, _)) = discarded.line.split_once(" part=") {
                    if identity.starts_with("DRIVER_LOG id=") {
                        for _ in 0..5 {
                            let same_record = self.lines.front().is_some_and(|entry| {
                                entry
                                    .line
                                    .split_once(" part=")
                                    .is_some_and(|(next, _)| next == identity)
                            });
                            if !same_record {
                                break;
                            }
                            let _ = self.lines.pop_front();
                            self.evicted = self.evicted.saturating_add(1);
                        }
                    }
                }
            }
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

    fn push_boot_audit_line(&mut self, line: &str, failure_receipt: bool) -> Result<(), u8> {
        if line.is_empty() || line.len() > DEFAULT_LINE_CAPACITY || line.contains(['\r', '\n']) {
            return Err(BOOT_AUDIT_INVALID_RECORD);
        }
        let limit = if failure_receipt {
            BOOT_AUDIT_RETENTION_LINES
        } else {
            BOOT_AUDIT_CAPTURE_LINES
        };
        if self.boot_audit.len() >= limit {
            return Err(BOOT_AUDIT_FULL);
        }
        if self.next_seq == u64::MAX {
            return Err(BOOT_AUDIT_SEQUENCE_EXHAUSTED);
        }
        let mut retained = HeaplessString::new();
        retained
            .push_str(line)
            .map_err(|_| BOOT_AUDIT_INVALID_RECORD)?;
        self.boot_audit
            .push_back(LogEntry {
                seq: self.next_seq,
                line: retained,
            })
            .map_err(|_| BOOT_AUDIT_FULL)?;
        // The caller owns this same ring lock for both insertions. No normal
        // writer can interleave: both immutable copies receive the same seq.
        self.push_line(line);
        Ok(())
    }

    fn record_boot_audit_failure(&mut self, failures: u8) {
        if failures == 0 || self.boot_audit_failure_recorded {
            return;
        }
        let mut line = HeaplessString::<160>::new();
        let formatted = write!(
            line,
            "[diag boot-audit/v1] state=failed reason=capture-unavailable first_observed_failures=0x{failures:x} recorded_at=log-export",
        );
        if formatted.is_ok() && self.push_boot_audit_line(line.as_str(), true).is_ok() {
            self.boot_audit_failure_recorded = true;
        }
    }

    fn export_entries(&self) -> impl DoubleEndedIterator<Item = &LogEntry> {
        let ordinary_first = self.lines.front().map_or(self.next_seq, |entry| entry.seq);
        self.boot_audit
            .iter()
            .filter(move |entry| entry.seq < ordinary_first)
            .chain(self.lines.iter())
    }

    /// Keep every fragment of one bounded Worker proof adjacent in the ring.
    /// The first sequence identifies the record across overlapping LOG exports.
    fn append_worker_record(&mut self, record: &str) {
        self.append_fragmented_record(record, "WORKER_LOG");
    }

    fn append_fragmented_record(&mut self, record: &str, envelope: &str) {
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
                "{} id={} part={} last={} data={}",
                envelope,
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
            .export_entries()
            .next()
            .map_or(self.next_seq, |entry| entry.seq);
        let bytes = self
            .export_entries()
            .map(|entry| entry.line.len() as u64)
            .sum();
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
        for entry in self.export_entries().rev() {
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

        // Recompute only the fixed audit prefix on each batch. If ordinary
        // records evict between reads, their saved audit copies become visible
        // at the same original sequence; already-read entries cannot duplicate.
        let ordinary_first = self.lines.front().map_or(self.next_seq, |entry| entry.seq);
        for line in &self.boot_audit {
            if line.seq < cursor.next_seq || line.seq >= ordinary_first {
                continue;
            }
            if line.seq >= cursor.end_seq {
                cursor.next_seq = cursor.end_seq;
                return true;
            }
            if output.is_full() {
                return false;
            }
            let mut entry: HeaplessString<LINE> = HeaplessString::new();
            let _ = entry.push_str(line.line.as_str());
            let _ = output.push(entry);
            cursor.next_seq = line.seq.saturating_add(1);
        }
        if output.is_full() {
            return cursor.is_exhausted();
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
        self.boot_audit.clear();
        self.boot_audit_failure_recorded = false;
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
static BOOT_AUDIT_FAILURES: AtomicU8 = AtomicU8::new(0);

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

fn try_append_boot_audit_line_to(ring: &Mutex<LogRing>, failures: &AtomicU8, line: &str) -> bool {
    let Some(mut ring) = ring.try_lock() else {
        failures.fetch_or(BOOT_AUDIT_CONTENTION, Ordering::Release);
        return false;
    };
    if let Err(reason) = ring.push_boot_audit_line(line, false) {
        failures.fetch_or(reason, Ordering::Release);
        return false;
    }
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

/// Identify existing internal driver proof records, without adding authority.
pub(crate) fn is_driver_proof_line(line: &str) -> bool {
    line.starts_with("DRIVER_TASK ")
        || line.starts_with("DRIVER_TASK_")
        || line.starts_with("SCHED_CONTRACT ")
}

/// Retain a complete internal driver proof in the existing bounded log ring.
/// Readers must validate the whole fragment set before using any proof field.
pub(crate) fn append_driver_record(record: &str) {
    if record.is_empty() || record.len() > WORKER_RECORD_BYTES || record.contains(['\r', '\n']) {
        append_log_line("DRIVER_LOG_ERROR reason=invalid-record");
        return;
    }
    let Some(mut ring) = LOG_RING.try_lock() else {
        LOG_CONTENTION_DROPPED_WRITES.fetch_add(1, Ordering::Relaxed);
        return;
    };
    ring.append_fragmented_record(record, "DRIVER_LOG");
}

/// Attempt one complete qlog line without waiting behind a preempted owner.
///
/// Mandatory callers retain their own bounded record until this returns true;
/// ordinary best-effort logging continues to use [`append_log_line`].
#[must_use]
pub(crate) fn try_append_retained_log_line(line: &str) -> bool {
    try_append_retained_log_line_to(&LOG_RING, line)
}

/// Retain one trusted bootstrap observation through ordinary log eviction.
/// This path is never selected by a user-controlled message prefix. Success
/// acknowledges both ordinary and private copies under the same nonblocking
/// lock. Failed captures latch a failure receipt for the next log export.
#[must_use]
pub(crate) fn try_append_boot_audit_line(line: &str) -> bool {
    try_append_boot_audit_line_to(&LOG_RING, &BOOT_AUDIT_FAILURES, line)
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
    let mut ring = LOG_RING.lock();
    ring.record_boot_audit_failure(BOOT_AUDIT_FAILURES.load(Ordering::Acquire));
    ring.cursor()
}

pub fn tail_cursor(lines: usize) -> LogCursor {
    let mut ring = LOG_RING.lock();
    ring.record_boot_audit_failure(BOOT_AUDIT_FAILURES.load(Ordering::Acquire));
    ring.tail_cursor(lines)
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
    BOOT_AUDIT_FAILURES.store(0, Ordering::Release);
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
    fn driver_fragments_preserve_a_complete_long_observation() {
        let mut guard = TEST_RING.lock();
        guard.clear_for_test();
        let record = format!(
            "DRIVER_TASK_DMA_PROOF contract=serial detail={}",
            "é".repeat(300)
        );
        guard.push_line(&record);
        let lines: std::vec::Vec<_> = guard.lines.iter().map(|row| row.line.clone()).collect();
        assert_eq!(lines.len(), 4);
        assert!(lines[0].starts_with("DRIVER_LOG id=0 part=0 last=0 data="));
        assert!(lines[3].starts_with("DRIVER_LOG id=0 part=3 last=1 data="));
        let rebuilt: std::string::String = lines
            .iter()
            .map(|line| line.split_once(" data=").unwrap().1)
            .collect();
        assert_eq!(rebuilt, record);
    }

    #[test]
    fn ordinary_eviction_retires_the_entire_driver_observation() {
        let mut guard = TEST_RING.lock();
        guard.clear_for_test();
        guard.push_line(&format!("DRIVER_TASK_BOOT {}", "x".repeat(300)));
        assert_eq!(guard.lines.len(), 2);
        for _ in 0..2046 {
            guard.push_line("ordinary");
        }
        guard.push_line("newest");
        assert_eq!(guard.evicted(), 2);
        assert_eq!(guard.lines.len(), 2047);
        assert!(guard
            .lines
            .iter()
            .all(|entry| !entry.line.starts_with("DRIVER_LOG ")));
        assert_eq!(guard.lines.back().unwrap().line.as_str(), "newest");
    }

    #[test]
    fn oversized_driver_record_keeps_an_explicit_failure_instead_of_partial_proof() {
        let mut guard = TEST_RING.lock();
        guard.clear_for_test();
        guard.push_line(&format!("DRIVER_TASK_BOOT {}", "x".repeat(1024)));
        assert_eq!(guard.lines.len(), 1);
        assert_eq!(
            guard.lines.front().unwrap().line.as_str(),
            "DRIVER_LOG_ERROR reason=invalid-record"
        );
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

    fn collect_cursor(ring: &LogRing, mut cursor: LogCursor) -> std::vec::Vec<std::string::String> {
        let mut lines = std::vec::Vec::new();
        loop {
            let mut batch: HeaplessVec<HeaplessString<256>, 7> = HeaplessVec::new();
            let exhausted = ring.read_cursor_into(&mut cursor, &mut batch);
            lines.extend(batch.iter().map(|line| line.as_str().to_owned()));
            if exhausted {
                return lines;
            }
        }
    }

    #[test]
    fn trusted_boot_records_keep_original_sequence_and_deduplicate_surviving_copies() {
        let mut ring = TEST_RING.lock();
        ring.clear_for_test();
        ring.push_line("ordinary");
        ring.push_boot_audit_line("audit-a", false).unwrap();
        ring.push_line("gap");
        ring.push_boot_audit_line("audit-b", false).unwrap();
        assert_eq!(ring.boot_audit.front().unwrap().seq, 1);
        assert_eq!(ring.boot_audit.back().unwrap().seq, 3);
        assert_eq!(ring.lines.back().unwrap().seq, 3);
        assert_eq!(
            collect_cursor(&ring, ring.cursor()),
            ["ordinary", "audit-a", "gap", "audit-b"]
        );
        assert_eq!(ring.cursor().bytes(), 25);
        for _ in 0..2048 {
            ring.push_line("noise");
        }
        let exported = collect_cursor(&ring, ring.cursor());
        assert_eq!(exported.len(), 2050);
        assert_eq!(&exported[..2], ["audit-a", "audit-b"]);
        assert!(exported[2..].iter().all(|line| line == "noise"));
        assert_eq!(ring.cursor().bytes(), 14 + 2048 * 5);
        assert_eq!(ring.lines.len(), 2048);
        assert_eq!(ring.boot_audit.len(), 2);
    }

    #[test]
    fn boot_audit_export_handles_eviction_between_batches_and_frozen_cutoff() {
        let mut ring = TEST_RING.lock();
        ring.clear_for_test();
        ring.push_boot_audit_line("audit-a", false).unwrap();
        ring.push_line("gap");
        ring.push_boot_audit_line("audit-b", false).unwrap();
        ring.push_line("tail");
        let mut cursor = ring.cursor();
        assert_eq!(cursor.bytes(), 21);
        let mut first: HeaplessVec<HeaplessString<256>, 1> = HeaplessVec::new();
        assert!(!ring.read_cursor_into(&mut cursor, &mut first));
        assert_eq!(first[0], "audit-a");
        for _ in 0..2048 {
            ring.push_line("later-ordinary");
        }
        ring.push_boot_audit_line("later-audit", false).unwrap();
        assert_eq!(collect_cursor(&ring, cursor), ["audit-b"]);
        // Already emitted audit-a cannot repeat, audit-b survives eviction,
        // and both late append classes are outside the original cutoff.
        assert_eq!(cursor.bytes(), 21);
    }

    #[test]
    fn boot_audit_batches_cross_saved_prefix_into_ordinary_ring_once() {
        let mut ring = TEST_RING.lock();
        ring.clear_for_test();
        ring.push_boot_audit_line("old-audit", false).unwrap();
        for _ in 0..2048 {
            ring.push_line("noise");
        }
        ring.push_boot_audit_line("live-audit", false).unwrap();
        let mut cursor = ring.cursor();
        let mut batch: HeaplessVec<HeaplessString<256>, 2> = HeaplessVec::new();
        assert!(!ring.read_cursor_into(&mut cursor, &mut batch));
        assert_eq!(batch[0], "old-audit");
        assert_eq!(batch[1], "noise");
        let rest = collect_cursor(&ring, cursor);
        assert_eq!(rest.len(), 2047);
        assert_eq!(rest.last().unwrap(), "live-audit");
        assert!(rest[..2046].iter().all(|line| line == "noise"));
    }

    #[test]
    fn boot_audit_tail_keeps_exact_latest_n_and_deduplicated_byte_count() {
        let mut ring = TEST_RING.lock();
        ring.clear_for_test();
        ring.push_boot_audit_line("old", false).unwrap();
        for _ in 0..2048 {
            ring.push_line("noise");
        }
        ring.push_line("last");
        assert!(collect_cursor(&ring, ring.tail_cursor(0)).is_empty());
        assert_eq!(collect_cursor(&ring, ring.tail_cursor(1)), ["last"]);
        let tail = collect_cursor(&ring, ring.tail_cursor(256));
        assert_eq!(tail.len(), 256);
        assert!(tail[..255].iter().all(|line| line == "noise"));
        assert_eq!(tail[255], "last");
        assert_eq!(ring.tail_cursor(256).bytes(), 255 * 5 + 4);
        let full = collect_cursor(&ring, ring.tail_cursor(2049));
        assert_eq!(full.len(), 2049);
        assert_eq!(full[0], "old");
        assert_eq!(full[2048], "last");
        assert_eq!(ring.tail_cursor(2049).bytes(), 3 + 2047 * 5 + 4);
    }

    #[test]
    fn boot_audit_capture_limits_reserve_one_explicit_late_failure_record() {
        let mut ring = TEST_RING.lock();
        ring.clear_for_test();
        for line in [
            "".to_owned(),
            "x".repeat(257),
            "two\nlines".to_owned(),
            "two\rlines".to_owned(),
        ] {
            assert_eq!(
                ring.push_boot_audit_line(&line, false),
                Err(BOOT_AUDIT_INVALID_RECORD)
            );
            assert_eq!(ring.next_seq, 0);
            assert!(ring.lines.is_empty());
            assert!(ring.boot_audit.is_empty());
        }
        ring.push_boot_audit_line(&"x".repeat(256), false).unwrap();
        assert_eq!(ring.lines.back().unwrap().line.len(), 256);
        assert_eq!(ring.boot_audit.back().unwrap().line.len(), 256);
        for _ in 1..63 {
            ring.push_boot_audit_line("captured", false).unwrap();
        }
        assert_eq!(
            ring.push_boot_audit_line("overflow", false),
            Err(BOOT_AUDIT_FULL)
        );
        assert_eq!(ring.next_seq, 63);
        ring.record_boot_audit_failure(BOOT_AUDIT_FULL);
        assert_eq!(ring.boot_audit.len(), 64);
        assert_eq!(ring.next_seq, 64);
        assert_eq!(ring.boot_audit.back().unwrap().seq, 63);
        assert_eq!(
            ring.boot_audit.back().unwrap().line,
            "[diag boot-audit/v1] state=failed reason=capture-unavailable first_observed_failures=0x4 recorded_at=log-export"
        );
        let frozen = ring.cursor();
        assert_eq!(frozen.end_seq, 64);
        assert!(collect_cursor(&ring, frozen)
            .last()
            .unwrap()
            .contains("state=failed"));
        ring.record_boot_audit_failure(BOOT_AUDIT_CONTENTION);
        assert_eq!(ring.next_seq, 64);
        assert_eq!(ring.boot_audit.len(), 64);
        ring.push_line("[diag root-text/v1] forged user prefix");
        assert_eq!(ring.boot_audit.len(), 64);
    }

    #[test]
    fn boot_audit_contention_latches_failure_without_waiting_or_partial_append() {
        let mut held = TEST_RING.lock();
        held.clear_for_test();
        let failures = AtomicU8::new(0);
        assert!(!try_append_boot_audit_line_to(
            &TEST_RING, &failures, "measured"
        ));
        assert_eq!(failures.load(Ordering::Acquire), BOOT_AUDIT_CONTENTION);
        assert!(held.lines.is_empty());
        assert!(held.boot_audit.is_empty());
        assert_eq!(held.next_seq, 0);
        held.record_boot_audit_failure(failures.load(Ordering::Acquire));
        assert_eq!(held.boot_audit.len(), 1);
        assert_eq!(held.lines.len(), 1);
        assert!(held
            .boot_audit
            .front()
            .unwrap()
            .line
            .contains("first_observed_failures=0x1"));
        assert_eq!(failures.load(Ordering::Acquire), BOOT_AUDIT_CONTENTION);
    }

    #[test]
    fn boot_audit_sequence_exhaustion_rejects_capture_without_mutation() {
        let mut ring = TEST_RING.lock();
        ring.clear_for_test();
        ring.next_seq = u64::MAX;
        assert_eq!(
            ring.push_boot_audit_line("measured", false),
            Err(BOOT_AUDIT_SEQUENCE_EXHAUSTED)
        );
        assert!(ring.lines.is_empty());
        assert!(ring.boot_audit.is_empty());
        assert_eq!(ring.next_seq, u64::MAX);
    }
}
