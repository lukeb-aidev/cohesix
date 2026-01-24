// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide coh gpu list/status/lease helpers.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use anyhow::{anyhow, Context, Result};
use cohsh::client::CohClient;
use cohsh::queen;
use cohsh_core::wire::AckStatus;
use cohsh_core::Secure9pTransport;
use serde::Deserialize;

use crate::{list_dir, read_file, write_append, CohAudit, MAX_DIR_LIST_BYTES};

const GPU_ROOT: &str = "/gpu";
const MAX_GPU_INFO_BYTES: usize = 16 * 1024;
const MAX_GPU_STATUS_BYTES: usize = 64 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GpuInfoPayload {
    id: String,
    name: String,
    memory_mb: u32,
    sm_count: u32,
    driver_version: String,
    runtime_version: String,
}

/// GPU lease request parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuLeaseArgs {
    /// GPU identifier to lease.
    pub gpu_id: String,
    /// Memory requested in MiB.
    pub mem_mb: u32,
    /// Stream count requested.
    pub streams: u8,
    /// Lease TTL in seconds.
    pub ttl_s: u32,
    /// Optional scheduling priority.
    pub priority: Option<u8>,
    /// Optional budget TTL override.
    pub budget_ttl_s: Option<u64>,
    /// Optional budget ops override.
    pub budget_ops: Option<u64>,
}

/// List GPUs and append output lines to the audit transcript.
pub fn list<T: Secure9pTransport>(
    client: &mut CohClient<T>,
    audit: &mut CohAudit,
) -> Result<()> {
    let entries = list_dir(client, GPU_ROOT, MAX_DIR_LIST_BYTES)?;
    audit.push_ack(AckStatus::Ok, "LS", Some("path=/gpu"));
    let gpus = entries
        .into_iter()
        .filter(|entry| entry != "models" && entry != "telemetry")
        .collect::<Vec<_>>();
    if gpus.is_empty() {
        audit.push_line("gpu: none".to_owned());
        return Ok(());
    }
    for gpu_id in gpus {
        let info_path = format!("/gpu/{gpu_id}/info");
        let payload = read_file(client, &info_path, MAX_GPU_INFO_BYTES)
            .with_context(|| format!("read {info_path}"))?;
        let detail = format!("path={info_path}");
        audit.push_ack(AckStatus::Ok, "CAT", Some(detail.as_str()));
        let info_text = std::str::from_utf8(&payload)
            .with_context(|| format!("{info_path} is not UTF-8"))?;
        let info: GpuInfoPayload = serde_json::from_str(info_text)
            .with_context(|| format!("invalid gpu info JSON in {info_path}"))?;
        audit.push_line(format!(
            "gpu id={} name={} mem_mb={} sm={} driver={} runtime={}",
            info.id,
            info.name,
            info.memory_mb,
            info.sm_count,
            info.driver_version,
            info.runtime_version
        ));
    }
    Ok(())
}

/// Fetch the latest GPU status line.
pub fn status<T: Secure9pTransport>(
    client: &mut CohClient<T>,
    audit: &mut CohAudit,
    gpu_id: &str,
) -> Result<()> {
    if gpu_id.trim().is_empty() {
        return Err(anyhow!("gpu id must not be empty"));
    }
    let status_path = format!("/gpu/{gpu_id}/status");
    let payload = read_file(client, &status_path, MAX_GPU_STATUS_BYTES)
        .with_context(|| format!("read {status_path}"))?;
    let detail = format!("path={status_path}");
    audit.push_ack(AckStatus::Ok, "CAT", Some(detail.as_str()));
    let text = String::from_utf8(payload)
        .with_context(|| format!("{status_path} is not UTF-8"))?;
    let line = text
        .lines()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .last()
        .unwrap_or("EMPTY");
    audit.push_line(format!("gpu id={gpu_id} status={line}"));
    Ok(())
}

/// Request a GPU lease via /queen/ctl.
pub fn lease<T: Secure9pTransport>(
    client: &mut CohClient<T>,
    audit: &mut CohAudit,
    args: &GpuLeaseArgs,
) -> Result<()> {
    if args.gpu_id.trim().is_empty() {
        return Err(anyhow!("gpu id must not be empty"));
    }
    let mut spawn_args = Vec::new();
    spawn_args.push(format!("gpu_id={}", args.gpu_id));
    spawn_args.push(format!("mem_mb={}", args.mem_mb));
    spawn_args.push(format!("streams={}", args.streams));
    spawn_args.push(format!("ttl_s={}", args.ttl_s));
    if let Some(priority) = args.priority {
        spawn_args.push(format!("priority={priority}"));
    }
    if let Some(ttl) = args.budget_ttl_s {
        spawn_args.push(format!("budget_ttl_s={ttl}"));
    }
    if let Some(ops) = args.budget_ops {
        spawn_args.push(format!("budget_ops={ops}"));
    }
    let payload = queen::spawn("gpu", spawn_args.iter().map(String::as_str))?;
    let written = write_append(client, queen::queen_ctl_path(), payload.as_bytes())?;
    let detail = format!("path={} bytes={written}", queen::queen_ctl_path());
    audit.push_ack(AckStatus::Ok, "ECHO", Some(&detail));
    audit.push_line(format!(
        "lease requested gpu_id={} mem_mb={} streams={} ttl_s={}",
        args.gpu_id, args.mem_mb, args.streams, args.ttl_s
    ));
    Ok(())
}
