// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Export deterministic evidence packs sourced from existing Cohesix surfaces.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Evidence pack exporter used for audits, due diligence, and incident review.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use cohsh_core::wire::AckStatus;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::policy::CohPolicy;
use crate::telemetry;
use crate::{CohAccess, CohAudit};

const EVIDENCE_META_SCHEMA: &str = "cohesix-evidence-pack/meta-v1";
const EVIDENCE_SUMMARY_SCHEMA: &str = "cohesix-evidence-pack/summary-v1";
const EVIDENCE_REDACTION_TICKET: &str = "sha256";

const DEFAULT_AUDIT_EXPORT_MAX_BYTES: usize = 1024;
const DEFAULT_AUDIT_FALLBACK_MAX_BYTES: usize = 16 * 1024;
const DEFAULT_REPLAY_STATUS_MAX_BYTES: usize = 1024;
const DEFAULT_PROC_BOOT_MAX_BYTES: usize = 64 * 1024;
const DEFAULT_LOG_MAX_BYTES: usize = 128 * 1024;
const DEFAULT_HOST_TICKET_MAX_BYTES: usize = 128 * 1024;
const REDACTED_VALUE: &str = "<redacted>";

/// Specification for exporting an evidence pack.
#[derive(Debug, Clone)]
pub struct EvidencePackSpec {
    /// Output directory to create/populate.
    pub out_dir: PathBuf,
    /// Include telemetry downloads under `telemetry/`.
    pub with_telemetry: bool,
}

/// Summary of an evidence pack export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidencePackSummary {
    /// Total files captured into the evidence pack.
    pub captured: usize,
    /// Total files missing (disabled or absent).
    pub missing: usize,
    /// Total capture errors encountered.
    pub errors: usize,
}

#[derive(Debug, Clone, Serialize)]
struct EvidenceMeta {
    schema: &'static str,
    manifest_sha256: &'static str,
    policy_sha256: &'static str,
    redaction_ticket: &'static str,
    with_telemetry: bool,
}

#[derive(Debug, Clone, Serialize)]
struct EvidenceSummary {
    schema: &'static str,
    captured: usize,
    missing: usize,
    errors: usize,
    items: Vec<EvidenceItem>,
}

#[derive(Debug, Clone, Serialize)]
struct EvidenceItem {
    /// Remote path under the Cohesix namespace.
    path: String,
    /// Relative path inside the evidence pack directory.
    saved_as: String,
    /// Operation used when capturing (`CAT` or `TAIL`).
    verb: String,
    /// `captured`, `missing`, or `error`.
    status: String,
    /// Byte size of the saved payload when captured.
    bytes: Option<usize>,
    /// Optional detail for missing/error records (never contains secrets).
    detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptureVerb {
    Cat,
    Tail,
}

impl CaptureVerb {
    fn as_str(self) -> &'static str {
        match self {
            Self::Cat => "CAT",
            Self::Tail => "TAIL",
        }
    }
}

