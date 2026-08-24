// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Maintain schedule/lease/export control state for NineDoor host mode.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::collections::VecDeque;

use secure9p_codec::ErrorCode;
use serde::Deserialize;

use super::observe::{ProcLeaseConfig, ProcScheduleConfig};
use crate::NineDoorError;

const MAX_SCHEDULE_ID_LEN: usize = 64;
const MAX_SCHEDULE_ROLE_LEN: usize = 16;
const MAX_LEASE_ID_LEN: usize = 32;
const MAX_LEASE_SUBJECT_LEN: usize = 32;
const MAX_LEASE_RESOURCE_LEN: usize = 48;
const MAX_LEASE_REASON_LEN: usize = 24;
const LEASE_REQUEST_TAG_BYTES: usize = 16;
const MAX_EXPORT_ID_LEN: usize = 64;
const EXPORT_MAX_WINDOWS: usize = 64;
const LEASE_STATE_ACTIVE: &str = "active";
const DEFAULT_SCHEDULE_QUEUE_MAX_ENTRIES: usize = 256;
const DEFAULT_LEASE_ACTIVE_MAX_ENTRIES: usize = 256;
const DEFAULT_LEASE_PREEMPTIONS_MAX_ENTRIES: usize = 256;

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
            queue_max_entries: DEFAULT_SCHEDULE_QUEUE_MAX_ENTRIES,
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
            active_max_entries: DEFAULT_LEASE_ACTIVE_MAX_ENTRIES,
            preemptions_max_entries: DEFAULT_LEASE_PREEMPTIONS_MAX_ENTRIES,
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
enum ScheduleLifecycleCommand {
    Dequeue { id: String },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum ScheduleCommand {
    Enqueue(ScheduleRequest),
    Lifecycle(ScheduleLifecycleCommand),
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
    #[serde(rename = "renew-bound")]
    RenewBound {
        id: String,
        subject: String,
        resource: String,
        request: String,
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
        let command: ScheduleCommand = parse_json_line(line, "schedule control")?;
        match command {
            ScheduleCommand::Enqueue(request) => {
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
                append_log_line(
                    &mut self.ctl_log,
                    line,
                    self.ctl_max_bytes,
                    "schedule control",
                )?;
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
            }
            ScheduleCommand::Lifecycle(ScheduleLifecycleCommand::Dequeue { id }) => {
                validate_schedule_id(&id)?;
                let front = self.queue.front().ok_or_else(|| {
                    NineDoorError::protocol(ErrorCode::Invalid, "schedule queue is empty")
                })?;
                if front.id != id {
                    return Err(NineDoorError::protocol(
                        ErrorCode::Invalid,
                        "schedule dequeue must match queue head",
                    ));
                }
                append_log_line(
                    &mut self.ctl_log,
                    line,
                    self.ctl_max_bytes,
                    "schedule control",
                )?;
                let _ = self.queue.pop_front();
                self.dequeued = self.dequeued.saturating_add(1);
            }
        }
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
    last_request_tag: Option<[u8; LEASE_REQUEST_TAG_BYTES]>,
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
    preemptions: VecDeque<LeasePreemption>,
    preemptions_total: u64,
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
            preemptions: VecDeque::new(),
            preemptions_total: 0,
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
                    last_request_tag: None,
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
            LeaseCommand::RenewBound {
                id,
                subject,
                resource,
                request,
                ttl_s,
                priority,
            } => {
                validate_lease_id(&id)?;
                validate_lease_subject(&subject)?;
                validate_lease_resource(&resource)?;
                let request = decode_lease_request_tag(&request)?;
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
                if entry.subject != subject || entry.resource != resource {
                    return Err(NineDoorError::protocol(
                        ErrorCode::Invalid,
                        "lease binding mismatch",
                    ));
                }
                if entry.last_request_tag == Some(request) {
                    if entry.ttl_s == ttl_s && entry.priority == priority {
                        return Ok(());
                    }
                    return Err(NineDoorError::protocol(
                        ErrorCode::Invalid,
                        "lease request replay changed parameters",
                    ));
                }
                append_log_line(&mut self.ctl_log, line, self.ctl_max_bytes, "lease control")?;
                let seq = self.next_seq;
                self.next_seq = self.next_seq.saturating_add(1);
                entry.ttl_s = ttl_s;
                entry.priority = priority;
                entry.seq = seq;
                entry.last_request_tag = Some(request);
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
                if self.preemptions_max_entries == 0 {
                    return Err(NineDoorError::protocol(
                        ErrorCode::TooBig,
                        "lease preemptions capacity is zero",
                    ));
                }
                append_log_line(&mut self.ctl_log, line, self.ctl_max_bytes, "lease control")?;
                let entry = self.active.swap_remove(position);
                let seq = self.next_seq;
                self.next_seq = self.next_seq.saturating_add(1);
                if self.preemptions.len() == self.preemptions_max_entries {
                    let _ = self.preemptions.pop_front();
                }
                self.preemptions.push_back(LeasePreemption {
                    id: entry.id,
                    subject: entry.subject,
                    resource: entry.resource,
                    reason,
                    seq,
                });
                self.preemptions_total = self.preemptions_total.saturating_add(1);
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
            self.preemptions_total,
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
            let line = self.active_line(entry)?;
            if out.len().saturating_add(line.len()) > self.proc_active_bytes {
                break;
            }
            out.push_str(&line);
        }
        Ok(out.into_bytes())
    }

    pub(crate) fn active_by_id_payloads(&self) -> Result<Vec<(String, Vec<u8>)>, NineDoorError> {
        self.active
            .iter()
            .map(|entry| {
                self.active_line(entry)
                    .map(|line| (entry.id.clone(), line.into_bytes()))
            })
            .collect()
    }

    fn active_line(&self, entry: &LeaseEntry) -> Result<String, NineDoorError> {
        let mut line = format!(
            "id={} subject={} resource={} ttl_s={} priority={} state={} seq={}",
            entry.id,
            entry.subject,
            entry.resource,
            entry.ttl_s,
            entry.priority,
            entry.state,
            entry.seq
        );
        if let Some(request) = entry.last_request_tag {
            line.push_str(" request=");
            line.push_str(hex::encode(request).as_str());
        }
        line.push('\n');
        ensure_len("proc/lease/by-id", line.len(), self.proc_active_bytes)?;
        Ok(line)
    }

    pub(crate) fn preemptions_payload(&self) -> Result<Vec<u8>, NineDoorError> {
        let mut out = String::new();
        for entry in &self.preemptions {
            let line = format!(
                "id={} subject={} resource={} reason={} seq={}\n",
                entry.id, entry.subject, entry.resource, entry.reason, entry.seq
            );
            ensure_len(
                "proc/lease/preemptions",
                line.len(),
                self.proc_preemptions_bytes,
            )?;
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
                append_log_line(
                    &mut self.ctl_log,
                    line,
                    self.ctl_max_bytes,
                    "export control",
                )?;
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
                append_log_line(
                    &mut self.ctl_log,
                    line,
                    self.ctl_max_bytes,
                    "export control",
                )?;
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
    let line_len = bytes.len().saturating_add(extra);
    if line_len > max_bytes {
        return Err(NineDoorError::protocol(
            ErrorCode::Invalid,
            format!("{label} exceeds max bytes {}", max_bytes),
        ));
    }
    let new_len = log.len().saturating_add(line_len);
    if new_len > max_bytes {
        let mut drop_len = new_len.saturating_sub(max_bytes);
        if drop_len < log.len() {
            if let Some(position) = log[drop_len..].iter().position(|byte| *byte == b'\n') {
                drop_len = drop_len.saturating_add(position + 1);
            } else {
                drop_len = log.len();
            }
        }
        log.drain(0..drop_len);
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
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' || ch == ':')
    {
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

fn decode_lease_request_tag(value: &str) -> Result<[u8; LEASE_REQUEST_TAG_BYTES], NineDoorError> {
    if value.len() != LEASE_REQUEST_TAG_BYTES * 2
        || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(NineDoorError::protocol(
            ErrorCode::Invalid,
            "lease request must be a 128-bit hexadecimal tag",
        ));
    }
    let mut tag = [0u8; LEASE_REQUEST_TAG_BYTES];
    hex::decode_to_slice(value.as_bytes(), &mut tag).map_err(|_| {
        NineDoorError::protocol(
            ErrorCode::Invalid,
            "lease request must be a 128-bit hexadecimal tag",
        )
    })?;
    Ok(tag)
}

fn validate_export_id(id: &str) -> Result<(), NineDoorError> {
    validate_simple_token(id, MAX_EXPORT_ID_LEN, "export id")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_log_retains_newest_complete_lines_within_bound() {
        let mut log = Vec::new();
        append_log_line(&mut log, "line1", 16, "control").expect("append line1");
        append_log_line(&mut log, "line2", 16, "control").expect("append line2");
        append_log_line(&mut log, "line3", 16, "control").expect("append line3");

        assert_eq!(log, b"line2\nline3\n");
    }

    #[test]
    fn control_log_rejects_one_oversize_line_without_mutation() {
        let mut log = b"kept\n".to_vec();
        let err = append_log_line(&mut log, "0123456789", 8, "control")
            .expect_err("one over-limit line must fail");

        assert!(matches!(
            err,
            NineDoorError::Protocol {
                code: ErrorCode::Invalid,
                ..
            }
        ));
        assert_eq!(log, b"kept\n");
    }

    #[test]
    fn schedule_queue_enforces_configured_capacity() {
        let mut state = ScheduleState::new(
            ScheduleControlConfig {
                enable: true,
                queue_max_entries: 2,
                ctl_max_bytes: 1024,
            },
            ProcScheduleConfig {
                summary: false,
                queue: false,
                summary_bytes: 0,
                queue_bytes: 0,
            },
        );
        let first =
            r#"{"id":"sched-1","role":"worker-heartbeat","priority":1,"ticks":1,"budget_ms":1}"#;
        let second =
            r#"{"id":"sched-2","role":"worker-heartbeat","priority":1,"ticks":1,"budget_ms":1}"#;
        let overflow =
            r#"{"id":"sched-3","role":"worker-heartbeat","priority":1,"ticks":1,"budget_ms":1}"#;
        state.append_line(first).expect("first schedule entry");
        state.append_line(second).expect("second schedule entry");
        let err = state
            .append_line(overflow)
            .expect_err("schedule queue should be full");
        assert!(matches!(
            &err,
            NineDoorError::Protocol {
                code: ErrorCode::TooBig,
                message,
            } if message == "schedule queue full"
        ));
    }

    #[test]
    fn schedule_state_keeps_accepting_valid_entries_when_control_log_rolls() {
        let mut state = ScheduleState::new(
            ScheduleControlConfig {
                enable: true,
                queue_max_entries: 3,
                ctl_max_bytes: 96,
            },
            ProcScheduleConfig {
                summary: false,
                queue: false,
                summary_bytes: 0,
                queue_bytes: 0,
            },
        );
        for id in ["s1", "s2", "s3"] {
            state
                .append_line(&format!(
                    r#"{{"id":"{id}","role":"worker-gpu","priority":1,"ticks":1,"budget_ms":1}}"#
                ))
                .expect("valid schedule entry");
        }

        assert_eq!(state.queue.len(), 3);
        assert!(state.ctl_log.len() <= 96);
        assert_eq!(
            state.ctl_log,
            b"{\"id\":\"s3\",\"role\":\"worker-gpu\",\"priority\":1,\"ticks\":1,\"budget_ms\":1}\n"
        );
    }

    #[test]
    fn lease_active_and_quota_lists_enforce_capacity_while_preemption_history_rolls() {
        let mut state = LeaseState::new(
            LeaseControlConfig {
                enable: true,
                active_max_entries: 2,
                preemptions_max_entries: 2,
                ctl_max_bytes: 1024,
            },
            ProcLeaseConfig {
                summary: false,
                active: false,
                preemptions: false,
                summary_bytes: 0,
                active_bytes: 0,
                preemptions_bytes: 0,
            },
        );
        for id in ["l1", "l2"] {
            state
                .append_line(&format!(
                    r#"{{"op":"grant","id":"{id}","subject":"s","resource":"r","ttl_s":1,"priority":1}}"#
                ))
                .expect("grant lease");
        }
        let active_err = state
            .append_line(
                r#"{"op":"grant","id":"l3","subject":"s","resource":"r","ttl_s":1,"priority":1}"#,
            )
            .expect_err("active list should be full");
        assert!(matches!(
            &active_err,
            NineDoorError::Protocol {
                code: ErrorCode::TooBig,
                message,
            } if message == "lease active list full"
        ));

        for id in ["l1", "l2"] {
            state
                .append_line(&format!(r#"{{"op":"preempt","id":"{id}","reason":"x"}}"#))
                .expect("preempt lease");
        }
        state
            .append_line(
                r#"{"op":"grant","id":"l3","subject":"s","resource":"r","ttl_s":1,"priority":1}"#,
            )
            .expect("grant after preemptions");
        state
            .append_line(r#"{"op":"preempt","id":"l3","reason":"x"}"#)
            .expect("preemption control must outlive its bounded evidence history");
        assert_eq!(state.preemptions_total, 3);
        assert_eq!(state.preemptions.len(), 2);
        assert_eq!(state.preemptions[0].id, "l2");
        assert_eq!(state.preemptions[1].id, "l3");

        for (subject, resource) in [("s1", "r1"), ("s2", "r2")] {
            state
                .append_line(&format!(
                    r#"{{"op":"quota","subject":"{subject}","resource":"{resource}","max_active":1,"max_preemptions":1}}"#
                ))
                .expect("set lease quota");
        }
        let quota_err = state
            .append_line(
                r#"{"op":"quota","subject":"s3","resource":"r3","max_active":1,"max_preemptions":1}"#,
            )
            .expect_err("quota list should be full");
        assert!(matches!(
            &quota_err,
            NineDoorError::Protocol {
                code: ErrorCode::TooBig,
                message,
            } if message == "lease quota list full"
        ));
    }

    #[test]
    fn lease_state_keeps_accepting_valid_transitions_when_control_log_rolls() {
        let mut state = LeaseState::new(
            LeaseControlConfig {
                enable: true,
                active_max_entries: 2,
                preemptions_max_entries: 2,
                ctl_max_bytes: 128,
            },
            ProcLeaseConfig {
                summary: false,
                active: false,
                preemptions: false,
                summary_bytes: 0,
                active_bytes: 0,
                preemptions_bytes: 0,
            },
        );
        state
            .append_line(
                r#"{"op":"grant","id":"l1","subject":"queen","resource":"gpu0","ttl_s":1,"priority":1}"#,
            )
            .expect("grant lease");
        state
            .append_line(
                r#"{"op":"quota","subject":"queen","resource":"gpu0","max_active":1,"max_preemptions":1}"#,
            )
            .expect("set quota");
        state
            .append_line(r#"{"op":"preempt","id":"l1","reason":"benchmark"}"#)
            .expect("preempt lease");

        assert!(state.active.is_empty());
        assert_eq!(state.preemptions.len(), 1);
        assert_eq!(state.quotas.len(), 1);
        assert!(state.ctl_log.len() <= 128);
        assert_eq!(
            state.ctl_log,
            b"{\"op\":\"preempt\",\"id\":\"l1\",\"reason\":\"benchmark\"}\n"
        );
    }

    #[test]
    fn lease_bound_renew_is_atomic_correlated_and_idempotent() {
        let mut state = LeaseState::new(
            LeaseControlConfig {
                enable: true,
                active_max_entries: 2,
                preemptions_max_entries: 2,
                ctl_max_bytes: 1024,
            },
            ProcLeaseConfig {
                summary: true,
                active: true,
                preemptions: true,
                summary_bytes: 1024,
                active_bytes: 1024,
                preemptions_bytes: 1024,
            },
        );
        state
            .append_line(
                r#"{"op":"grant","id":"l1","subject":"worker-1","resource":"gpu0","ttl_s":30,"priority":1}"#,
            )
            .expect("grant");
        let renew = r#"{"op":"renew-bound","id":"l1","subject":"worker-1","resource":"gpu0","request":"00112233445566778899aabbccddeeff","ttl_s":60,"priority":2}"#;
        state.append_line(renew).expect("bound renew");

        assert_eq!(
            String::from_utf8(state.active_payload().expect("active payload"))
                .expect("utf8 active payload"),
            "id=l1 subject=worker-1 resource=gpu0 ttl_s=60 priority=2 state=active seq=2 request=00112233445566778899aabbccddeeff\n"
        );
        let next_seq = state.next_seq;
        let log = state.ctl_log.clone();
        state.append_line(renew).expect("exact replay");
        assert_eq!(state.next_seq, next_seq);
        assert_eq!(state.ctl_log, log);

        let changed_replay = r#"{"op":"renew-bound","id":"l1","subject":"worker-1","resource":"gpu0","request":"00112233445566778899aabbccddeeff","ttl_s":61,"priority":2}"#;
        assert!(state.append_line(changed_replay).is_err());
        let wrong_binding = r#"{"op":"renew-bound","id":"l1","subject":"worker-2","resource":"gpu0","request":"ffeeddccbbaa99887766554433221100","ttl_s":60,"priority":2}"#;
        assert!(state.append_line(wrong_binding).is_err());
        assert_eq!(state.next_seq, next_seq);
        assert_eq!(state.ctl_log, log);
    }

    #[test]
    fn lease_by_id_payloads_are_not_truncated_by_aggregate_budget() {
        let control = LeaseControlConfig {
            enable: true,
            active_max_entries: 4,
            preemptions_max_entries: 4,
            ctl_max_bytes: 1024,
        };
        let mut sizing = LeaseState::new(
            control,
            ProcLeaseConfig {
                summary: true,
                active: true,
                preemptions: true,
                summary_bytes: 1024,
                active_bytes: 1024,
                preemptions_bytes: 1024,
            },
        );
        sizing
            .append_line(
                r#"{"op":"grant","id":"lease-1","subject":"worker-1","resource":"gpu0","ttl_s":30,"priority":7}"#,
            )
            .expect("grant sizing lease");
        let active_bytes = sizing.active_payload().expect("sizing payload").len();

        let mut state = LeaseState::new(
            control,
            ProcLeaseConfig {
                summary: true,
                active: true,
                preemptions: true,
                summary_bytes: 1024,
                active_bytes,
                preemptions_bytes: 1024,
            },
        );
        for index in 1..=3 {
            state
                .append_line(
                    format!(
                        r#"{{"op":"grant","id":"lease-{index}","subject":"worker-{index}","resource":"gpu0","ttl_s":30,"priority":7}}"#
                    )
                    .as_str(),
                )
                .expect("grant lease");
        }

        let aggregate = String::from_utf8(state.active_payload().expect("aggregate payload"))
            .expect("utf8 aggregate");
        assert_eq!(aggregate.lines().count(), 1);
        let by_id = state.active_by_id_payloads().expect("exact payloads");
        assert_eq!(by_id.len(), 3);
        assert_eq!(by_id[2].0, "lease-3");
        assert!(String::from_utf8_lossy(&by_id[2].1).starts_with("id=lease-3 "));
    }

    #[test]
    fn export_state_keeps_accepting_valid_transitions_when_control_log_rolls() {
        let mut state = ExportState::new(ExportControlConfig {
            enable: true,
            ctl_max_bytes: 64,
        });
        state
            .append_line(r#"{"op":"open","id":"w1","ttl_s":1}"#)
            .expect("open first export window");
        state
            .append_line(r#"{"op":"open","id":"w2","ttl_s":1}"#)
            .expect("open second export window");
        state
            .append_line(r#"{"op":"close","id":"w1","reason":"complete"}"#)
            .expect("close first export window");

        assert_eq!(state.windows.len(), 1);
        assert_eq!(state.windows[0].id, "w2");
        assert!(state.ctl_log.len() <= 64);
        assert_eq!(
            state.ctl_log,
            b"{\"op\":\"close\",\"id\":\"w1\",\"reason\":\"complete\"}\n"
        );
    }

    #[test]
    fn export_windows_enforce_configured_capacity() {
        let mut state = ExportState::new(ExportControlConfig {
            enable: true,
            ctl_max_bytes: 4096,
        });
        for index in 0..EXPORT_MAX_WINDOWS {
            state
                .append_line(&format!(
                    r#"{{"op":"open","id":"window-{index}","ttl_s":1}}"#
                ))
                .expect("open export window");
        }

        let err = state
            .append_line(r#"{"op":"open","id":"window-overflow","ttl_s":1}"#)
            .expect_err("export window list should be full");
        assert!(matches!(
            &err,
            NineDoorError::Protocol {
                code: ErrorCode::TooBig,
                message,
            } if message == "export window list full"
        ));
    }
}
