// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Coh peft import helpers for adapter staging.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use sha2::{Digest, Sha256};

use crate::peft::{
    write_atomic, EXPORT_BASE_MODEL_FILE, EXPORT_POLICY_FILE, EXPORT_TELEMETRY_FILE,
    IMPORT_ADAPTER_FILE, IMPORT_LORA_FILE, IMPORT_METRICS_FILE, REGISTRY_AVAILABLE_DIR,
};
use crate::policy::{CohPeftExportPolicy, CohPolicy};
use crate::{validate_component, CohAudit};

/// PEFT import request parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeftImportSpec {
    /// Model identifier for the adapter.
    pub model_id: String,
    /// Adapter artifacts directory.
    pub adapter_dir: PathBuf,
    /// Export root containing LoRA jobs.
    pub export_root: PathBuf,
    /// Job identifier for provenance.
    pub job_id: String,
    /// Registry root for staged adapters.
    pub registry_root: PathBuf,
}

/// Summary of a PEFT import operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeftImportSummary {
    /// Model identifier.
    pub model_id: String,
    /// Manifest path written to the registry.
    pub manifest_path: PathBuf,
    /// Adapter bytes staged.
    pub adapter_bytes: u64,
    /// LoRA metadata bytes staged.
    pub lora_bytes: u64,
    /// Metrics bytes staged, if present.
    pub metrics_bytes: Option<u64>,
}

/// Import adapter artifacts into the host registry.
pub fn import_adapter(
    policy: &CohPolicy,
    spec: &PeftImportSpec,
    _audit: &mut CohAudit,
) -> Result<PeftImportSummary> {
    validate_component(&spec.model_id)?;
    validate_component(&spec.job_id)?;
    enforce_id_bytes(&spec.model_id, policy.peft.activate.max_model_id_bytes)?;
    enforce_id_bytes(&spec.job_id, policy.peft.activate.max_model_id_bytes)?;

    let export_policy = &policy.peft.export;
    let import_policy = &policy.peft.import;

    let adapter_dir = &spec.adapter_dir;
    ensure_dir(adapter_dir, "adapter directory")?;

    let export_dir = spec.export_root.join(&spec.job_id);
    ensure_dir(&export_dir, "export job directory")?;

    let base_model_path = export_dir.join(EXPORT_BASE_MODEL_FILE);
    let base_model_ref = read_base_model(&base_model_path, export_policy)?;

    let policy_path = export_dir.join(EXPORT_POLICY_FILE);
    let policy_hash = hash_file(&policy_path, export_policy.max_policy_bytes as u64)?;

    let telemetry_path = export_dir.join(EXPORT_TELEMETRY_FILE);
    let telemetry_hash = hash_file(&telemetry_path, export_policy.max_telemetry_bytes as u64)?;

    let adapter_path = adapter_dir.join(IMPORT_ADAPTER_FILE);
    let lora_path = adapter_dir.join(IMPORT_LORA_FILE);
    if !adapter_path.is_file() {
        return Err(anyhow!("missing adapter file {}", adapter_path.display()));
    }
    if !lora_path.is_file() {
        return Err(anyhow!(
            "missing lora metadata file {}",
            lora_path.display()
        ));
    }

    let target_dir = spec
        .registry_root
        .join(REGISTRY_AVAILABLE_DIR)
        .join(&spec.model_id);
    if target_dir.join("manifest.toml").exists() {
        return Err(anyhow!("model {} already imported", spec.model_id));
    }

    fs::create_dir_all(&target_dir)
        .with_context(|| format!("create registry dir {}", target_dir.display()))?;

    let adapter_target = target_dir.join(IMPORT_ADAPTER_FILE);
    let adapter_hash = copy_with_hash(
        &adapter_path,
        &adapter_target,
        import_policy.max_adapter_bytes,
    )?;

    let lora_target = target_dir.join(IMPORT_LORA_FILE);
    let lora_hash = copy_with_hash(
        &lora_path,
        &lora_target,
        import_policy.max_lora_bytes as u64,
    )?;

    let metrics_path = adapter_dir.join(IMPORT_METRICS_FILE);
    let metrics_hash = if metrics_path.is_file() {
        let metrics_target = target_dir.join(IMPORT_METRICS_FILE);
        Some(copy_with_hash(
            &metrics_path,
            &metrics_target,
            import_policy.max_metrics_bytes as u64,
        )?)
    } else {
        None
    };

    let manifest = render_manifest(
        &spec.model_id,
        &base_model_ref,
        &spec.job_id,
        &adapter_hash,
        &lora_hash,
        metrics_hash.as_ref(),
        &policy_hash,
        &telemetry_hash,
    )?;
    if manifest.len() > import_policy.max_manifest_bytes as usize {
        return Err(anyhow!(
            "manifest bytes {} exceeds max_manifest_bytes {}",
            manifest.len(),
            import_policy.max_manifest_bytes
        ));
    }
    let manifest_path = target_dir.join("manifest.toml");
    write_atomic(&manifest_path, manifest.as_bytes())?;

    Ok(PeftImportSummary {
        model_id: spec.model_id.clone(),
        manifest_path,
        adapter_bytes: adapter_hash.bytes,
        lora_bytes: lora_hash.bytes,
        metrics_bytes: metrics_hash.map(|value| value.bytes),
    })
}

