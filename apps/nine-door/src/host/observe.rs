// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Observability providers for /proc/9p and /proc/ingest nodes.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::time::Instant;

use log::info;

use super::cbor::{CborError, CborWriter};
use super::namespace::Namespace;
use super::pipeline::PipelineMetrics;
use super::session::SessionPhase;
use super::ui::{UiProviderConfig, UI_MAX_STREAM_BYTES};
use crate::NineDoorError;
use secure9p_codec::SessionId;

/// Configuration for /proc/9p observability files.
#[derive(Debug, Clone, Copy)]
pub struct Proc9pConfig {
    /// Enable `/proc/9p/sessions`.
    pub sessions: bool,
    /// Enable `/proc/9p/outstanding`.
    pub outstanding: bool,
    /// Enable `/proc/9p/short_writes`.
    pub short_writes: bool,
    /// Maximum bytes for `/proc/9p/sessions` payload.
    pub sessions_bytes: usize,
    /// Maximum bytes for `/proc/9p/outstanding` payload.
    pub outstanding_bytes: usize,
    /// Maximum bytes for `/proc/9p/short_writes` payload.
    pub short_writes_bytes: usize,
}

impl Proc9pConfig {
    pub(crate) fn enabled(self) -> bool {
        self.sessions || self.outstanding || self.short_writes
    }
}

/// Configuration for `/proc/9p/session` observability files.
#[derive(Debug, Clone, Copy)]
pub struct Proc9pSessionConfig {
    /// Enable `/proc/9p/session/active`.
    pub active: bool,
    /// Enable `/proc/9p/session/<id>/state`.
    pub state: bool,
    /// Enable `/proc/9p/session/<id>/since_ms`.
    pub since_ms: bool,
    /// Enable `/proc/9p/session/<id>/owner`.
    pub owner: bool,
    /// Maximum bytes for `/proc/9p/session/active` payload.
    pub active_bytes: usize,
    /// Maximum bytes for `/proc/9p/session/<id>/state` payload.
    pub state_bytes: usize,
    /// Maximum bytes for `/proc/9p/session/<id>/since_ms` payload.
    pub since_ms_bytes: usize,
    /// Maximum bytes for `/proc/9p/session/<id>/owner` payload.
    pub owner_bytes: usize,
}

impl Proc9pSessionConfig {
    pub(crate) fn enabled(self) -> bool {
        self.active || self.state || self.since_ms || self.owner
    }
}

/// Configuration for /proc/ingest observability files.
#[derive(Debug, Clone, Copy)]
pub struct ProcIngestConfig {
    /// Enable `/proc/ingest/p50_ms`.
    pub p50_ms: bool,
    /// Enable `/proc/ingest/p95_ms`.
    pub p95_ms: bool,
    /// Enable `/proc/ingest/backpressure`.
    pub backpressure: bool,
    /// Enable `/proc/ingest/dropped`.
    pub dropped: bool,
    /// Enable `/proc/ingest/queued`.
    pub queued: bool,
    /// Enable `/proc/ingest/watch`.
    pub watch: bool,
    /// Maximum bytes for `/proc/ingest/p50_ms` payload.
    pub p50_ms_bytes: usize,
    /// Maximum bytes for `/proc/ingest/p95_ms` payload.
    pub p95_ms_bytes: usize,
    /// Maximum bytes for `/proc/ingest/backpressure` payload.
    pub backpressure_bytes: usize,
    /// Maximum bytes for `/proc/ingest/dropped` payload.
    pub dropped_bytes: usize,
    /// Maximum bytes for `/proc/ingest/queued` payload.
    pub queued_bytes: usize,
    /// Maximum retained entries for `/proc/ingest/watch`.
    pub watch_max_entries: usize,
    /// Maximum bytes per `/proc/ingest/watch` line.
    pub watch_line_bytes: usize,
    /// Minimum interval between watch samples in milliseconds.
    pub watch_min_interval_ms: u64,
    /// Rolling latency sample count for percentiles.
    pub latency_samples: usize,
    /// Allowed latency drift (milliseconds) for regression comparisons.
    pub latency_tolerance_ms: u64,
    /// Allowed counter drift for regression comparisons.
    pub counter_tolerance: u64,
}

impl ProcIngestConfig {
    pub(crate) fn enabled(self) -> bool {
        self.p50_ms || self.p95_ms || self.backpressure || self.dropped || self.queued || self.watch
    }
}