/// Export an evidence pack into the supplied output directory.
pub fn export_pack<C: CohAccess>(
    client: &mut C,
    policy: &CohPolicy,
    bounds: &cohesix_rest::BoundsResponse,
    spec: &EvidencePackSpec,
    audit: &mut CohAudit,
) -> Result<EvidencePackSummary> {
    fs::create_dir_all(&spec.out_dir)
        .with_context(|| format!("create evidence pack output dir {}", spec.out_dir.display()))?;

    let meta = EvidenceMeta {
        schema: EVIDENCE_META_SCHEMA,
        manifest_sha256: CohPolicy::manifest_hash(),
        policy_sha256: CohPolicy::policy_hash(),
        redaction_ticket: EVIDENCE_REDACTION_TICKET,
        with_telemetry: spec.with_telemetry,
    };
    write_json(&spec.out_dir.join("meta.json"), &meta)?;

    write_json(&spec.out_dir.join("bounds.json"), bounds)?;

    let mut items = Vec::<EvidenceItem>::new();

    capture_file(
        client,
        &spec.out_dir,
        "/proc/boot",
        CaptureVerb::Cat,
        DEFAULT_PROC_BOOT_MAX_BYTES,
        audit,
        &mut items,
        None,
    )?;

    capture_proc_schedule(client, bounds, spec, audit, &mut items)?;
    capture_proc_lease(client, bounds, spec, audit, &mut items)?;

    let log_path = bounds.paths.log.as_str();
    capture_file(
        client,
        &spec.out_dir,
        log_path,
        CaptureVerb::Tail,
        DEFAULT_LOG_MAX_BYTES,
        audit,
        &mut items,
        None,
    )?;

    capture_audit(client, spec, audit, &mut items)?;
    capture_host_tickets(client, spec, audit, &mut items)?;
    capture_file(
        client,
        &spec.out_dir,
        "/replay/status",
        CaptureVerb::Cat,
        DEFAULT_REPLAY_STATUS_MAX_BYTES,
        audit,
        &mut items,
        None,
    )?;

    if spec.with_telemetry {
        let telemetry_dir = spec.out_dir.join("telemetry");
        let pull_summary = telemetry::pull(client, policy, &telemetry_dir, audit);
        match pull_summary {
            Ok(summary) => {
                audit.push_line(format!(
                    "evidence telemetry devices={} segments={} bytes={} saved=telemetry/",
                    summary.devices, summary.segments, summary.bytes
                ));
            }
            Err(err) => {
                items.push(EvidenceItem {
                    path: "/queen/telemetry".to_owned(),
                    saved_as: "telemetry/".to_owned(),
                    verb: "PULL".to_owned(),
                    status: "error".to_owned(),
                    bytes: None,
                    detail: Some(safe_detail(&err)),
                });
            }
        }
    }

    items.sort_by(|left, right| left.saved_as.cmp(&right.saved_as));

    let mut summary = EvidencePackSummary {
        captured: 0,
        missing: 0,
        errors: 0,
    };
    for item in &items {
        match item.status.as_str() {
            "captured" => summary.captured += 1,
            "missing" => summary.missing += 1,
            "error" => summary.errors += 1,
            _ => {}
        }
    }

    let summary_json = EvidenceSummary {
        schema: EVIDENCE_SUMMARY_SCHEMA,
        captured: summary.captured,
        missing: summary.missing,
        errors: summary.errors,
        items,
    };
    write_json(&spec.out_dir.join("summary.json"), &summary_json)?;

    audit.push_line(format!(
        "evidence pack saved={} captured={} missing={} errors={}",
        spec.out_dir.display(),
        summary.captured,
        summary.missing,
        summary.errors
    ));

    Ok(summary)
}

fn capture_proc_schedule<C: CohAccess>(
    client: &mut C,
    bounds: &cohesix_rest::BoundsResponse,
    spec: &EvidencePackSpec,
    audit: &mut CohAudit,
    items: &mut Vec<EvidenceItem>,
) -> Result<()> {
    let schedule = &bounds.observability.proc_schedule;
    if schedule.summary {
        capture_file(
            client,
            &spec.out_dir,
            "/proc/schedule/summary",
            CaptureVerb::Cat,
            schedule.summary_bytes as usize,
            audit,
            items,
            None,
        )?;
    } else {
        items.push(missing_item(
            "/proc/schedule/summary",
            "proc/schedule/summary",
        ));
    }
    if schedule.queue {
        capture_file(
            client,
            &spec.out_dir,
            "/proc/schedule/queue",
            CaptureVerb::Cat,
            schedule.queue_bytes as usize,
            audit,
            items,
            None,
        )?;
    } else {
        items.push(missing_item("/proc/schedule/queue", "proc/schedule/queue"));
    }
    Ok(())
}

fn capture_proc_lease<C: CohAccess>(
    client: &mut C,
    bounds: &cohesix_rest::BoundsResponse,
    spec: &EvidencePackSpec,
    audit: &mut CohAudit,
    items: &mut Vec<EvidenceItem>,
) -> Result<()> {
    let lease = &bounds.observability.proc_lease;
    if lease.summary {
        capture_file(
            client,
            &spec.out_dir,
            "/proc/lease/summary",
            CaptureVerb::Cat,
            lease.summary_bytes as usize,
            audit,
            items,
            None,
        )?;
    } else {
        items.push(missing_item("/proc/lease/summary", "proc/lease/summary"));
    }
    if lease.active {
        capture_file(
            client,
            &spec.out_dir,
            "/proc/lease/active",
            CaptureVerb::Cat,
            lease.active_bytes as usize,
            audit,
            items,
            None,
        )?;
    } else {
        items.push(missing_item("/proc/lease/active", "proc/lease/active"));
    }
    if lease.preemptions {
        capture_file(
            client,
            &spec.out_dir,
            "/proc/lease/preemptions",
            CaptureVerb::Cat,
            lease.preemptions_bytes as usize,
            audit,
            items,
            None,
        )?;
    } else {
        items.push(missing_item(
            "/proc/lease/preemptions",
            "proc/lease/preemptions",
        ));
    }
    Ok(())
}

