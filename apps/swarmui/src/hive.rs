// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: SwarmUI Live Hive event modeling, replay, and bounded polling helpers.
// Author: Lukas Bower

#[cfg(feature = "rest")]
use cohesix_ticket::Role;
use cohsh::client::CohClient;
#[cfg(feature = "rest")]
use cohsh::RestTransport as CohshRestTransport;
use cohsh::{Session, Transport};
use cohsh_core::{BoundedLineBuffer, Secure9pTransport, TailPollPolicy, TailPoller};
use secure9p_codec::OpenMode;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
#[cfg(feature = "rest")]
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{SwarmUiError, SwarmUiTranscript};

const HIVE_SNAPSHOT_VERSION: u8 = 1;
const DEFAULT_LINE_CAP_BYTES: usize = 160;

/// Hive renderer defaults emitted by coh-rtc.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmUiHiveConfig {
    /// Maximum frames per second for Live Hive rendering.
    pub frame_cap_fps: u16,
    /// Fixed simulation step in milliseconds.
    pub step_ms: u16,
    /// Zoom threshold for switching to cluster LOD.
    pub lod_zoom_out: f32,
    /// Zoom threshold for switching to detail LOD.
    pub lod_zoom_in: f32,
    /// Event budget per simulation step.
    pub lod_event_budget: u32,
    /// Maximum events allowed in cached snapshots.
    pub snapshot_max_events: u32,
    /// Number of lines to show in the per-worker overlay.
    pub overlay_lines: u16,
    /// Number of lines to retain for the detail panel.
    pub detail_lines: u16,
    /// Maximum bytes per telemetry line.
    pub line_cap_bytes: u32,
    /// Maximum bytes retained per worker buffer.
    pub per_worker_bytes: u32,
    /// Maximum pending telemetry lines retained per worker before dropping oldest.
    pub pending_lines_per_worker: u16,
    /// Maximum queued events retained before dropping oldest.
    pub pending_event_cap: u32,
    /// Maximum workers polled per ingest tick.
    pub poll_workers_per_tick: u16,
    /// Minimum milliseconds between status snapshot refreshes.
    pub status_poll_ms: u32,
    /// Pressure threshold for degraded rendering.
    pub degrade_pressure: f32,
}

/// Descriptor for a hive agent (queen or worker).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmUiHiveAgent {
    /// Agent identifier.
    pub id: String,
    /// Role label for the agent.
    pub role: String,
    /// Namespace path for the agent.
    pub namespace: String,
}

/// Event kinds derived from telemetry streams.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SwarmUiHiveEventKind {
    /// Telemetry line from an agent.
    Telemetry,
    /// Error line from an agent.
    Error,
}

/// Normalized event record consumed by the Live Hive view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmUiHiveEvent {
    /// Monotonic sequence number for ordering.
    pub seq: u64,
    /// Event classification.
    pub kind: SwarmUiHiveEventKind,
    /// Optional refusal reason tag for ERR lines.
    #[serde(default)]
    pub reason: Option<String>,
    /// Agent identifier that emitted the event.
    pub agent: String,
    /// Optional role label associated with the agent.
    #[serde(default)]
    pub role: Option<String>,
    /// Namespace path for the agent.
    pub namespace: String,
    /// Optional detail payload (truncated).
    pub detail: Option<String>,
}

/// Per-agent overlay lines rendered alongside the Live Hive canvas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmUiHiveOverlay {
    /// Agent identifier.
    pub agent: String,
    /// Latest telemetry lines for the agent.
    pub lines: Vec<String>,
}

/// Detail panel payload for a selected agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmUiHiveDetail {
    /// Agent identifier.
    pub agent: String,
    /// Bounded telemetry lines for the agent.
    pub lines: Vec<String>,
}

/// Root reachability summary for Live Hive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmUiHiveRootStatus {
    /// True when the queen/root is reachable.
    pub reachable: bool,
    /// Cut reason label when unreachable.
    pub cut_reason: String,
}

/// Session summary for Live Hive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmUiHiveSessionSummary {
    /// Active session count.
    pub active: u64,
    /// Draining session count.
    pub draining: u64,
}

/// Pressure counters for Live Hive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmUiHivePressureCounters {
    /// Busy/backpressure events.
    pub busy: u64,
    /// Quota-related refusals.
    pub quota: u64,
    /// Cut-related refusals.
    pub cut: u64,
    /// Policy-related refusals.
    pub policy: u64,
}

/// Scheduler summary counters for Live Hive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmUiHiveScheduleSummary {
    /// Queued entries.
    pub queue: u64,
    /// Dequeued entries.
    pub dequeued: u64,
    /// Dropped entries.
    pub dropped: u64,
    /// Max queue entries.
    pub max_entries: u64,
}

/// Scheduler queue entry for Live Hive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmUiHiveScheduleEntry {
    /// Schedule identifier.
    pub id: String,
    /// Target role label.
    pub role: String,
    /// Priority value.
    pub priority: u32,
    /// Tick budget.
    pub ticks: u32,
    /// Millisecond budget.
    pub budget_ms: u32,
    /// Sequence identifier.
    pub seq: u64,
}

