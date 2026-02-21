// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Generate deterministic timelines from exported Cohesix evidence packs.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Offline timeline generator for evidence packs created by `coh evidence pack`.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const TIMELINE_SCHEMA: &str = "cohesix-evidence-pack/timeline-v1";

/// Summary of a timeline generation run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineSummary {
    /// Total events emitted.
    pub events: usize,
    /// Output NDJSON path.
    pub ndjson_path: PathBuf,
    /// Output markdown path.
    pub markdown_path: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
struct AuditJournalEntry {
    seq: u64,
    kind: String,
    path: String,
    payload: String,
    outcome: String,
    #[serde(default)]
    error: Option<String>,
    role: String,
    ticket: String,
}

#[derive(Debug, Clone, Deserialize)]
struct DecisionEntry {
    seq: u64,
    kind: String,
    outcome: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    path: Option<String>,
    role: String,
    ticket: String,
}

#[derive(Debug, Clone, Deserialize)]
struct HostTicketSpecEntry {
    id: String,
    idempotency_key: String,
    action: String,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    source_hive: Option<String>,
    #[serde(default)]
    target_hive: Option<String>,
    #[serde(default)]
    relay_hop: Option<u16>,
    #[serde(default)]
    relay_correlation_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct HostTicketResultEntry {
    id: String,
    idempotency_key: String,
    action: String,
    state: String,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    source_hive: Option<String>,
    #[serde(default)]
    target_hive: Option<String>,
    #[serde(default)]
    relay_hop: Option<u16>,
    #[serde(default)]
    relay_correlation_id: Option<String>,
}

#[derive(Debug, Clone)]
struct HostTicketIdentity {
    id: String,
    idempotency_key: String,
    action: String,
    state: Option<String>,
    target: Option<String>,
    source_hive: Option<String>,
    target_hive: Option<String>,
    relay_hop: Option<u16>,
    relay_correlation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct TimelineEvent {
    schema: &'static str,
    kind: String,
    source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    seq: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lease_seq: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    outcome: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ticket: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    idempotency_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    correlation_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ticket_action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_hive: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_hive: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    relay_hop: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    relay_correlation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    subject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resource: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ttl_s: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    priority: Option<u8>,
}

/// Generate `timeline.ndjson` and `timeline.md` in the supplied evidence pack directory.
pub fn write_timeline(pack_dir: &Path) -> Result<TimelineSummary> {
    let events = build_events(pack_dir)?;
    let ndjson_path = pack_dir.join("timeline.ndjson");
    let markdown_path = pack_dir.join("timeline.md");
    write_ndjson(&ndjson_path, &events)?;
    write_markdown(&markdown_path, &events)?;
    Ok(TimelineSummary {
        events: events.len(),
        ndjson_path,
        markdown_path,
    })
}

fn build_events(pack_dir: &Path) -> Result<Vec<TimelineEvent>> {
    let mut events = Vec::new();

    let journal_path = pack_dir.join("audit").join("journal");
    if journal_path.is_file() {
        for entry in parse_jsonl::<AuditJournalEntry>(&journal_path, "audit/journal")? {
            let ticket_identity =
                parse_ticket_identity_from_payload(entry.path.as_str(), entry.payload.as_str());
            events.push(TimelineEvent {
                schema: TIMELINE_SCHEMA,
                kind: entry.kind,
                source: "audit/journal".to_owned(),
                seq: Some(entry.seq),
                lease_seq: None,
                path: Some(entry.path),
                outcome: Some(entry.outcome),
                error: entry.error,
                role: Some(entry.role),
                ticket: Some(entry.ticket),
                payload: Some(entry.payload),
                id: ticket_identity.as_ref().map(|value| value.id.clone()),
                idempotency_key: ticket_identity
                    .as_ref()
                    .map(|value| value.idempotency_key.clone()),
                correlation_key: ticket_identity.as_ref().map(|value| {
                    ticket_correlation_key(
                        &value.id,
                        &value.idempotency_key,
                        value.source_hive.as_deref(),
                        value.target_hive.as_deref(),
                    )
                }),
                ticket_action: ticket_identity.as_ref().map(|value| value.action.clone()),
                source_hive: ticket_identity
                    .as_ref()
                    .and_then(|value| value.source_hive.clone()),
                target_hive: ticket_identity
                    .as_ref()
                    .and_then(|value| value.target_hive.clone()),
                relay_hop: ticket_identity.as_ref().and_then(|value| value.relay_hop),
                relay_correlation_id: ticket_identity
                    .as_ref()
                    .and_then(|value| value.relay_correlation_id.clone()),
                target: ticket_identity
                    .as_ref()
                    .and_then(|value| value.target.clone()),
                subject: None,
                resource: None,
                state: ticket_identity
                    .as_ref()
                    .and_then(|value| value.state.clone()),
                ttl_s: None,
                priority: None,
            });
        }
    }

    let decisions_path = pack_dir.join("audit").join("decisions");
    if decisions_path.is_file() {
        for entry in parse_jsonl::<DecisionEntry>(&decisions_path, "audit/decisions")? {
            events.push(TimelineEvent {
                schema: TIMELINE_SCHEMA,
                kind: entry.kind,
                source: "audit/decisions".to_owned(),
                seq: Some(entry.seq),
                lease_seq: None,
                path: entry.path,
                outcome: Some(entry.outcome),
                error: None,
                role: Some(entry.role),
                ticket: Some(entry.ticket),
                payload: None,
                id: entry.id,
                idempotency_key: None,
                correlation_key: None,
                ticket_action: None,
                source_hive: None,
                target_hive: None,
                relay_hop: None,
                relay_correlation_id: None,
                target: entry.target,
                subject: None,
                resource: None,
                state: None,
                ttl_s: None,
                priority: None,
            });
        }
    }

    let host_ticket_spec = pack_dir.join("host").join("tickets").join("spec");
    if host_ticket_spec.is_file() {
        for entry in parse_jsonl::<HostTicketSpecEntry>(&host_ticket_spec, "host/tickets/spec")? {
            events.push(TimelineEvent {
                schema: TIMELINE_SCHEMA,
                kind: "host-ticket.spec".to_owned(),
                source: "host/tickets/spec".to_owned(),
                seq: None,
                lease_seq: None,
                path: Some("/host/tickets/spec".to_owned()),
                outcome: None,
                error: None,
                role: None,
                ticket: None,
                payload: None,
                id: Some(entry.id.clone()),
                idempotency_key: Some(entry.idempotency_key.clone()),
                correlation_key: Some(ticket_correlation_key(
                    entry.id.as_str(),
                    entry.idempotency_key.as_str(),
                    entry.source_hive.as_deref(),
                    entry.target_hive.as_deref(),
                )),
                ticket_action: Some(entry.action),
                source_hive: entry.source_hive,
                target_hive: entry.target_hive,
                relay_hop: entry.relay_hop,
                relay_correlation_id: entry.relay_correlation_id,
                target: entry.target,
                subject: None,
                resource: None,
                state: Some("queued".to_owned()),
                ttl_s: None,
                priority: None,
            });
        }
    }

    let host_ticket_status = pack_dir.join("host").join("tickets").join("status");
    if host_ticket_status.is_file() {
        for entry in
            parse_jsonl::<HostTicketResultEntry>(&host_ticket_status, "host/tickets/status")?
        {
            events.push(TimelineEvent {
                schema: TIMELINE_SCHEMA,
                kind: "host-ticket.status".to_owned(),
                source: "host/tickets/status".to_owned(),
                seq: None,
                lease_seq: None,
                path: Some("/host/tickets/status".to_owned()),
                outcome: Some(entry.state.clone()),
                error: None,
                role: None,
                ticket: None,
                payload: entry.message,
                id: Some(entry.id.clone()),
                idempotency_key: Some(entry.idempotency_key.clone()),
                correlation_key: Some(ticket_correlation_key(
                    entry.id.as_str(),
                    entry.idempotency_key.as_str(),
                    entry.source_hive.as_deref(),
                    entry.target_hive.as_deref(),
                )),
                ticket_action: Some(entry.action),
                source_hive: entry.source_hive,
                target_hive: entry.target_hive,
                relay_hop: entry.relay_hop,
                relay_correlation_id: entry.relay_correlation_id,
                target: None,
                subject: None,
                resource: None,
                state: Some(entry.state),
                ttl_s: None,
                priority: None,
            });
        }
    }

    let host_ticket_deadletter = pack_dir.join("host").join("tickets").join("deadletter");
    if host_ticket_deadletter.is_file() {
        for entry in parse_jsonl::<HostTicketResultEntry>(
            &host_ticket_deadletter,
            "host/tickets/deadletter",
        )? {
            events.push(TimelineEvent {
                schema: TIMELINE_SCHEMA,
                kind: "host-ticket.deadletter".to_owned(),
                source: "host/tickets/deadletter".to_owned(),
                seq: None,
                lease_seq: None,
                path: Some("/host/tickets/deadletter".to_owned()),
                outcome: Some(entry.state.clone()),
                error: None,
                role: None,
                ticket: None,
                payload: entry.message,
                id: Some(entry.id.clone()),
                idempotency_key: Some(entry.idempotency_key.clone()),
                correlation_key: Some(ticket_correlation_key(
                    entry.id.as_str(),
                    entry.idempotency_key.as_str(),
                    entry.source_hive.as_deref(),
                    entry.target_hive.as_deref(),
                )),
                ticket_action: Some(entry.action),
                source_hive: entry.source_hive,
                target_hive: entry.target_hive,
                relay_hop: entry.relay_hop,
                relay_correlation_id: entry.relay_correlation_id,
                target: None,
                subject: None,
                resource: None,
                state: Some(entry.state),
                ttl_s: None,
                priority: None,
            });
        }
    }

    let lease_active = pack_dir.join("proc").join("lease").join("active");
    if lease_active.is_file() {
        let entries = parse_lease_active(&lease_active)?;
        for entry in entries {
            events.push(TimelineEvent {
                schema: TIMELINE_SCHEMA,
                kind: "lease.active".to_owned(),
                source: "proc/lease/active".to_owned(),
                seq: None,
                lease_seq: Some(entry.seq),
                path: None,
                outcome: None,
                error: None,
                role: None,
                ticket: None,
                payload: None,
                id: Some(entry.id),
                idempotency_key: None,
                correlation_key: None,
                ticket_action: None,
                source_hive: None,
                target_hive: None,
                relay_hop: None,
                relay_correlation_id: None,
                target: None,
                subject: Some(entry.subject),
                resource: Some(entry.resource),
                state: Some(entry.state),
                ttl_s: Some(entry.ttl_s),
                priority: Some(entry.priority),
            });
        }
    }

    events.sort_by(|left, right| {
        let left_seq = left.seq.unwrap_or(u64::MAX);
        let right_seq = right.seq.unwrap_or(u64::MAX);
        if left_seq != right_seq {
            return left_seq.cmp(&right_seq);
        }
        let left_lease = left.lease_seq.unwrap_or(u64::MAX);
        let right_lease = right.lease_seq.unwrap_or(u64::MAX);
        if left_lease != right_lease {
            return left_lease.cmp(&right_lease);
        }
        let left_corr = left.correlation_key.as_deref().unwrap_or("");
        let right_corr = right.correlation_key.as_deref().unwrap_or("");
        if left_corr != right_corr {
            return left_corr.cmp(right_corr);
        }
        left.kind.cmp(&right.kind)
    });

    Ok(events)
}

fn parse_jsonl<T: for<'de> Deserialize<'de>>(path: &Path, label: &str) -> Result<Vec<T>> {
    let payload =
        fs::read(path).with_context(|| format!("read {label} from {}", path.display()))?;
    let text = std::str::from_utf8(&payload)
        .with_context(|| format!("{label} is not UTF-8 (path {})", path.display()))?;
    let mut out = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parsed: T = serde_json::from_str(trimmed)
            .with_context(|| format!("{label} line {} is not valid JSON", idx + 1))?;
        out.push(parsed);
    }
    Ok(out)
}

fn parse_ticket_identity_from_payload(path: &str, payload: &str) -> Option<HostTicketIdentity> {
    if !path.starts_with("/host/tickets/") {
        return None;
    }
    let trimmed = payload.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(parsed) = serde_json::from_str::<HostTicketResultEntry>(trimmed) {
        return Some(HostTicketIdentity {
            id: parsed.id,
            idempotency_key: parsed.idempotency_key,
            action: parsed.action,
            state: Some(parsed.state),
            target: None,
            source_hive: parsed.source_hive,
            target_hive: parsed.target_hive,
            relay_hop: parsed.relay_hop,
            relay_correlation_id: parsed.relay_correlation_id,
        });
    }
    if let Ok(parsed) = serde_json::from_str::<HostTicketSpecEntry>(trimmed) {
        return Some(HostTicketIdentity {
            id: parsed.id,
            idempotency_key: parsed.idempotency_key,
            action: parsed.action,
            state: Some("queued".to_owned()),
            target: parsed.target,
            source_hive: parsed.source_hive,
            target_hive: parsed.target_hive,
            relay_hop: parsed.relay_hop,
            relay_correlation_id: parsed.relay_correlation_id,
        });
    }
    None
}

fn ticket_correlation_key(
    id: &str,
    idempotency_key: &str,
    source_hive: Option<&str>,
    target_hive: Option<&str>,
) -> String {
    match (source_hive, target_hive) {
        (Some(source_hive), Some(target_hive)) => {
            format!("{id}:{idempotency_key}:{source_hive}:{target_hive}")
        }
        _ => format!("{id}:{idempotency_key}"),
    }
}

#[derive(Debug, Clone)]
struct LeaseActiveEntry {
    id: String,
    subject: String,
    resource: String,
    ttl_s: u32,
    priority: u8,
    state: String,
    seq: u64,
}

fn parse_lease_active(path: &Path) -> Result<Vec<LeaseActiveEntry>> {
    let payload =
        fs::read(path).with_context(|| format!("read proc/lease/active {}", path.display()))?;
    let text = std::str::from_utf8(&payload)
        .with_context(|| format!("proc/lease/active is not UTF-8 ({})", path.display()))?;
    let mut out = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let fields = parse_kv_line(line);
        let Some(id) = fields.get("id") else { continue };
        let Some(subject) = fields.get("subject") else {
            continue;
        };
        let Some(resource) = fields.get("resource") else {
            continue;
        };
        let ttl_s = fields
            .get("ttl_s")
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(0);
        let priority = fields
            .get("priority")
            .and_then(|value| value.parse::<u8>().ok())
            .unwrap_or(0);
        let state = fields.get("state").cloned().unwrap_or_default();
        let seq = fields
            .get("seq")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        out.push(LeaseActiveEntry {
            id: id.to_owned(),
            subject: subject.to_owned(),
            resource: resource.to_owned(),
            ttl_s,
            priority,
            state,
            seq,
        });
    }
    Ok(out)
}

