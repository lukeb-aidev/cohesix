// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Shared LoRa duty-cycle and tamper primitives for host/kernel use.
// Author: Lukas Bower

//! Shared LoRa duty-cycle enforcement and tamper logging primitives.

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::vec::Vec;

/// Duty-cycle configuration for LoRa scheduling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DutyCycleConfig {
    /// Duty-cycle limit as a percentage (1..=100).
    pub duty_cycle_percent: u8,
    /// Sliding window duration in milliseconds.
    pub window_ms: u64,
    /// Maximum allowed payload size in bytes.
    pub max_payload_bytes: u32,
}

/// Decision produced by the duty-cycle guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DutyCycleDecision {
    /// Transmission is allowed.
    Allowed,
    /// Transmission is throttled.
    Throttled,
}

/// Tamper log reasons for throttled transmissions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TamperReason {
    /// Payload exceeded the configured maximum.
    PayloadOversize,
    /// Duty-cycle budget exceeded.
    DutyCycleExceeded,
}

/// Tamper log entry captured on denial.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TamperEntry {
    /// Monotonic timestamp when the event was recorded.
    pub timestamp_ms: u64,
    /// Reason for denial.
    pub reason: TamperReason,
    /// Payload size that triggered the event.
    pub payload_bytes: u32,
}

/// Bounded tamper log with deterministic retention.
#[derive(Debug, Clone)]
pub struct TamperLog {
    entries: VecDeque<TamperEntry>,
    max_entries: usize,
}

impl TamperLog {
    /// Create a tamper log with the provided maximum entries.
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            max_entries,
        }
    }

    /// Record a tamper event, dropping the oldest entry if full.
    pub fn push(&mut self, entry: TamperEntry) {
        if self.max_entries == 0 {
            return;
        }
        if self.entries.len() >= self.max_entries {
            let _ = self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    /// Snapshot the current tamper entries in FIFO order.
    #[must_use]
    pub fn snapshot(&self) -> Vec<TamperEntry> {
        self.entries.iter().copied().collect()
    }
}

/// Duty-cycle guard enforcing per-window payload budgets.
#[derive(Debug, Clone)]
pub struct DutyCycleGuard {
    config: DutyCycleConfig,
    window_start_ms: u64,
    used_ms: u64,
}

impl DutyCycleGuard {
    /// Create a new guard instance.
    pub fn new(config: DutyCycleConfig) -> Self {
        Self {
            config,
            window_start_ms: 0,
            used_ms: 0,
        }
    }

    fn budget_ms(&self) -> u64 {
        let percent = u64::from(self.config.duty_cycle_percent);
        self.config.window_ms.saturating_mul(percent) / 100
    }

    /// Attempt to transmit a payload at the given time.
    pub fn attempt(&mut self, now_ms: u64, payload_bytes: u32) -> Result<(), TamperReason> {
        if payload_bytes > self.config.max_payload_bytes {
            return Err(TamperReason::PayloadOversize);
        }
        if now_ms.saturating_sub(self.window_start_ms) >= self.config.window_ms {
            self.window_start_ms = now_ms;
            self.used_ms = 0;
        }
        let budget = self.budget_ms();
        let cost = u64::from(payload_bytes);
        if cost == 0 || cost > budget {
            return Err(TamperReason::DutyCycleExceeded);
        }
        if self.used_ms.saturating_add(cost) > budget {
            return Err(TamperReason::DutyCycleExceeded);
        }
        self.used_ms = self.used_ms.saturating_add(cost);
        Ok(())
    }
}
