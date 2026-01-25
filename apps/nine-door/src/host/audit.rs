// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Maintain AuditFS journal/decisions state and replayable control entries.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::VecDeque;

use serde::Serialize;
use secure9p_codec::ErrorCode;
use secure9p_core::append_only_write_bounds;

use crate::NineDoorError;

/// Replay configuration derived from the manifest.
#[derive(Debug, Clone, Copy)]
pub struct ReplayConfig {
    enabled: bool,
    max_entries: usize,
    ctl_max_bytes: usize,
    status_max_bytes: usize,
}

impl ReplayConfig {
    /// Construct a disabled replay configuration.
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            max_entries: 0,
            ctl_max_bytes: 0,
            status_max_bytes: 0,
        }
    }

    /// Construct an enabled replay configuration with explicit limits.
    pub fn enabled(max_entries: usize, ctl_max_bytes: usize, status_max_bytes: usize) -> Self {
        Self {
            enabled: true,
            max_entries,
            ctl_max_bytes,
            status_max_bytes,
        }
    }

    pub(crate) fn enabled_flag(&self) -> bool {
        self.enabled
    }

    pub(crate) fn max_entries(&self) -> usize {
        self.max_entries
    }

    pub(crate) fn ctl_max_bytes(&self) -> usize {
        self.ctl_max_bytes
    }

    pub(crate) fn status_max_bytes(&self) -> usize {
        self.status_max_bytes
    }
}

/// Manifest-derived audit configuration.
#[derive(Debug, Clone)]
pub struct AuditConfig {
    enabled: bool,
    limits: AuditLimits,
    replay: ReplayConfig,
}

impl AuditConfig {
    /// Construct a disabled audit configuration.
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            limits: AuditLimits::default(),
            replay: ReplayConfig::disabled(),
        }
    }

    /// Construct an enabled audit configuration with explicit limits.
    pub fn enabled(limits: AuditLimits, replay: ReplayConfig) -> Self {
        Self {
            enabled: true,
            limits,
            replay,
        }
    }

    pub(crate) fn enabled_flag(&self) -> bool {
        self.enabled
    }

    pub(crate) fn limits(&self) -> AuditLimits {
        self.limits
    }

    pub(crate) fn replay(&self) -> ReplayConfig {
        self.replay
    }
}

/// Storage bounds for AuditFS.
#[derive(Debug, Clone, Copy)]
pub struct AuditLimits {
    /// Maximum journal bytes retained.
    pub journal_max_bytes: usize,
    /// Maximum decision log bytes retained.
    pub decisions_max_bytes: usize,
}

impl Default for AuditLimits {
    fn default() -> Self {
        Self {
            journal_max_bytes: 8192,
            decisions_max_bytes: 4096,
        }
    }
}

/// Audit log storage and replayable action inventory.
#[derive(Debug)]
pub(crate) struct AuditStore {
    config: AuditConfig,
    journal: BoundedLog,
    decisions: BoundedLog,
    replay_entries: VecDeque<ReplayEntry>,
    sequence: u64,
    export_snapshot: Vec<u8>,
}

impl AuditStore {
    pub fn new(config: AuditConfig) -> Self {
        let journal = BoundedLog::new(config.limits().journal_max_bytes);
        let decisions = BoundedLog::new(config.limits().decisions_max_bytes);
        let mut store = Self {
            config,
            journal,
            decisions,
            replay_entries: VecDeque::new(),
            sequence: 0,
            export_snapshot: Vec::new(),
        };
        store.refresh_export_snapshot();
        store
    }

    pub fn enabled(&self) -> bool {
        self.config.enabled_flag()
    }

    pub fn replay_config(&self) -> ReplayConfig {
        self.config.replay()
    }

    #[allow(dead_code)]
    pub fn journal_bounds(&self) -> LogBounds {
        self.journal.bounds()
    }

    #[allow(dead_code)]
    pub fn decisions_bounds(&self) -> LogBounds {
        self.decisions.bounds()
    }

    pub fn journal_payload(&self) -> Vec<u8> {
        self.journal.snapshot()
    }

    pub fn decisions_payload(&self) -> Vec<u8> {
        self.decisions.snapshot()
    }

    pub fn export_snapshot(&self) -> &[u8] {
        &self.export_snapshot
    }

    pub fn append_manual_journal(
        &mut self,
        offset: u64,
        data: &[u8],
    ) -> Result<AuditAppendOutcome, NineDoorError> {
        ensure_json_lines(data, "audit journal")?;
        let append = self.append_journal(offset, data, None)?;
        Ok(append)
    }