/// Scheduler snapshot for Live Hive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmUiHiveScheduleSnapshot {
    /// Optional summary line.
    #[serde(default)]
    pub summary: Option<SwarmUiHiveScheduleSummary>,
    /// Queue entries.
    #[serde(default)]
    pub queue: Vec<SwarmUiHiveScheduleEntry>,
}

/// Lease summary counters for Live Hive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmUiHiveLeaseSummary {
    /// Active lease count.
    pub active: u64,
    /// Preemption count.
    pub preemptions: u64,
    /// Quota count.
    pub quotas: u64,
    /// Max active entries.
    pub max_active: u64,
    /// Max preemptions entries.
    pub max_preemptions: u64,
}

/// Active lease entry for Live Hive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmUiHiveLeaseEntry {
    /// Lease identifier.
    pub id: String,
    /// Subject identifier.
    pub subject: String,
    /// Resource identifier.
    pub resource: String,
    /// Lease TTL in seconds.
    pub ttl_s: u32,
    /// Priority value.
    pub priority: u32,
    /// Lease state label.
    pub state: String,
    /// Sequence identifier.
    pub seq: u64,
}

/// Lease preemption entry for Live Hive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmUiHiveLeasePreemption {
    /// Lease identifier.
    pub id: String,
    /// Subject identifier.
    pub subject: String,
    /// Resource identifier.
    pub resource: String,
    /// Preemption reason label.
    pub reason: String,
    /// Sequence identifier.
    pub seq: u64,
}

/// Lease snapshot for Live Hive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmUiHiveLeaseSnapshot {
    /// Optional summary line.
    #[serde(default)]
    pub summary: Option<SwarmUiHiveLeaseSummary>,
    /// Active leases.
    #[serde(default)]
    pub active: Vec<SwarmUiHiveLeaseEntry>,
    /// Lease preemptions.
    #[serde(default)]
    pub preemptions: Vec<SwarmUiHiveLeasePreemption>,
}

/// Serialized snapshot used for replay and offline inspection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmUiHiveSnapshot {
    /// Snapshot schema version.
    pub version: u8,
    /// Snapshot creation time in milliseconds.
    pub created_ms: u64,
    /// Agents included in the snapshot.
    pub agents: Vec<SwarmUiHiveAgent>,
    /// Event records included in the snapshot.
    pub events: Vec<SwarmUiHiveEvent>,
}

impl SwarmUiHiveSnapshot {
    /// Build a snapshot from a transcript payload.
    pub fn from_transcript(agent: &SwarmUiHiveAgent, transcript: &SwarmUiTranscript) -> Self {
        let mut seq = 0u64;
        let mut events = Vec::new();
        for line in &transcript.lines {
            if let Some(event) = parse_line_to_event(agent, line, &mut seq, DEFAULT_LINE_CAP_BYTES)
            {
                events.push(event);
            }
        }
        Self {
            version: HIVE_SNAPSHOT_VERSION,
            created_ms: now_ms(),
            agents: vec![agent.clone()],
            events,
        }
    }

    /// Validate snapshot version and bounds.
    pub fn validate(&self, max_events: usize) -> Result<(), String> {
        if self.version != HIVE_SNAPSHOT_VERSION {
            return Err(format!(
                "hive snapshot version {} unsupported",
                self.version
            ));
        }
        if self.events.len() > max_events {
            return Err(format!("hive snapshot exceeds max events ({})", max_events));
        }
        Ok(())
    }
}

/// Bootstrap payload for the Live Hive renderer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmUiHiveBootstrap {
    /// Known agents with namespace metadata.
    pub agents: Vec<SwarmUiHiveAgent>,
    /// Namespace roots used by SwarmUI panels.
    pub namespace_roots: Vec<String>,
    /// Hive renderer defaults.
    pub hive: SwarmUiHiveConfig,
    /// True when the bootstrap is a replay source.
    pub replay: bool,
}

/// Incremental hive event batch for UI polling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmUiHiveBatch {
    /// Event payloads to apply.
    pub events: Vec<SwarmUiHiveEvent>,
    /// Pressure ratio derived from backlog vs budget.
    pub pressure: f32,
    /// Queue depth after ingest.
    pub backlog: usize,
    /// Events dropped due to queue bounds.
    pub dropped: u64,
    /// Root reachability status snapshot.
    #[serde(default)]
    pub root: Option<SwarmUiHiveRootStatus>,
    /// Session summary snapshot.
    #[serde(default)]
    pub sessions: Option<SwarmUiHiveSessionSummary>,
    /// Pressure counter snapshot.
    #[serde(default)]
    pub pressure_counters: Option<SwarmUiHivePressureCounters>,
    /// Scheduler snapshot.
    #[serde(default)]
    pub schedule: Option<SwarmUiHiveScheduleSnapshot>,
    /// Lease snapshot.
    #[serde(default)]
    pub lease: Option<SwarmUiHiveLeaseSnapshot>,
    /// Per-agent overlay lines.
    #[serde(default)]
    pub overlays: Vec<SwarmUiHiveOverlay>,
    /// Selected agent detail panel payload.
    #[serde(default)]
    pub detail: Option<SwarmUiHiveDetail>,
    /// True when replay is complete.
    pub done: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct HiveReplay {
    snapshot: SwarmUiHiveSnapshot,
    cursor: usize,
}

