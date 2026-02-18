// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Execute GPU lease ticket actions using existing queen and lease control paths.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use anyhow::{anyhow, Context, Result};
use cohsh::{queen, Session, Transport, CLIENT_QUEEN_LEASE_CTL_PATH};
use serde::Deserialize;
use serde_json::json;

use super::{arg_u64, read_last_line, target_components, ExecutorConfig};
use crate::HostTicketSpec;

/// Execute GPU lease ticket actions.
pub fn execute(
    transport: &mut dyn Transport,
    session: &Session,
    spec: &HostTicketSpec,
    _config: &ExecutorConfig,
) -> Result<String> {
    match spec.action.as_str() {
        "gpu.lease.grant" => execute_grant(transport, session, spec),
        "gpu.lease.renew" => execute_renew(transport, session, spec),
        "gpu.lease.release" => execute_release(transport, session, spec),
        other => Err(anyhow!("unsupported gpu action {other}")),
    }
}

fn execute_grant(
    transport: &mut dyn Transport,
    session: &Session,
    spec: &HostTicketSpec,
) -> Result<String> {
    let gpu_id = resolve_gpu_id(spec)?;
    let mem_mb = to_u32(arg_u64(spec, "mem_mb").unwrap_or(4096), "mem_mb")?;
    let streams = to_u8(arg_u64(spec, "streams").unwrap_or(1), "streams")?;
    let ttl_s = to_u32(arg_u64(spec, "ttl_s").unwrap_or(120), "ttl_s")?;
    let priority = arg_u64(spec, "priority").map(|value| to_u8(value, "priority")).transpose()?;

    let mut args = vec![
        format!("gpu_id={gpu_id}"),
        format!("mem_mb={mem_mb}"),
        format!("streams={streams}"),
        format!("ttl_s={ttl_s}"),
    ];
    if let Some(priority) = priority {
        args.push(format!("priority={priority}"));
    }
    if let Some(budget_ttl_s) = arg_u64(spec, "budget_ttl_s") {
        args.push(format!("budget_ttl_s={budget_ttl_s}"));
    }
    if let Some(budget_ops) = arg_u64(spec, "budget_ops") {
        args.push(format!("budget_ops={budget_ops}"));
    }

    let payload = queen::spawn("gpu", args.iter().map(String::as_str))?;
    transport
        .write(session, queen::queen_ctl_path(), payload.as_bytes())
        .context("write gpu lease grant to /queen/ctl")?;

    let lease_path = format!("/gpu/{gpu_id}/lease");
    let verification = read_last_line(transport, session, lease_path.as_str())
        .ok()
        .flatten()
        .filter(|line| line.contains("\"state\":\"ACTIVE\"") && line.contains(&gpu_id))
        .map(|_| "verified")
        .unwrap_or("queued");
    Ok(format!(
        "gpu lease grant gpu_id={gpu_id} mem_mb={mem_mb} streams={streams} ttl_s={ttl_s} status={verification}"
    ))
}

fn execute_renew(
    transport: &mut dyn Transport,
    session: &Session,
    spec: &HostTicketSpec,
) -> Result<String> {
    let lease_id = spec
        .args
        .get("lease_id")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(spec.id.as_str());
    let ttl_s = to_u32(arg_u64(spec, "ttl_s").unwrap_or(120), "ttl_s")?;
    let mut payload = json!({
        "op": "renew",
        "id": lease_id,
        "ttl_s": ttl_s
    });
    if let Some(priority) = arg_u64(spec, "priority") {
        payload["priority"] = serde_json::Value::from(priority);
    }
    let encoded = serde_json::to_string(&payload).context("serialize renew payload")?;
    transport
        .write(session, CLIENT_QUEEN_LEASE_CTL_PATH, format!("{encoded}\n").as_bytes())
        .context("write gpu lease renew to /queen/lease/ctl")?;
    Ok(format!("gpu lease renew id={lease_id} ttl_s={ttl_s} queued"))
}

