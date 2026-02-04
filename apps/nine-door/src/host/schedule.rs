// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Maintain schedule/lease/export control state for NineDoor host mode.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::collections::VecDeque;

use serde::Deserialize;
use secure9p_codec::ErrorCode;

use super::observe::{ProcLeaseConfig, ProcScheduleConfig};
use crate::NineDoorError;

const MAX_SCHEDULE_ID_LEN: usize = 64;
const MAX_SCHEDULE_ROLE_LEN: usize = 16;
const MAX_LEASE_ID_LEN: usize = 32;
const MAX_LEASE_SUBJECT_LEN: usize = 32;
const MAX_LEASE_RESOURCE_LEN: usize = 48;
const MAX_LEASE_REASON_LEN: usize = 24;
const MAX_EXPORT_ID_LEN: usize = 64;
const EXPORT_MAX_WINDOWS: usize = 64;
const LEASE_STATE_ACTIVE: &str = "active";

/// Schedule control sizing limits.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ScheduleControlConfig {
    pub(crate) enable: bool,
    pub(crate) queue_max_entries: usize,
    pub(crate) ctl_max_bytes: usize,
}

impl Default for ScheduleControlConfig {
    fn default() -> Self {
        Self {
            enable: true,
            queue_max_entries: 64,
            ctl_max_bytes: 8192,
        }
    }
}

/// Lease control sizing limits.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LeaseControlConfig {
    pub(crate) enable: bool,
    pub(crate) active_max_entries: usize,
    pub(crate) preemptions_max_entries: usize,
    pub(crate) ctl_max_bytes: usize,
}

impl Default for LeaseControlConfig {
    fn default() -> Self {
        Self {
            enable: true,
            active_max_entries: 64,
            preemptions_max_entries: 64,
            ctl_max_bytes: 8192,
        }
    }
}

/// Export control sizing limits.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ExportControlConfig {
    pub(crate) enable: bool,
    pub(crate) ctl_max_bytes: usize,
}

impl Default for ExportControlConfig {
    fn default() -> Self {
        Self {
            enable: true,
            ctl_max_bytes: 2048,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScheduleRequest {
    id: String,
    role: String,
    priority: u32,
    ticks: u32,
    budget_ms: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase", deny_unknown_fields)]
enum LeaseCommand {
    Grant {
        id: String,
        subject: String,
        resource: String,
        ttl_s: u32,
        priority: u32,
    },
    Renew {
        id: String,
        ttl_s: u32,
        priority: u32,
    },
    Preempt {
        id: String,
        reason: String,
    },
    Quota {
        subject: String,
        resource: String,
        max_active: u32,
        max_preemptions: u32,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase", deny_unknown_fields)]
enum ExportCommand {
    Open { id: String, ttl_s: u32 },
    Close { id: String, reason: String },
}

#[derive(Debug, Clone)]
struct ScheduleEntry {
    id: String,
    role: String,
    priority: u32,
    ticks: u32,
    budget_ms: u32,
    seq: u64,
}

/// Schedule control state.
#[derive(Debug, Clone)]
pub(crate) struct ScheduleState {
    enabled: bool,
    queue_max_entries: usize,
    ctl_max_bytes: usize,
    ctl_log: Vec<u8>,
    queue: VecDeque<ScheduleEntry>,
    dequeued: u64,
    dropped: u64,
    next_seq: u64,
    proc_summary: bool,
    proc_queue: bool,
    proc_summary_bytes: usize,
    proc_queue_bytes: usize,
}

impl ScheduleState {
    pub(crate) fn new(control: ScheduleControlConfig, proc: ProcScheduleConfig) -> Self {
        Self {
            enabled: control.enable,
            queue_max_entries: control.queue_max_entries,
            ctl_max_bytes: control.ctl_max_bytes,
            ctl_log: Vec::new(),
            queue: VecDeque::new(),
            dequeued: 0,
            dropped: 0,
            next_seq: 1,
            proc_summary: proc.summary,
            proc_queue: proc.queue,
            proc_summary_bytes: proc.summary_bytes,
            proc_queue_bytes: proc.queue_bytes,
        }
    }

    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) fn proc_enabled(&self) -> bool {
        self.proc_summary || self.proc_queue
    }

    pub(crate) fn proc_summary_enabled(&self) -> bool {
        self.proc_summary
    }

    pub(crate) fn proc_queue_enabled(&self) -> bool {
        self.proc_queue
    }

    pub(crate) fn ctl_log(&self) -> &[u8] {
        &self.ctl_log
    }

    pub(crate) fn append_line(&mut self, line: &str) -> Result<(), NineDoorError> {
        if !self.enabled {
            return Err(NineDoorError::protocol(
                ErrorCode::NotFound,
                "schedule control disabled",
            ));
        }
        let request: ScheduleRequest = parse_json_line(line, "schedule control")?;
        validate_schedule_id(&request.id)?;
        validate_schedule_role(&request.role)?;
        if request.ticks == 0 || request.budget_ms == 0 {
            return Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                "schedule ticks and budget_ms must be > 0",
            ));
        }
        if self.queue_max_entries == 0 {
            return Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                "schedule queue capacity is zero",
            ));
        }
        if self.queue.len() >= self.queue_max_entries {
            self.dropped = self.dropped.saturating_add(1);
            return Err(NineDoorError::protocol(
                ErrorCode::TooBig,
                "schedule queue full",
            ));
        }
        if self.queue.iter().any(|entry| entry.id == request.id) {
            return Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                "schedule id already exists",
            ));
        }
        append_log_line(&mut self.ctl_log, line, self.ctl_max_bytes, "schedule control")?;
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        self.queue.push_back(ScheduleEntry {
            id: request.id,
            role: request.role,
            priority: request.priority,
            ticks: request.ticks,
            budget_ms: request.budget_ms,
            seq,
        });
        Ok(())
    }

    pub(crate) fn summary_payload(&self) -> Result<Vec<u8>, NineDoorError> {
        let line = format!(
            "queue={} dequeued={} dropped={} max_entries={}\n",
            self.queue.len(),
            self.dequeued,
            self.dropped,
            self.queue_max_entries
        );
        ensure_len("proc/schedule/summary", line.len(), self.proc_summary_bytes)?;
        Ok(line.into_bytes())
    }

    pub(crate) fn queue_payload(&self) -> Result<Vec<u8>, NineDoorError> {
        let mut out = String::new();
        for entry in &self.queue {
            let line = format!(
                "id={} role={} priority={} ticks={} budget_ms={} seq={}\n",
                entry.id, entry.role, entry.priority, entry.ticks, entry.budget_ms, entry.seq
            );
            ensure_len("proc/schedule/queue", line.len(), self.proc_queue_bytes)?;
            if out.len().saturating_add(line.len()) > self.proc_queue_bytes {
                break;
            }
            out.push_str(&line);
        }
        Ok(out.into_bytes())
    }
}

