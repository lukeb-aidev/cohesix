// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Track telemetry ingest segments, quotas, and eviction policy.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::collections::{HashMap, VecDeque};

/// Maximum bytes permitted per telemetry ingest record.
pub const MAX_TELEMETRY_RECORD_BYTES: usize = 4096;

/// Eviction policy when telemetry ingest quotas are exceeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelemetryIngestEvictionPolicy {
    /// Refuse new segments or writes once the quota is exceeded.
    Refuse,
    /// Evict the oldest segment(s) to make room.
    EvictOldest,
}

/// Manifest-driven telemetry ingest quota configuration.
#[derive(Debug, Clone, Copy)]
pub struct TelemetryIngestConfig {
    /// Maximum number of segments per device.
    pub max_segments_per_device: usize,
    /// Maximum bytes per segment.
    pub max_bytes_per_segment: usize,
    /// Maximum total bytes across all segments for a device.
    pub max_total_bytes_per_device: usize,
    /// Eviction policy applied when quotas are exceeded.
    pub eviction_policy: TelemetryIngestEvictionPolicy,
}

impl TelemetryIngestConfig {
    /// Return true when ingest is enabled.
    pub fn enabled(&self) -> bool {
        self.max_segments_per_device > 0
            && self.max_bytes_per_segment > 0
            && self.max_total_bytes_per_device > 0
    }
}