fn execute_release(
    transport: &mut dyn Transport,
    session: &Session,
    spec: &HostTicketSpec,
) -> Result<String> {
    let gpu_id = resolve_gpu_id(spec)?;
    let worker_id = spec
        .args
        .get("worker_id")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| active_worker_for_gpu(transport, session, gpu_id.as_str()).ok().flatten())
        .ok_or_else(|| anyhow!("gpu release requires worker_id or active /gpu/{gpu_id}/lease entry"))?;

    let payload = queen::kill(worker_id.as_str())?;
    transport
        .write(session, queen::queen_ctl_path(), payload.as_bytes())
        .context("write gpu release kill to /queen/ctl")?;

    let lease_path = format!("/gpu/{gpu_id}/lease");
    let verification = read_last_line(transport, session, lease_path.as_str())
        .ok()
        .flatten()
        .filter(|line| line.contains("\"state\":\"RELEASED\""))
        .map(|_| "released")
        .unwrap_or("queued");
    Ok(format!(
        "gpu lease release gpu_id={gpu_id} worker_id={worker_id} status={verification}"
    ))
}

fn resolve_gpu_id(spec: &HostTicketSpec) -> Result<String> {
    if let Some(gpu_id) = spec
        .args
        .get("gpu_id")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(gpu_id.to_owned());
    }
    let target = target_components(spec);
    if target.len() >= 2 && target[0] == "gpu" {
        return Ok(target[1].to_owned());
    }
    Err(anyhow!(
        "gpu action {} requires args.gpu_id or target /gpu/<id>/...",
        spec.action
    ))
}

fn active_worker_for_gpu(
    transport: &mut dyn Transport,
    session: &Session,
    gpu_id: &str,
) -> Result<Option<String>> {
    let path = format!("/gpu/{gpu_id}/lease");
    let lines = transport
        .read(session, path.as_str())
        .with_context(|| format!("read {path}"))?;
    for line in lines.iter().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let entry: GpuLeaseLine = match serde_json::from_str(trimmed) {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if entry.gpu_id == gpu_id && entry.state == "ACTIVE" {
            return Ok(Some(entry.worker_id));
        }
    }
    Ok(None)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GpuLeaseLine {
    gpu_id: String,
    worker_id: String,
    state: String,
}

fn to_u32(value: u64, field: &str) -> Result<u32> {
    let converted = u32::try_from(value).map_err(|_| anyhow!("{field} exceeds u32"))?;
    if converted == 0 {
        return Err(anyhow!("{field} must be > 0"));
    }
    Ok(converted)
}

fn to_u8(value: u64, field: &str) -> Result<u8> {
    let converted = u8::try_from(value).map_err(|_| anyhow!("{field} exceeds u8"))?;
    if converted == 0 && field != "priority" {
        return Err(anyhow!("{field} must be > 0"));
    }
    Ok(converted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn resolve_gpu_from_target_components() {
        let spec = HostTicketSpec {
            schema: "host-ticket/v1".to_owned(),
            id: "t1".to_owned(),
            idempotency_key: "k1".to_owned(),
            action: "gpu.lease.grant".to_owned(),
            target: Some("/gpu/GPU-0/lease".to_owned()),
            args: Value::Null,
            expires_unix_ms: None,
        };
        let gpu_id = resolve_gpu_id(&spec).expect("gpu id");
        assert_eq!(gpu_id, "GPU-0");
    }

    #[test]
    fn resolve_gpu_prefers_args() {
        let spec = HostTicketSpec {
            schema: "host-ticket/v1".to_owned(),
            id: "t1".to_owned(),
            idempotency_key: "k1".to_owned(),
            action: "gpu.lease.grant".to_owned(),
            target: Some("/gpu/GPU-0/lease".to_owned()),
            args: serde_json::json!({ "gpu_id": "GPU-9" }),
            expires_unix_ms: None,
        };
        let gpu_id = resolve_gpu_id(&spec).expect("gpu id");
        assert_eq!(gpu_id, "GPU-9");
    }
}