/// Configuration for `/proc/root` observability files.
#[derive(Debug, Clone, Copy)]
pub struct ProcRootConfig {
    /// Enable `/proc/root/reachable`.
    pub reachable: bool,
    /// Enable `/proc/root/last_seen_ms`.
    pub last_seen_ms: bool,
    /// Enable `/proc/root/cut_reason`.
    pub cut_reason: bool,
    /// Maximum bytes for `/proc/root/reachable` payload.
    pub reachable_bytes: usize,
    /// Maximum bytes for `/proc/root/last_seen_ms` payload.
    pub last_seen_ms_bytes: usize,
    /// Maximum bytes for `/proc/root/cut_reason` payload.
    pub cut_reason_bytes: usize,
}

impl ProcRootConfig {
    pub(crate) fn enabled(self) -> bool {
        self.reachable || self.last_seen_ms || self.cut_reason
    }
}

/// Configuration for `/proc/pressure` observability files.
#[derive(Debug, Clone, Copy)]
pub struct ProcPressureConfig {
    /// Enable `/proc/pressure/busy`.
    pub busy: bool,
    /// Enable `/proc/pressure/quota`.
    pub quota: bool,
    /// Enable `/proc/pressure/cut`.
    pub cut: bool,
    /// Enable `/proc/pressure/policy`.
    pub policy: bool,
    /// Maximum bytes for `/proc/pressure/busy` payload.
    pub busy_bytes: usize,
    /// Maximum bytes for `/proc/pressure/quota` payload.
    pub quota_bytes: usize,
    /// Maximum bytes for `/proc/pressure/cut` payload.
    pub cut_bytes: usize,
    /// Maximum bytes for `/proc/pressure/policy` payload.
    pub policy_bytes: usize,
}

impl ProcPressureConfig {
    pub(crate) fn enabled(self) -> bool {
        self.busy || self.quota || self.cut || self.policy
    }
}

/// Top-level observability configuration for the NineDoor host.
#[derive(Debug, Clone, Copy)]
pub struct ObserveConfig {
    /// `/proc/9p` observability settings.
    pub proc_9p: Proc9pConfig,
    /// `/proc/9p/session` observability settings.
    pub proc_9p_session: Proc9pSessionConfig,
    /// `/proc/ingest` observability settings.
    pub proc_ingest: ProcIngestConfig,
    /// `/proc/root` observability settings.
    pub proc_root: ProcRootConfig,
    /// `/proc/pressure` observability settings.
    pub proc_pressure: ProcPressureConfig,
}

impl ObserveConfig {
    /// Return true when any /proc observability nodes are enabled.
    pub fn enabled(self) -> bool {
        self.proc_9p.enabled()
            || self.proc_9p_session.enabled()
            || self.proc_ingest.enabled()
            || self.proc_root.enabled()
            || self.proc_pressure.enabled()
    }
}

impl Default for ObserveConfig {
    fn default() -> Self {
        Self {
            proc_9p: Proc9pConfig {
                sessions: true,
                outstanding: true,
                short_writes: true,
                sessions_bytes: 8192,
                outstanding_bytes: 128,
                short_writes_bytes: 128,
            },
            proc_9p_session: Proc9pSessionConfig {
                active: true,
                state: true,
                since_ms: true,
                owner: true,
                active_bytes: 128,
                state_bytes: 64,
                since_ms_bytes: 64,
                owner_bytes: 96,
            },
            proc_ingest: ProcIngestConfig {
                p50_ms: true,
                p95_ms: true,
                backpressure: true,
                dropped: true,
                queued: true,
                watch: true,
                p50_ms_bytes: 64,
                p95_ms_bytes: 64,
                backpressure_bytes: 64,
                dropped_bytes: 64,
                queued_bytes: 64,
                watch_max_entries: 16,
                watch_line_bytes: 192,
                watch_min_interval_ms: 50,
                latency_samples: 32,
                latency_tolerance_ms: 5,
                counter_tolerance: 1,
            },
            proc_root: ProcRootConfig {
                reachable: true,
                last_seen_ms: true,
                cut_reason: true,
                reachable_bytes: 32,
                last_seen_ms_bytes: 64,
                cut_reason_bytes: 64,
            },
            proc_pressure: ProcPressureConfig {
                busy: true,
                quota: true,
                cut: true,
                policy: true,
                busy_bytes: 64,
                quota_bytes: 64,
                cut_bytes: 64,
                policy_bytes: 64,
            },
        }
    }
}