fn capture_audit<C: CohAccess>(
    client: &mut C,
    spec: &EvidencePackSpec,
    audit: &mut CohAudit,
    items: &mut Vec<EvidenceItem>,
) -> Result<()> {
    let export_bytes = match read_optional(
        client,
        "/audit/export",
        DEFAULT_AUDIT_EXPORT_MAX_BYTES,
        CaptureVerb::Cat,
        audit,
        items,
    )? {
        Some(payload) => payload,
        None => return Ok(()),
    };
    write_payload(&spec.out_dir, "/audit/export", &export_bytes)?;

    let (journal_max, decisions_max) = parse_audit_export_bounds(&export_bytes).unwrap_or((
        DEFAULT_AUDIT_FALLBACK_MAX_BYTES,
        DEFAULT_AUDIT_FALLBACK_MAX_BYTES,
    ));

    if let Some(payload) = read_optional(
        client,
        "/audit/journal",
        journal_max,
        CaptureVerb::Cat,
        audit,
        items,
    )? {
        let redacted = redact_ticket_json_lines(&payload)?;
        write_payload(&spec.out_dir, "/audit/journal", &redacted)?;
    }

    if let Some(payload) = read_optional(
        client,
        "/audit/decisions",
        decisions_max,
        CaptureVerb::Cat,
        audit,
        items,
    )? {
        let redacted = redact_ticket_json_lines(&payload)?;
        write_payload(&spec.out_dir, "/audit/decisions", &redacted)?;
    }

    Ok(())
}

fn capture_host_tickets<C: CohAccess>(
    client: &mut C,
    spec: &EvidencePackSpec,
    audit: &mut CohAudit,
    items: &mut Vec<EvidenceItem>,
) -> Result<()> {
    for path in [
        "/host/tickets/spec",
        "/host/tickets/status",
        "/host/tickets/deadletter",
    ] {
        if let Some(payload) = read_optional(
            client,
            path,
            DEFAULT_HOST_TICKET_MAX_BYTES,
            CaptureVerb::Cat,
            audit,
            items,
        )? {
            let redacted = redact_host_ticket_json_lines(&payload)?;
            write_payload(&spec.out_dir, path, &redacted)?;
        }
    }
    Ok(())
}

fn redact_ticket_json_lines(payload: &[u8]) -> Result<Vec<u8>> {
    let text = std::str::from_utf8(payload).context("audit payload must be UTF-8")?;
    let mut out = String::new();
    for (idx, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut value: serde_json::Value = serde_json::from_str(trimmed).with_context(|| {
            format!(
                "audit JSONL line {} is not valid JSON; refusing to export unsanitised payload",
                idx + 1
            )
        })?;
        if let Some(ticket) = value.get_mut("ticket") {
            if let Some(ticket_str) = ticket.as_str() {
                if ticket_str != "none" {
                    *ticket = serde_json::Value::String(hash_ticket(ticket_str));
                }
            }
        }
        redact_sensitive_value(&mut value);
        let encoded =
            serde_json::to_string(&value).context("serialize redacted audit JSON line")?;
        out.push_str(&encoded);
        out.push('\n');
    }
    Ok(out.into_bytes())
}

fn redact_host_ticket_json_lines(payload: &[u8]) -> Result<Vec<u8>> {
    let text = std::str::from_utf8(payload).context("host ticket payload must be UTF-8")?;
    let mut out = String::new();
    for (idx, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut value: serde_json::Value = serde_json::from_str(trimmed).with_context(|| {
            format!(
                "host ticket JSONL line {} is not valid JSON; refusing unsanitised export",
                idx + 1
            )
        })?;
        redact_sensitive_value(&mut value);
        let encoded =
            serde_json::to_string(&value).context("serialize redacted host ticket JSON line")?;
        out.push_str(&encoded);
        out.push('\n');
    }
    Ok(out.into_bytes())
}

