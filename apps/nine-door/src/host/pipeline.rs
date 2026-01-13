// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Enforce Secure9P batching, queue depth, and short-write retry policy.
// Author: Lukas Bower

//! Secure9P pipeline helpers for batching and back-pressure accounting.

use std::io::{self, Write};
use std::thread;
use std::time::Duration;

use secure9p_core::{SessionLimits, ShortWritePolicy};

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

    /// Write a batch of frames using the configured short-write policy.
    pub fn write_batch(
        &mut self,
        writer: &mut impl Write,
        frames: &[Vec<u8>],
    ) -> io::Result<()> {
        for frame in frames {
            self.write_with_policy(writer, frame)?;
        }
        Ok(())
    }

    fn write_with_policy(&mut self, writer: &mut impl Write, buffer: &[u8]) -> io::Result<()> {
        let mut offset = 0;
        let mut attempts = 0u8;
        while offset < buffer.len() {
            let written = writer.write(&buffer[offset..])?;
            if written == 0 || written < buffer.len() - offset {
                self.metrics.short_writes += 1;
                let Some(delay_ms) = self.config.short_write_policy.retry_delay_ms(attempts) else {
                    return Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "short write policy exhausted",
                    ));
                };
                self.metrics.short_write_retries += 1;
                attempts = attempts.saturating_add(1);
                if delay_ms > 0 {
                    thread::sleep(Duration::from_millis(delay_ms));
                }
            }
            offset = offset.saturating_add(written);
        }
        Ok(())
    }
}