/// Observability state for host-side /proc providers.
#[derive(Debug)]
pub struct ObserveState {
    config: ObserveConfig,
    ui: UiProviderConfig,
    start: Instant,
    ingest: IngestState,
    pressure: PressureState,
}

impl ObserveState {
    /// Create a new observability state seeded with the supplied configuration.
    pub fn new(config: ObserveConfig, ui: UiProviderConfig, start: Instant) -> Self {
        Self {
            ingest: IngestState::new(config.proc_ingest),
            pressure: PressureState::default(),
            config,
            ui,
            start,
        }
    }

    /// Return the active observability configuration.
    pub fn config(&self) -> ObserveConfig {
        self.config
    }

    /// Convert a timestamp to milliseconds since the observability start.
    pub fn elapsed_ms(&self, now: Instant) -> u64 {
        now.duration_since(self.start).as_millis() as u64
    }

    /// Record a pressure event.
    pub fn record_pressure(&mut self, kind: PressureKind) {
        self.pressure.record(kind);
    }

    /// Snapshot current pressure counters.
    pub fn pressure_snapshot(&self) -> PressureSnapshot {
        self.pressure.snapshot()
    }

    /// Update /proc/9p/sessions with the supplied session counts.
    pub fn update_sessions(
        &self,
        namespace: &mut Namespace,
        total_sessions: usize,
        worker_sessions: usize,
        shard_bits: u8,
        shard_labels: &[String],
        shard_counts: &[usize],
    ) -> Result<(), NineDoorError> {
        if !self.config.proc_9p.sessions || !self.ui.proc_9p.sessions {
            return Ok(());
        }
        let mut payload = String::new();
        let _ = writeln!(
            payload,
            "sessions total={} worker={} shard_bits={} shard_count={}",
            total_sessions,
            worker_sessions,
            shard_bits,
            shard_labels.len()
        );
        for (idx, label) in shard_labels.iter().enumerate() {
            let count = shard_counts.get(idx).copied().unwrap_or(0);
            let _ = writeln!(payload, "shard {} {}", label, count);
        }
        ensure_len(
            "proc/9p/sessions",
            payload.len(),
            self.config.proc_9p.sessions_bytes,
        )?;
        ensure_stream_len("proc/9p/sessions", payload.len())?;
        namespace.set_proc_sessions_payload(payload.as_bytes())?;

        let cbor = build_proc_9p_sessions_cbor(
            total_sessions as u64,
            worker_sessions as u64,
            shard_bits,
            shard_labels,
            shard_counts,
        )?;
        ensure_stream_len("proc/9p/sessions.cbor", cbor.len())?;
        namespace.set_proc_sessions_cbor_payload(&cbor)
    }

    /// Update `/proc/9p/session/active` with active/draining counts.
    pub fn update_proc_9p_session_active(
        &self,
        namespace: &mut Namespace,
        active: usize,
        draining: usize,
    ) -> Result<(), NineDoorError> {
        if !self.config.proc_9p_session.active {
            return Ok(());
        }
        let mut line = String::new();
        let _ = writeln!(line, "active={} draining={}", active, draining);
        ensure_len(
            "proc/9p/session/active",
            line.len(),
            self.config.proc_9p_session.active_bytes,
        )?;
        ensure_stream_len("proc/9p/session/active", line.len())?;
        namespace.set_proc_session_active_payload(line.as_bytes())
    }

    /// Update `/proc/9p/session/<id>/*` for the supplied session snapshot.
    pub fn update_proc_9p_session_entry(
        &self,
        namespace: &mut Namespace,
        session: SessionId,
        phase: SessionPhase,
        since_ms: u64,
        owner: Option<&str>,
    ) -> Result<(), NineDoorError> {
        if !self.config.proc_9p_session.enabled() {
            return Ok(());
        }
        namespace.ensure_proc_session_entry(session, self.config.proc_9p_session)?;
        if self.config.proc_9p_session.state {
            let mut line = String::new();
            let _ = writeln!(line, "state={}", phase.as_str());
            ensure_len(
                "proc/9p/session/<id>/state",
                line.len(),
                self.config.proc_9p_session.state_bytes,
            )?;
            ensure_stream_len("proc/9p/session/<id>/state", line.len())?;
            namespace.set_proc_session_state_payload(session, line.as_bytes())?;
        }
        if self.config.proc_9p_session.since_ms {
            let mut line = String::new();
            let _ = writeln!(line, "since_ms={since_ms}");
            ensure_len(
                "proc/9p/session/<id>/since_ms",
                line.len(),
                self.config.proc_9p_session.since_ms_bytes,
            )?;
            ensure_stream_len("proc/9p/session/<id>/since_ms", line.len())?;
            namespace.set_proc_session_since_payload(session, line.as_bytes())?;
        }
        if self.config.proc_9p_session.owner {
            let label = owner.unwrap_or("none");
            let mut line = String::new();
            let _ = writeln!(line, "owner={label}");
            ensure_len(
                "proc/9p/session/<id>/owner",
                line.len(),
                self.config.proc_9p_session.owner_bytes,
            )?;
            ensure_stream_len("proc/9p/session/<id>/owner", line.len())?;
            namespace.set_proc_session_owner_payload(session, line.as_bytes())?;
        }
        Ok(())
    }

