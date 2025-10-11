// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use cohesix_ticket::BudgetSpec;
use serde::Deserialize;

use crate::NineDoorError;

/// Commands accepted by `/queen/ctl`.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum QueenCommand {
    /// Spawn a new worker instance.
    Spawn(SpawnCommand),
    /// Kill an existing worker instance.
    Kill(KillCommand),
    /// Update default budget settings for subsequent spawns.
    Budget(BudgetCommand),
}

impl QueenCommand {
    /// Parse a JSON line into a queen command, enforcing schema restrictions.
    pub fn parse(line: &str) -> Result<Self, NineDoorError> {
        serde_json::from_str(line).map_err(|err| {
            NineDoorError::protocol(
                secure9p_wire::ErrorCode::Invalid,
                format!("invalid queen command: {err}"),
            )
        })
    }
}

/// Spawn request specifying worker type and budget overrides.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnCommand {
    /// Worker kind to spawn.
    pub spawn: SpawnTarget,
    /// Number of scheduler ticks to allocate to the worker.
    pub ticks: u64,
    /// Optional budget overrides.
    #[serde(default)]
    pub budget: Option<BudgetFields>,
}

/// Supported worker spawn targets.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpawnTarget {
    /// Heartbeat worker that emits telemetry.
    Heartbeat,
}

impl SpawnCommand {
    /// Construct the final budget for the spawn request.
    pub fn budget_spec(&self, defaults: BudgetSpec) -> BudgetSpec {
        let with_ticks = defaults.with_ticks(Some(self.ticks));
        if let Some(fields) = &self.budget {
            fields.apply(with_ticks)
        } else {
            with_ticks
        }
    }
}

/// Kill command specifying the worker identifier.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KillCommand {
    /// Identifier of the worker to kill.
    pub kill: String,
}

/// Budget override command used to adjust defaults.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetCommand {
    /// Budget override payload.
    pub budget: BudgetFields,
}

impl BudgetCommand {
    /// Apply overrides to the provided budget spec.
    pub fn apply(&self, defaults: BudgetSpec) -> BudgetSpec {
        self.budget.apply(defaults)
    }
}

/// Budget fields that may appear in spawn and budget commands.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetFields {
    /// Optional time-to-live override in seconds.
    #[serde(default)]
    pub ttl_s: Option<u64>,
    /// Optional operation budget override.
    #[serde(default)]
    pub ops: Option<u64>,
}

impl BudgetFields {
    fn apply(&self, spec: BudgetSpec) -> BudgetSpec {
        let with_ttl = spec.with_ttl(self.ttl_s);
        with_ttl.with_ops(self.ops)
    }
}
