// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Execute admitted systemd actions through D-Bus and require the exact native terminal postcondition.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use cohesix_authority::standing::AdmissionFacts;
use cohsh::{Session, Transport};
use host_sidecar_bridge::native::{
    dispatch_systemd, observe_systemd_before, systemd_postcondition, validate_native_id,
};

use super::{arg_str, provider_pending, target_components, ExecutorConfig};
use crate::HostTicketSpec;

/// Execute through the Manager API; command completion alone cannot publish success.
pub fn execute(
    _transport: &mut dyn Transport,
    _session: &Session,
    spec: &HostTicketSpec,
    config: &ExecutorConfig,
) -> Result<String> {
    let unit = resolve_unit(spec)?;
    validate_native_id(&unit)?;
    let deadline = Instant::now() + Duration::from_secs(30);
    let before = observe_systemd_before(&unit, deadline)?;
    if spec.action == "systemd.status-check" {
        return super::observation::retain_native(
            config,
            spec,
            &before,
            &format!(
                "systemd:{}:{}",
                before.unit,
                before.invocation_id.as_deref().unwrap_or("no-invocation")
            ),
        );
    }
    let action = match spec.action.as_str() {
        "systemd.start" => "start",
        "systemd.stop" => "stop",
        "systemd.restart" => "restart",
        _ => return Err(anyhow!("EPERM unsupported-systemd-action")),
    };
    super::observation::prepare(config)?;
    if let Some(record) = crate::standing::selected_record(spec, config)? {
        let (state_epoch, resource_generation) =
            crate::standing::service_generations(&serde_json::to_value(&before)?)?;
        let now = crate::unix_time_ms_now();
        crate::standing::begin_dispatch(
            spec,
            config,
            &AdmissionFacts {
                observed_unix_ms: now,
                state_epoch,
                resource_generation,
                policy_sha256: record.binding.policy_sha256,
            },
            now,
        )?;
    }
    let job = dispatch_systemd(&unit, action, deadline)?;
    loop {
        let observed = observe_systemd_before(&unit, deadline).map_err(|_| {
            provider_pending(format!(
                "systemd action={action} unit={unit} job={job} observation=unavailable"
            ))
        })?;
        if systemd_postcondition(action, &before, &observed) {
            let evidence_ref = super::observation::retain_native(
                config,
                spec,
                &serde_json::json!({
                    "before": before, "after": observed, "job": job,
                }),
                &format!("systemd:{}:job:{}", observed.unit, job),
            )
            .map_err(|error| {
                provider_pending(format!(
                    "systemd observation persistence unavailable: {error}"
                ))
            })?;
            return Ok(evidence_ref);
        }
        if observed.job_id == 0 && observed.service_result != "success" {
            return Err(anyhow!(
                "systemd terminal failure unit={} invocation={} result={}",
                observed.unit,
                observed.invocation_id.as_deref().unwrap_or("unavailable"),
                observed.service_result
            ));
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(provider_pending(format!(
                "systemd action={action} unit={unit} job={job} observation=timeout"
            )));
        }
        std::thread::sleep(remaining.min(Duration::from_millis(10)));
    }
}

fn resolve_unit(spec: &HostTicketSpec) -> Result<String> {
    if let Some(unit) = arg_str(spec, "unit") {
        return Ok(unit.to_owned());
    }
    let target = target_components(spec);
    if target.len() >= 3 && target[0] == "host" && target[1] == "systemd" {
        return Ok(target[2].to_owned());
    }
    if target.len() >= 2 && target[0] == "systemd" {
        return Ok(target[1].to_owned());
    }
    Err(anyhow!(
        "systemd action {} requires args.unit or target /host/systemd/<unit>/...",
        spec.action
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn resolve_unit_from_target() {
        let spec = HostTicketSpec {
            schema: "host-ticket/v1".to_owned(),
            id: "id".to_owned(),
            idempotency_key: "k".to_owned(),
            action: "systemd.restart".to_owned(),
            target: Some("/host/systemd/cohesix-agent.service/restart".to_owned()),
            args: Value::Null,
            expires_unix_ms: None,
            source_hive: None,
            target_hive: None,
            relay_hop: None,
            relay_correlation_id: None,
            ..HostTicketSpec::default()
        };
        let unit = resolve_unit(&spec).expect("unit");
        assert_eq!(unit, "cohesix-agent.service");
    }
}