#[derive(Debug, Clone)]
struct FileHash {
    sha256: String,
    bytes: u64,
}

fn hash_file(path: &Path, max_bytes: u64) -> Result<FileHash> {
    let mut file = fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    let mut total = 0u64;
    loop {
        let read = file.read(&mut buf)?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(read as u64);
        if total > max_bytes {
            return Err(anyhow!(
                "{} exceeds max bytes {}",
                path.display(),
                max_bytes
            ));
        }
        hasher.update(&buf[..read]);
    }
    if total == 0 {
        return Err(anyhow!("{} is empty", path.display()));
    }
    Ok(FileHash {
        sha256: hex::encode(hasher.finalize()),
        bytes: total,
    })
}

fn copy_with_hash(path: &Path, dest: &Path, max_bytes: u64) -> Result<FileHash> {
    let mut input = fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create dir {}", parent.display()))?;
    }
    let tmp = dest.with_extension("partial");
    let mut output = fs::File::create(&tmp).with_context(|| format!("create {}", tmp.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    let mut total = 0u64;
    loop {
        let read = input.read(&mut buf)?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(read as u64);
        if total > max_bytes {
            return Err(anyhow!(
                "{} exceeds max bytes {}",
                path.display(),
                max_bytes
            ));
        }
        hasher.update(&buf[..read]);
        output.write_all(&buf[..read])?;
    }
    if total == 0 {
        return Err(anyhow!("{} is empty", path.display()));
    }
    output.sync_all().ok();
    fs::rename(&tmp, dest).with_context(|| format!("commit {}", dest.display()))?;
    Ok(FileHash {
        sha256: hex::encode(hasher.finalize()),
        bytes: total,
    })
}

fn read_base_model(path: &Path, policy: &CohPeftExportPolicy) -> Result<String> {
    let bytes = read_bounded(path, policy.max_base_model_bytes as usize)?;
    let text = String::from_utf8(bytes).map_err(|_| anyhow!("{} is not UTF-8", path.display()))?;
    let line = text
        .lines()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("{} is empty", path.display()))?;
    Ok(line.to_owned())
}

fn read_bounded(path: &Path, max_bytes: usize) -> Result<Vec<u8>> {
    let mut file = fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        let read = file.read(&mut tmp)?;
        if read == 0 {
            break;
        }
        if buf.len().saturating_add(read) > max_bytes {
            return Err(anyhow!(
                "{} exceeds max bytes {}",
                path.display(),
                max_bytes
            ));
        }
        buf.extend_from_slice(&tmp[..read]);
    }
    Ok(buf)
}

fn render_manifest(
    model_id: &str,
    base_model: &str,
    job_id: &str,
    adapter: &FileHash,
    lora: &FileHash,
    metrics: Option<&FileHash>,
    policy_hash: &FileHash,
    telemetry_hash: &FileHash,
) -> Result<String> {
    let mut out = String::new();
    out.push_str("[model]\n");
    out.push_str(&format!("id = \"{}\"\n", model_id));
    out.push_str(&format!("base = \"{}\"\n", base_model));
    out.push_str(&format!("adapter = \"{}\"\n", IMPORT_ADAPTER_FILE));
    out.push_str(&format!("lora = \"{}\"\n", IMPORT_LORA_FILE));
    if metrics.is_some() {
        out.push_str(&format!("metrics = \"{}\"\n", IMPORT_METRICS_FILE));
    }
    out.push_str("\n[provenance]\n");
    out.push_str(&format!("job_id = \"{}\"\n", job_id));
    out.push_str("approval = \"pending\"\n");
    out.push_str("\n[hashes]\n");
    out.push_str(&format!("adapter_sha256 = \"{}\"\n", adapter.sha256));
    out.push_str(&format!("adapter_bytes = {}\n", adapter.bytes));
    out.push_str(&format!("lora_sha256 = \"{}\"\n", lora.sha256));
    out.push_str(&format!("lora_bytes = {}\n", lora.bytes));
    if let Some(metrics) = metrics {
        out.push_str(&format!("metrics_sha256 = \"{}\"\n", metrics.sha256));
        out.push_str(&format!("metrics_bytes = {}\n", metrics.bytes));
    }
    out.push_str(&format!("policy_sha256 = \"{}\"\n", policy_hash.sha256));
    out.push_str(&format!("policy_bytes = {}\n", policy_hash.bytes));
    out.push_str(&format!(
        "telemetry_sha256 = \"{}\"\n",
        telemetry_hash.sha256
    ));
    out.push_str(&format!("telemetry_bytes = {}\n", telemetry_hash.bytes));
    Ok(out)
}

fn ensure_dir(path: &Path, label: &str) -> Result<()> {
    if !path.is_dir() {
        return Err(anyhow!("{} {} does not exist", label, path.display()));
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
