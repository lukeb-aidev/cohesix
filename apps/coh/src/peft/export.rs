// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Coh peft export helpers for LoRA jobs.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use cohsh_core::wire::AckStatus;

use crate::peft::{
    write_atomic, EXPORT_BASE_MODEL_FILE, EXPORT_POLICY_FILE, EXPORT_TELEMETRY_FILE,
};
use crate::policy::{CohPeftExportPolicy, CohPolicy};
use crate::{validate_component, CohAccess, CohAudit, MAX_DIR_LIST_BYTES};

/// PEFT export request parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeftExportSpec {
    /// Job identifier under `/queen/export/lora_jobs`.
    pub job_id: String,
    /// Output directory for exported artifacts.
    pub out_dir: PathBuf,
}

/// Summary of a PEFT export operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeftExportSummary {
    /// Files exported.
    pub files: usize,
    /// Total bytes written.
    pub bytes: usize,
}

/// Export a LoRA job directory into host storage.
pub fn export_job<C: CohAccess>(
    client: &mut C,
    policy: &CohPolicy,
    spec: &PeftExportSpec,
    audit: &mut CohAudit,
) -> Result<PeftExportSummary> {
    validate_component(&spec.job_id)?;
    enforce_id_bytes(&spec.job_id, policy.peft.activate.max_model_id_bytes)?;

    fs::create_dir_all(&spec.out_dir)
        .with_context(|| format!("create export root {}", spec.out_dir.display()))?;

    let export_root = policy.peft.export.root.as_str();
    let jobs = client.list_dir(export_root, MAX_DIR_LIST_BYTES)?;
    let detail = format!("path={export_root}");
    audit.push_ack(AckStatus::Ok, "LS", Some(detail.as_str()));
    if !jobs.iter().any(|entry| entry == &spec.job_id) {
        return Err(anyhow!("export job {} not found", spec.job_id));
    }

    let job_root = format!("{}/{}", export_root, spec.job_id);
    let entries = client.list_dir(&job_root, MAX_DIR_LIST_BYTES)?;
    let detail = format!("path={job_root}");
    audit.push_ack(AckStatus::Ok, "LS", Some(detail.as_str()));

    validate_export_entries(&entries)?;

    let mut summary = PeftExportSummary { files: 0, bytes: 0 };

    summary += export_file(
        client,
        &policy.peft.export,
        &job_root,
        EXPORT_TELEMETRY_FILE,
        ExportLimit::Telemetry,
        &spec.out_dir,
        &spec.job_id,
        audit,
    )?;
    summary += export_file(
        client,
        &policy.peft.export,
        &job_root,
        EXPORT_BASE_MODEL_FILE,
        ExportLimit::BaseModel,
        &spec.out_dir,
        &spec.job_id,
        audit,
    )?;
    summary += export_file(
        client,
        &policy.peft.export,
        &job_root,
        EXPORT_POLICY_FILE,
        ExportLimit::Policy,
        &spec.out_dir,
        &spec.job_id,
        audit,
    )?;

    Ok(summary)
}

#[derive(Clone, Copy)]
enum ExportLimit {
    Telemetry,
    BaseModel,
    Policy,
}

impl ExportLimit {
    fn max_bytes(self, policy: &CohPeftExportPolicy) -> usize {
        match self {
            ExportLimit::Telemetry => policy.max_telemetry_bytes as usize,
            ExportLimit::BaseModel => policy.max_base_model_bytes as usize,
            ExportLimit::Policy => policy.max_policy_bytes as usize,
        }
    }
}

fn export_file<C: CohAccess>(
    client: &mut C,
    policy: &CohPeftExportPolicy,
    job_root: &str,
    name: &str,
    limit: ExportLimit,
    out_dir: &Path,
    job_id: &str,
    audit: &mut CohAudit,
) -> Result<PeftExportSummary> {
    let path = format!("{job_root}/{name}");
    let payload = client.read_file(&path, limit.max_bytes(policy))?;
    let detail = format!("path={path}");
    audit.push_ack(AckStatus::Ok, "CAT", Some(detail.as_str()));
    if payload.is_empty() {
        return Err(anyhow!("export file {path} is empty"));
    }
    let output_path = out_dir.join(job_id).join(name);
    write_if_missing(&output_path, &payload)?;
    Ok(PeftExportSummary {
        files: 1,
        bytes: payload.len(),
    })
}

fn write_if_missing(path: &Path, payload: &[u8]) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    write_atomic(path, payload)
}

fn validate_export_entries(entries: &[String]) -> Result<()> {
    let expected = [
        EXPORT_TELEMETRY_FILE,
        EXPORT_BASE_MODEL_FILE,
        EXPORT_POLICY_FILE,
    ];
    for req in expected {
        if !entries.iter().any(|entry| entry == req) {
            return Err(anyhow!("export missing required file {req}"));
        }
    }
    for entry in entries {
        if !expected.contains(&entry.as_str()) {
            return Err(anyhow!("export unexpected file {entry}"));
        }
    }
    Ok(())
}

fn enforce_id_bytes(value: &str, max_bytes: u32) -> Result<()> {
    let len = value.as_bytes().len() as u32;
    if len > max_bytes {
        return Err(anyhow!("id length {len} exceeds max {max_bytes}"));
    }
    Ok(())
}

impl std::ops::AddAssign for PeftExportSummary {
    fn add_assign(&mut self, other: Self) {
        self.files = self.files.saturating_add(other.files);
        self.bytes = self.bytes.saturating_add(other.bytes);
    }
}
