// Author: Lukas Bower
// Purpose: Execute compiled field-bus maps under admitted tickets and reconcile remote acknowledgements without replaying controls.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]

use super::{observation, provider_pending, ExecutorConfig, ReconcileOutcome};
use crate::HostTicketSpec;
use anyhow::{anyhow, bail, Result};
use sidecar_bus::live::{Entry, Request, Spool, State};

fn request(spec: &HostTicketSpec) -> Result<Request> {
    crate::provider::validate(spec)?;
    Ok(Request {
        schema: "cohesix-field-bus-request/v1".into(),
        id: spec.id.clone(),
        idempotency_key: spec.idempotency_key.clone(),
        endpoint: spec.args["endpoint"]
            .as_str()
            .ok_or_else(|| anyhow!("invalid field-bus endpoint"))?
            .into(),
        point: spec.args["point"]
            .as_str()
            .ok_or_else(|| anyhow!("invalid field-bus point"))?
            .into(),
    })
}

fn spool(config: &ExecutorConfig) -> Result<Spool> {
    let root = config
        .field_bus_state_root
        .as_deref()
        .ok_or_else(|| anyhow!("not_enabled field-bus-state-root"))?;
    Ok(Spool::open(root)?)
}

/// Execute one admitted exact map; dispatch alone never returns success.
pub fn execute(spec: &HostTicketSpec, config: &ExecutorConfig) -> Result<String> {
    let request = request(spec)?;
    observation::prepare(config)?;
    let mut spool = spool(config)?;
    let entry = if spec.action.ends_with(".control") {
        let authority = crate::causal::open(config, spec)?
            .ok_or_else(|| anyhow!("EPERM field-bus control requires independent signed grant"))?;
        spool.control(&request, &serde_json::to_value(spec)?, &authority)?
    } else {
        spool.read(&request)?
    };
    match entry.state {
        State::Delivered => retain(config, spec, &entry),
        State::Failed => bail!(
            "field_bus_failed {}",
            entry.error.as_deref().unwrap_or("protocol_refusal")
        ),
        _ => Err(provider_pending(
            "field_bus_unconfirmed; reconcile retained WAL before any replay",
        )),
    }
}

/// Resume only bounded pending reads. An uncertain control is never retransmitted.
pub fn reconcile(spec: &HostTicketSpec, config: &ExecutorConfig) -> Result<ReconcileOutcome> {
    let request = request(spec)?;
    let mut spool = spool(config)?;
    let Some(mut entry) = spool.retained(&request)? else {
        return Ok(ReconcileOutcome::Ambiguous);
    };
    if spec.action.ends_with(".read") && matches!(entry.state, State::Prepared | State::Attempting)
    {
        crate::causal::preflight(config, spec)?;
        entry = spool.read(&request)?;
    }
    match entry.state {
        State::Delivered => Ok(ReconcileOutcome::Committed(retain(config, spec, &entry)?)),
        State::Failed => Ok(ReconcileOutcome::Rejected(format!(
            "field_bus_failed {}",
            entry.error.as_deref().unwrap_or("protocol_refusal")
        ))),
        _ => Ok(ReconcileOutcome::Ambiguous),
    }
}

fn retain(config: &ExecutorConfig, spec: &HostTicketSpec, entry: &Entry) -> Result<String> {
    let observed = entry
        .observed_unix_ms
        .ok_or_else(|| anyhow!("unconfirmed field-bus ACK time"))?;
    // Unit/outstation identity comes from the validated protocol response and exact
    // compiled map. Neither protocol cryptographically attests the physical device.
    let identity = format!(
        "{}:{}:{}",
        entry.request.endpoint, entry.operation_sha256, entry.sequence
    );
    observation::retain_native_at(config, spec, entry, &identity, observed)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_maps_and_unconfigured_wal_refuse_before_io() {
        let spec = HostTicketSpec {
            action: "modbus.read".into(),
            target: Some("absent".into()),
            args: serde_json::json!({"endpoint":"absent","point":"value"}),
            ..HostTicketSpec::default()
        };
        assert!(request(&spec).is_err());
        assert!(spool(&ExecutorConfig::default()).is_err());
    }
}