fn redact_sensitive_value(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, nested) in map.iter_mut() {
                if sensitive_key(key.as_str()) {
                    *nested = serde_json::Value::String(REDACTED_VALUE.to_owned());
                } else {
                    redact_sensitive_value(nested);
                }
            }
        }
        serde_json::Value::Array(list) => {
            for nested in list {
                redact_sensitive_value(nested);
            }
        }
        _ => {}
    }
}

fn sensitive_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower.contains("token")
        || lower.contains("authorization")
        || lower.contains("auth_token")
        || lower == "auth_ref"
        || lower == "auth"
        || lower.contains("secret")
        || lower.contains("password")
        || lower.contains("signing_key")
        || lower.contains("api_key")
}

fn hash_ticket(ticket: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(ticket.as_bytes());
    let digest = hasher.finalize();
    format!("sha256:{}", hex::encode(digest))
}

fn parse_audit_export_bounds(payload: &[u8]) -> Option<(usize, usize)> {
    let text = std::str::from_utf8(payload).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(text.trim()).ok()?;
    let journal_base = parsed.get("journal_base")?.as_u64()?;
    let journal_next = parsed.get("journal_next")?.as_u64()?;
    let decisions_base = parsed.get("decisions_base")?.as_u64()?;
    let decisions_next = parsed.get("decisions_next")?.as_u64()?;
    let journal_window = journal_next.saturating_sub(journal_base);
    let decisions_window = decisions_next.saturating_sub(decisions_base);
    Some((
        usize::try_from(journal_window).ok()?,
        usize::try_from(decisions_window).ok()?,
    ))
}

#[allow(clippy::too_many_arguments)]
fn capture_file<C: CohAccess>(
    client: &mut C,
    out_dir: &Path,
    path: &str,
    verb: CaptureVerb,
    max_bytes: usize,
    audit: &mut CohAudit,
    items: &mut Vec<EvidenceItem>,
    override_saved_as: Option<&str>,
) -> Result<()> {
    match verb {
        CaptureVerb::Cat => match client.read_file(path, max_bytes) {
            Ok(payload) => {
                audit.push_ack(AckStatus::Ok, "CAT", Some(format!("path={path}").as_str()));
                write_payload(out_dir, path, &payload)?;
                items.push(EvidenceItem {
                    path: path.to_owned(),
                    saved_as: override_saved_as
                        .unwrap_or_else(|| strip_leading_slash(path))
                        .to_owned(),
                    verb: verb.as_str().to_owned(),
                    status: "captured".to_owned(),
                    bytes: Some(payload.len()),
                    detail: None,
                });
                Ok(())
            }
            Err(err) if is_missing(&err) => {
                items.push(missing_item(
                    path,
                    override_saved_as.unwrap_or(strip_leading_slash(path)),
                ));
                Ok(())
            }
            Err(err) => {
                items.push(error_item(
                    path,
                    override_saved_as.unwrap_or(strip_leading_slash(path)),
                    verb,
                    &err,
                ));
                Err(err)
            }
        },
        CaptureVerb::Tail => match client.tail_file(path, max_bytes) {
            Ok(payload) => {
                audit.push_ack(AckStatus::Ok, "TAIL", Some(format!("path={path}").as_str()));
                write_payload(out_dir, path, &payload)?;
                items.push(EvidenceItem {
                    path: path.to_owned(),
                    saved_as: override_saved_as
                        .unwrap_or_else(|| strip_leading_slash(path))
                        .to_owned(),
                    verb: verb.as_str().to_owned(),
                    status: "captured".to_owned(),
                    bytes: Some(payload.len()),
                    detail: None,
                });
                Ok(())
            }
            Err(err) if is_missing(&err) => {
                items.push(missing_item(
                    path,
                    override_saved_as.unwrap_or(strip_leading_slash(path)),
                ));
                Ok(())
            }
            Err(err) => {
                items.push(error_item(
                    path,
                    override_saved_as.unwrap_or(strip_leading_slash(path)),
                    verb,
                    &err,
                ));
                Err(err)
            }
        },
    }
}