fn parse_kv_line(line: &str) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    for part in line.split_whitespace() {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        out.insert(key.to_owned(), value.to_owned());
    }
    out
}

fn write_ndjson(path: &Path, events: &[TimelineEvent]) -> Result<()> {
    let mut out = String::new();
    for event in events {
        let line = serde_json::to_string(event).context("serialize timeline event")?;
        out.push_str(&line);
        out.push('\n');
    }
    write_atomic(path, out.as_bytes())
}

fn write_markdown(path: &Path, events: &[TimelineEvent]) -> Result<()> {
    let mut out = String::new();
    out.push_str("# Evidence timeline\n\n");
    out.push_str(&format!("events: {}\n\n", events.len()));
    for event in events {
        match (event.seq, event.lease_seq.as_ref()) {
            (Some(seq), _) => {
                out.push_str(&format!(
                    "- seq={} kind={} source={} outcome={} path={}\n",
                    seq,
                    event.kind,
                    event.source,
                    event.outcome.as_deref().unwrap_or(""),
                    event.path.as_deref().unwrap_or("")
                ));
            }
            (None, Some(lease_seq)) => {
                out.push_str(&format!(
                    "- lease_seq={} id={} subject={} resource={} state={}\n",
                    lease_seq,
                    event.id.as_deref().unwrap_or(""),
                    event.subject.as_deref().unwrap_or(""),
                    event.resource.as_deref().unwrap_or(""),
                    event.state.as_deref().unwrap_or("")
                ));
            }
            _ if event.correlation_key.is_some() => {
                out.push_str(&format!(
                    "- ticket={} action={} state={} source_hive={} target_hive={} stream={} target={} relay_hop={}\n",
                    event.correlation_key.as_deref().unwrap_or(""),
                    event.ticket_action.as_deref().unwrap_or(""),
                    event.state.as_deref().unwrap_or(""),
                    event.source_hive.as_deref().unwrap_or(""),
                    event.target_hive.as_deref().unwrap_or(""),
                    event.source,
                    event.target.as_deref().unwrap_or(""),
                    event
                        .relay_hop
                        .map(|value| value.to_string())
                        .unwrap_or_default()
                ));
            }
            _ => {}
        }
    }
    write_atomic(path, out.as_bytes())
}

fn write_atomic(path: &Path, payload: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create timeline dir {}", parent.display()))?;
    }
    let tmp = path.with_extension("partial");
    fs::write(&tmp, payload).with_context(|| format!("write {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("commit {}", path.display()))?;
    Ok(())
}
