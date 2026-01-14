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

use super::pipeline::PipelineMetrics;
use super::namespace::Namespace;
use crate::NineDoorError;

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
        self.p50_ms
            || self.p95_ms
            || self.backpressure
            || self.dropped
            || self.queued
            || self.watch
    }
}

/// Top-level observability configuration for the NineDoor host.
#[derive(Debug, Clone, Copy)]
pub struct ObserveConfig {
    /// `/proc/9p` observability settings.
    pub proc_9p: Proc9pConfig,
    /// `/proc/ingest` observability settings.
    pub proc_ingest: ProcIngestConfig,
}

impl ObserveConfig {
    /// Return true when any /proc observability nodes are enabled.
    pub fn enabled(self) -> bool {
        self.proc_9p.enabled() || self.proc_ingest.enabled()
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
        }
    }
}

/// Observability state for host-side /proc providers.
#[derive(Debug)]
pub struct ObserveState {
    config: ObserveConfig,
    start: Instant,
    ingest: IngestState,
}

impl ObserveState {
    /// Create a new observability state seeded with the supplied configuration.
    pub fn new(config: ObserveConfig, start: Instant) -> Self {
        Self {
            ingest: IngestState::new(config.proc_ingest),
            config,
            start,
        }
    }

    /// Return the active observability configuration.
    pub fn config(&self) -> ObserveConfig {
        self.config
    }

    /// Update /proc/9p/sessions with the supplied session counts.
    pub fn update_sessions(
        &self,
        namespace: &mut Namespace,
        payload: &str,
    ) -> Result<(), NineDoorError> {
        if !self.config.proc_9p.sessions {
            return Ok(());
        }
        ensure_len(
            "proc/9p/sessions",
            payload.len(),
            self.config.proc_9p.sessions_bytes,
        )?;
        namespace.set_proc_sessions_payload(payload.as_bytes())
    }

    /// Update /proc/9p metrics derived from pipeline state.
    pub fn update_proc_9p(
        &self,
        namespace: &mut Namespace,
        metrics: PipelineMetrics,
    ) -> Result<(), NineDoorError> {
        if self.config.proc_9p.outstanding {
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
            namespace.set_proc_outstanding_payload(line.as_bytes())?;
        }
        if self.config.proc_9p.short_writes {
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
            namespace.set_proc_short_writes_payload(line.as_bytes())?;
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
        if config.p50_ms {
            let mut line = String::new();
            let _ = writeln!(line, "p50_ms={}", snapshot.p50_ms);
            ensure_len("proc/ingest/p50_ms", line.len(), config.p50_ms_bytes)?;
            namespace.set_proc_ingest_p50_payload(line.as_bytes())?;
        }
        if config.p95_ms {
            let mut line = String::new();
            let _ = writeln!(line, "p95_ms={}", snapshot.p95_ms);
            ensure_len("proc/ingest/p95_ms", line.len(), config.p95_ms_bytes)?;
            namespace.set_proc_ingest_p95_payload(line.as_bytes())?;
        }
        if config.backpressure {
            let mut line = String::new();
            let _ = writeln!(line, "backpressure={}", snapshot.backpressure);
            ensure_len(
                "proc/ingest/backpressure",
                line.len(),
                config.backpressure_bytes,
            )?;
            namespace.set_proc_ingest_backpressure_payload(line.as_bytes())?;
        }
        if config.dropped {
            let mut line = String::new();
            let _ = writeln!(line, "dropped={}", snapshot.dropped);
            ensure_len(
                "proc/ingest/dropped",
                line.len(),
                config.dropped_bytes,
            )?;
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
            watch: WatchRing::new(config.watch_max_entries, config.watch_line_bytes, config.watch_min_interval_ms),
        }
    }

    fn snapshot(&self, metrics: PipelineMetrics) -> IngestSnapshot {
        IngestSnapshot {
            p50_ms: self.latency.percentile(50),
            p95_ms: self.latency.percentile(95),
            backpressure: metrics.backpressure_events,
            dropped: self.dropped,
            queued: metrics.queue_depth as u32,
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
    ) -> WatchAppend {
        if !config.watch || self.max_entries == 0 || self.line_bytes == 0 {
            return WatchAppend::Disabled;
        }
        if let Some(last) = self.last_emit_ms {
            if now_ms < last.saturating_add(self.min_interval_ms) {
                let delay_ms = last.saturating_add(self.min_interval_ms).saturating_sub(now_ms);
                return WatchAppend::Throttled(delay_ms);
            }
        }
        let mut line = String::new();
        let _ = writeln!(
            line,
            "watch ts_ms={} p50_ms={} p95_ms={} queued={} backpressure={} dropped={}",
            now_ms, p50_ms, p95_ms, queued, backpressure, dropped
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