impl HiveReplay {
    pub(crate) fn new(snapshot: SwarmUiHiveSnapshot) -> Self {
        Self {
            snapshot,
            cursor: 0,
        }
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, String> {
        let snapshot_result = serde_cbor::from_slice::<SwarmUiHiveSnapshot>(bytes);
        if let Ok(snapshot) = snapshot_result {
            return Ok(Self::new(snapshot));
        }
        let transcript = serde_cbor::from_slice::<SwarmUiTranscript>(bytes)
            .map_err(|err| format!("replay decode error: {err}"))?;
        let agent = SwarmUiHiveAgent {
            id: "worker-replay".to_owned(),
            role: "worker".to_owned(),
            namespace: "/worker/worker-replay".to_owned(),
        };
        Ok(Self::new(SwarmUiHiveSnapshot::from_transcript(
            &agent,
            &transcript,
        )))
    }

    pub(crate) fn bootstrap(
        &self,
        config: SwarmUiHiveConfig,
        roots: Vec<String>,
    ) -> SwarmUiHiveBootstrap {
        SwarmUiHiveBootstrap {
            agents: self.snapshot.agents.clone(),
            namespace_roots: roots,
            hive: config,
            replay: true,
        }
    }

    pub(crate) fn next_batch(&mut self, max_events: usize, budget: u32) -> SwarmUiHiveBatch {
        let remaining = self.snapshot.events.len().saturating_sub(self.cursor);
        let take = max_events.min(remaining);
        let events = if take == 0 {
            Vec::new()
        } else {
            self.snapshot.events[self.cursor..self.cursor + take].to_vec()
        };
        self.cursor = self.cursor.saturating_add(take);
        let backlog = self.snapshot.events.len().saturating_sub(self.cursor);
        let pressure = if budget == 0 {
            0.0
        } else {
            backlog as f32 / budget as f32
        };
        SwarmUiHiveBatch {
            events,
            pressure,
            backlog,
            dropped: 0,
            root: None,
            sessions: None,
            pressure_counters: None,
            schedule: None,
            lease: None,
            overlays: Vec::new(),
            detail: None,
            done: self.cursor >= self.snapshot.events.len(),
        }
    }

    pub(crate) fn reset(&mut self) {
        self.cursor = 0;
    }

    pub(crate) fn snapshot(&self) -> &SwarmUiHiveSnapshot {
        &self.snapshot
    }
}

#[derive(Debug, Clone)]
struct OverlayCache {
    revision: u64,
    lines: usize,
    items: Vec<SwarmUiHiveOverlay>,
}

#[derive(Debug, Clone)]
struct DetailCache {
    revision: u64,
    lines: usize,
    agent: String,
    detail: Option<SwarmUiHiveDetail>,
}

#[derive(Debug)]
pub(crate) struct HiveSessionState {
    workers: Vec<String>,
    roles: HashMap<String, String>,
    cursors: HashMap<String, HiveTelemetryCursor>,
    buffers: HashMap<String, BoundedLineBuffer>,
    tail_policy: TailPollPolicy,
    queue: VecDeque<SwarmUiHiveEvent>,
    seq: u64,
    dropped: u64,
    worker_cursor: usize,
    buffers_revision: u64,
    overlay_cache: Option<OverlayCache>,
    detail_cache: Option<DetailCache>,
}

impl HiveSessionState {
    pub(crate) fn new(
        workers: Vec<String>,
        roles: HashMap<String, String>,
        tail_policy: TailPollPolicy,
    ) -> Self {
        Self {
            workers,
            roles,
            cursors: HashMap::new(),
            buffers: HashMap::new(),
            tail_policy,
            queue: VecDeque::new(),
            seq: 0,
            dropped: 0,
            worker_cursor: 0,
            buffers_revision: 0,
            overlay_cache: None,
            detail_cache: None,
        }
    }

