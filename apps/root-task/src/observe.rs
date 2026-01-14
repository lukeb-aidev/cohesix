// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Define ingest observability metrics shared across the root task.
// Author: Lukas Bower
#![allow(dead_code)]

use crate::generated;

const LATENCY_SAMPLES: usize =
    generated::OBSERVABILITY_CONFIG.proc_ingest.latency_samples as usize;

/// Snapshot of ingest observability metrics.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct IngestSnapshot {
    pub p50_ms: u32,
    pub p95_ms: u32,
    pub backpressure: u64,
    pub dropped: u64,
    pub queued: u32,
}

/// Mutable ingest metric tracker used by the event pump.
#[derive(Debug, Default)]
pub struct IngestMetrics {
    backpressure: u64,
    dropped: u64,
    latency: LatencySamples,
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
