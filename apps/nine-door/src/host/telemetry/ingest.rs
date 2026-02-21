// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Track telemetry ingest segments, quotas, and eviction policy.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::collections::{HashMap, VecDeque};

/// Maximum bytes permitted per telemetry ingest record.
pub const MAX_TELEMETRY_RECORD_BYTES: usize = 4096;
const TELEMETRY_REFERENCE_CHUNK_SCHEMA: &str = "coh-ref-c/v1";
const TELEMETRY_REFERENCE_DIGEST_MAX_BYTES: usize = 64;

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
    /// Maximum number of reference entries accepted for a segment manifest.
    pub max_reference_entries_per_segment: usize,
    /// Maximum encoded bytes in reference records for a segment manifest.
    pub max_reference_manifest_bytes_per_segment: usize,
    /// Maximum logical bytes described by reference records for a segment.
    pub max_reference_bytes_per_segment: u64,
    /// Eviction policy applied when quotas are exceeded.
    pub eviction_policy: TelemetryIngestEvictionPolicy,
}

impl TelemetryIngestConfig {
    /// Return true when ingest is enabled.
    pub fn enabled(&self) -> bool {
        self.max_segments_per_device > 0
            && self.max_bytes_per_segment > 0
            && self.max_total_bytes_per_device > 0
            && self.max_reference_entries_per_segment > 0
            && self.max_reference_manifest_bytes_per_segment > 0
            && self.max_reference_bytes_per_segment > 0
    }
}

