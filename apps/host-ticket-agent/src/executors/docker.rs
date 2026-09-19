// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Execute bounded Docker remediation ticket actions.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use cohsh::{Session, Transport};
use host_sidecar_bridge::docker::DockerEngine;

use super::{arg_str, target_components, ExecutorConfig};
use crate::HostTicketSpec;

/// Execute Docker ticket actions.
pub fn execute(
    _transport: &mut dyn Transport,
    _session: &Session,
    spec: &HostTicketSpec,
    config: &ExecutorConfig,
) -> Result<String> {
    match spec.action.as_str() {
        "docker.restart" => execute_action(config, spec, "restart"),
        "docker.stop" => execute_action(config, spec, "stop"),
        "docker.status-check" => execute_status_check(config, spec),
        other => Err(anyhow!("unsupported docker action {other}")),
    }
}

fn execute_action(config: &ExecutorConfig, spec: &HostTicketSpec, action: &str) -> Result<String> {
    let container = resolve_container(spec)?;
    super::observation::prepare(config)?;
    let engine = engine()?;
    let observation = engine
        .transition(&container, action)
        .map_err(|error| super::provider_pending(format!("docker outcome unverified: {error}")))?;
    let evidence_ref = super::observation::retain_native(
        config,
        spec,
        &observation,
        &format!(
            "docker:{}:{}",
            observation.after.container_id, observation.after.started_at
        ),
    )
    .map_err(|error| {
        super::provider_pending(format!(
            "docker observation persistence unavailable: {error}"
        ))
    })?;
    // Native identity/event/log correlation is carried into the durable result.
    Ok(evidence_ref)
}

fn engine() -> Result<DockerEngine> {
    DockerEngine::connect(
        Path::new("/var/run/docker.sock"),
        Instant::now() + Duration::from_secs(30),
    )
}

fn execute_status_check(config: &ExecutorConfig, spec: &HostTicketSpec) -> Result<String> {
    let engine = engine()?;
    if let Some(container) = resolve_container_optional(spec) {
        let observation = engine.inspect(&container)?;
        return super::observation::retain_native(
            config,
            spec,
            &observation,
            &format!(
                "docker:{}:{}",
                observation.container_id, observation.started_at
            ),
        );
    }
    let status = engine.status()?;
    super::observation::retain_native(
        config,
        spec,
        &serde_json::json!({
            "source":"docker-engine-api", "version":status.version,
            "containers":status.containers, "running":status.running,
            "paused":status.paused, "stopped":status.stopped,
        }),
        "docker-engine:local-unix-socket",
    )
}

fn resolve_container(spec: &HostTicketSpec) -> Result<String> {
    resolve_container_optional(spec).ok_or_else(|| {
        anyhow!(
            "docker action {} requires args.container or target /host/docker/<container>/...",
            spec.action
        )
    })
}

fn resolve_container_optional(spec: &HostTicketSpec) -> Option<String> {
    if let Some(container) = arg_str(spec, "container") {
        return Some(container.to_owned());
    }
    let target = target_components(spec);
    if target.len() >= 3 && target[0] == "host" && target[1] == "docker" {
        return Some(target[2].to_owned());
    }
    if target.len() >= 2 && target[0] == "docker" {
        return Some(target[1].to_owned());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn resolve_container_from_args() {
        let spec = HostTicketSpec {
            schema: "host-ticket/v1".to_owned(),
            id: "id".to_owned(),
            idempotency_key: "k".to_owned(),
            action: "docker.restart".to_owned(),
            target: None,
            args: serde_json::json!({ "container": "cohesix-agent" }),
            expires_unix_ms: None,
            source_hive: None,
            target_hive: None,
            relay_hop: None,
            relay_correlation_id: None,
            ..HostTicketSpec::default()
        };
        assert_eq!(
            resolve_container_optional(&spec).as_deref(),
            Some("cohesix-agent")
        );
    }

    #[test]
    fn resolve_container_from_target() {
        let spec = HostTicketSpec {
            schema: "host-ticket/v1".to_owned(),
            id: "id".to_owned(),
            idempotency_key: "k".to_owned(),
            action: "docker.restart".to_owned(),
            target: Some("/host/docker/worker-1/restart".to_owned()),
            args: Value::Null,
            expires_unix_ms: None,
            source_hive: None,
            target_hive: None,
            relay_hop: None,
            relay_correlation_id: None,
            ..HostTicketSpec::default()
        };
        assert_eq!(
            resolve_container_optional(&spec).as_deref(),
            Some("worker-1")
        );
    }
}