    /// Update /proc/9p metrics derived from pipeline state.
    pub fn update_proc_9p(
        &self,
        namespace: &mut Namespace,
        metrics: PipelineMetrics,
    ) -> Result<(), NineDoorError> {
        if self.config.proc_9p.outstanding && self.ui.proc_9p.outstanding {
            let mut line = String::new();
            let _ = writeln!(
                line,
                "outstanding current={} limit={}",
                metrics.queue_depth, metrics.queue_limit
            );
            ensure_len(
                "proc/9p/outstanding",
                line.len(),
                self.config.proc_9p.outstanding_bytes,
            )?;
            ensure_stream_len("proc/9p/outstanding", line.len())?;
            namespace.set_proc_outstanding_payload(line.as_bytes())?;

            let cbor = build_proc_9p_outstanding_cbor(
                metrics.queue_depth as u64,
                metrics.queue_limit as u64,
            )?;
            ensure_stream_len("proc/9p/outstanding.cbor", cbor.len())?;
            namespace.set_proc_outstanding_cbor_payload(&cbor)?;
        }
        if self.config.proc_9p.short_writes && self.ui.proc_9p.short_writes {
            let mut line = String::new();
            let _ = writeln!(
                line,
                "short_writes total={} retries={}",
                metrics.short_writes, metrics.short_write_retries
            );
            ensure_len(
                "proc/9p/short_writes",
                line.len(),
                self.config.proc_9p.short_writes_bytes,
            )?;
            ensure_stream_len("proc/9p/short_writes", line.len())?;
            namespace.set_proc_short_writes_payload(line.as_bytes())?;

            let cbor =
                build_proc_9p_short_writes_cbor(metrics.short_writes, metrics.short_write_retries)?;
            ensure_stream_len("proc/9p/short_writes.cbor", cbor.len())?;
            namespace.set_proc_short_writes_cbor_payload(&cbor)?;
        }
        Ok(())
    }

    /// Record an ingest latency sample in milliseconds.
    pub fn record_ingest_latency(&mut self, latency_ms: u32) {
        self.ingest.latency.record(latency_ms);
    }

    /// Increment the dropped ingest counter by the supplied amount.
    pub fn record_ingest_dropped(&mut self, dropped: u64) {
        if dropped == 0 {
            return;
        }
        self.ingest.dropped = self.ingest.dropped.saturating_add(dropped);
    }

