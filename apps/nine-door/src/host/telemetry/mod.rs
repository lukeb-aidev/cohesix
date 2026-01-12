// Author: Lukas Bower
// Purpose: Coordinate telemetry ring buffers, cursor state, and audit output.

pub mod cursor;
pub mod ring;

use cursor::{CursorError, CursorResolution, TelemetryCursor};
use ring::{RingReadError, RingReadOutcome, RingWriteError, TelemetryRing};
use secure9p_core::append_only_write_bounds;

/// Severity level for telemetry audit lines.
#[derive(Debug, Clone, Copy)]
pub enum TelemetryAuditLevel {
    /// Informational audit entry.
    Info,
    /// Warning audit entry for bounded violations.
    Warn,
}

/// Audit payload emitted alongside telemetry operations.
#[derive(Debug, Clone)]
pub struct TelemetryAudit {
    /// Severity label for the audit entry.
    pub level: TelemetryAuditLevel,
    /// Human-readable audit message.
    pub message: String,
}

impl TelemetryAudit {
    pub fn new(level: TelemetryAuditLevel, message: String) -> Self {
        Self { level, message }
    }
}

/// Cursor retention configuration for telemetry rings.
#[derive(Debug, Clone, Copy)]
pub struct TelemetryCursorConfig {
    /// Preserve cursor state after reboot when true.
    pub retain_on_boot: bool,
}

/// Schema selector for worker telemetry frames.
#[derive(Debug, Clone, Copy)]
pub enum TelemetryFrameSchema {
    /// Legacy UTF-8 newline-delimited frames.
    LegacyPlaintext,
    /// CBOR framed telemetry (`telemetry-frame/v1`).
    CborV1,
}

/// Manifest-derived telemetry ring configuration.
#[derive(Debug, Clone, Copy)]
pub struct TelemetryConfig {
    /// Ring capacity per worker.
    pub ring_bytes_per_worker: usize,
    /// Selected telemetry frame schema.
    pub frame_schema: TelemetryFrameSchema,
    /// Cursor retention settings.
    pub cursor: TelemetryCursorConfig,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            ring_bytes_per_worker: 1024,
            frame_schema: TelemetryFrameSchema::LegacyPlaintext,
            cursor: TelemetryCursorConfig {
                retain_on_boot: false,
            },
        }
    }
}

/// Append-only telemetry file backed by a ring buffer and cursor.
#[derive(Debug, Clone)]
pub struct TelemetryFile {
    ring: TelemetryRing,
    cursor: TelemetryCursor,
    frame_schema: TelemetryFrameSchema,
}

/// Result of a telemetry append operation.
#[derive(Debug, Clone)]
pub struct TelemetryAppendOutcome {
    /// Number of bytes accepted.
    pub count: u32,
    /// Optional audit entry to persist.
    pub audit: Option<TelemetryAudit>,
}

/// Result of a telemetry read operation.
#[derive(Debug, Clone)]
pub struct TelemetryReadOutcome {
    /// Data returned to the caller.
    pub data: Vec<u8>,
    /// Optional audit entry to persist.
    pub audit: Option<TelemetryAudit>,
}

/// Error returned when telemetry IO fails.
#[derive(Debug, Clone)]
pub struct TelemetryError {
    /// Human-readable error message.
    pub message: String,
    /// Optional audit entry to persist.
    pub audit: Option<TelemetryAudit>,
    /// Classification used to map to Secure9P errors.
    pub kind: TelemetryErrorKind,
}

/// Classification of telemetry IO failures.
#[derive(Debug, Clone, Copy)]
pub enum TelemetryErrorKind {
    /// Invalid append offset or protocol misuse.
    InvalidOffset,
    /// Frame rejected due to schema validation.
    InvalidFrame,
    /// Append would exceed ring quota.
    QuotaExceeded,
    /// Cursor fell behind the ring window.
    CursorStale,
}

impl TelemetryFile {
    /// Construct a telemetry file using the supplied configuration.
    pub fn new(config: TelemetryConfig) -> Self {
        let ring = TelemetryRing::new(config.ring_bytes_per_worker);
        let cursor = TelemetryCursor::new(config.cursor.retain_on_boot, ring.capacity());
        Self {
            ring,
            cursor,
            frame_schema: config.frame_schema,
        }
    }