    pub(crate) fn ingest<T: Secure9pTransport>(
        &mut self,
        client: &mut CohClient<T>,
        worker_root: &str,
        msize: u32,
        config: &SwarmUiHiveConfig,
    ) -> Result<(), SwarmUiError> {
        let now_ms = now_ms();
        let mut budget = config.lod_event_budget as usize;
        let max_queue = config.pending_event_cap as usize;
        let worker_count = self.workers.len();
        if worker_count == 0 {
            return Ok(());
        }
        let pending_cap = config.pending_lines_per_worker as usize;
        let max_workers = (config.poll_workers_per_tick as usize).min(worker_count);
        let start_index = self.worker_cursor % worker_count;
        let mut processed = 0usize;
        for offset in 0..worker_count {
            if processed >= max_workers || budget == 0 {
                break;
            }
            let idx = (start_index + offset) % worker_count;
            let worker_id = self.workers[idx].clone();
            let cursor = match self.cursors.get_mut(&worker_id) {
                Some(cursor) => cursor,
                None => {
                    let path = format!("{worker_root}/{worker_id}/telemetry");
                    let fid = client
                        .open(&path, OpenMode::read_only())
                        .map_err(|err| SwarmUiError::Transport(err.to_string()))?;
                    self.cursors.entry(worker_id.clone()).or_insert_with(|| {
                        HiveTelemetryCursor::new(
                            &worker_id,
                            fid,
                            TailPoller::new(self.tail_policy, None),
                        )
                    })
                }
            };
            cursor.fill_pending(
                client,
                msize,
                budget,
                pending_cap,
                now_ms,
                &mut self.dropped,
            )?;
            let detail_lines = config.detail_lines as usize;
            let line_cap = config.line_cap_bytes as usize;
            let per_worker = config.per_worker_bytes as usize;
            let mut buffer = self
                .buffers
                .remove(&worker_id)
                .unwrap_or_else(|| BoundedLineBuffer::new(detail_lines, per_worker, line_cap));
            let role = self.roles.get(&worker_id).map(|value| value.as_str());
            let (consumed, touched) = cursor.drain_events(
                worker_root,
                &mut self.seq,
                &mut self.queue,
                budget,
                &mut buffer,
                role,
                config.line_cap_bytes as usize,
            );
            self.buffers.insert(worker_id.clone(), buffer);
            if touched {
                self.buffers_revision = self.buffers_revision.wrapping_add(1);
            }
            budget = budget.saturating_sub(consumed);
            self.trim_queue(max_queue);
            processed = processed.saturating_add(1);
        }
        if processed > 0 {
            self.worker_cursor = (start_index + processed) % worker_count;
        }
        Ok(())
    }

    pub(crate) fn drain(&mut self, max_events: usize) -> Vec<SwarmUiHiveEvent> {
        let mut events = Vec::new();
        for _ in 0..max_events {
            if let Some(event) = self.queue.pop_front() {
                events.push(event);
            } else {
                break;
            }
        }
        events
    }

    pub(crate) fn queue_len(&self) -> usize {
        self.queue.len()
    }

    pub(crate) fn dropped(&self) -> u64 {
        self.dropped
    }

    pub(crate) fn take_fids(&mut self) -> Vec<u32> {
        let fids = self.cursors.values().map(|cursor| cursor.fid).collect();
        self.cursors.clear();
        self.buffers.clear();
        self.queue.clear();
        self.seq = 0;
        self.dropped = 0;
        self.worker_cursor = 0;
        self.buffers_revision = 0;
        self.overlay_cache = None;
        self.detail_cache = None;
        fids
    }

    fn trim_queue(&mut self, max_queue: usize) {
        if max_queue == 0 {
            return;
        }
        while self.queue.len() > max_queue {
            let _ = self.queue.pop_front();
            self.dropped = self.dropped.saturating_add(1);
        }
    }

    pub(crate) fn overlays(&mut self, overlay_lines: usize) -> Vec<SwarmUiHiveOverlay> {
        if let Some(cache) = &self.overlay_cache {
            if cache.revision == self.buffers_revision && cache.lines == overlay_lines {
                return cache.items.clone();
            }
        }
        let mut items = self
            .buffers
            .iter()
            .filter(|(_, buffer)| !buffer.is_empty())
            .collect::<Vec<_>>();
        items.sort_by_key(|(agent, _)| *agent);
        let overlays = items
            .into_iter()
            .map(|(agent, buffer)| SwarmUiHiveOverlay {
                agent: (*agent).to_owned(),
                lines: buffer.tail(overlay_lines),
            })
            .collect::<Vec<_>>();
        self.overlay_cache = Some(OverlayCache {
            revision: self.buffers_revision,
            lines: overlay_lines,
            items: overlays.clone(),
        });
        overlays
    }