impl Default for TelemetryIngestConfig {
    fn default() -> Self {
        Self {
            max_segments_per_device: 4,
            max_bytes_per_segment: 32 * 1024,
            max_total_bytes_per_device: 128 * 1024,
            max_reference_entries_per_segment: 1024,
            max_reference_manifest_bytes_per_segment: 32 * 1024,
            max_reference_bytes_per_segment: 1_073_741_824,
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
    InvalidPayload,
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
    mode: TelemetrySegmentMode,
    reference_entries: usize,
    reference_manifest_bytes: usize,
    reference_total_bytes: u64,
    reference_next_seq: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TelemetrySegmentMode {
    Unknown,
    Plain,
    ReferenceManifest,
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

    pub fn create_segment(
        &mut self,
        device_id: &str,
    ) -> Result<TelemetryCreateOutcome, TelemetryIngestError> {
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
            mode: TelemetrySegmentMode::Unknown,
            reference_entries: 0,
            reference_manifest_bytes: 0,
            reference_total_bytes: 0,
            reference_next_seq: 1,
        });
        Ok(TelemetryCreateOutcome { seg_id, evicted })
    }

    pub fn append_record(
        &mut self,
        device_id: &str,
        seg_id: &str,
        payload: &[u8],
    ) -> Result<TelemetryAppendOutcome, TelemetryIngestError> {
        if !self.config.enabled() {
            return Err(TelemetryIngestError {
                kind: TelemetryIngestErrorKind::Disabled,
                message: "telemetry ingest is disabled".to_owned(),
            });
        }
        let bytes = payload.len();
        let device = self
            .devices
            .get_mut(device_id)
            .ok_or_else(|| TelemetryIngestError {
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
        let reference_chunk = parse_telemetry_reference_chunk(payload)?;
        if let Some(segment) = device
            .segments
            .iter_mut()
            .find(|segment| segment.id == seg_id)
        {
            match reference_chunk {
                Some(reference) => {
                    if matches!(segment.mode, TelemetrySegmentMode::Plain) {
                        return Err(TelemetryIngestError {
                            kind: TelemetryIngestErrorKind::InvalidPayload,
                            message: "telemetry segment cannot mix inline and reference records"
                                .to_owned(),
                        });
                    }
                    if reference.seq != segment.reference_next_seq {
                        return Err(TelemetryIngestError {
                            kind: TelemetryIngestErrorKind::InvalidPayload,
                            message: "telemetry reference sequence is not monotonic".to_owned(),
                        });
                    }
                    if reference.offset != segment.reference_total_bytes {
                        return Err(TelemetryIngestError {
                            kind: TelemetryIngestErrorKind::InvalidPayload,
                            message: "telemetry reference offset is not contiguous".to_owned(),
                        });
                    }
                    if segment.reference_entries.saturating_add(1)
                        > self.config.max_reference_entries_per_segment
                    {
                        return Err(TelemetryIngestError {
                            kind: TelemetryIngestErrorKind::QuotaExceeded,
                            message: "telemetry reference entry quota exceeded".to_owned(),
                        });
                    }
                    if segment.reference_manifest_bytes.saturating_add(bytes)
                        > self.config.max_reference_manifest_bytes_per_segment
                    {
                        return Err(TelemetryIngestError {
                            kind: TelemetryIngestErrorKind::QuotaExceeded,
                            message: "telemetry reference manifest byte quota exceeded".to_owned(),
                        });
                    }
                    let referenced_total = segment
                        .reference_total_bytes
                        .saturating_add(reference.chunk_bytes);
                    if referenced_total > self.config.max_reference_bytes_per_segment {
                        return Err(TelemetryIngestError {
                            kind: TelemetryIngestErrorKind::QuotaExceeded,
                            message: "telemetry referenced byte quota exceeded".to_owned(),
                        });
                    }
                    segment.mode = TelemetrySegmentMode::ReferenceManifest;
                    segment.reference_entries = segment.reference_entries.saturating_add(1);
                    segment.reference_manifest_bytes =
                        segment.reference_manifest_bytes.saturating_add(bytes);
                    segment.reference_total_bytes = referenced_total;
                    segment.reference_next_seq = segment.reference_next_seq.saturating_add(1);
                }
                None => {
                    if matches!(segment.mode, TelemetrySegmentMode::ReferenceManifest) {
                        return Err(TelemetryIngestError {
                            kind: TelemetryIngestErrorKind::InvalidPayload,
                            message: "telemetry segment cannot append inline data after reference manifest"
                                .to_owned(),
                        });
                    }
                    if matches!(segment.mode, TelemetrySegmentMode::Unknown) {
                        segment.mode = TelemetrySegmentMode::Plain;
                    }
                }
            }
            segment.bytes = segment.bytes.saturating_add(bytes);
            device.total_bytes = device.total_bytes.saturating_add(bytes);
        }
        Ok(TelemetryAppendOutcome { evicted })
    }
}

#[derive(Debug, Clone, Copy)]
struct TelemetryReferenceChunk {
    seq: u64,
    offset: u64,
    chunk_bytes: u64,
}

fn parse_telemetry_reference_chunk(
    payload: &[u8],
) -> Result<Option<TelemetryReferenceChunk>, TelemetryIngestError> {
    let Ok(text) = std::str::from_utf8(payload) else {
        return Ok(None);
    };
    let Some(schema) = parse_json_string_field(text, "schema") else {
        return Ok(None);
    };
    if schema != TELEMETRY_REFERENCE_CHUNK_SCHEMA {
        return Ok(None);
    }
    let seq = parse_json_u64_field(text, "seq").ok_or_else(|| TelemetryIngestError {
        kind: TelemetryIngestErrorKind::InvalidPayload,
        message: "telemetry reference chunk missing seq".to_owned(),
    })?;
    let offset = parse_json_u64_field(text, "off").ok_or_else(|| TelemetryIngestError {
        kind: TelemetryIngestErrorKind::InvalidPayload,
        message: "telemetry reference chunk missing off".to_owned(),
    })?;
    let chunk_bytes = parse_json_u64_field(text, "len").ok_or_else(|| TelemetryIngestError {
        kind: TelemetryIngestErrorKind::InvalidPayload,
        message: "telemetry reference chunk missing len".to_owned(),
    })?;
    if chunk_bytes == 0 {
        return Err(TelemetryIngestError {
            kind: TelemetryIngestErrorKind::InvalidPayload,
            message: "telemetry reference chunk len must be >= 1".to_owned(),
        });
    }
    let digest = parse_json_string_field(text, "sha256").ok_or_else(|| TelemetryIngestError {
        kind: TelemetryIngestErrorKind::InvalidPayload,
        message: "telemetry reference chunk missing sha256".to_owned(),
    })?;
    if !is_valid_reference_digest(digest) {
        return Err(TelemetryIngestError {
            kind: TelemetryIngestErrorKind::InvalidPayload,
            message: "telemetry reference chunk sha256 is invalid".to_owned(),
        });
    }
    Ok(Some(TelemetryReferenceChunk {
        seq,
        offset,
        chunk_bytes,
    }))
}

fn parse_json_u64_field(input: &str, key: &str) -> Option<u64> {
    let mut cursor = 0usize;
    while let Some(found) = input[cursor..].find(key) {
        let index = cursor + found;
        let before = index.checked_sub(1)?;
        let after = index + key.len();
        let bytes = input.as_bytes();
        if bytes.get(before) != Some(&b'"') || bytes.get(after) != Some(&b'"') {
            cursor = after;
            continue;
        }
        let mut rest = &input[after + 1..];
        let colon = rest.find(':')?;
        rest = rest[colon + 1..].trim_start();
        let mut end = 0usize;
        for ch in rest.chars() {
            if !ch.is_ascii_digit() {
                break;
            }
            end = end.saturating_add(ch.len_utf8());
        }
        if end == 0 {
            return None;
        }
        return rest[..end].parse().ok();
    }
    None
}

fn parse_json_string_field<'a>(input: &'a str, key: &str) -> Option<&'a str> {
    let mut cursor = 0usize;
    while let Some(found) = input[cursor..].find(key) {
        let index = cursor + found;
        let before = index.checked_sub(1)?;
        let after = index + key.len();
        let bytes = input.as_bytes();
        if bytes.get(before) != Some(&b'"') || bytes.get(after) != Some(&b'"') {
            cursor = after;
            continue;
        }
        let mut rest = &input[after + 1..];
        let colon = rest.find(':')?;
        rest = rest[colon + 1..].trim_start();
        if !rest.starts_with('"') {
            return None;
        }
        rest = &rest[1..];
        let end = rest.find('"')?;
        return Some(&rest[..end]);
    }
    None
}

fn is_valid_reference_digest(value: &str) -> bool {
    if value.is_empty() || value.len() > TELEMETRY_REFERENCE_DIGEST_MAX_BYTES {
        return false;
    }
    value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '+' | '/' | '=' | '.'))
}
