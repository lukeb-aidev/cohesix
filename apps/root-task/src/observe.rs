// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Define ingest observability metrics shared across the root task.
// Author: Lukas Bower
#![allow(dead_code)]

use core::sync::atomic::{AtomicU64, Ordering};

use crate::generated;

const LATENCY_SAMPLES: usize = generated::OBSERVABILITY_CONFIG.proc_ingest.latency_samples as usize;

/// Snapshot of ingest observability metrics.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct IngestSnapshot {
    pub p50_ms: u32,
    pub p95_ms: u32,
    pub backpressure: u64,
    pub dropped: u64,
    pub queued: u32,
    pub ui_reads: u64,
    pub ui_denies: u64,
}

/// Mutable ingest metric tracker used by the event pump.
#[derive(Debug, Default)]
pub struct IngestMetrics {
    backpressure: u64,
    dropped: u64,
    latency: LatencySamples,
    ui_reads: u64,
    ui_denies: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PressureKind {
    Busy,
    Quota,
    Cut,
    Policy,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PressureSnapshot {
    pub busy: u64,
    pub quota: u64,
    pub cut: u64,
    pub policy: u64,
}

static PRESSURE_BUSY: AtomicU64 = AtomicU64::new(0);
static PRESSURE_QUOTA: AtomicU64 = AtomicU64::new(0);
static PRESSURE_CUT: AtomicU64 = AtomicU64::new(0);
static PRESSURE_POLICY: AtomicU64 = AtomicU64::new(0);

pub fn record_pressure(kind: PressureKind) {
    match kind {
        PressureKind::Busy => {
            PRESSURE_BUSY.fetch_add(1, Ordering::SeqCst);
        }
        PressureKind::Quota => {
            PRESSURE_QUOTA.fetch_add(1, Ordering::SeqCst);
        }
        PressureKind::Cut => {
            PRESSURE_CUT.fetch_add(1, Ordering::SeqCst);
        }
        PressureKind::Policy => {
            PRESSURE_POLICY.fetch_add(1, Ordering::SeqCst);
        }
    }
}

pub fn pressure_snapshot() -> PressureSnapshot {
    PressureSnapshot {
        busy: PRESSURE_BUSY.load(Ordering::SeqCst),
        quota: PRESSURE_QUOTA.load(Ordering::SeqCst),
        cut: PRESSURE_CUT.load(Ordering::SeqCst),
        policy: PRESSURE_POLICY.load(Ordering::SeqCst),
    }
}

impl IngestMetrics {
    /// Record a back-pressure event.
    pub fn record_backpressure(&mut self) {
        self.backpressure = self.backpressure.saturating_add(1);
    }

    /// Record a dropped ingest entry.
    pub fn record_drop(&mut self) {
        self.dropped = self.dropped.saturating_add(1);
    }

    /// Record a UI read.
    pub fn record_ui_read(&mut self) {
        self.ui_reads = self.ui_reads.saturating_add(1);
    }

    /// Record a UI denial.
    pub fn record_ui_deny(&mut self) {
        self.ui_denies = self.ui_denies.saturating_add(1);
    }

    /// Record an ingest latency sample (milliseconds).
    pub fn record_latency_ms(&mut self, latency_ms: u64) {
        let value = latency_ms.try_into().unwrap_or(u32::MAX);
        self.latency.record(value);
    }

    /// Capture a snapshot of the current metrics.
    pub fn snapshot(&self, queued: usize) -> IngestSnapshot {
        IngestSnapshot {
            p50_ms: self.latency.percentile(50),
            p95_ms: self.latency.percentile(95),
            backpressure: self.backpressure,
            dropped: self.dropped,
            queued: queued as u32,
            ui_reads: self.ui_reads,
            ui_denies: self.ui_denies,
        }
    }
}

#[derive(Debug, Default)]
struct LatencySamples {
    samples: [u32; LATENCY_SAMPLES],
    len: usize,
    next: usize,
}

impl LatencySamples {
    fn record(&mut self, sample: u32) {
        if LATENCY_SAMPLES == 0 {
            return;
        }
        if self.len < LATENCY_SAMPLES {
            self.samples[self.len] = sample;
            self.len = self.len.saturating_add(1);
            if self.len == LATENCY_SAMPLES {
                self.next = 0;
            }
            return;
        }
        self.samples[self.next] = sample;
        self.next = (self.next + 1) % LATENCY_SAMPLES;
    }

    fn percentile(&self, pct: u32) -> u32 {
        if self.len == 0 {
            return 0;
        }
        let mut scratch = [0u32; LATENCY_SAMPLES];
        let count = if self.len < LATENCY_SAMPLES {
            scratch[..self.len].copy_from_slice(&self.samples[..self.len]);
            self.len
        } else {
            scratch.copy_from_slice(&self.samples);
            LATENCY_SAMPLES
        };
        scratch[..count].sort_unstable();
        let idx = count.saturating_sub(1) * pct as usize / 100;
        scratch.get(idx).copied().unwrap_or(0)
    }
}
