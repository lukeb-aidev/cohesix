// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Execute bounded Docker remediation ticket actions.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::process::Command;

use anyhow::{anyhow, Context, Result};
use cohsh::{Session, Transport};
use host_sidecar_bridge::providers::{format_docker_status_line, parse_docker_info_output};

use super::{arg_str, target_components, ExecutorConfig};
use crate::HostTicketSpec;

const MAX_CAPTURE_BYTES: usize = 256;

/// Execute Docker ticket actions.
pub fn execute(
    transport: &mut dyn Transport,
    session: &Session,
    spec: &HostTicketSpec,
    config: &ExecutorConfig,
) -> Result<String> {
    match spec.action.as_str() {
        "docker.restart" => execute_action(transport, session, config, spec, "restart"),
        "docker.stop" => execute_action(transport, session, config, spec, "stop"),
        "docker.status-check" => execute_status_check(transport, session, config, spec),
        other => Err(anyhow!("unsupported docker action {other}")),
    }
}

fn execute_action(
    transport: &mut dyn Transport,
    session: &Session,
    config: &ExecutorConfig,
    spec: &HostTicketSpec,
    action: &str,
) -> Result<String> {
    let container = resolve_container(spec)?;
    let output = run_docker(&[action, container.as_str()])
        .with_context(|| format!("docker {action} {container}"))?;
    let control_path = format!("{}/docker/{action}", config.mount);
    let control_line = format!("ticket={} container={container} action={action}\n", spec.id);
    transport
        .write(session, control_path.as_str(), control_line.as_bytes())
        .with_context(|| format!("write {}", control_path))?;
    let state = inspect_container_state(container.as_str())?;
    publish_container_status(
        transport,
        session,
        config,
        container.as_str(),
        state.as_str(),
    )?;
    Ok(format!(
        "docker action={action} container={container} state={} cmd={}",
        state,
        summarize_output(output.as_str())
    ))
}

fn execute_status_check(
    transport: &mut dyn Transport,
    session: &Session,
    config: &ExecutorConfig,
    spec: &HostTicketSpec,
) -> Result<String> {
    if let Some(container) = resolve_container_optional(spec) {
        let state = inspect_container_state(container.as_str())?;
        publish_container_status(
            transport,
            session,
            config,
            container.as_str(),
            state.as_str(),
        )?;
        return Ok(format!(
            "docker status-check container={container} state={state}"
        ));
    }
    let line = docker_info_status_line()?;
    publish_line(transport, session, config, line.as_str())?;
    Ok(format!("docker status-check {}", line))
}

fn docker_info_status_line() -> Result<String> {
    let output = run_docker(&[
        "info",
        "--format",
        "{{.ServerVersion}} {{.Containers}} {{.ContainersRunning}} {{.ContainersPaused}} {{.ContainersStopped}}",
    ])?;
    let status = parse_docker_info_output(output.as_str())
        .ok_or_else(|| anyhow!("unable to parse docker info output"))?;
    Ok(format_docker_status_line(&status))
}

fn inspect_container_state(container: &str) -> Result<String> {
    let output = run_docker(&["inspect", "--format", "{{.State.Status}}", container])
        .with_context(|| format!("docker inspect {container}"))?;
    let state = output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .ok_or_else(|| anyhow!("docker inspect returned empty state for {container}"))?;
    Ok(state.to_owned())
}

fn publish_container_status(
    transport: &mut dyn Transport,
    session: &Session,
    config: &ExecutorConfig,
    container: &str,
    state: &str,
) -> Result<()> {
    let line = format!(
        "state={} container={}\n",
        sanitize_token(state),
        sanitize_token(container)
    );
    publish_line(transport, session, config, line.as_str())
}

fn publish_line(
    transport: &mut dyn Transport,
    session: &Session,
    config: &ExecutorConfig,
    line: &str,
) -> Result<()> {
    let path = format!("{}/docker/status", config.mount);
    transport
        .write(session, path.as_str(), line.as_bytes())
        .with_context(|| format!("write {}", path))
}

fn run_docker(args: &[&str]) -> Result<String> {
    let output = Command::new("docker")
        .args(args)
        .output()
        .with_context(|| format!("run docker {}", args.join(" ")))?;
    let stdout = bounded_utf8(&output.stdout);
    let stderr = bounded_utf8(&output.stderr);
    if !output.status.success() {
        return Err(anyhow!(
            "docker {} failed status={} stdout='{}' stderr='{}'",
            args.join(" "),
            output
                .status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "signal".to_owned()),
            summarize_output(stdout.as_str()),
            summarize_output(stderr.as_str())
        ));
    }
    if stdout.trim().is_empty() {
        return Ok(stderr);
    }
    Ok(stdout)
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

fn bounded_utf8(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes).replace(['\n', '\r'], " ");
    if text.len() <= MAX_CAPTURE_BYTES {
        return text.trim().to_owned();
    }
    text[..MAX_CAPTURE_BYTES].trim().to_owned()
}

fn summarize_output(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return "none".to_owned();
    }
    trimmed.to_owned()
}

fn sanitize_token(input: &str) -> String {
    let mut out = String::new();
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':' | '/') {
            out.push(ch);
        }
    }
    if out.is_empty() {
        out.push_str("unknown");
    }
    out
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
        };
        assert_eq!(
            resolve_container_optional(&spec).as_deref(),
            Some("worker-1")
        );
    }
}
