// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Capability ticket primitives shared across Cohesix crates, reflecting
//! `docs/ARCHITECTURE.md` §1-§3.

/// Roles recognised by the Cohesix capability system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Queen orchestration role controlling worker lifecycles.
    Queen,
    /// Worker responsible for emitting heartbeat telemetry.
    WorkerHeartbeat,
    /// Future GPU worker role.
    WorkerGpu,
}

/// Budget specification describing limits applied to a ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetSpec {
    ticks: Option<u64>,
    ops: Option<u64>,
    ttl_s: Option<u64>,
}

impl BudgetSpec {
    /// Budget without restrictions, used during bootstrap flows.
    #[must_use]
    pub fn unbounded() -> Self {
        Self {
            ticks: None,
            ops: None,
            ttl_s: None,
        }
    }

    /// Default limits for heartbeat workers; tuned as real scheduling logic arrives.
    #[must_use]
    pub fn default_heartbeat() -> Self {
        Self {
            ticks: Some(1_000),
            ops: Some(10_000),
            ttl_s: Some(300),
        }
    }

    /// Default limits for GPU workers mirroring lease guardrails.
    #[must_use]
    pub fn default_gpu() -> Self {
        Self {
            ticks: None,
            ops: Some(64),
            ttl_s: Some(120),
        }
    }

    /// Override the tick budget.
    #[must_use]
    pub fn with_ticks(mut self, ticks: Option<u64>) -> Self {
        self.ticks = ticks;
        self
    }

    /// Override the operation budget.
    #[must_use]
    pub fn with_ops(mut self, ops: Option<u64>) -> Self {
        self.ops = ops;
        self
    }

    /// Override the time-to-live budget in seconds.
    #[must_use]
    pub fn with_ttl(mut self, ttl_s: Option<u64>) -> Self {
        self.ttl_s = ttl_s;
        self
    }

    /// Retrieve the configured tick budget.
    #[must_use]
    pub fn ticks(&self) -> Option<u64> {
        self.ticks
    }

    /// Retrieve the configured operation budget.
    #[must_use]
    pub fn ops(&self) -> Option<u64> {
        self.ops
    }

    /// Retrieve the configured time-to-live budget in seconds.
    #[must_use]
    pub fn ttl_s(&self) -> Option<u64> {
        self.ttl_s
    }
}

impl Default for BudgetSpec {
    fn default() -> Self {
        Self::unbounded()
    }
}

/// Template for minting capability tickets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketTemplate {
    role: Role,
    budget: BudgetSpec,
}

impl TicketTemplate {
    /// Create a new ticket template for the supplied role and budget.
    #[must_use]
    pub fn new(role: Role, budget: BudgetSpec) -> Self {
        Self { role, budget }
    }

    /// Retrieve the ticket role.
    #[must_use]
    pub fn role(&self) -> Role {
        self.role
    }

    /// Retrieve the ticket budget configuration.
    #[must_use]
    pub fn budget(&self) -> BudgetSpec {
        self.budget
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_heartbeat_limits_are_finite() {
        let budget = BudgetSpec::default_heartbeat();
        assert!(budget.ticks.is_some());
        assert!(budget.ops.is_some());
        assert!(budget.ttl_s.is_some());
    }

    #[test]
    fn default_gpu_limits_enforce_ttl_and_ops() {
        let budget = BudgetSpec::default_gpu();
        assert!(budget.ticks().is_none());
        assert_eq!(budget.ops(), Some(64));
        assert_eq!(budget.ttl_s(), Some(120));
    }
}
