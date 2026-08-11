// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Execute bounded systemd remediation ticket actions.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::process::Command;

use anyhow::{anyhow, Context, Result};
use cohsh::{Session, Transport};
use host_sidecar_bridge::providers::{
    format_systemd_status_line, parse_systemd_show_output, SystemdStatus,
};

use super::{arg_str, target_components, ExecutorConfig};
use crate::{text::bounded_utf8_lossy, HostTicketSpec};

const MAX_CAPTURE_BYTES: usize = 256;

/// Execute systemd ticket actions.
pub fn execute(
    transport: &mut dyn Transport,
    session: &Session,
    spec: &HostTicketSpec,
    config: &ExecutorConfig,
) -> Result<String> {
    let unit = resolve_unit(spec)?;
    let action = spec.action.as_str();
    match action {
        "systemd.start" => execute_action(transport, session, config, spec, unit.as_str(), "start"),
        "systemd.stop" => execute_action(transport, session, config, spec, unit.as_str(), "stop"),
        "systemd.restart" => {
            execute_action(transport, session, config, spec, unit.as_str(), "restart")
        }
        "systemd.status-check" => execute_status(transport, session, config, unit.as_str()),
        other => Err(anyhow!("unsupported systemd action {other}")),
    }
}

fn execute_action(
    transport: &mut dyn Transport,
    session: &Session,
    config: &ExecutorConfig,
    spec: &HostTicketSpec,
    unit: &str,
    action: &str,
) -> Result<String> {
    let output =
        run_systemctl(&[action, unit]).with_context(|| format!("systemctl {action} {unit}"))?;
    let control_path = format!("{}/systemd/{unit}/{action}", config.mount);
    let control_line = format!("ticket={} action={action}\n", spec.id);
    transport
        .write(session, control_path.as_str(), control_line.as_bytes())
        .with_context(|| format!("write {}", control_path))?;
    let status = collect_status(unit)?;
    publish_status_line(transport, session, config, unit, &status)?;
    Ok(format!(
        "systemd action={action} unit={unit} state={} sub={} cmd={}",
        status.state,
        status.sub,
        summarize_output(output.as_str())
    ))
}

fn execute_status(
    transport: &mut dyn Transport,
    session: &Session,
    config: &ExecutorConfig,
    unit: &str,
) -> Result<String> {
    let status = collect_status(unit)?;
    publish_status_line(transport, session, config, unit, &status)?;
    Ok(format!(
        "systemd status-check unit={unit} state={} sub={}",
        status.state, status.sub
    ))
}

fn collect_status(unit: &str) -> Result<SystemdStatus> {
    let output = run_systemctl(&["show", unit, "--property=ActiveState,SubState"])
        .with_context(|| format!("systemctl show {unit}"))?;
    parse_systemd_show_output(output.as_str())
        .ok_or_else(|| anyhow!("unable to parse systemctl show output for unit {unit}"))
}

fn publish_status_line(
    transport: &mut dyn Transport,
    session: &Session,
    config: &ExecutorConfig,
    unit: &str,
    status: &SystemdStatus,
) -> Result<()> {
    let path = format!("{}/systemd/{unit}/status", config.mount);
    let line = format!("{}\n", format_systemd_status_line(status));
    transport
        .write(session, path.as_str(), line.as_bytes())
        .with_context(|| format!("write {}", path))
}

fn run_systemctl(args: &[&str]) -> Result<String> {
    let output = Command::new("systemctl")
        .args(args)
        .output()
        .with_context(|| format!("run systemctl {}", args.join(" ")))?;
    let stdout = bounded_utf8_lossy(&output.stdout, MAX_CAPTURE_BYTES);
    let stderr = bounded_utf8_lossy(&output.stderr, MAX_CAPTURE_BYTES);
    if !output.status.success() {
        return Err(anyhow!(
            "systemctl {} failed status={} stdout='{}' stderr='{}'",
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

fn summarize_output(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return "none".to_owned();
    }
    trimmed.to_owned()
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
