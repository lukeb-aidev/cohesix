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
                id: None,
                target: None,
                subject: None,
                resource: None,
                state: None,
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
                target: entry.target,
                subject: None,
                resource: None,
                state: None,
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
        let Some(subject) = fields.get("subject") else { continue };
        let Some(resource) = fields.get("resource") else { continue };
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