    pub(crate) fn detail(
        &mut self,
        agent: Option<&str>,
        detail_lines: usize,
    ) -> Option<SwarmUiHiveDetail> {
        let agent = agent?;
        if let Some(cache) = &self.detail_cache {
            if cache.revision == self.buffers_revision
                && cache.lines == detail_lines
                && cache.agent == agent
            {
                return cache.detail.clone();
            }
        }
        let buffer = self.buffers.get(agent)?;
        if buffer.is_empty() {
            self.detail_cache = Some(DetailCache {
                revision: self.buffers_revision,
                lines: detail_lines,
                agent: agent.to_owned(),
                detail: None,
            });
            return None;
        }
        let detail = SwarmUiHiveDetail {
            agent: agent.to_owned(),
            lines: buffer.tail(detail_lines),
        };
        let detail_clone = detail.clone();
        self.detail_cache = Some(DetailCache {
            revision: self.buffers_revision,
            lines: detail_lines,
            agent: agent.to_owned(),
            detail: Some(detail),
        });
        Some(detail_clone)
    }
}

#[derive(Debug)]
struct HiveTelemetryCursor {
    worker_id: String,
    fid: u32,
    offset: u64,
    buffer: Vec<u8>,
    pending: VecDeque<String>,
    poller: TailPoller,
}

impl HiveTelemetryCursor {
    fn new(worker_id: &str, fid: u32, poller: TailPoller) -> Self {
        Self {
            worker_id: worker_id.to_owned(),
            fid,
            offset: 0,
            buffer: Vec::new(),
            pending: VecDeque::new(),
            poller,
        }
    }

    fn fill_pending<T: Secure9pTransport>(
        &mut self,
        client: &mut CohClient<T>,
        msize: u32,
        _budget: usize,
        pending_cap: usize,
        now_ms: u64,
        dropped: &mut u64,
    ) -> Result<(), SwarmUiError> {
        if pending_cap == 0 || self.pending.len() >= pending_cap {
            return Ok(());
        }
        if !self.poller.should_poll(now_ms) {
            return Ok(());
        }
        let chunk = client
            .read(self.fid, self.offset, msize)
            .map_err(|err| SwarmUiError::Transport(err.to_string()))?;
        self.poller.mark_polled(now_ms);
        if chunk.is_empty() {
            return Ok(());
        }
        self.offset = self
            .offset
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| SwarmUiError::Transport("telemetry offset overflow".to_owned()))?;
        self.buffer.extend_from_slice(&chunk);
        self.extract_lines(pending_cap, dropped)?;
        Ok(())
    }

    fn drain_events(
        &mut self,
        worker_root: &str,
        seq: &mut u64,
        queue: &mut VecDeque<SwarmUiHiveEvent>,
        budget: usize,
        buffer: &mut BoundedLineBuffer,
        role: Option<&str>,
        line_cap_bytes: usize,
    ) -> (usize, bool) {
        let mut consumed = 0usize;
        let mut touched = false;
        let namespace = format!("{worker_root}/{}/telemetry", self.worker_id);
        while consumed < budget {
            let Some(line) = self.pending.pop_front() else {
                break;
            };
            let Some(normalized) = normalize_telemetry_line(&line) else {
                continue;
            };
            buffer.push_line(normalized);
            touched = true;
            if let Some(event) = parse_line_to_event_with_namespace(
                &self.worker_id,
                &namespace,
                role,
                normalized,
                seq,
                line_cap_bytes,
            ) {
                queue.push_back(event);
                consumed = consumed.saturating_add(1);
            }
        }
        (consumed, touched)
    }

    fn extract_lines(&mut self, pending_cap: usize, dropped: &mut u64) -> Result<(), SwarmUiError> {
        let mut start = 0usize;
        let mut idx = 0usize;
        while idx < self.buffer.len() {
            if self.buffer[idx] == b'\n' {
                let line_bytes = &self.buffer[start..idx];
                let line = decode_line(line_bytes)?;
                self.pending.push_back(line);
                if self.pending.len() > pending_cap {
                    let _ = self.pending.pop_front();
                    *dropped = dropped.saturating_add(1);
                }
                start = idx + 1;
            }
            idx += 1;
        }
        if start > 0 {
            let remaining = self.buffer.len().saturating_sub(start);
            self.buffer.copy_within(start.., 0);
            self.buffer.truncate(remaining);
        }
        Ok(())
    }
}

fn parse_line_to_event(
    agent: &SwarmUiHiveAgent,
    line: &str,
    seq: &mut u64,
    line_cap_bytes: usize,
) -> Option<SwarmUiHiveEvent> {
    parse_line_to_event_with_namespace(
        &agent.id,
        &agent.namespace,
        Some(agent.role.as_str()),
        line,
        seq,
        line_cap_bytes,
    )
}

pub(crate) fn parse_line_to_event_with_namespace(
    agent: &str,
    namespace: &str,
    role: Option<&str>,
    line: &str,
    seq: &mut u64,
    line_cap_bytes: usize,
) -> Option<SwarmUiHiveEvent> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with("OK ") || trimmed == "END" {
        return None;
    }
    let kind = if trimmed.starts_with("ERR") {
        SwarmUiHiveEventKind::Error
    } else {
        SwarmUiHiveEventKind::Telemetry
    };
    let reason = if matches!(kind, SwarmUiHiveEventKind::Error) {
        parse_error_reason(trimmed)
    } else {
        None
    };
    let detail = truncate_detail(trimmed, line_cap_bytes);
    let event = SwarmUiHiveEvent {
        seq: *seq,
        kind,
        reason,
        agent: agent.to_owned(),
        role: role.map(|value| value.to_owned()),
        namespace: namespace.to_owned(),
        detail,
    };
    *seq = seq.saturating_add(1);
    Some(event)
}

