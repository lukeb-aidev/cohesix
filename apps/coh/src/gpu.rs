// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide coh gpu list/status/lease helpers.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::fs;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use cohsh::queen;
use cohsh_core::wire::AckStatus;
use serde::Deserialize;
use serde::Serialize;

use crate::{CohAccess, CohAudit, MAX_DIR_LIST_BYTES};

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
pub fn list<C: CohAccess>(client: &mut C, audit: &mut CohAudit) -> Result<()> {
    let entries = client.list_dir(GPU_ROOT, MAX_DIR_LIST_BYTES)?;
    audit.push_ack(AckStatus::Ok, "LS", Some("path=/gpu"));
    let gpus = entries
        .into_iter()
        .filter(|entry| entry != "models" && entry != "telemetry" && entry != "bridge")
        .collect::<Vec<_>>();
    if gpus.is_empty() {
        audit.push_line("gpu: none".to_owned());
        return Ok(());
    }
    for gpu_id in gpus {
        let info_path = format!("/gpu/{gpu_id}/info");
        let payload = client
            .read_file(&info_path, MAX_GPU_INFO_BYTES)
            .with_context(|| format!("read {info_path}"))?;
        let detail = format!("path={info_path}");
        audit.push_ack(AckStatus::Ok, "CAT", Some(detail.as_str()));
        let info_text =
            std::str::from_utf8(&payload).with_context(|| format!("{info_path} is not UTF-8"))?;
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
pub fn status<C: CohAccess>(client: &mut C, audit: &mut CohAudit, gpu_id: &str) -> Result<()> {
    if gpu_id.trim().is_empty() {
        return Err(anyhow!("gpu id must not be empty"));
    }
    let status_path = format!("/gpu/{gpu_id}/status");
    let payload = client
        .read_file(&status_path, MAX_GPU_STATUS_BYTES)
        .with_context(|| format!("read {status_path}"))?;
    let detail = format!("path={status_path}");
    audit.push_ack(AckStatus::Ok, "CAT", Some(detail.as_str()));
    let text = String::from_utf8(payload).with_context(|| format!("{status_path} is not UTF-8"))?;
    let line = text
        .lines()
        .map(str::trim)
        .rfind(|value| !value.is_empty())
        .unwrap_or("EMPTY");
    audit.push_line(format!("gpu id={gpu_id} status={line}"));
    Ok(())
}

/// Request a GPU lease via /queen/ctl.
pub fn lease<C: CohAccess>(
    client: &mut C,
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
    let priority = args.priority.unwrap_or(0);
    spawn_args.push(format!("priority={priority}"));
    if let Some(ttl) = args.budget_ttl_s {
        spawn_args.push(format!("budget_ttl_s={ttl}"));
    }
    if let Some(ops) = args.budget_ops {
        spawn_args.push(format!("budget_ops={ops}"));
    }
    let payload = queen::spawn("gpu", spawn_args.iter().map(String::as_str))?;
    let written = client.write_append(queen::queen_ctl_path(), payload.as_bytes())?;
    let detail = format!("path={} bytes={written}", queen::queen_ctl_path());
    audit.push_ack(AckStatus::Ok, "ECHO", Some(&detail));
    audit.push_line(format!(
        "lease requested gpu_id={} mem_mb={} streams={} ttl_s={}",
        args.gpu_id, args.mem_mb, args.streams, args.ttl_s
    ));
    Ok(())
}

/// Write a receipt-backed GPU lease request.
pub fn lease_with_receipt<C: CohAccess>(
    client: &mut C,
    audit: &mut CohAudit,
    args: &GpuLeaseArgs,
    receipt_out: Option<&Path>,
    bounds: &cohesix_rest::BoundsResponse,
) -> Result<()> {
    let result = lease(client, audit, args);
    let Some(receipt_path) = receipt_out else {
        return result;
    };
    let proc_lease = snapshot_proc_lease(client, bounds);
    let receipt = LeaseReceipt {
        schema: "cohesix-receipt-v1",
        kind: "gpu-lease",
        manifest_sha256: bounds.manifest_sha256.as_str(),
        request: LeaseReceiptRequest::from(args),
        status: if result.is_ok() { "ok" } else { "err" },
        error: result.as_ref().err().map(safe_error_detail),
        ack: find_ack_line(audit, "ECHO"),
        proc_lease,
    };
    match write_receipt_json(receipt_path, &receipt) {
        Ok(()) => result,
        Err(err) => match result {
            Ok(()) => Err(err),
            Err(lease_err) => Err(anyhow!("lease failed: {lease_err}; receipt failed: {err}")),
        },
    }
}

#[derive(Debug, Clone, Serialize)]
struct LeaseReceipt<'a> {
    schema: &'static str,
    kind: &'static str,
    manifest_sha256: &'a str,
    request: LeaseReceiptRequest,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ack: Option<String>,
    proc_lease: ProcLeaseSnapshot,
}

#[derive(Debug, Clone, Serialize)]
struct LeaseReceiptRequest {
    gpu_id: String,
    mem_mb: u32,
    streams: u8,
    ttl_s: u32,
    priority: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    budget_ttl_s: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    budget_ops: Option<u64>,
}

impl From<&GpuLeaseArgs> for LeaseReceiptRequest {
    fn from(args: &GpuLeaseArgs) -> Self {
        Self {
            gpu_id: args.gpu_id.clone(),
            mem_mb: args.mem_mb,
            streams: args.streams,
            ttl_s: args.ttl_s,
            priority: args.priority.unwrap_or(0),
            budget_ttl_s: args.budget_ttl_s,
            budget_ops: args.budget_ops,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct ProcLeaseSnapshot {
    summary: Option<String>,
    active: Option<String>,
    preemptions: Option<String>,
    active_entries: Vec<ProcLeaseActiveEntry>,
}

#[derive(Debug, Clone, Serialize)]
struct ProcLeaseActiveEntry {
    id: String,
    subject: String,
    resource: String,
    state: String,
    seq: u64,
}

fn snapshot_proc_lease<C: CohAccess>(
    client: &mut C,
    bounds: &cohesix_rest::BoundsResponse,
) -> ProcLeaseSnapshot {
    let proc = &bounds.observability.proc_lease;
    let summary = read_optional_text(
        client,
        "/proc/lease/summary",
        proc.summary.then_some(proc.summary_bytes as usize),
    );
    let active_text = read_optional_text(
        client,
        "/proc/lease/active",
        proc.active.then_some(proc.active_bytes as usize),
    );
    let preemptions = read_optional_text(
        client,
        "/proc/lease/preemptions",
        proc.preemptions.then_some(proc.preemptions_bytes as usize),
    );
    let active_entries = active_text
        .as_deref()
        .map(parse_proc_lease_active)
        .unwrap_or_default();
    ProcLeaseSnapshot {
        summary,
        active: active_text,
        preemptions,
        active_entries,
    }
}

fn parse_proc_lease_active(text: &str) -> Vec<ProcLeaseActiveEntry> {
    let mut out = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let fields = parse_kv_line(line);
        let seq = fields
            .get("seq")
            .and_then(|value| value.parse::<u64>().ok());
        let (Some(id), Some(subject), Some(resource), Some(state), Some(seq)) = (
            fields.get("id"),
            fields.get("subject"),
            fields.get("resource"),
            fields.get("state"),
            seq,
        ) else {
            continue;
        };
        out.push(ProcLeaseActiveEntry {
            id: (*id).to_owned(),
            subject: (*subject).to_owned(),
            resource: (*resource).to_owned(),
            state: (*state).to_owned(),
            seq,
        });
    }
    out
}

fn parse_kv_line(line: &str) -> std::collections::BTreeMap<&str, &str> {
    let mut out = std::collections::BTreeMap::new();
    for part in line.split_whitespace() {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        out.insert(key, value);
    }
    out
}

fn read_optional_text<C: CohAccess>(
    client: &mut C,
    path: &str,
    max_bytes: Option<usize>,
) -> Option<String> {
    let max_bytes = max_bytes?;
    let payload = client.read_file(path, max_bytes).ok()?;
    String::from_utf8(payload).ok()
}

fn find_ack_line(audit: &CohAudit, verb: &str) -> Option<String> {
    let ok_prefix = format!("OK {verb} ");
    let err_prefix = format!("ERR {verb} ");
    audit
        .lines()
        .iter()
        .rev()
        .find(|line| line.starts_with(&ok_prefix) || line.starts_with(&err_prefix))
        .cloned()
}

fn write_receipt_json<T: Serialize>(path: &Path, receipt: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create receipt dir {}", parent.display()))?;
    }
    let tmp = path.with_extension("partial");
    let payload = serde_json::to_vec_pretty(receipt).context("serialize receipt")?;
    fs::write(&tmp, &payload).with_context(|| format!("write receipt {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("commit receipt {}", path.display()))?;
    Ok(())
}

fn safe_error_detail(err: &anyhow::Error) -> String {
    const MAX_DETAIL: usize = 256;
    let text = err.to_string();
    if text.len() <= MAX_DETAIL {
        return text;
    }
    text[..MAX_DETAIL].to_owned()
}