    /// Update /proc/ingest metrics and watch snapshots.
    pub fn update_proc_ingest(
        &mut self,
        namespace: &mut Namespace,
        now: Instant,
        metrics: PipelineMetrics,
    ) -> Result<(), NineDoorError> {
        let config = self.config.proc_ingest;
        if !config.enabled() {
            return Ok(());
        }
        let now_ms = now
            .duration_since(self.start)
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX);
        let snapshot = self.ingest.snapshot(metrics);
        if config.p50_ms && self.ui.proc_ingest.p50_ms {
            let mut line = String::new();
            let _ = writeln!(line, "p50_ms={}", snapshot.p50_ms);
            ensure_len("proc/ingest/p50_ms", line.len(), config.p50_ms_bytes)?;
            ensure_stream_len("proc/ingest/p50_ms", line.len())?;
            namespace.set_proc_ingest_p50_payload(line.as_bytes())?;

            let cbor = build_proc_ingest_p50_cbor(snapshot.p50_ms)?;
            ensure_stream_len("proc/ingest/p50_ms.cbor", cbor.len())?;
            namespace.set_proc_ingest_p50_cbor_payload(&cbor)?;
        }
        if config.p95_ms && self.ui.proc_ingest.p95_ms {
            let mut line = String::new();
            let _ = writeln!(line, "p95_ms={}", snapshot.p95_ms);
            ensure_len("proc/ingest/p95_ms", line.len(), config.p95_ms_bytes)?;
            ensure_stream_len("proc/ingest/p95_ms", line.len())?;
            namespace.set_proc_ingest_p95_payload(line.as_bytes())?;

            let cbor = build_proc_ingest_p95_cbor(snapshot.p95_ms)?;
            ensure_stream_len("proc/ingest/p95_ms.cbor", cbor.len())?;
            namespace.set_proc_ingest_p95_cbor_payload(&cbor)?;
        }
        if config.backpressure && self.ui.proc_ingest.backpressure {
            let mut line = String::new();
            let _ = writeln!(line, "backpressure={}", snapshot.backpressure);
            ensure_len(
                "proc/ingest/backpressure",
                line.len(),
                config.backpressure_bytes,
            )?;
            ensure_stream_len("proc/ingest/backpressure", line.len())?;
            namespace.set_proc_ingest_backpressure_payload(line.as_bytes())?;

            let cbor = build_proc_ingest_backpressure_cbor(snapshot.backpressure)?;
            ensure_stream_len("proc/ingest/backpressure.cbor", cbor.len())?;
            namespace.set_proc_ingest_backpressure_cbor_payload(&cbor)?;
        }
        if config.dropped {
            let mut line = String::new();
            let _ = writeln!(line, "dropped={}", snapshot.dropped);
            ensure_len("proc/ingest/dropped", line.len(), config.dropped_bytes)?;
            namespace.set_proc_ingest_dropped_payload(line.as_bytes())?;
        }
        if config.queued {
            let mut line = String::new();
            let _ = writeln!(line, "queued={}", snapshot.queued);
            ensure_len("proc/ingest/queued", line.len(), config.queued_bytes)?;
            namespace.set_proc_ingest_queued_payload(line.as_bytes())?;
        }
        if config.watch {
            if let WatchAppend::Throttled(delay_ms) = self.ingest.watch.try_append(
                now_ms,
                config,
                snapshot.p50_ms,
                snapshot.p95_ms,
                snapshot.queued,
                snapshot.backpressure,
                snapshot.dropped,
                snapshot.ui_reads,
                snapshot.ui_denies,
            ) {
                info!(
                    "[observe] ingest watch throttled delay_ms={delay_ms} min_interval_ms={}",
                    config.watch_min_interval_ms
                );
            }
            let payload = self.ingest.watch.render();
            namespace.set_proc_ingest_watch_payload(payload.as_bytes())?;
        }
        Ok(())
    }

    /// Update `/proc/root/*` observability nodes.
    pub fn update_proc_root(
        &self,
        namespace: &mut Namespace,
        reachable: bool,
        last_seen_ms: u64,
        cut_reason: &str,
    ) -> Result<(), NineDoorError> {
        let config = self.config.proc_root;
        if !config.enabled() {
            return Ok(());
        }
        if config.reachable {
            let mut line = String::new();
            let value = if reachable { "yes" } else { "no" };
            let _ = writeln!(line, "reachable={value}");
            ensure_len("proc/root/reachable", line.len(), config.reachable_bytes)?;
            ensure_stream_len("proc/root/reachable", line.len())?;
            namespace.set_proc_root_reachable_payload(line.as_bytes())?;
        }
        if config.last_seen_ms {
            let mut line = String::new();
            let _ = writeln!(line, "last_seen_ms={last_seen_ms}");
            ensure_len(
                "proc/root/last_seen_ms",
                line.len(),
                config.last_seen_ms_bytes,
            )?;
            ensure_stream_len("proc/root/last_seen_ms", line.len())?;
            namespace.set_proc_root_last_seen_payload(line.as_bytes())?;
        }
        if config.cut_reason {
            let mut line = String::new();
            let _ = writeln!(line, "cut_reason={cut_reason}");
            ensure_len("proc/root/cut_reason", line.len(), config.cut_reason_bytes)?;
            ensure_stream_len("proc/root/cut_reason", line.len())?;
            namespace.set_proc_root_cut_reason_payload(line.as_bytes())?;
        }
        Ok(())
    }

    /// Update `/proc/pressure/*` observability nodes.
    pub fn update_proc_pressure(&self, namespace: &mut Namespace) -> Result<(), NineDoorError> {
        let config = self.config.proc_pressure;
        if !config.enabled() {
            return Ok(());
        }
        let snapshot = self.pressure.snapshot();
        if config.busy {
            let mut line = String::new();
            let _ = writeln!(line, "busy={}", snapshot.busy);
            ensure_len("proc/pressure/busy", line.len(), config.busy_bytes)?;
            ensure_stream_len("proc/pressure/busy", line.len())?;
            namespace.set_proc_pressure_busy_payload(line.as_bytes())?;
        }
        if config.quota {
            let mut line = String::new();
            let _ = writeln!(line, "quota={}", snapshot.quota);
            ensure_len("proc/pressure/quota", line.len(), config.quota_bytes)?;
            ensure_stream_len("proc/pressure/quota", line.len())?;
            namespace.set_proc_pressure_quota_payload(line.as_bytes())?;
        }
        if config.cut {
            let mut line = String::new();
            let _ = writeln!(line, "cut={}", snapshot.cut);
            ensure_len("proc/pressure/cut", line.len(), config.cut_bytes)?;
            ensure_stream_len("proc/pressure/cut", line.len())?;
            namespace.set_proc_pressure_cut_payload(line.as_bytes())?;
        }
        if config.policy {
            let mut line = String::new();
            let _ = writeln!(line, "policy={}", snapshot.policy);
            ensure_len("proc/pressure/policy", line.len(), config.policy_bytes)?;
            ensure_stream_len("proc/pressure/policy", line.len())?;
            namespace.set_proc_pressure_policy_payload(line.as_bytes())?;
        }
        Ok(())
    }
}