#[derive(Debug)]
pub(crate) struct ConsoleHiveSessionState {
    workers: Vec<String>,
    roles: HashMap<String, String>,
    queue: VecDeque<SwarmUiHiveEvent>,
    buffers: HashMap<String, BoundedLineBuffer>,
    pollers: HashMap<String, TailPoller>,
    tail_policy: TailPollPolicy,
    seq: u64,
    dropped: u64,
    worker_cursor: usize,
    buffers_revision: u64,
    overlay_cache: Option<OverlayCache>,
    detail_cache: Option<DetailCache>,
}

impl ConsoleHiveSessionState {
    pub(crate) fn new(
        workers: Vec<String>,
        roles: HashMap<String, String>,
        tail_policy: TailPollPolicy,
    ) -> Self {
        Self {
            workers,
            roles,
            queue: VecDeque::new(),
            buffers: HashMap::new(),
            pollers: HashMap::new(),
            tail_policy,
            seq: 0,
            dropped: 0,
            worker_cursor: 0,
            buffers_revision: 0,
            overlay_cache: None,
            detail_cache: None,
        }
    }

    pub(crate) fn ingest<T: Transport>(
        &mut self,
        transport: &mut T,
        session: &Session,
        worker_root: &str,
        config: &SwarmUiHiveConfig,
    ) -> Result<(), SwarmUiError> {
        let now_ms = now_ms();
        let mut budget = config.lod_event_budget as usize;
        let max_queue = config.pending_event_cap as usize;
        let worker_count = self.workers.len();
        if worker_count == 0 {
            return Ok(());
        }
        let pending_cap = config.pending_lines_per_worker as usize;
        let max_workers = (config.poll_workers_per_tick as usize).min(worker_count);
        let start_index = self.worker_cursor % worker_count;
        let mut processed = 0usize;
        for offset in 0..worker_count {
            if processed >= max_workers || budget == 0 {
                break;
            }
            let idx = (start_index + offset) % worker_count;
            let worker_id = self.workers[idx].clone();
            let poller = self
                .pollers
                .entry(worker_id.clone())
                .or_insert_with(|| TailPoller::new(self.tail_policy, None));
            if !poller.should_poll(now_ms) {
                processed = processed.saturating_add(1);
                continue;
            }
            let path = format!("{worker_root}/{worker_id}/telemetry");
            let mut lines = transport
                .tail(session, &path)
                .map_err(|err| SwarmUiError::Transport(err.to_string()))?;
            let _ = transport.drain_acknowledgements();
            poller.mark_polled(now_ms);
            if pending_cap > 0 && lines.len() > pending_cap {
                let keep_from = lines.len().saturating_sub(pending_cap);
                lines = lines.split_off(keep_from);
                self.dropped = self.dropped.saturating_add(keep_from as u64);
            }
            let namespace = format!("{worker_root}/{worker_id}/telemetry");
            let detail_lines = config.detail_lines as usize;
            let line_cap = config.line_cap_bytes as usize;
            let per_worker = config.per_worker_bytes as usize;
            let mut buffer = self
                .buffers
                .remove(&worker_id)
                .unwrap_or_else(|| BoundedLineBuffer::new(detail_lines, per_worker, line_cap));
            let role = self.roles.get(&worker_id).map(|value| value.as_str());
            let mut touched = false;
            for line in lines {
                if budget == 0 {
                    break;
                }
                let Some(normalized) = normalize_telemetry_line(&line) else {
                    continue;
                };
                buffer.push_line(normalized);
                touched = true;
                if let Some(event) = parse_line_to_event_with_namespace(
                    &worker_id,
                    &namespace,
                    role,
                    normalized,
                    &mut self.seq,
                    config.line_cap_bytes as usize,
                ) {
                    self.queue.push_back(event);
                    budget = budget.saturating_sub(1);
                }
            }
            self.buffers.insert(worker_id.clone(), buffer);
            if touched {
                self.buffers_revision = self.buffers_revision.wrapping_add(1);
            }
            self.trim_queue(max_queue);
            processed = processed.saturating_add(1);
        }
        if processed > 0 {
            self.worker_cursor = (start_index + processed) % worker_count;
        }
        Ok(())
    }