    pub fn record_control(
        &mut self,
        path: &str,
        payload: &[u8],
        outcome: ControlOutcome,
        role: Option<&str>,
        ticket: Option<&str>,
    ) -> Result<AuditAppendOutcome, NineDoorError> {
        let kind = if path == "/queen/ctl" {
            "queen-ctl"
        } else if path == "/queen/lifecycle/ctl" {
            "queen-lifecycle"
        } else {
            "host-control"
        };
        let payload_text = String::from_utf8_lossy(payload);
        let entry = AuditJournalEntry {
            seq: self.next_sequence(),
            kind,
            path,
            payload: payload_text.as_ref(),
            outcome: outcome.status_label(),
            error: outcome.error_detail(),
            role: role.unwrap_or("none"),
            ticket: ticket.unwrap_or("none"),
        };
        let bytes = encode_json_line(&entry)?;
        let replay_entry = Some(ReplayEntry::new(
            bytes.len() as u64,
            outcome.ack_line(),
        ));
        let append = self.append_journal(u64::MAX, &bytes, replay_entry)?;
        Ok(append)
    }

    pub fn record_decision_action(
        &mut self,
        action: &PolicyActionDecision<'_>,
        role: Option<&str>,
        ticket: Option<&str>,
    ) -> Result<AuditAppendOutcome, NineDoorError> {
        let entry = DecisionEntry {
            seq: self.next_sequence(),
            kind: "policy-action",
            outcome: action.decision,
            id: Some(action.id),
            target: Some(action.target),
            path: None,
            role: role.unwrap_or("none"),
            ticket: ticket.unwrap_or("none"),
        };
        let bytes = encode_json_line(&entry)?;
        self.append_decisions(&bytes)
    }

    pub fn record_decision_gate(
        &mut self,
        decision: &PolicyGateDecision<'_>,
        role: Option<&str>,
        ticket: Option<&str>,
    ) -> Result<AuditAppendOutcome, NineDoorError> {
        let entry = DecisionEntry {
            seq: self.next_sequence(),
            kind: "policy-gate",
            outcome: decision.outcome,
            id: decision.id,
            target: decision.target,
            path: Some(decision.path),
            role: role.unwrap_or("none"),
            ticket: ticket.unwrap_or("none"),
        };
        let bytes = encode_json_line(&entry)?;
        self.append_decisions(&bytes)
    }

    pub fn replay_summary(
        &self,
        from: u64,
        max_entries: usize,
    ) -> Result<ReplaySummary, ReplayWindowError> {
        let bounds = self.journal.bounds();
        if from < bounds.base_offset {
            return Err(ReplayWindowError::Stale {
                requested: from,
                available_start: bounds.base_offset,
            });
        }
        if from > bounds.next_offset {
            return Err(ReplayWindowError::Future {
                requested: from,
                available_end: bounds.next_offset,
            });
        }
        let mut sequence = String::new();
        let mut count = 0usize;
        for entry in self.replay_entries.iter() {
            if entry.offset_end <= from {
                continue;
            }
            count += 1;
            if count > max_entries {
                return Err(ReplayWindowError::TooManyEntries {
                    requested: count,
                    max: max_entries,
                });
            }
            sequence.push_str(entry.ack_line.as_str());
            sequence.push('\n');
        }
        Ok(ReplaySummary {
            from,
            to: bounds.next_offset,
            entries: count,
            sequence,
        })
    }

    fn append_journal(
        &mut self,
        offset: u64,
        data: &[u8],
        replay_entry: Option<ReplayEntry>,
    ) -> Result<AuditAppendOutcome, NineDoorError> {
        let payload = ensure_line_terminated(data);
        let expected_offset = self.journal.bounds().next_offset;
        let provided_offset = if offset == u64::MAX {
            expected_offset
        } else {
            offset
        };
        let bounds = append_only_write_bounds(
            expected_offset,
            provided_offset,
            self.config.limits().journal_max_bytes,
            payload.len(),
        )
        .map_err(|err| {
            NineDoorError::protocol(
                ErrorCode::Invalid,
                format!("audit journal append offset rejected: {err}"),
            )
        })?;
        if bounds.short {
            return Err(NineDoorError::protocol(
                ErrorCode::TooBig,
                format!(
                    "audit journal entry exceeds max bytes {}",
                    self.config.limits().journal_max_bytes
                ),
            ));
        }
        let outcome = self.journal.append(payload)?;
        if let Some(mut replay_entry) = replay_entry {
            replay_entry.offset_start = outcome.offset_start;
            replay_entry.offset_end = outcome.offset_end;
            self.replay_entries.push_back(replay_entry);
        }
        self.trim_replay_entries();
        self.refresh_export_snapshot();
        Ok(outcome)
    }