/// Pressure counter buckets for refusal tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PressureKind {
    /// Queue or processing backpressure.
    Busy,
    /// Quota or rate limiting.
    Quota,
    /// Root or session cut.
    Cut,
    /// Policy-based refusal.
    Policy,
}

/// Snapshot of pressure counters for `/proc/pressure/*`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PressureSnapshot {
    /// Busy pressure events.
    pub busy: u64,
    /// Quota pressure events.
    pub quota: u64,
    /// Cut pressure events.
    pub cut: u64,
    /// Policy pressure events.
    pub policy: u64,
}

#[derive(Debug, Default)]
struct PressureState {
    busy: u64,
    quota: u64,
    cut: u64,
    policy: u64,
}

impl PressureState {
    fn record(&mut self, kind: PressureKind) {
        match kind {
            PressureKind::Busy => {
                self.busy = self.busy.saturating_add(1);
            }
            PressureKind::Quota => {
                self.quota = self.quota.saturating_add(1);
            }
            PressureKind::Cut => {
                self.cut = self.cut.saturating_add(1);
            }
            PressureKind::Policy => {
                self.policy = self.policy.saturating_add(1);
            }
        }
    }

    fn snapshot(&self) -> PressureSnapshot {
        PressureSnapshot {
            busy: self.busy,
            quota: self.quota,
            cut: self.cut,
            policy: self.policy,
        }
    }
}

#[derive(Debug)]
struct IngestState {
    latency: LatencySamples,
    dropped: u64,
    watch: WatchRing,
}

impl IngestState {
    fn new(config: ProcIngestConfig) -> Self {
        Self {
            latency: LatencySamples::new(config.latency_samples),
            dropped: 0,
            watch: WatchRing::new(
                config.watch_max_entries,
                config.watch_line_bytes,
                config.watch_min_interval_ms,
            ),
        }
    }

