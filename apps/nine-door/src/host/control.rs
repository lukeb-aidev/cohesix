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
    /// Bind a canonical namespace path to a session-scoped mount point.
    Bind(BindCommand),
    /// Mount a registered service at a session-scoped mount point.
    Mount(MountCommand),
}

impl QueenCommand {
    /// Parse a JSON line into a queen command, enforcing schema restrictions.
    pub fn parse(line: &str) -> Result<Self, NineDoorError> {
        serde_json::from_str(line).map_err(|err| {
            NineDoorError::protocol(
                secure9p_codec::ErrorCode::Invalid,
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
    #[serde(default)]
    pub ticks: Option<u64>,
    /// Optional budget overrides.
    #[serde(default)]
    pub budget: Option<BudgetFields>,
    /// Optional GPU lease specification.
    #[serde(default)]
    pub lease: Option<GpuLeaseFields>,
}

/// Supported worker spawn targets.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpawnTarget {
    /// Heartbeat worker that emits telemetry.
    Heartbeat,
    /// GPU worker that proxies host GPU leases.
    Gpu,
}

impl SpawnCommand {
    /// Construct the final budget for the spawn request.
    pub fn budget_spec(&self, defaults: BudgetSpec) -> Result<BudgetSpec, NineDoorError> {
        match self.spawn {
            SpawnTarget::Heartbeat => {
                let ticks = self.ticks.ok_or_else(|| {
                    NineDoorError::protocol(
                        secure9p_codec::ErrorCode::Invalid,
                        "heartbeat spawn requires ticks",
                    )
                })?;
                let with_ticks = defaults.with_ticks(Some(ticks));
                Ok(self
                    .budget
                    .as_ref()
                    .map(|fields| fields.apply(with_ticks))
                    .unwrap_or(with_ticks))
            }
            SpawnTarget::Gpu => {
                let lease = self.lease.as_ref().ok_or_else(|| {
                    NineDoorError::protocol(
                        secure9p_codec::ErrorCode::Invalid,
                        "gpu spawn requires lease",
                    )
                })?;
                let ttl_budget = defaults.with_ttl(Some(lease.ttl_s.into()));
                let ops_budget = ttl_budget.with_ops(Some(lease.streams as u64 * 8));
                Ok(self
                    .budget
                    .as_ref()
                    .map(|fields| fields.apply(ops_budget))
                    .unwrap_or(ops_budget))
            }
        }
    }

    /// Retrieve the GPU lease if present.
    pub fn gpu_lease(&self) -> Option<&GpuLeaseFields> {
        self.lease.as_ref()
    }
}

/// GPU lease specification parsed from queen commands.
#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct GpuLeaseFields {
    /// Identifier of the GPU to lease.
    pub gpu_id: String,
    /// Memory in mebibytes.
    pub mem_mb: u32,
    /// Concurrent stream limit.
    pub streams: u8,
    /// Lease time-to-live in seconds.
    pub ttl_s: u32,
    /// Requested priority for scheduling.
    pub priority: u8,
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

/// Bind command payload wrapping canonical source and mount paths.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BindCommand {
    /// Bind specification.
    pub bind: BindSpec,
}

impl BindCommand {
    /// Consume the command, returning raw strings and parsed components.
    pub fn into_parts(self) -> Result<(String, String, Vec<String>, Vec<String>), NineDoorError> {
        let BindSpec { from, to } = self.bind;
        let source = parse_absolute_path(&from)?;
        let mount = parse_absolute_path(&to)?;
        Ok((from, to, source, mount))
    }
}

/// Bind specification describing canonical and mount paths.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BindSpec {
    /// Canonical namespace path to expose.
    pub from: String,
    /// Session-scoped mount point receiving the binding.
    pub to: String,
}

/// Mount command payload referencing a registered service.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MountCommand {
    /// Mount specification.
    pub mount: MountSpec,
}

impl MountCommand {
    /// Consume the command, returning the service, raw mount string, and parsed path.
    pub fn into_parts(self) -> Result<(String, String, Vec<String>), NineDoorError> {
        let MountSpec { service, at } = self.mount;
        let mount = parse_absolute_path(&at)?;
        Ok((service, at, mount))
    }
}

/// Mount specification referencing a service and mount point.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MountSpec {
    /// Registered service to mount.
    pub service: String,
    /// Session-scoped mount point receiving the service namespace.
    pub at: String,
}

fn parse_absolute_path(input: &str) -> Result<Vec<String>, NineDoorError> {
    if !input.starts_with('/') {
        return Err(NineDoorError::protocol(
            secure9p_codec::ErrorCode::Invalid,
            format!("path '{input}' must be absolute"),
        ));
    }
    let mut components = Vec::new();
    for component in input.split('/').filter(|segment| !segment.is_empty()) {
        if component == "." || component == ".." {
            return Err(NineDoorError::protocol(
                secure9p_codec::ErrorCode::Invalid,
                format!("path '{input}' contains traversal component"),
            ));
        }
        components.push(component.to_owned());
    }
    Ok(components)
}