    fn append_decisions(&mut self, data: &[u8]) -> Result<AuditAppendOutcome, NineDoorError> {
        let payload = ensure_line_terminated(data);
        let expected_offset = self.decisions.bounds().next_offset;
        let bounds = append_only_write_bounds(
            expected_offset,
            expected_offset,
            self.config.limits().decisions_max_bytes,
            payload.len(),
        )
        .map_err(|err| {
            NineDoorError::protocol(
                ErrorCode::Invalid,
                format!("audit decisions append offset rejected: {err}"),
            )
        })?;
        if bounds.short {
            return Err(NineDoorError::protocol(
                ErrorCode::TooBig,
                format!(
                    "audit decisions entry exceeds max bytes {}",
                    self.config.limits().decisions_max_bytes
                ),
            ));
        }
        let outcome = self.decisions.append(payload)?;
        self.refresh_export_snapshot();
        Ok(outcome)
    }

    fn trim_replay_entries(&mut self) {
        let base = self.journal.bounds().base_offset;
        while let Some(entry) = self.replay_entries.front() {
            if entry.offset_end <= base {
                let _ = self.replay_entries.pop_front();
            } else {
                break;
            }
        }
    }

    fn refresh_export_snapshot(&mut self) {
        let journal_bounds = self.journal.bounds();
        let decisions_bounds = self.decisions.bounds();
        self.export_snapshot = format!(
            "{{\"journal_base\":{},\"journal_next\":{},\"decisions_base\":{},\"decisions_next\":{},\"replay_enabled\":{},\"replay_max_entries\":{}}}\n",
            journal_bounds.base_offset,
            journal_bounds.next_offset,
            decisions_bounds.base_offset,
            decisions_bounds.next_offset,
            self.config.replay().enabled_flag(),
            self.config.replay().max_entries()
        )
        .into_bytes();
    }

    fn next_sequence(&mut self) -> u64 {
        self.sequence = self.sequence.saturating_add(1);
        self.sequence
    }
}

/// Outcome of a control action recorded in the audit log.
pub struct ControlOutcome {
    status: ControlStatus,
    error: Option<ControlError>,
}

impl ControlOutcome {
    pub fn ok() -> Self {
        Self {
            status: ControlStatus::Ok,
            error: None,
        }
    }

    pub fn err(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            status: ControlStatus::Err,
            error: Some(ControlError {
                code,
                message: message.into(),
            }),
        }
    }

    pub fn from_error(error: &NineDoorError) -> Self {
        match error {
            NineDoorError::Protocol { code, message } => {
                ControlOutcome::err(*code, message.clone())
            }
            other => ControlOutcome::err(ErrorCode::Invalid, other.to_string()),
        }
    }

    fn status_label(&self) -> &'static str {
        match self.status {
            ControlStatus::Ok => "ok",
            ControlStatus::Err => "err",
        }
    }

    fn error_detail(&self) -> Option<AuditErrorDetail> {
        self.error.as_ref().map(|err| AuditErrorDetail {
            code: err.code.to_string(),
            message: err.message.clone(),
        })
    }

    fn ack_line(&self) -> String {
        match &self.error {
            None => "OK".to_owned(),
            Some(err) => format!("ERR {} {}", err.code, err.message),
        }
    }
}

#[derive(Clone, Copy)]
enum ControlStatus {
    Ok,
    Err,
}

#[derive(Clone)]
struct ControlError {
    code: ErrorCode,
    message: String,
}

/// Policy action decision recorded in `/audit/decisions`.
pub(crate) struct PolicyActionDecision<'a> {
    pub id: &'a str,
    pub decision: &'a str,
    pub target: &'a str,
}

/// Policy gate outcome recorded in `/audit/decisions`.
pub(crate) struct PolicyGateDecision<'a> {
    pub outcome: &'a str,
    pub id: Option<&'a str>,
    pub target: Option<&'a str>,
    pub path: &'a str,
}

#[derive(Serialize)]
struct AuditJournalEntry<'a> {
    seq: u64,
    kind: &'a str,
    path: &'a str,
    payload: &'a str,
    outcome: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<AuditErrorDetail>,
    role: &'a str,
    ticket: &'a str,
}

#[derive(Serialize)]
struct AuditErrorDetail {
    code: String,
    message: String,
}