    #[cfg(feature = "rest")]
    pub(crate) fn ingest_rest_parallel(
        &mut self,
        worker_root: &str,
        config: &SwarmUiHiveConfig,
        rest_url: &str,
        request_auth_token: Option<&str>,
        parallel_limit: usize,
        role: Role,
        ticket: Option<&str>,
    ) -> Result<(), SwarmUiError> {
        let now_ms = now_ms();
        let mut budget = config.lod_event_budget as usize;
        let max_queue = config.pending_event_cap as usize;
        let worker_count = self.workers.len();
        if worker_count == 0 {
            return Ok(());
        }
        let pending_cap = config.pending_lines_per_worker as usize;
        let max_workers = (config.poll_workers_per_tick as usize).min(worker_count);
        let start_index = self.worker_cursor % worker_count;
        let mut processed = 0usize;
        let mut offset = 0usize;
        let parallel_limit = parallel_limit.max(1);
        let ticket_owned = ticket.map(str::to_owned);
        let request_auth_token_owned = request_auth_token.map(str::to_owned);

        'outer: while offset < worker_count && processed < max_workers && budget > 0 {
            let mut batch = Vec::new();
            while offset < worker_count
                && processed + batch.len() < max_workers
                && batch.len() < parallel_limit
                && budget > 0
            {
                let idx = (start_index + offset) % worker_count;
                let worker_id = self.workers[idx].clone();
                let poller = self
                    .pollers
                    .entry(worker_id.clone())
                    .or_insert_with(|| TailPoller::new(self.tail_policy, None));
                let should_poll = poller.should_poll(now_ms);
                batch.push((worker_id, should_poll));
                offset = offset.saturating_add(1);
            }

            let mut tail_results: HashMap<String, Vec<String>> = HashMap::new();
            let poll_targets = batch
                .iter()
                .filter(|(_, should_poll)| *should_poll)
                .map(|(worker_id, _)| worker_id.clone())
                .collect::<Vec<_>>();
            if !poll_targets.is_empty() {
                let rest_url = rest_url.to_owned();
                let chunk_results = thread::scope(|scope| {
                    let mut handles = Vec::with_capacity(poll_targets.len());
                    for worker_id in &poll_targets {
                        let rest_url = rest_url.clone();
                        let request_auth_token = request_auth_token_owned.clone();
                        let ticket = ticket_owned.clone();
                        let worker_id = worker_id.clone();
                        handles.push(scope.spawn(move || {
                            let mut transport =
                                CohshRestTransport::new(rest_url, request_auth_token);
                            let session = transport
                                .attach(role, ticket.as_deref())
                                .map_err(|err| SwarmUiError::Transport(err.to_string()))?;
                            let path = format!("{worker_root}/{worker_id}/telemetry");
                            let lines = transport
                                .tail(&session, &path)
                                .map_err(|err| SwarmUiError::Transport(err.to_string()))?;
                            Ok::<(String, Vec<String>), SwarmUiError>((worker_id, lines))
                        }));
                    }
                    let mut out = Vec::with_capacity(handles.len());
                    for handle in handles {
                        match handle.join() {
                            Ok(Ok(result)) => out.push(Ok(result)),
                            Ok(Err(err)) => out.push(Err(err)),
                            Err(_) => out.push(Err(SwarmUiError::Transport(
                                "rest tail thread panicked".to_owned(),
                            ))),
                        }
                    }
                    out
                });
                for result in chunk_results {
                    let (worker_id, lines) = result?;
                    tail_results.insert(worker_id, lines);
                }
            }

            for (worker_id, should_poll) in batch {
                if processed >= max_workers || budget == 0 {
                    break 'outer;
                }
                processed = processed.saturating_add(1);
                if !should_poll {
                    continue;
                }
                let Some(lines) = tail_results.remove(&worker_id) else {
                    return Err(SwarmUiError::Transport(format!(
                        "missing REST tail result for {worker_id}"
                    )));
                };
                let poller = self.pollers.get_mut(&worker_id).expect("poller exists");
                poller.mark_polled(now_ms);
                let mut lines = lines;
                if pending_cap > 0 && lines.len() > pending_cap {
                    let keep_from = lines.len().saturating_sub(pending_cap);
                    lines = lines.split_off(keep_from);
                    self.dropped = self.dropped.saturating_add(keep_from as u64);
                }
                let namespace = format!("{worker_root}/{worker_id}/telemetry");
                let detail_lines = config.detail_lines as usize;
                let line_cap = config.line_cap_bytes as usize;
                let per_worker = config.per_worker_bytes as usize;
                let mut buffer = self
                    .buffers
                    .remove(&worker_id)
                    .unwrap_or_else(|| BoundedLineBuffer::new(detail_lines, per_worker, line_cap));
                let role = self.roles.get(&worker_id).map(|value| value.as_str());
                let mut touched = false;
                for line in lines {
                    if budget == 0 {
                        break;
                    }
                    let Some(normalized) = normalize_telemetry_line(&line) else {
                        continue;
                    };
                    buffer.push_line(normalized);
                    touched = true;
                    if let Some(event) = parse_line_to_event_with_namespace(
                        &worker_id,
                        &namespace,
                        role,
                        normalized,
                        &mut self.seq,
                        config.line_cap_bytes as usize,
                    ) {
                        self.queue.push_back(event);
                        budget = budget.saturating_sub(1);
                    }
                }
                self.buffers.insert(worker_id.clone(), buffer);
                if touched {
                    self.buffers_revision = self.buffers_revision.wrapping_add(1);
                }
                self.trim_queue(max_queue);
            }
        }