#[derive(Debug, Clone)]
struct LeaseEntry {
    id: String,
    subject: String,
    resource: String,
    ttl_s: u32,
    priority: u32,
    state: &'static str,
    seq: u64,
}

#[derive(Debug, Clone)]
struct LeasePreemption {
    id: String,
    subject: String,
    resource: String,
    reason: String,
    seq: u64,
}

#[derive(Debug, Clone)]
struct LeaseQuota {
    subject: String,
    resource: String,
    max_active: u32,
    max_preemptions: u32,
}

/// Lease control state.
#[derive(Debug, Clone)]
pub(crate) struct LeaseState {
    enabled: bool,
    active_max_entries: usize,
    preemptions_max_entries: usize,
    ctl_max_bytes: usize,
    ctl_log: Vec<u8>,
    active: Vec<LeaseEntry>,
    preemptions: Vec<LeasePreemption>,
    quotas: Vec<LeaseQuota>,
    next_seq: u64,
    proc_summary: bool,
    proc_active: bool,
    proc_preemptions: bool,
    proc_summary_bytes: usize,
    proc_active_bytes: usize,
    proc_preemptions_bytes: usize,
}

impl LeaseState {
    pub(crate) fn new(control: LeaseControlConfig, proc: ProcLeaseConfig) -> Self {
        Self {
            enabled: control.enable,
            active_max_entries: control.active_max_entries,
            preemptions_max_entries: control.preemptions_max_entries,
            ctl_max_bytes: control.ctl_max_bytes,
            ctl_log: Vec::new(),
            active: Vec::new(),
            preemptions: Vec::new(),
            quotas: Vec::new(),
            next_seq: 1,
            proc_summary: proc.summary,
            proc_active: proc.active,
            proc_preemptions: proc.preemptions,
            proc_summary_bytes: proc.summary_bytes,
            proc_active_bytes: proc.active_bytes,
            proc_preemptions_bytes: proc.preemptions_bytes,
        }
    }

    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) fn proc_enabled(&self) -> bool {
        self.proc_summary || self.proc_active || self.proc_preemptions
    }

    pub(crate) fn proc_summary_enabled(&self) -> bool {
        self.proc_summary
    }

    pub(crate) fn proc_active_enabled(&self) -> bool {
        self.proc_active
    }

    pub(crate) fn proc_preemptions_enabled(&self) -> bool {
        self.proc_preemptions
    }

    pub(crate) fn ctl_log(&self) -> &[u8] {
        &self.ctl_log
    }

    pub(crate) fn append_line(&mut self, line: &str) -> Result<(), NineDoorError> {
        if !self.enabled {
            return Err(NineDoorError::protocol(
                ErrorCode::NotFound,
                "lease control disabled",
            ));
        }
        let command: LeaseCommand = parse_json_line(line, "lease control")?;
        match command {
            LeaseCommand::Grant {
                id,
                subject,
                resource,
                ttl_s,
                priority,
            } => {
                validate_lease_id(&id)?;
                validate_lease_subject(&subject)?;
                validate_lease_resource(&resource)?;
                if ttl_s == 0 {
                    return Err(NineDoorError::protocol(
                        ErrorCode::Invalid,
                        "lease ttl_s must be > 0",
                    ));
                }
                if self.active_max_entries == 0 {
                    return Err(NineDoorError::protocol(
                        ErrorCode::Invalid,
                        "lease capacity is zero",
                    ));
                }
                if self.active.len() >= self.active_max_entries {
                    return Err(NineDoorError::protocol(
                        ErrorCode::TooBig,
                        "lease active list full",
                    ));
                }
                if self.active.iter().any(|entry| entry.id == id) {
                    return Err(NineDoorError::protocol(
                        ErrorCode::Invalid,
                        "lease id already exists",
                    ));
                }
                append_log_line(&mut self.ctl_log, line, self.ctl_max_bytes, "lease control")?;
                let seq = self.next_seq;
                self.next_seq = self.next_seq.saturating_add(1);
                self.active.push(LeaseEntry {
                    id,
                    subject,
                    resource,
                    ttl_s,
                    priority,
                    state: LEASE_STATE_ACTIVE,
                    seq,
                });
            }
            LeaseCommand::Renew {
                id,
                ttl_s,
                priority,
            } => {
                validate_lease_id(&id)?;
                if ttl_s == 0 {
                    return Err(NineDoorError::protocol(
                        ErrorCode::Invalid,
                        "lease ttl_s must be > 0",
                    ));
                }
                let entry = self
                    .active
                    .iter_mut()
                    .find(|entry| entry.id == id)
                    .ok_or_else(|| {
                        NineDoorError::protocol(ErrorCode::Invalid, "lease id not found")
                    })?;
                append_log_line(&mut self.ctl_log, line, self.ctl_max_bytes, "lease control")?;
                let seq = self.next_seq;
                self.next_seq = self.next_seq.saturating_add(1);
                entry.ttl_s = ttl_s;
                entry.priority = priority;
                entry.seq = seq;
            }
            LeaseCommand::Preempt { id, reason } => {
                validate_lease_id(&id)?;
                validate_lease_reason(&reason)?;
                let position = self
                    .active
                    .iter()
                    .position(|entry| entry.id == id)
                    .ok_or_else(|| {
                        NineDoorError::protocol(ErrorCode::Invalid, "lease id not found")
                    })?;
                if self.preemptions_max_entries == 0
                    || self.preemptions.len() >= self.preemptions_max_entries
                {
                    return Err(NineDoorError::protocol(
                        ErrorCode::TooBig,
                        "lease preemptions list full",
                    ));
                }
                append_log_line(&mut self.ctl_log, line, self.ctl_max_bytes, "lease control")?;
                let entry = self.active.swap_remove(position);
                let seq = self.next_seq;
                self.next_seq = self.next_seq.saturating_add(1);
                self.preemptions.push(LeasePreemption {
                    id: entry.id,
                    subject: entry.subject,
                    resource: entry.resource,
                    reason,
                    seq,
                });
            }
            LeaseCommand::Quota {
                subject,
                resource,
                max_active,
                max_preemptions,
            } => {
                validate_lease_subject(&subject)?;
                validate_lease_resource(&resource)?;
                if max_active == 0 || max_preemptions == 0 {
                    return Err(NineDoorError::protocol(
                        ErrorCode::Invalid,
                        "lease quota values must be > 0",
                    ));
                }
                if self.active_max_entries == 0 {
                    return Err(NineDoorError::protocol(
                        ErrorCode::Invalid,
                        "lease quota capacity is zero",
                    ));
                }
                let existing_index = self
                    .quotas
                    .iter()
                    .position(|entry| entry.subject == subject && entry.resource == resource);
                if existing_index.is_none() && self.quotas.len() >= self.active_max_entries {
                    return Err(NineDoorError::protocol(
                        ErrorCode::TooBig,
                        "lease quota list full",
                    ));
                }
                append_log_line(&mut self.ctl_log, line, self.ctl_max_bytes, "lease control")?;
                if let Some(index) = existing_index {
                    let entry = &mut self.quotas[index];
                    entry.max_active = max_active;
                    entry.max_preemptions = max_preemptions;
                } else {
                    self.quotas.push(LeaseQuota {
                        subject,
                        resource,
                        max_active,
                        max_preemptions,
                    });
                }
            }
        }
        Ok(())
    }

    pub(crate) fn summary_payload(&self) -> Result<Vec<u8>, NineDoorError> {
        let line = format!(
            "active={} preemptions={} quotas={} max_active={} max_preemptions={}\n",
            self.active.len(),
            self.preemptions.len(),
            self.quotas.len(),
            self.active_max_entries,
            self.preemptions_max_entries
        );
        ensure_len("proc/lease/summary", line.len(), self.proc_summary_bytes)?;
        Ok(line.into_bytes())
    }

    pub(crate) fn active_payload(&self) -> Result<Vec<u8>, NineDoorError> {
        let mut out = String::new();
        for entry in &self.active {
            let line = format!(
                "id={} subject={} resource={} ttl_s={} priority={} state={} seq={}\n",
                entry.id,
                entry.subject,
                entry.resource,
                entry.ttl_s,
                entry.priority,
                entry.state,
                entry.seq
            );
            ensure_len("proc/lease/active", line.len(), self.proc_active_bytes)?;
            if out.len().saturating_add(line.len()) > self.proc_active_bytes {
                break;
            }
            out.push_str(&line);
        }
        Ok(out.into_bytes())
    }

    pub(crate) fn preemptions_payload(&self) -> Result<Vec<u8>, NineDoorError> {
        let mut out = String::new();
        for entry in &self.preemptions {
            let line = format!(
                "id={} subject={} resource={} reason={} seq={}\n",
                entry.id, entry.subject, entry.resource, entry.reason, entry.seq
            );
            ensure_len("proc/lease/preemptions", line.len(), self.proc_preemptions_bytes)?;
            if out.len().saturating_add(line.len()) > self.proc_preemptions_bytes {
                break;
            }
            out.push_str(&line);
        }
        Ok(out.into_bytes())
    }
}