impl Default for TelemetryIngestConfig {
    fn default() -> Self {
        Self {
            max_segments_per_device: 4,
            max_bytes_per_segment: 32 * 1024,
            max_total_bytes_per_device: 128 * 1024,
            eviction_policy: TelemetryIngestEvictionPolicy::EvictOldest,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TelemetryCreateOutcome {
    pub seg_id: String,
    pub evicted: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct TelemetryAppendOutcome {
    pub evicted: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TelemetryIngestErrorKind {
    Disabled,
    QuotaExceeded,
    SegmentMissing,
}

#[derive(Debug, Clone)]
pub(crate) struct TelemetryIngestError {
    pub kind: TelemetryIngestErrorKind,
    pub message: String,
}

#[derive(Debug, Clone)]
struct TelemetrySegmentState {
    id: String,
    bytes: usize,
}

#[derive(Debug, Clone)]
struct TelemetryDeviceState {
    next_id: u64,
    total_bytes: usize,
    segments: VecDeque<TelemetrySegmentState>,
}

impl TelemetryDeviceState {
    fn new() -> Self {
        Self {
            next_id: 1,
            total_bytes: 0,
            segments: VecDeque::new(),
        }
    }

    fn allocate_id(&mut self) -> String {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        format!("seg-{:06}", id)
    }
}

/// In-memory telemetry ingest state tracked per device.
#[derive(Debug, Default)]
pub(crate) struct TelemetryIngestState {
    config: TelemetryIngestConfig,
    devices: HashMap<String, TelemetryDeviceState>,
}

impl TelemetryIngestState {
    pub fn new(config: TelemetryIngestConfig) -> Self {
        Self {
            config,
            devices: HashMap::new(),
        }
    }

    pub fn config(&self) -> TelemetryIngestConfig {
        self.config
    }

    pub fn enabled(&self) -> bool {
        self.config.enabled()
    }

    pub fn ensure_device(&mut self, device_id: &str) {
        self.devices
            .entry(device_id.to_owned())
            .or_insert_with(TelemetryDeviceState::new);
    }

    pub fn create_segment(&mut self, device_id: &str) -> Result<TelemetryCreateOutcome, TelemetryIngestError> {
        if !self.config.enabled() {
            return Err(TelemetryIngestError {
                kind: TelemetryIngestErrorKind::Disabled,
                message: "telemetry ingest is disabled".to_owned(),
            });
        }
        let device = self
            .devices
            .entry(device_id.to_owned())
            .or_insert_with(TelemetryDeviceState::new);
        let mut evicted = Vec::new();
        let max_segments = self.config.max_segments_per_device;
        if max_segments == 0 {
            return Err(TelemetryIngestError {
                kind: TelemetryIngestErrorKind::Disabled,
                message: "telemetry ingest is disabled".to_owned(),
            });
        }
        if device.segments.len().saturating_add(1) > max_segments {
            match self.config.eviction_policy {
                TelemetryIngestEvictionPolicy::Refuse => {
                    return Err(TelemetryIngestError {
                        kind: TelemetryIngestErrorKind::QuotaExceeded,
                        message: "telemetry segment quota exceeded".to_owned(),
                    });
                }
                TelemetryIngestEvictionPolicy::EvictOldest => {
                    while device.segments.len().saturating_add(1) > max_segments {
                        if let Some(segment) = device.segments.pop_front() {
                            device.total_bytes = device.total_bytes.saturating_sub(segment.bytes);
                            evicted.push(segment.id);
                        } else {
                            break;
                        }
                    }
                }
            }
        }
        let seg_id = device.allocate_id();
        device.segments.push_back(TelemetrySegmentState {
            id: seg_id.clone(),
            bytes: 0,
        });
        Ok(TelemetryCreateOutcome { seg_id, evicted })
    }

    pub fn append_record(
        &mut self,
        device_id: &str,
        seg_id: &str,
        bytes: usize,
    ) -> Result<TelemetryAppendOutcome, TelemetryIngestError> {
        if !self.config.enabled() {
            return Err(TelemetryIngestError {
                kind: TelemetryIngestErrorKind::Disabled,
                message: "telemetry ingest is disabled".to_owned(),
            });
        }
        let device = self.devices.get_mut(device_id).ok_or_else(|| TelemetryIngestError {
            kind: TelemetryIngestErrorKind::SegmentMissing,
            message: format!("telemetry device {device_id} not found"),
        })?;
        let segment_bytes = match device
            .segments
            .iter()
            .find(|segment| segment.id == seg_id)
            .map(|segment| segment.bytes)
        {
            Some(bytes) => bytes,
            None => {
                return Err(TelemetryIngestError {
                    kind: TelemetryIngestErrorKind::SegmentMissing,
                    message: format!("telemetry segment {seg_id} not found"),
                })
            }
        };
        if segment_bytes.saturating_add(bytes) > self.config.max_bytes_per_segment {
            return Err(TelemetryIngestError {
                kind: TelemetryIngestErrorKind::QuotaExceeded,
                message: "telemetry segment size quota exceeded".to_owned(),
            });
        }
        let mut evicted = Vec::new();
        let total_after = device.total_bytes.saturating_add(bytes);
        if total_after > self.config.max_total_bytes_per_device {
            match self.config.eviction_policy {
                TelemetryIngestEvictionPolicy::Refuse => {
                    return Err(TelemetryIngestError {
                        kind: TelemetryIngestErrorKind::QuotaExceeded,
                        message: "telemetry total byte quota exceeded".to_owned(),
                    });
                }
                TelemetryIngestEvictionPolicy::EvictOldest => {
                    let needed = total_after - self.config.max_total_bytes_per_device;
                    let mut freed = 0usize;
                    let mut scan = 0usize;
                    while freed < needed && scan < device.segments.len() {
                        if device.segments.get(scan).map(|seg| seg.id.as_str()) == Some(seg_id) {
                            scan = scan.saturating_add(1);
                            continue;
                        }
                        if let Some(segment) = device.segments.remove(scan) {
                            device.total_bytes = device.total_bytes.saturating_sub(segment.bytes);
                            freed = freed.saturating_add(segment.bytes);
                            evicted.push(segment.id);
                            continue;
                        }
                        break;
                    }
                    if freed < needed {
                        return Err(TelemetryIngestError {
                            kind: TelemetryIngestErrorKind::QuotaExceeded,
                            message: "telemetry total byte quota exceeded".to_owned(),
                        });
                    }
                }
            }
        }
        if let Some(segment) = device
            .segments
            .iter_mut()
            .find(|segment| segment.id == seg_id)
        {
            segment.bytes = segment.bytes.saturating_add(bytes);
            device.total_bytes = device.total_bytes.saturating_add(bytes);
        }
        Ok(TelemetryAppendOutcome { evicted })
    }

}