    fn snapshot(&self, metrics: PipelineMetrics) -> IngestSnapshot {
        IngestSnapshot {
            p50_ms: self.latency.percentile(50),
            p95_ms: self.latency.percentile(95),
            backpressure: metrics.backpressure_events,
            dropped: self.dropped,
            queued: metrics.queue_depth as u32,
            ui_reads: metrics.ui_reads,
            ui_denies: metrics.ui_denies,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct IngestSnapshot {
    p50_ms: u32,
    p95_ms: u32,
    backpressure: u64,
    dropped: u64,
    queued: u32,
    ui_reads: u64,
    ui_denies: u64,
}

#[derive(Debug)]
struct LatencySamples {
    samples: Vec<u32>,
    capacity: usize,
    next: usize,
}

impl LatencySamples {
    fn new(capacity: usize) -> Self {
        Self {
            samples: Vec::with_capacity(capacity),
            capacity,
            next: 0,
        }
    }

    fn record(&mut self, sample: u32) {
        if self.capacity == 0 {
            return;
        }
        if self.samples.len() < self.capacity {
            self.samples.push(sample);
            if self.samples.len() == self.capacity {
                self.next = 0;
            }
            return;
        }
        self.samples[self.next] = sample;
        self.next = (self.next + 1) % self.capacity;
    }

    fn percentile(&self, pct: u32) -> u32 {
        if self.samples.is_empty() {
            return 0;
        }
        let mut values = self.samples.clone();
        values.sort_unstable();
        let idx = (values.len().saturating_sub(1) * pct as usize) / 100;
        values.get(idx).copied().unwrap_or(0)
    }
}

#[derive(Debug)]
struct WatchRing {
    entries: VecDeque<String>,
    max_entries: usize,
    line_bytes: usize,
    min_interval_ms: u64,
    last_emit_ms: Option<u64>,
}

impl WatchRing {
    fn new(max_entries: usize, line_bytes: usize, min_interval_ms: u64) -> Self {
        Self {
            entries: VecDeque::with_capacity(max_entries),
            max_entries,
            line_bytes,
            min_interval_ms,
            last_emit_ms: None,
        }
    }

    fn try_append(
        &mut self,
        now_ms: u64,
        config: ProcIngestConfig,
        p50_ms: u32,
        p95_ms: u32,
        queued: u32,
        backpressure: u64,
        dropped: u64,
        ui_reads: u64,
        ui_denies: u64,
    ) -> WatchAppend {
        if !config.watch || self.max_entries == 0 || self.line_bytes == 0 {
            return WatchAppend::Disabled;
        }
        if let Some(last) = self.last_emit_ms {
            if now_ms < last.saturating_add(self.min_interval_ms) {
                let delay_ms = last
                    .saturating_add(self.min_interval_ms)
                    .saturating_sub(now_ms);
                return WatchAppend::Throttled(delay_ms);
            }
        }
        let mut line = String::new();
        let _ = writeln!(
            line,
            "watch ts_ms={} p50_ms={} p95_ms={} queued={} backpressure={} dropped={} ui_reads={} ui_denies={}",
            now_ms, p50_ms, p95_ms, queued, backpressure, dropped, ui_reads, ui_denies
        );
        if line.len() > self.line_bytes {
            return WatchAppend::Disabled;
        }
        if self.entries.len() == self.max_entries {
            self.entries.pop_front();
        }
        self.entries.push_back(line);
        self.last_emit_ms = Some(now_ms);
        WatchAppend::Appended
    }

    fn render(&self) -> String {
        if self.entries.is_empty() {
            return String::new();
        }
        let mut buffer = String::with_capacity(self.max_entries.saturating_mul(self.line_bytes));
        for entry in &self.entries {
            buffer.push_str(entry);
        }
        buffer
    }
}

#[derive(Debug)]
enum WatchAppend {
    Appended,
    Throttled(u64),
    Disabled,
}

fn ensure_len(label: &str, len: usize, max: usize) -> Result<(), NineDoorError> {
    if len > max {
        return Err(NineDoorError::protocol(
            secure9p_codec::ErrorCode::TooBig,
            format!("{label} output exceeds {max} bytes"),
        ));
    }
    Ok(())
}

fn ensure_stream_len(label: &str, len: usize) -> Result<(), NineDoorError> {
    if len > UI_MAX_STREAM_BYTES {
        return Err(NineDoorError::protocol(
            secure9p_codec::ErrorCode::TooBig,
            format!("{label} output exceeds {} bytes", UI_MAX_STREAM_BYTES),
        ));
    }
    Ok(())
}

fn build_proc_9p_sessions_cbor(
    total: u64,
    worker: u64,
    shard_bits: u8,
    shard_labels: &[String],
    shard_counts: &[usize],
) -> Result<Vec<u8>, NineDoorError> {
    let mut writer = CborWriter::new(UI_MAX_STREAM_BYTES);
    writer
        .map(5)
        .map_err(|err| cbor_error("proc/9p/sessions.cbor", err))?;
    writer
        .text("total")
        .and_then(|_| writer.unsigned(total))
        .map_err(|err| cbor_error("proc/9p/sessions.cbor", err))?;
    writer
        .text("worker")
        .and_then(|_| writer.unsigned(worker))
        .map_err(|err| cbor_error("proc/9p/sessions.cbor", err))?;
    writer
        .text("shard_bits")
        .and_then(|_| writer.unsigned(shard_bits as u64))
        .map_err(|err| cbor_error("proc/9p/sessions.cbor", err))?;
    writer
        .text("shard_count")
        .and_then(|_| writer.unsigned(shard_labels.len() as u64))
        .map_err(|err| cbor_error("proc/9p/sessions.cbor", err))?;
    writer
        .text("shards")
        .and_then(|_| writer.array(shard_labels.len()))
        .map_err(|err| cbor_error("proc/9p/sessions.cbor", err))?;
    for (idx, label) in shard_labels.iter().enumerate() {
        let count = shard_counts.get(idx).copied().unwrap_or(0) as u64;
        writer
            .map(2)
            .and_then(|_| writer.text("label"))
            .and_then(|_| writer.text(label))
            .and_then(|_| writer.text("count"))
            .and_then(|_| writer.unsigned(count))
            .map_err(|err| cbor_error("proc/9p/sessions.cbor", err))?;
    }
    Ok(writer.into_bytes())
}

fn build_proc_9p_outstanding_cbor(current: u64, limit: u64) -> Result<Vec<u8>, NineDoorError> {
    let mut writer = CborWriter::new(UI_MAX_STREAM_BYTES);
    writer
        .map(2)
        .map_err(|err| cbor_error("proc/9p/outstanding.cbor", err))?;
    writer
        .text("current")
        .and_then(|_| writer.unsigned(current))
        .map_err(|err| cbor_error("proc/9p/outstanding.cbor", err))?;
    writer
        .text("limit")
        .and_then(|_| writer.unsigned(limit))
        .map_err(|err| cbor_error("proc/9p/outstanding.cbor", err))?;
    Ok(writer.into_bytes())
}

fn build_proc_9p_short_writes_cbor(total: u64, retries: u64) -> Result<Vec<u8>, NineDoorError> {
    let mut writer = CborWriter::new(UI_MAX_STREAM_BYTES);
    writer
        .map(2)
        .map_err(|err| cbor_error("proc/9p/short_writes.cbor", err))?;
    writer
        .text("total")
        .and_then(|_| writer.unsigned(total))
        .map_err(|err| cbor_error("proc/9p/short_writes.cbor", err))?;
    writer
        .text("retries")
        .and_then(|_| writer.unsigned(retries))
        .map_err(|err| cbor_error("proc/9p/short_writes.cbor", err))?;
    Ok(writer.into_bytes())
}

fn build_proc_ingest_p50_cbor(value: u32) -> Result<Vec<u8>, NineDoorError> {
    let mut writer = CborWriter::new(UI_MAX_STREAM_BYTES);
    writer
        .map(1)
        .map_err(|err| cbor_error("proc/ingest/p50_ms.cbor", err))?;
    writer
        .text("p50_ms")
        .and_then(|_| writer.unsigned(value as u64))
        .map_err(|err| cbor_error("proc/ingest/p50_ms.cbor", err))?;
    Ok(writer.into_bytes())
}

fn build_proc_ingest_p95_cbor(value: u32) -> Result<Vec<u8>, NineDoorError> {
    let mut writer = CborWriter::new(UI_MAX_STREAM_BYTES);
    writer
        .map(1)
        .map_err(|err| cbor_error("proc/ingest/p95_ms.cbor", err))?;
    writer
        .text("p95_ms")
        .and_then(|_| writer.unsigned(value as u64))
        .map_err(|err| cbor_error("proc/ingest/p95_ms.cbor", err))?;
    Ok(writer.into_bytes())
}

fn build_proc_ingest_backpressure_cbor(value: u64) -> Result<Vec<u8>, NineDoorError> {
    let mut writer = CborWriter::new(UI_MAX_STREAM_BYTES);
    writer
        .map(1)
        .map_err(|err| cbor_error("proc/ingest/backpressure.cbor", err))?;
    writer
        .text("backpressure")
        .and_then(|_| writer.unsigned(value))
        .map_err(|err| cbor_error("proc/ingest/backpressure.cbor", err))?;
    Ok(writer.into_bytes())
}

fn cbor_error(label: &str, err: CborError) -> NineDoorError {
    match err {
        CborError::TooLarge => NineDoorError::protocol(
            secure9p_codec::ErrorCode::TooBig,
            format!("{label} output exceeds {} bytes", UI_MAX_STREAM_BYTES),
        ),
    }
}