#[derive(Debug, Clone)]
struct ExportWindow {
    id: String,
    ttl_s: u32,
    seq: u64,
}

/// Export control state.
#[derive(Debug, Clone)]
pub(crate) struct ExportState {
    enabled: bool,
    ctl_max_bytes: usize,
    ctl_log: Vec<u8>,
    windows: Vec<ExportWindow>,
    next_seq: u64,
}

impl ExportState {
    pub(crate) fn new(control: ExportControlConfig) -> Self {
        Self {
            enabled: control.enable,
            ctl_max_bytes: control.ctl_max_bytes,
            ctl_log: Vec::new(),
            windows: Vec::new(),
            next_seq: 1,
        }
    }

    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) fn ctl_log(&self) -> &[u8] {
        &self.ctl_log
    }

    pub(crate) fn append_line(&mut self, line: &str) -> Result<(), NineDoorError> {
        if !self.enabled {
            return Err(NineDoorError::protocol(
                ErrorCode::NotFound,
                "export control disabled",
            ));
        }
        let command: ExportCommand = parse_json_line(line, "export control")?;
        match command {
            ExportCommand::Open { id, ttl_s } => {
                validate_export_id(&id)?;
                if ttl_s == 0 {
                    return Err(NineDoorError::protocol(
                        ErrorCode::Invalid,
                        "export ttl_s must be > 0",
                    ));
                }
                let existing = self.windows.iter().position(|entry| entry.id == id);
                if existing.is_none() && self.windows.len() >= EXPORT_MAX_WINDOWS {
                    return Err(NineDoorError::protocol(
                        ErrorCode::TooBig,
                        "export window list full",
                    ));
                }
                append_log_line(&mut self.ctl_log, line, self.ctl_max_bytes, "export control")?;
                let seq = self.next_seq;
                self.next_seq = self.next_seq.saturating_add(1);
                if let Some(index) = existing {
                    let entry = &mut self.windows[index];
                    entry.ttl_s = ttl_s;
                    entry.seq = seq;
                } else {
                    self.windows.push(ExportWindow { id, ttl_s, seq });
                }
            }
            ExportCommand::Close { id, reason } => {
                validate_export_id(&id)?;
                validate_lease_reason(&reason)?;
                let position = self
                    .windows
                    .iter()
                    .position(|entry| entry.id == id)
                    .ok_or_else(|| {
                        NineDoorError::protocol(ErrorCode::Invalid, "export id not found")
                    })?;
                append_log_line(&mut self.ctl_log, line, self.ctl_max_bytes, "export control")?;
                let _ = self.windows.swap_remove(position);
            }
        }
        Ok(())
    }
}

