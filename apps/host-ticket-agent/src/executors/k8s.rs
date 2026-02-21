// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Execute bounded Kubernetes coexistence ticket actions.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::process::Command;

use anyhow::{anyhow, Context, Result};
use cohsh::{Session, Transport};

use super::{arg_str, target_components, ExecutorConfig};
use crate::HostTicketSpec;

const MAX_CAPTURE_BYTES: usize = 256;

/// Execute Kubernetes ticket actions.
pub fn execute(
    transport: &mut dyn Transport,
    session: &Session,
    spec: &HostTicketSpec,
    config: &ExecutorConfig,
) -> Result<String> {
    match spec.action.as_str() {
        "k8s.cordon" => execute_node_action(transport, session, config, spec, "cordon"),
        "k8s.drain" => execute_drain(transport, session, config, spec),
        "k8s.lease.sync" => execute_lease_sync(transport, session, config, spec),
        other => Err(anyhow!("unsupported k8s action {other}")),
    }
}

fn execute_node_action(
    transport: &mut dyn Transport,
    session: &Session,
    config: &ExecutorConfig,
    spec: &HostTicketSpec,
    action: &str,
) -> Result<String> {
    let node = resolve_node(spec)?;
    let output = run_kubectl(&[action, node.as_str()])
        .with_context(|| format!("kubectl {action} {node}"))?;
    let control_path = format!("{}/k8s/node/{node}/{action}", config.mount);
    let line = format!("ticket={} node={} action={action}\n", spec.id, node);
    transport
        .write(session, control_path.as_str(), line.as_bytes())
        .with_context(|| format!("write {}", control_path))?;
    let status = read_node_status_line(node.as_str())?;
    publish_status(transport, session, config, node.as_str(), status.as_str())?;
    Ok(format!(
        "k8s action={action} node={node} status={} cmd={}",
        status,
        summarize_output(output.as_str())
    ))
}

fn execute_drain(
    transport: &mut dyn Transport,
    session: &Session,
    config: &ExecutorConfig,
    spec: &HostTicketSpec,
) -> Result<String> {
    let node = resolve_node(spec)?;
    let output = run_kubectl(&[
        "drain",
        node.as_str(),
        "--ignore-daemonsets",
        "--delete-emptydir-data",
        "--force",
        "--grace-period=30",
        "--timeout=120s",
    ])
    .with_context(|| format!("kubectl drain {node}"))?;
    let control_path = format!("{}/k8s/node/{node}/drain", config.mount);
    let line = format!("ticket={} node={} action=drain\n", spec.id, node);
    transport
        .write(session, control_path.as_str(), line.as_bytes())
        .with_context(|| format!("write {}", control_path))?;
    let status = read_node_status_line(node.as_str())?;
    publish_status(transport, session, config, node.as_str(), status.as_str())?;
    Ok(format!(
        "k8s action=drain node={node} status={} cmd={}",
        status,
        summarize_output(output.as_str())
    ))
}

fn execute_lease_sync(
    transport: &mut dyn Transport,
    session: &Session,
    config: &ExecutorConfig,
    spec: &HostTicketSpec,
) -> Result<String> {
    let node = resolve_node(spec)?;
    let status = read_node_status_line(node.as_str())?;
    publish_status(transport, session, config, node.as_str(), status.as_str())?;
    Ok(format!("k8s lease sync node={node} status={status}"))
}

fn read_node_status_line(node: &str) -> Result<String> {
    let output = run_kubectl(&["get", "node", node, "--no-headers"])
        .with_context(|| format!("kubectl get node {node}"))?;
    let line = output
        .lines()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("kubectl get node {node} returned no rows"))?;
    let tokens = line.split_whitespace().collect::<Vec<_>>();
    if tokens.len() < 2 {
        return Err(anyhow!("kubectl get node {node} output malformed"));
    }
    let state = sanitize_token(tokens[1].to_ascii_lowercase().as_str());
    let role = sanitize_token(tokens.get(2).copied().unwrap_or("unknown"));
    let version = sanitize_token(tokens.last().copied().unwrap_or("unknown"));
    Ok(format!("state={state} role={role} version={version}"))
}

fn publish_status(
    transport: &mut dyn Transport,
    session: &Session,
    config: &ExecutorConfig,
    node: &str,
    status: &str,
) -> Result<()> {
    let path = format!("{}/k8s/node/{node}/status", config.mount);
    let line = format!("{status}\n");
    transport
        .write(session, path.as_str(), line.as_bytes())
        .with_context(|| format!("write {}", path))
}

fn run_kubectl(args: &[&str]) -> Result<String> {
    let output = Command::new("kubectl")
        .args(args)
        .output()
        .with_context(|| format!("run kubectl {}", args.join(" ")))?;
    let stdout = bounded_utf8(&output.stdout);
    let stderr = bounded_utf8(&output.stderr);
    if !output.status.success() {
        return Err(anyhow!(
            "kubectl {} failed status={} stdout='{}' stderr='{}'",
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

fn resolve_node(spec: &HostTicketSpec) -> Result<String> {
    if let Some(node) = arg_str(spec, "node") {
        return Ok(node.to_owned());
    }
    let target = target_components(spec);
    if target.len() >= 4 && target[0] == "host" && target[1] == "k8s" && target[2] == "node" {
        return Ok(target[3].to_owned());
    }
    if target.len() >= 3 && target[0] == "k8s" && target[1] == "node" {
        return Ok(target[2].to_owned());
    }
    Err(anyhow!(
        "k8s action {} requires args.node or target /host/k8s/node/<name>/...",
        spec.action
    ))
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
        } else if ch.is_whitespace() {
            out.push('_');
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
    fn resolve_node_from_target() {
        let spec = HostTicketSpec {
            schema: "host-ticket/v1".to_owned(),
            id: "id".to_owned(),
            idempotency_key: "k".to_owned(),
            action: "k8s.cordon".to_owned(),
            target: Some("/host/k8s/node/node-1/cordon".to_owned()),
            args: Value::Null,
            expires_unix_ms: None,
            source_hive: None,
            target_hive: None,
            relay_hop: None,
            relay_correlation_id: None,
        };
        let node = resolve_node(&spec).expect("node");
        assert_eq!(node, "node-1");
    }
}