fn read_optional<C: CohAccess>(
    client: &mut C,
    path: &str,
    max_bytes: usize,
    verb: CaptureVerb,
    audit: &mut CohAudit,
    items: &mut Vec<EvidenceItem>,
) -> Result<Option<Vec<u8>>> {
    let payload = match verb {
        CaptureVerb::Cat => client.read_file(path, max_bytes),
        CaptureVerb::Tail => client.tail_file(path, max_bytes),
    };
    match payload {
        Ok(payload) => {
            audit.push_ack(
                AckStatus::Ok,
                verb.as_str(),
                Some(format!("path={path}").as_str()),
            );
            items.push(EvidenceItem {
                path: path.to_owned(),
                saved_as: strip_leading_slash(path).to_owned(),
                verb: verb.as_str().to_owned(),
                status: "captured".to_owned(),
                bytes: Some(payload.len()),
                detail: None,
            });
            Ok(Some(payload))
        }
        Err(err) if is_missing(&err) => {
            items.push(missing_item(path, strip_leading_slash(path)));
            Ok(None)
        }
        Err(err) => {
            items.push(error_item(path, strip_leading_slash(path), verb, &err));
            Ok(None)
        }
    }
}

fn write_payload(out_dir: &Path, remote_path: &str, payload: &[u8]) -> Result<()> {
    let relative = strip_leading_slash(remote_path);
    let path = out_dir.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create evidence pack dir {}", parent.display()))?;
    }
    let tmp_path = path.with_extension("partial");
    fs::write(&tmp_path, payload)
        .with_context(|| format!("write evidence payload {}", tmp_path.display()))?;
    fs::rename(&tmp_path, &path)
        .with_context(|| format!("commit evidence payload {}", path.display()))?;
    Ok(())
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create evidence pack dir {}", parent.display()))?;
    }
    let tmp_path = path.with_extension("partial");
    let payload = serde_json::to_vec_pretty(value).context("serialize evidence json")?;
    fs::write(&tmp_path, &payload)
        .with_context(|| format!("write evidence json {}", tmp_path.display()))?;
    fs::rename(&tmp_path, path)
        .with_context(|| format!("commit evidence json {}", path.display()))?;
    Ok(())
}

fn strip_leading_slash(path: &str) -> &str {
    path.strip_prefix('/').unwrap_or(path)
}

fn missing_item(path: &str, saved_as: &str) -> EvidenceItem {
    EvidenceItem {
        path: path.to_owned(),
        saved_as: saved_as.to_owned(),
        verb: "CAT".to_owned(),
        status: "missing".to_owned(),
        bytes: None,
        detail: Some("not-found".to_owned()),
    }
}

fn error_item(path: &str, saved_as: &str, verb: CaptureVerb, err: &anyhow::Error) -> EvidenceItem {
    EvidenceItem {
        path: path.to_owned(),
        saved_as: saved_as.to_owned(),
        verb: verb.as_str().to_owned(),
        status: "error".to_owned(),
        bytes: None,
        detail: Some(safe_detail(err)),
    }
}

fn is_missing(err: &anyhow::Error) -> bool {
    for cause in err.chain() {
        let msg = cause.to_string();
        if msg.contains("NotFound")
            || msg.contains("not found")
            || msg.contains("disabled")
            || msg.contains("does not exist")
            || msg.contains("404")
        {
            return true;
        }
    }
    false
}

fn safe_detail(err: &anyhow::Error) -> String {
    // Avoid leaking any sensitive payloads embedded in error strings by truncating
    // to a conservative bound.
    const MAX_DETAIL: usize = 256;
    let text = err.to_string();
    if text.len() <= MAX_DETAIL {
        return text;
    }
    text[..MAX_DETAIL].to_owned()
}