#[derive(Serialize)]
struct DecisionEntry<'a> {
    seq: u64,
    kind: &'a str,
    outcome: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<&'a str>,
    role: &'a str,
    ticket: &'a str,
}

/// Replay summary returned for `/replay/status`.
pub struct ReplaySummary {
    pub from: u64,
    pub to: u64,
    pub entries: usize,
    pub sequence: String,
}

/// Errors when a replay cursor is outside the retained window.
pub enum ReplayWindowError {
    Stale { requested: u64, available_start: u64 },
    Future { requested: u64, available_end: u64 },
    TooManyEntries { requested: usize, max: usize },
}

/// Append outcome including any truncation details.
pub struct AuditAppendOutcome {
    pub count: u32,
    pub dropped_bytes: u64,
    pub new_base: u64,
    pub offset_start: u64,
    pub offset_end: u64,
}

#[derive(Debug)]
struct BoundedLog {
    entries: VecDeque<LogEntry>,
    capacity: usize,
    total_bytes: usize,
    base_offset: u64,
    next_offset: u64,
}

impl BoundedLog {
    fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            entries: VecDeque::new(),
            capacity,
            total_bytes: 0,
            base_offset: 0,
            next_offset: 0,
        }
    }

    fn bounds(&self) -> LogBounds {
        LogBounds {
            base_offset: self.base_offset,
            next_offset: self.next_offset,
        }
    }

    fn snapshot(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.total_bytes);
        for entry in &self.entries {
            out.extend_from_slice(entry.bytes.as_slice());
        }
        out
    }

    fn append(&mut self, bytes: Vec<u8>) -> Result<AuditAppendOutcome, NineDoorError> {
        if bytes.len() > self.capacity {
            return Err(NineDoorError::protocol(
                ErrorCode::TooBig,
                format!("audit entry exceeds max bytes {}", self.capacity),
            ));
        }
        let mut dropped_bytes = 0u64;
        while self.total_bytes + bytes.len() > self.capacity {
            if let Some(entry) = self.entries.pop_front() {
                dropped_bytes = dropped_bytes.saturating_add(entry.bytes.len() as u64);
                self.total_bytes = self.total_bytes.saturating_sub(entry.bytes.len());
                self.base_offset = entry.offset_end;
            } else {
                break;
            }
        }
        let offset_start = self.next_offset;
        let offset_end = offset_start.saturating_add(bytes.len() as u64);
        self.entries.push_back(LogEntry {
            bytes,
            offset_end,
        });
        self.total_bytes = self.total_bytes.saturating_add(self.entries.back().unwrap().bytes.len());
        self.next_offset = offset_end;
        Ok(AuditAppendOutcome {
            count: (offset_end - offset_start) as u32,
            dropped_bytes,
            new_base: self.base_offset,
            offset_start,
            offset_end,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LogBounds {
    pub base_offset: u64,
    pub next_offset: u64,
}

#[derive(Debug)]
struct LogEntry {
    bytes: Vec<u8>,
    offset_end: u64,
}

#[derive(Debug)]
struct ReplayEntry {
    offset_start: u64,
    offset_end: u64,
    ack_line: String,
}

impl ReplayEntry {
    fn new(length: u64, ack_line: String) -> Self {
        Self {
            offset_start: 0,
            offset_end: length,
            ack_line,
        }
    }
}

fn ensure_json_lines(data: &[u8], label: &str) -> Result<(), NineDoorError> {
    let text = std::str::from_utf8(data).map_err(|err| {
        NineDoorError::protocol(
            ErrorCode::Invalid,
            format!("{label} must be utf-8: {err}"),
        )
    })?;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        serde_json::from_str::<serde_json::Value>(trimmed).map_err(|err| {
            NineDoorError::protocol(
                ErrorCode::Invalid,
                format!("invalid {label} entry: {err}"),
            )
        })?;
    }
    Ok(())
}

fn encode_json_line<T: Serialize>(value: &T) -> Result<Vec<u8>, NineDoorError> {
    let json = serde_json::to_string(value).map_err(|err| {
        NineDoorError::protocol(
            ErrorCode::Invalid,
            format!("failed to encode audit entry: {err}"),
        )
    })?;
    Ok(ensure_line_terminated(json.as_bytes()))
}

fn ensure_line_terminated(data: &[u8]) -> Vec<u8> {
    let needs_newline = !data.ends_with(b"\n");
    if !needs_newline {
        return data.to_vec();
    }
    let mut out = data.to_vec();
    out.push(b'\n');
    out
}
