// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Track telemetry cursor offsets and bounded rewind behavior.
// Author: Lukas Bower

use super::{TelemetryAudit, TelemetryAuditLevel};

const DEFAULT_MAX_REWIND_BYTES: u64 = 1024;

/// Cursor tracker for append-only telemetry reads.
#[derive(Debug, Clone)]
pub struct TelemetryCursor {
    retain_on_boot: bool,
    last_offset: Option<u64>,
    max_rewind: u64,
}

/// Snapshot of cursor state for reboot resumption.
#[derive(Debug, Clone, Copy)]
pub struct TelemetryCursorSnapshot {
    pub last_offset: Option<u64>,
}

/// Cursor resolution result including optional audit metadata.
#[derive(Debug, Clone)]
pub struct CursorResolution {
    /// Offset that should be used for the read.
    pub offset: u64,
    /// Optional audit entry to persist.
    pub audit: Option<TelemetryAudit>,
}

/// Errors raised when cursor requests are outside bounds.
#[derive(Debug, Clone)]
pub enum CursorError {
    /// Cursor request is behind the retained ring window.
    Stale {
        /// Requested offset.
        requested: u64,
        /// Offset to rewind to.
        rewind_to: u64,
        /// Audit entry describing the rejection.
        audit: TelemetryAudit,
    },
    /// Cursor rewind exceeded the permitted bound.
    RewindExceeded {
        /// Requested offset.
        requested: u64,
        /// Most recent offset observed.
        last_offset: u64,
        /// Offset to rewind to.
        rewind_to: u64,
        /// Audit entry describing the rejection.
        audit: TelemetryAudit,
    },
}

impl TelemetryCursor {
    /// Create a new cursor tracker for the given ring capacity.
    pub fn new(retain_on_boot: bool, ring_capacity: usize) -> Self {
        let max_rewind = DEFAULT_MAX_REWIND_BYTES.min(ring_capacity as u64);
        Self {
            retain_on_boot,
            last_offset: None,
            max_rewind,
        }
    }

    /// Snapshot the current cursor state.
    pub fn snapshot(&self) -> TelemetryCursorSnapshot {
        TelemetryCursorSnapshot {
            last_offset: self.last_offset,
        }
    }

    /// Restore a persisted last offset when it remains within the ring bounds.
    pub fn restore_last_offset(
        &mut self,
        last_offset: Option<u64>,
        base_offset: u64,
        next_offset: u64,
    ) {
        let Some(offset) = last_offset else {
            return;
        };
        if offset < base_offset || offset > next_offset {
            return;
        }
        self.last_offset = Some(offset);
    }

    /// Validate and normalise a requested offset against ring bounds.
    pub fn resolve(
        &mut self,
        requested: u64,
        base_offset: u64,
        next_offset: u64,
    ) -> Result<CursorResolution, CursorError> {
        if requested < base_offset {
            let audit = TelemetryAudit::new(
                TelemetryAuditLevel::Warn,
                format!(
                    "telemetry cursor stale requested={} rewind_to={} retain_on_boot={}",
                    requested, base_offset, self.retain_on_boot
                ),
            );
            return Err(CursorError::Stale {
                requested,
                rewind_to: base_offset,
                audit,
            });
        }
        if let Some(last_offset) = self.last_offset {
            if requested < last_offset {
                let rewind = last_offset.saturating_sub(requested);
                if rewind > self.max_rewind {
                    let audit = TelemetryAudit::new(
                        TelemetryAuditLevel::Warn,
                        format!(
                            "telemetry cursor rewind exceeded requested={} last={} max_rewind={} rewind_to={} retain_on_boot={}",
                            requested, last_offset, self.max_rewind, base_offset, self.retain_on_boot
                        ),
                    );
                    return Err(CursorError::RewindExceeded {
                        requested,
                        last_offset,
                        rewind_to: base_offset,
                        audit,
                    });
                }
                let audit = TelemetryAudit::new(
                    TelemetryAuditLevel::Info,
                    format!(
                        "telemetry cursor rewind requested={} last={} bytes={}",
                        requested, last_offset, rewind
                    ),
                );
                return Ok(CursorResolution {
                    offset: requested,
                    audit: Some(audit),
                });
            }
        }

        if requested > next_offset {
            return Ok(CursorResolution {
                offset: next_offset,
                audit: Some(TelemetryAudit::new(
                    TelemetryAuditLevel::Info,
                    format!(
                        "telemetry cursor clamped requested={} end={}",
                        requested, next_offset
                    ),
                )),
            });
        }

        Ok(CursorResolution {
            offset: requested,
            audit: None,
        })
    }

    /// Record the most recent successful offset.
    pub fn advance(&mut self, offset: u64) {
        self.last_offset = Some(offset);
    }
}