/// Build a bounds snapshot matching `GET /v1/meta/bounds` for non-REST evidence exports.
#[must_use]
pub fn build_local_bounds() -> cohesix_rest::BoundsResponse {
    cohesix_rest::BoundsResponse {
        manifest_sha256: CohPolicy::manifest_hash().to_owned(),
        secure9p: cohesix_rest::Secure9pBounds {
            msize: cohsh::SECURE9P_MSIZE,
            walk_depth: cohsh::SECURE9P_WALK_DEPTH,
        },
        console: cohesix_rest::ConsoleBounds {
            max_line_len: cohsh_core::MAX_LINE_LEN,
            max_path_len: cohsh_core::MAX_PATH_LEN,
            max_json_len: cohsh_core::MAX_JSON_LEN,
            max_id_len: cohsh_core::MAX_ID_LEN,
            max_echo_len: cohsh_core::MAX_ECHO_LEN,
            max_ticket_len: cohsh_core::MAX_TICKET_LEN,
        },
        paths: cohesix_rest::PathBounds {
            queen_ctl: cohsh::CLIENT_QUEEN_CTL_PATH.to_owned(),
            queen_lifecycle_ctl: cohsh::CLIENT_QUEEN_LIFECYCLE_CTL_PATH.to_owned(),
            queen_schedule_ctl: cohsh::CLIENT_QUEEN_SCHEDULE_CTL_PATH.to_owned(),
            queen_lease_ctl: cohsh::CLIENT_QUEEN_LEASE_CTL_PATH.to_owned(),
            queen_export_ctl: cohsh::CLIENT_QUEEN_EXPORT_CTL_PATH.to_owned(),
            policy_ctl: cohsh::CLIENT_POLICY_CTL_PATH.to_owned(),
            log: cohsh::CLIENT_LOG_PATH.to_owned(),
        },
        control_plane: cohesix_rest::ControlPlaneBounds {
            schedule: cohesix_rest::ScheduleBounds {
                enable: cohsh::CONTROL_SCHEDULE_ENABLED,
                queue_max_entries: cohsh::CONTROL_SCHEDULE_QUEUE_MAX_ENTRIES,
                ctl_max_bytes: cohsh::CONTROL_SCHEDULE_CTL_MAX_BYTES,
            },
            lease: cohesix_rest::LeaseBounds {
                enable: cohsh::CONTROL_LEASE_ENABLED,
                active_max_entries: cohsh::CONTROL_LEASE_ACTIVE_MAX_ENTRIES,
                preemptions_max_entries: cohsh::CONTROL_LEASE_PREEMPTIONS_MAX_ENTRIES,
                ctl_max_bytes: cohsh::CONTROL_LEASE_CTL_MAX_BYTES,
            },
            export: cohesix_rest::ExportBounds {
                enable: cohsh::CONTROL_EXPORT_ENABLED,
                ctl_max_bytes: cohsh::CONTROL_EXPORT_CTL_MAX_BYTES,
            },
        },
        policy: cohesix_rest::PolicyBounds {
            enable: cohsh::POLICY_ENABLED,
            queue_max_entries: cohsh::POLICY_QUEUE_MAX_ENTRIES,
            queue_max_bytes: cohsh::POLICY_QUEUE_MAX_BYTES,
            ctl_max_bytes: cohsh::POLICY_CTL_MAX_BYTES,
        },
        observability: cohesix_rest::ObservabilityBounds {
            proc_schedule: cohesix_rest::ProcScheduleBounds {
                summary: cohsh::PROC_SCHEDULE_SUMMARY_ENABLED,
                queue: cohsh::PROC_SCHEDULE_QUEUE_ENABLED,
                summary_bytes: cohsh::PROC_SCHEDULE_SUMMARY_BYTES,
                queue_bytes: cohsh::PROC_SCHEDULE_QUEUE_BYTES,
            },
            proc_lease: cohesix_rest::ProcLeaseBounds {
                summary: cohsh::PROC_LEASE_SUMMARY_ENABLED,
                active: cohsh::PROC_LEASE_ACTIVE_ENABLED,
                preemptions: cohsh::PROC_LEASE_PREEMPTIONS_ENABLED,
                summary_bytes: cohsh::PROC_LEASE_SUMMARY_BYTES,
                active_bytes: cohsh::PROC_LEASE_ACTIVE_BYTES,
                preemptions_bytes: cohsh::PROC_LEASE_PREEMPTIONS_BYTES,
            },
        },
    }
}
