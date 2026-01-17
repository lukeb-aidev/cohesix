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
use crate::{DutyCycleConfig, DutyCycleDecision, DutyCycleGuard, TamperEntry, TamperLog};

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
    use crate::TamperReason;

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

    #[test]
    fn duty_cycle_stress_is_bounded() {
        let config = DutyCycleConfig {
            duty_cycle_percent: 20,
            window_ms: 100,
            max_payload_bytes: 16,
        };
        let mut worker =
            LoraWorker::new(SessionId::from_raw(3), "scope", "/lora", "lora-1", config, 4)
                .expect("worker");
        let mut throttled = 0usize;
        for tick in 0..20 {
            if worker.attempt_tx(tick, &[0u8; 5]) == DutyCycleDecision::Throttled {
                throttled += 1;
            }
        }
        assert!(throttled > 0);
        assert_eq!(worker.tamper_snapshot().len(), 4);
    }
}
