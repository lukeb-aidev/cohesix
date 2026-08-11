// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Enforce Secure9P batching, queue depth, and short-write retry policy.
// Author: Lukas Bower

//! Secure9P pipeline helpers for batching and back-pressure accounting.

use std::io::{self, Write};
use std::thread;
use std::time::Duration;

use secure9p_core::{SessionLimits, ShortWritePolicy};
use secure9p_transport::{PartialWrite, TransportError, WriteProgress, WriteRetryPolicy};

/// Configuration used by the Secure9P pipeline.
#[derive(Debug, Clone, Copy)]
pub struct PipelineConfig {
    /// Maximum number of frames allowed per batch.
    pub batch_frames: usize,
    /// Maximum number of outstanding requests per session.
    pub queue_depth: usize,
    /// Short write retry policy.
    pub short_write_policy: ShortWritePolicy,
}

impl PipelineConfig {
    /// Build a pipeline configuration from session limits.
    #[must_use]
    pub fn from_limits(limits: SessionLimits) -> Self {
        Self {
            batch_frames: limits.batch_frames.max(1),
            queue_depth: limits.queue_depth_limit().max(1),
            short_write_policy: limits.short_write_policy,
        }
    }
}

/// Observability counters for the Secure9P pipeline.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PipelineMetrics {
    /// Current outstanding queue depth.
    pub queue_depth: usize,
    /// Configured queue depth limit.
    pub queue_limit: usize,
    /// Number of back-pressure refusals.
    pub backpressure_events: u64,
    /// Number of short write events.
    pub short_writes: u64,
    /// Number of retries triggered by short writes.
    pub short_write_retries: u64,
    /// Successful UI-oriented reads.
    pub ui_reads: u64,
    /// UI denials due to ticket scope or quota enforcement.
    pub ui_denies: u64,
}

/// Pipeline helper tracking batching and write retry behavior.
#[derive(Debug)]
pub struct Pipeline {
    config: PipelineConfig,
    metrics: PipelineMetrics,
}

impl Pipeline {
    /// Create a new pipeline helper.
    #[must_use]
    pub fn new(config: PipelineConfig) -> Self {
        Self {
            metrics: PipelineMetrics {
                queue_limit: config.queue_depth,
                ..PipelineMetrics::default()
            },
            config,
        }
    }

    /// Return the current pipeline metrics.
    #[must_use]
    pub fn metrics(&self) -> PipelineMetrics {
        self.metrics
    }

    /// Update the observed queue depth.
    pub fn record_queue_depth(&mut self, depth: usize) {
        self.metrics.queue_depth = depth;
    }

    /// Increment back-pressure refusal counters.
    pub fn record_backpressure(&mut self) {
        self.metrics.backpressure_events += 1;
    }

    /// Increment UI read counters.
    pub fn record_ui_read(&mut self) {
        self.metrics.ui_reads = self.metrics.ui_reads.saturating_add(1);
    }

    /// Increment UI denial counters.
    pub fn record_ui_deny(&mut self) {
        self.metrics.ui_denies = self.metrics.ui_denies.saturating_add(1);
    }

    /// Write a batch of frames using the configured short-write policy.
    pub fn write_batch(&mut self, writer: &mut impl Write, frames: &[Vec<u8>]) -> io::Result<()> {
        for frame in frames {
            self.write_with_policy(writer, frame)?;
        }
        Ok(())
    }

    fn write_with_policy(&mut self, writer: &mut impl Write, buffer: &[u8]) -> io::Result<()> {
        if buffer.is_empty() {
            return Ok(());
        }
        let policy = match self.config.short_write_policy {
            ShortWritePolicy::Reject => WriteRetryPolicy::Reject,
            ShortWritePolicy::Retry => WriteRetryPolicy::Retry,
        };
        let mut progress =
            PartialWrite::new(buffer.len(), policy).map_err(transport_write_error)?;
        loop {
            let written = writer.write(&buffer[progress.offset()..])?;
            let short_writes_before = progress.short_writes();
            let retries_before = progress.retries();
            let step = progress.advance(written).map_err(transport_write_error);
            self.metrics.short_writes = self
                .metrics
                .short_writes
                .saturating_add(progress.short_writes().saturating_sub(short_writes_before));
            self.metrics.short_write_retries = self
                .metrics
                .short_write_retries
                .saturating_add(progress.retries().saturating_sub(retries_before));
            match step? {
                WriteProgress::Complete => return Ok(()),
                WriteProgress::Continue => {}
                WriteProgress::RetryAfter { delay_ms } => {
                    if delay_ms > 0 {
                        thread::sleep(Duration::from_millis(delay_ms));
                    }
                }
            }
        }
    }
}

fn transport_write_error(error: TransportError) -> io::Error {
    let kind = match error {
        TransportError::ShortWriteExhausted => io::ErrorKind::WriteZero,
        _ => io::ErrorKind::InvalidInput,
    };
    io::Error::new(kind, error.to_string())
}