fn parse_json_line<T: for<'de> Deserialize<'de>>(
    line: &str,
    label: &str,
) -> Result<T, NineDoorError> {
    serde_json::from_str(line).map_err(|err| {
        NineDoorError::protocol(ErrorCode::Invalid, format!("invalid {label} entry: {err}"))
    })
}

fn append_log_line(
    log: &mut Vec<u8>,
    line: &str,
    max_bytes: usize,
    label: &str,
) -> Result<(), NineDoorError> {
    let bytes = line.as_bytes();
    let needs_newline = !bytes.ends_with(b"\n");
    let extra = if needs_newline { 1 } else { 0 };
    let new_len = log
        .len()
        .saturating_add(bytes.len())
        .saturating_add(extra);
    if new_len > max_bytes {
        return Err(NineDoorError::protocol(
            ErrorCode::Invalid,
            format!("{label} exceeds max bytes {}", max_bytes),
        ));
    }
    log.extend_from_slice(bytes);
    if needs_newline {
        log.push(b'\n');
    }
    Ok(())
}

fn ensure_len(label: &str, len: usize, max_len: usize) -> Result<(), NineDoorError> {
    if len > max_len {
        return Err(NineDoorError::protocol(
            ErrorCode::TooBig,
            format!("{label} exceeds max bytes {}", max_len),
        ));
    }
    Ok(())
}

