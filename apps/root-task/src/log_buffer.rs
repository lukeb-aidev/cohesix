// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Bounded log ring backing /log/queen.log after the console handoff.
// Author: Lukas Bower

#![cfg(feature = "kernel")]

use core::sync::atomic::{AtomicBool, Ordering};

use heapless::{Deque, String as HeaplessString, Vec as HeaplessVec};
use spin::Mutex;

use crate::serial::DEFAULT_LINE_CAPACITY;

const LOG_RING_CAPACITY: usize = 128;
pub const LOG_SNAPSHOT_LINES: usize = 64;
const USER_RING_CAPACITY: usize = 16;
pub const LOG_USER_SNAPSHOT_LINES: usize = 16;

struct LogRing {
    lines: Deque<HeaplessString<DEFAULT_LINE_CAPACITY>, LOG_RING_CAPACITY>,
}

impl LogRing {
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

    fn append_bytes(&mut self, payload: &[u8]) {
        let Ok(text) = core::str::from_utf8(payload) else {
            return;
        };
        for line in text.lines() {
            self.push_line(line);
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

pub fn log_channel_active() -> bool {
    LOG_CHANNEL_ACTIVE.load(Ordering::Acquire)
}

pub fn enable_log_channel() -> bool {
    LOG_CHANNEL_ACTIVE
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
}

pub fn append_log_bytes(payload: &[u8]) {
    LOG_RING.lock().append_bytes(payload);
}

pub fn append_log_line(line: &str) {
    LOG_RING.lock().push_line(line);
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

pub fn snapshot_user_lines<const LINE: usize, const LIMIT: usize>(
) -> HeaplessVec<HeaplessString<LINE>, LIMIT> {
    USER_RING.lock().snapshot::<LINE, LIMIT>()
}

pub fn snapshot_user_lines_into<const LINE: usize, const LIMIT: usize>(
    output: &mut HeaplessVec<HeaplessString<LINE>, LIMIT>,
) {
    USER_RING.lock().snapshot_into(output);
}
