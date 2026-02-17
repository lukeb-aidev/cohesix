// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide coh telemetry pull helpers.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use cohsh_core::wire::AckStatus;
use serde::Deserialize;

use crate::policy::{CohPolicy, CohTelemetryPolicy};
use crate::{validate_component, CohAccess, CohAudit, MAX_DIR_LIST_BYTES};

const TELEMETRY_REFERENCE_SCHEMA: &str = "coh-ref-c/v1";

/// Summary of a telemetry pull operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TelemetryPullSummary {
    /// Devices processed.
    pub devices: usize,
    /// Segments downloaded.
    pub segments: usize,
    /// Total bytes downloaded.
    pub bytes: usize,
}

/// Pull telemetry segments from the queen ingest namespace into host storage.
pub fn pull<C: CohAccess>(
    client: &mut C,
    policy: &CohPolicy,
    out_dir: &Path,
    audit: &mut CohAudit,
) -> Result<TelemetryPullSummary> {
    fs::create_dir_all(out_dir)
        .with_context(|| format!("create telemetry output dir {}", out_dir.display()))?;
    let telemetry = &policy.telemetry;
    let device_entries = client.list_dir(telemetry.root.as_str(), MAX_DIR_LIST_BYTES)?;
    let detail = format!("path={}", telemetry.root);
    audit.push_ack(AckStatus::Ok, "LS", Some(detail.as_str()));
    if device_entries.len() > telemetry.max_devices as usize {
        return Err(anyhow!(
            "telemetry devices {} exceeds max_devices {}",
            device_entries.len(),
            telemetry.max_devices
        ));
    }
    let mut summary = TelemetryPullSummary {
        devices: 0,
        segments: 0,
        bytes: 0,
    };
    for device_id in device_entries {
        validate_component(&device_id)?;
        let (device_summary, device_bytes) =
            pull_device(client, telemetry, out_dir, &device_id, audit)?;
        summary.devices += 1;
        summary.segments += device_summary;
        summary.bytes += device_bytes;
    }
    if summary.devices == 0 {
        audit.push_line("telemetry: none".to_owned());
    }
    Ok(summary)
}

fn pull_device<C: CohAccess>(
    client: &mut C,
    telemetry: &CohTelemetryPolicy,
    out_dir: &Path,
    device_id: &str,
    audit: &mut CohAudit,
) -> Result<(usize, usize)> {
    let seg_root = format!("{}/{device_id}/seg", telemetry.root);
    let segments = client.list_dir(&seg_root, MAX_DIR_LIST_BYTES)?;
    let detail = format!("path={seg_root}");
    audit.push_ack(AckStatus::Ok, "LS", Some(detail.as_str()));
    if segments.len() > telemetry.max_segments_per_device as usize {
        return Err(anyhow!(
            "telemetry segments {} exceeds max_segments_per_device {} for device {}",
            segments.len(),
            telemetry.max_segments_per_device,
            device_id
        ));
    }
    let mut total_bytes = 0usize;
    let mut segment_count = 0usize;
    for seg_id in segments {
        validate_component(&seg_id)?;
        let seg_path = format!("{seg_root}/{seg_id}");
        let payload = client.read_file(&seg_path, telemetry.max_bytes_per_segment as usize)?;
        let detail = format!("path={seg_path}");
        audit.push_ack(AckStatus::Ok, "CAT", Some(detail.as_str()));
        total_bytes = total_bytes.saturating_add(payload.len());
        if total_bytes > telemetry.max_total_bytes_per_device as usize {
            return Err(anyhow!(
                "telemetry bytes {} exceeds max_total_bytes_per_device {} for device {}",
                total_bytes,
                telemetry.max_total_bytes_per_device,
                device_id
            ));
        }
        let relative = PathBuf::from(device_id).join("seg").join(&seg_id);
        let output_path = out_dir.join(&relative);
        write_segment(&output_path, &payload)?;
        if let Some(reference) = parse_reference_manifest(&payload) {
            audit.push_line(format!(
                "telemetry device={} segment={} bytes={} refs={} source_bytes={} mode=reference saved={}",
                device_id,
                seg_id,
                payload.len(),
                reference.refs,
                reference.source_bytes,
                relative.display()
            ));
        } else {
            audit.push_line(format!(
                "telemetry device={} segment={} bytes={} saved={}",
                device_id,
                seg_id,
                payload.len(),
                relative.display()
            ));
        }
        segment_count = segment_count.saturating_add(1);
    }
    Ok((segment_count, total_bytes))
}

fn write_segment(path: &Path, payload: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create telemetry dir {}", parent.display()))?;
    }
    if path.exists() {
        return Ok(());
    }
    let tmp_path = path.with_extension("partial");
    fs::write(&tmp_path, payload)
        .with_context(|| format!("write telemetry segment {}", tmp_path.display()))?;
    fs::rename(&tmp_path, path)
        .with_context(|| format!("commit telemetry segment {}", path.display()))?;
    Ok(())
}

#[derive(Debug)]
struct TelemetryReferenceSummary {
    refs: usize,
    source_bytes: u64,
}

#[derive(Debug, Deserialize)]
struct TelemetryReferenceRecord {
    schema: String,
    seq: u64,
    off: u64,
    len: u64,
    sha256: String,
}

fn parse_reference_manifest(payload: &[u8]) -> Option<TelemetryReferenceSummary> {
    let text = std::str::from_utf8(payload).ok()?;
    let mut refs = 0usize;
    let mut expected_seq = 1u64;
    let mut expected_off = 0u64;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let record: TelemetryReferenceRecord = serde_json::from_str(trimmed).ok()?;
        if record.schema != TELEMETRY_REFERENCE_SCHEMA {
            return None;
        }
        if record.seq != expected_seq || record.off != expected_off {
            return None;
        }
        if record.len == 0 || record.sha256.trim().is_empty() {
            return None;
        }
        refs = refs.saturating_add(1);
        expected_seq = expected_seq.saturating_add(1);
        expected_off = expected_off.saturating_add(record.len);
    }
    if refs == 0 {
        return None;
    }
    Some(TelemetryReferenceSummary {
        refs,
        source_bytes: expected_off,
    })
}
