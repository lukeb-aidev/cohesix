// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Shared tail polling policy and bounded line buffering helpers.
// Author: Lukas Bower
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Tail polling policy bounds for client-side consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TailPollPolicy {
    /// Default polling interval in milliseconds.
    pub poll_ms_default: u64,
    /// Minimum polling interval in milliseconds.
    pub poll_ms_min: u64,
    /// Maximum polling interval in milliseconds.
    pub poll_ms_max: u64,
}

impl TailPollPolicy {
    /// Clamp a polling interval to the configured min/max bounds.
    #[must_use]
    pub fn clamp(self, desired_ms: Option<u64>) -> u64 {
        let mut value = desired_ms.unwrap_or(self.poll_ms_default);
        if value < self.poll_ms_min {
            value = self.poll_ms_min;
        }
        if value > self.poll_ms_max {
            value = self.poll_ms_max;
        }
        value
    }
}

/// Polling state tracker used to throttle repeated tail operations.
#[derive(Debug, Clone)]
pub struct TailPoller {
    poll_ms: u64,
    last_poll_ms: Option<u64>,
}

impl TailPoller {
    /// Construct a poller using the supplied policy and desired interval.
    #[must_use]
    pub fn new(policy: TailPollPolicy, desired_ms: Option<u64>) -> Self {
        Self {
            poll_ms: policy.clamp(desired_ms),
            last_poll_ms: None,
        }
    }

    /// Return the effective polling interval in milliseconds.
    #[must_use]
    pub fn poll_ms(&self) -> u64 {
        self.poll_ms
    }

    /// Return true if a poll should be issued at the supplied timestamp.
    #[must_use]
    pub fn should_poll(&self, now_ms: u64) -> bool {
        match self.last_poll_ms {
            None => true,
            Some(last) => now_ms.saturating_sub(last) >= self.poll_ms,
        }
    }

    /// Return the remaining delay until the next poll is due.
    #[must_use]
    pub fn next_delay_ms(&self, now_ms: u64) -> u64 {
        match self.last_poll_ms {
            None => 0,
            Some(last) => {
                let elapsed = now_ms.saturating_sub(last);
                if elapsed >= self.poll_ms {
                    0
                } else {
                    self.poll_ms.saturating_sub(elapsed)
                }
            }
        }
    }

    /// Record that a poll has been issued at the supplied timestamp.
    pub fn mark_polled(&mut self, now_ms: u64) {
        self.last_poll_ms = Some(now_ms);
    }

    /// Reset the poller to an unpolled state.
    pub fn reset(&mut self) {
        self.last_poll_ms = None;
    }
}

/// Bounded line buffer used for Live Hive overlays and detail panels.
#[derive(Debug, Clone)]
pub struct BoundedLineBuffer {
    lines: VecDeque<String>,
    max_lines: usize,
    max_bytes: usize,
    line_cap: usize,
    total_bytes: usize,
}

impl BoundedLineBuffer {
    /// Create a new bounded line buffer.
    #[must_use]
    pub fn new(max_lines: usize, max_bytes: usize, line_cap: usize) -> Self {
        Self {
            lines: VecDeque::new(),
            max_lines,
            max_bytes,
            line_cap,
            total_bytes: 0,
        }
    }

    /// Return true when the buffer has no stored lines.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Return the number of lines stored in the buffer.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Return the total bytes stored in the buffer.
    #[must_use]
    pub fn total_bytes(&self) -> usize {
        self.total_bytes
    }

    /// Clear all buffered lines.
    pub fn clear(&mut self) {
        self.lines.clear();
        self.total_bytes = 0;
    }

    /// Push a line into the buffer, applying truncation and bounds.
    pub fn push_line(&mut self, line: &str) {
        if self.max_lines == 0 || self.max_bytes == 0 || self.line_cap == 0 {
            return;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return;
        }
        let line = truncate_to_boundary(trimmed, self.line_cap);
        let len = line.len();
        self.lines.push_back(line);
        self.total_bytes = self.total_bytes.saturating_add(len);
        self.trim();
    }

    /// Append multiple lines into the buffer.
    pub fn extend_lines<'a, I>(&mut self, lines: I)
    where
        I: IntoIterator<Item = &'a str>,
    {
        for line in lines {
            self.push_line(line);
        }
    }

    /// Return all buffered lines in order.
    #[must_use]
    pub fn snapshot(&self) -> Vec<String> {
        self.lines.iter().cloned().collect()
    }

    /// Return the last N lines from the buffer.
    #[must_use]
    pub fn tail(&self, max_lines: usize) -> Vec<String> {
        if max_lines == 0 || self.lines.is_empty() {
            return Vec::new();
        }
        let count = core::cmp::min(max_lines, self.lines.len());
        self.lines
            .iter()
            .skip(self.lines.len().saturating_sub(count))
            .cloned()
            .collect()
    }

    fn trim(&mut self) {
        while self.lines.len() > self.max_lines || self.total_bytes > self.max_bytes {
            if let Some(line) = self.lines.pop_front() {
                self.total_bytes = self.total_bytes.saturating_sub(line.len());
            } else {
                break;
            }
        }
    }
}

fn truncate_to_boundary(input: &str, max_bytes: usize) -> String {
    if input.len() <= max_bytes {
        return input.to_string();
    }
    let mut end = 0usize;
    for (idx, ch) in input.char_indices() {
        let next = idx + ch.len_utf8();
        if next > max_bytes {
            break;
        }
        end = next;
    }
    input[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_policy_clamps_values() {
        let policy = TailPollPolicy {
            poll_ms_default: 1000,
            poll_ms_min: 250,
            poll_ms_max: 10_000,
        };
        assert_eq!(policy.clamp(None), 1000);
        assert_eq!(policy.clamp(Some(100)), 250);
        assert_eq!(policy.clamp(Some(20_000)), 10_000);
    }

    #[test]
    fn bounded_buffer_trims_on_line_and_byte_limits() {
        let mut buffer = BoundedLineBuffer::new(3, 10, 5);
        buffer.push_line("alpha");
        buffer.push_line("bravo");
        buffer.push_line("charlie");
        buffer.push_line("delta");
        let snapshot = buffer.snapshot();
        assert!(snapshot.len() <= 3);
        assert!(buffer.total_bytes() <= 10);
        assert!(snapshot.last().unwrap().starts_with('d'));
    }
}