        if processed > 0 {
            self.worker_cursor = (start_index + processed) % worker_count;
        }
        Ok(())
    }

    pub(crate) fn drain(&mut self, max_events: usize) -> Vec<SwarmUiHiveEvent> {
        let mut events = Vec::new();
        for _ in 0..max_events {
            if let Some(event) = self.queue.pop_front() {
                events.push(event);
            } else {
                break;
            }
        }
        events
    }

    pub(crate) fn queue_len(&self) -> usize {
        self.queue.len()
    }

    pub(crate) fn dropped(&self) -> u64 {
        self.dropped
    }

    pub(crate) fn reset(&mut self) {
        self.queue.clear();
        self.buffers.clear();
        self.pollers.clear();
        self.seq = 0;
        self.dropped = 0;
        self.worker_cursor = 0;
        self.buffers_revision = 0;
        self.overlay_cache = None;
        self.detail_cache = None;
    }

    pub(crate) fn overlays(&mut self, overlay_lines: usize) -> Vec<SwarmUiHiveOverlay> {
        if let Some(cache) = &self.overlay_cache {
            if cache.revision == self.buffers_revision && cache.lines == overlay_lines {
                return cache.items.clone();
            }
        }
        let mut items = self
            .buffers
            .iter()
            .filter(|(_, buffer)| !buffer.is_empty())
            .collect::<Vec<_>>();
        items.sort_by_key(|(agent, _)| *agent);
        let overlays = items
            .into_iter()
            .map(|(agent, buffer)| SwarmUiHiveOverlay {
                agent: (*agent).to_owned(),
                lines: buffer.tail(overlay_lines),
            })
            .collect::<Vec<_>>();
        self.overlay_cache = Some(OverlayCache {
            revision: self.buffers_revision,
            lines: overlay_lines,
            items: overlays.clone(),
        });
        overlays
    }

    pub(crate) fn detail(
        &mut self,
        agent: Option<&str>,
        detail_lines: usize,
    ) -> Option<SwarmUiHiveDetail> {
        let agent = agent?;
        if let Some(cache) = &self.detail_cache {
            if cache.revision == self.buffers_revision
                && cache.lines == detail_lines
                && cache.agent == agent
            {
                return cache.detail.clone();
            }
        }
        let buffer = self.buffers.get(agent)?;
        if buffer.is_empty() {
            self.detail_cache = Some(DetailCache {
                revision: self.buffers_revision,
                lines: detail_lines,
                agent: agent.to_owned(),
                detail: None,
            });
            return None;
        }
        let detail = SwarmUiHiveDetail {
            agent: agent.to_owned(),
            lines: buffer.tail(detail_lines),
        };
        let detail_clone = detail.clone();
        self.detail_cache = Some(DetailCache {
            revision: self.buffers_revision,
            lines: detail_lines,
            agent: agent.to_owned(),
            detail: Some(detail),
        });
        Some(detail_clone)
    }

    fn trim_queue(&mut self, max_queue: usize) {
        if max_queue == 0 {
            return;
        }
        while self.queue.len() > max_queue {
            let _ = self.queue.pop_front();
            self.dropped = self.dropped.saturating_add(1);
        }
    }
}

pub(crate) fn role_for_agent_id(id: &str) -> &str {
    if id.eq_ignore_ascii_case("queen") {
        return "queen";
    }
    let lower = id.to_ascii_lowercase();
    if lower.starts_with("worker-gpu") {
        return "worker-gpu";
    }
    if lower.starts_with("worker-lora") {
        return "worker-lora";
    }
    if lower.starts_with("worker-bus") {
        return "worker-bus";
    }
    if lower.starts_with("worker-heartbeat") || lower.starts_with("worker-heart") {
        return "worker-heartbeat";
    }
    "worker"
}

fn truncate_detail(line: &str, line_cap_bytes: usize) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(truncate_to_boundary(trimmed, line_cap_bytes))
}

fn normalize_telemetry_line(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with("OK ") || trimmed == "END" {
        return None;
    }
    Some(trimmed)
}

fn truncate_to_boundary(input: &str, max_bytes: usize) -> String {
    if input.len() <= max_bytes {
        return input.to_owned();
    }
    let mut end = 0usize;
    for (idx, ch) in input.char_indices() {
        let next = idx + ch.len_utf8();
        if next > max_bytes {
            break;
        }
        end = next;
    }
    input[..end].to_owned()
}

fn parse_error_reason(line: &str) -> Option<String> {
    for part in line.split_whitespace() {
        if let Some(value) = part.strip_prefix("reason=") {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return None;
            }
            return Some(trimmed.to_owned());
        }
    }
    None
}

fn decode_line(bytes: &[u8]) -> Result<String, SwarmUiError> {
    String::from_utf8(bytes.to_vec())
        .map_err(|_| SwarmUiError::Transport("telemetry line is not valid UTF-8".to_owned()))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