    /// Append telemetry bytes at the provided offset.
    pub fn append(&mut self, offset: u64, data: &[u8]) -> Result<TelemetryAppendOutcome, TelemetryError> {
        if matches!(self.frame_schema, TelemetryFrameSchema::LegacyPlaintext)
            && std::str::from_utf8(data).is_err()
        {
            return Err(TelemetryError {
                message: "telemetry frame must be UTF-8 under legacy schema".to_owned(),
                audit: Some(TelemetryAudit::new(
                    TelemetryAuditLevel::Warn,
                    "telemetry frame rejected: non-utf8 payload under legacy schema".to_owned(),
                )),
                kind: TelemetryErrorKind::InvalidFrame,
            });
        }
        let expected_offset = self.ring.bounds().next_offset;
        let provided_offset = if offset == u64::MAX { expected_offset } else { offset };
        let bounds = append_only_write_bounds(
            expected_offset,
            provided_offset,
            self.ring.capacity(),
            data.len(),
        )
        .map_err(|err| TelemetryError {
            message: format!("telemetry append offset rejected: {err}"),
            audit: Some(TelemetryAudit::new(
                TelemetryAuditLevel::Warn,
                format!("telemetry append offset rejected: {err}"),
            )),
            kind: TelemetryErrorKind::InvalidOffset,
        })?;
        if bounds.short {
            return Err(TelemetryError {
                message: format!(
                    "telemetry frame exceeds ring quota: {} > {}",
                    data.len(),
                    self.ring.capacity()
                ),
                audit: Some(TelemetryAudit::new(
                    TelemetryAuditLevel::Warn,
                    format!(
                        "telemetry quota reject bytes={} quota={}",
                        data.len(),
                        self.ring.capacity()
                    ),
                )),
                kind: TelemetryErrorKind::QuotaExceeded,
            });
        }
        let outcome = self.ring.append(data).map_err(|err| match err {
            RingWriteError::Oversize { requested, capacity } => TelemetryError {
                message: format!("telemetry frame exceeds ring quota: {requested} > {capacity}"),
                audit: Some(TelemetryAudit::new(
                    TelemetryAuditLevel::Warn,
                    format!("telemetry quota reject bytes={requested} quota={capacity}"),
                )),
                kind: TelemetryErrorKind::QuotaExceeded,
            },
        })?;
        let audit = if outcome.dropped_bytes > 0 {
            Some(TelemetryAudit::new(
                TelemetryAuditLevel::Warn,
                format!(
                    "telemetry ring wrap dropped_bytes={} new_base={}",
                    outcome.dropped_bytes, outcome.new_base
                ),
            ))
        } else {
            None
        };
        Ok(TelemetryAppendOutcome {
            count: outcome.count,
            audit,
        })
    }

    /// Read telemetry bytes from the supplied offset.
    pub fn read(&mut self, offset: u64, count: u32) -> Result<TelemetryReadOutcome, TelemetryError> {
        let bounds = self.ring.bounds();
        let CursorResolution { offset, audit } = self
            .cursor
            .resolve(offset, bounds.base_offset, bounds.next_offset)
            .map_err(|err| match err {
                CursorError::Stale {
                    requested,
                    rewind_to,
                    audit,
                } => TelemetryError {
                    message: format!(
                        "telemetry cursor stale requested={} rewind_to={}",
                        requested, rewind_to
                    ),
                    audit: Some(audit),
                    kind: TelemetryErrorKind::CursorStale,
                },
                CursorError::RewindExceeded {
                    requested,
                    last_offset,
                    rewind_to,
                    audit,
                } => TelemetryError {
                    message: format!(
                        "telemetry cursor rewind exceeded requested={} last={} rewind_to={}",
                        requested, last_offset, rewind_to
                    ),
                    audit: Some(audit),
                    kind: TelemetryErrorKind::CursorStale,
                },
            })?;
        let RingReadOutcome { data } = self.ring.read(offset, count).map_err(|err| match err {
            RingReadError::Stale {
                requested,
                available_start,
            } => TelemetryError {
                message: format!(
                    "telemetry cursor stale requested={} rewind_to={}",
                    requested, available_start
                ),
                audit: Some(TelemetryAudit::new(
                    TelemetryAuditLevel::Warn,
                    format!(
                        "telemetry cursor stale requested={} rewind_to={}",
                        requested, available_start
                    ),
                )),
                kind: TelemetryErrorKind::CursorStale,
            },
        })?;
        let next = offset.saturating_add(data.len() as u64);
        self.cursor.advance(next);
        Ok(TelemetryReadOutcome { data, audit })
    }
}
