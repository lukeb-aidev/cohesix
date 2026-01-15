// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide host-side LoRa worker scheduling and tamper logging.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! LoRa worker scaffolding with deterministic duty-cycle enforcement.

use anyhow::{anyhow, Result};
use cohesix_ticket::{BudgetSpec, MountSpec, Role, TicketClaims};
use secure9p_codec::SessionId;
use std::collections::VecDeque;

/// Paths exposed for a LoRa mount under `/lora`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoraPaths {
    /// Root mount path (`/lora/<mount>`).
    pub root: String,
    /// Control file path (`/lora/<mount>/ctl`).
    pub ctl: String,
    /// Telemetry file path (`/lora/<mount>/telemetry`).
    pub telemetry: String,
    /// Tamper log path (`/lora/<mount>/tamper`).
    pub tamper: String,
}

impl LoraPaths {
    /// Build LoRa paths for the given mount root and label.
    pub fn new(mount_at: &str, mount: &str) -> Result<Self> {
        let mount_at = mount_at.trim_end_matches('/');
        if mount_at.is_empty() || !mount_at.starts_with('/') {
            return Err(anyhow!("mount_at must be an absolute path"));
        }
        let root = format!("{}/{}", mount_at, mount);
        Ok(Self {
            root: root.clone(),
            ctl: format!("{root}/ctl"),
            telemetry: format!("{root}/telemetry"),
            tamper: format!("{root}/tamper"),
        })
    }
}

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

/// Worker descriptor for LoRa adapters.
#[derive(Debug, Clone)]
pub struct LoraWorker {
    ticket: TicketClaims,
    session: SessionId,
    scope: String,
    paths: LoraPaths,
    guard: DutyCycleGuard,
    tamper: TamperLog,
}

impl LoraWorker {
    /// Create a LoRa worker bound to a scope and mount label.
    pub fn new(
        session: SessionId,
        scope: impl Into<String>,
        mount_at: impl Into<String>,
        mount: impl Into<String>,
        config: DutyCycleConfig,
        tamper_max_entries: usize,
    ) -> Result<Self> {
        let scope = scope.into();
        let mount_at = mount_at.into();
        let mount = mount.into();
        let paths = LoraPaths::new(mount_at.as_str(), mount.as_str())?;
        let ticket = TicketClaims::new(
            Role::WorkerLora,
            BudgetSpec::default_heartbeat(),
            Some(scope.clone()),
            MountSpec::empty(),
            0,
        );
        Ok(Self {
            ticket,
            session,
            scope,
            paths,
            guard: DutyCycleGuard::new(config),
            tamper: TamperLog::new(tamper_max_entries),
        })
    }

    /// Return the capability ticket template.
    #[must_use]
    pub fn ticket(&self) -> &TicketClaims {
        &self.ticket
    }

    /// Return the scope identifier for this worker.
    #[must_use]
    pub fn scope(&self) -> &str {
        &self.scope
    }

    /// Return the generated LoRa paths.
    #[must_use]
    pub fn paths(&self) -> &LoraPaths {
        &self.paths
    }

    /// Return the session identifier bound to the worker.
    #[must_use]
    pub fn session(&self) -> SessionId {
        self.session
    }

    /// Attempt to transmit a payload, recording tamper entries on denial.
    pub fn attempt_tx(&mut self, now_ms: u64, payload: &[u8]) -> DutyCycleDecision {
        match self.guard.attempt(now_ms, payload.len() as u32) {
            Ok(()) => DutyCycleDecision::Allowed,
            Err(reason) => {
                self.tamper.push(TamperEntry {
                    timestamp_ms: now_ms,
                    reason,
                    payload_bytes: payload.len() as u32,
                });
                DutyCycleDecision::Throttled
            }
        }
    }

    /// Snapshot current tamper log entries.
    #[must_use]
    pub fn tamper_snapshot(&self) -> Vec<TamperEntry> {
        self.tamper.snapshot()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duty_cycle_throttles_and_logs() {
        let config = DutyCycleConfig {
            duty_cycle_percent: 10,
            window_ms: 100,
            max_payload_bytes: 32,
        };
        let mut worker =
            LoraWorker::new(SessionId::from_raw(1), "scope", "/lora", "lora-1", config, 4)
                .expect("worker");
        assert_eq!(
            worker.attempt_tx(0, &[0u8; 4]),
            DutyCycleDecision::Allowed
        );
        assert_eq!(
            worker.attempt_tx(1, &[0u8; 4]),
            DutyCycleDecision::Allowed
        );
        assert_eq!(
            worker.attempt_tx(2, &[0u8; 4]),
            DutyCycleDecision::Throttled
        );
        let entries = worker.tamper_snapshot();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].reason, TamperReason::DutyCycleExceeded);
    }

    #[test]
    fn oversize_payload_is_rejected() {
        let config = DutyCycleConfig {
            duty_cycle_percent: 50,
            window_ms: 200,
            max_payload_bytes: 4,
        };
        let mut worker =
            LoraWorker::new(SessionId::from_raw(2), "scope", "/lora", "lora-1", config, 2)
                .expect("worker");
        assert_eq!(
            worker.attempt_tx(10, &[0u8; 8]),
            DutyCycleDecision::Throttled
        );
        let entries = worker.tamper_snapshot();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].reason, TamperReason::PayloadOversize);
    }
}