fn validate_simple_token(value: &str, max_len: usize, label: &str) -> Result<(), NineDoorError> {
    if value.is_empty() {
        return Err(NineDoorError::protocol(
            ErrorCode::Invalid,
            format!("{label} must not be empty"),
        ));
    }
    if value.len() > max_len {
        return Err(NineDoorError::protocol(
            ErrorCode::Invalid,
            format!("{label} exceeds max length {}", max_len),
        ));
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(NineDoorError::protocol(
            ErrorCode::Invalid,
            format!("{label} must be alphanumeric, '-' or '_'"),
        ));
    }
    Ok(())
}

fn validate_extended_token(value: &str, max_len: usize, label: &str) -> Result<(), NineDoorError> {
    if value.is_empty() {
        return Err(NineDoorError::protocol(
            ErrorCode::Invalid,
            format!("{label} must not be empty"),
        ));
    }
    if value.len() > max_len {
        return Err(NineDoorError::protocol(
            ErrorCode::Invalid,
            format!("{label} exceeds max length {}", max_len),
        ));
    }
    if value == "." || value == ".." {
        return Err(NineDoorError::protocol(
            ErrorCode::Invalid,
            format!("{label} must not be '.' or '..'"),
        ));
    }
    if !value.chars().all(|ch| {
        ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' || ch == ':'
    }) {
        return Err(NineDoorError::protocol(
            ErrorCode::Invalid,
            format!("{label} must be alphanumeric, '-', '_', '.', or ':'"),
        ));
    }
    Ok(())
}

fn validate_schedule_id(id: &str) -> Result<(), NineDoorError> {
    validate_simple_token(id, MAX_SCHEDULE_ID_LEN, "schedule id")
}

fn validate_schedule_role(role: &str) -> Result<(), NineDoorError> {
    validate_simple_token(role, MAX_SCHEDULE_ROLE_LEN, "schedule role")
}

fn validate_lease_id(id: &str) -> Result<(), NineDoorError> {
    validate_simple_token(id, MAX_LEASE_ID_LEN, "lease id")
}

fn validate_lease_subject(subject: &str) -> Result<(), NineDoorError> {
    validate_simple_token(subject, MAX_LEASE_SUBJECT_LEN, "lease subject")
}

fn validate_lease_resource(resource: &str) -> Result<(), NineDoorError> {
    validate_extended_token(resource, MAX_LEASE_RESOURCE_LEN, "lease resource")
}

fn validate_lease_reason(reason: &str) -> Result<(), NineDoorError> {
    validate_extended_token(reason, MAX_LEASE_REASON_LEN, "lease reason")
}

fn validate_export_id(id: &str) -> Result<(), NineDoorError> {
    validate_simple_token(id, MAX_EXPORT_ID_LEN, "export id")
}
