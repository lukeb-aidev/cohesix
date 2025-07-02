// CLASSIFICATION: COMMUNITY
// Filename: core.rs v1.5
// Author: Lukas Bower
// Date Modified: 2026-09-14

use crate::prelude::*;
/// Telemetry Core Module
//
/// The Cohesix telemetry system collects runtime metrics, service health data, diagnostic events,
/// and trace information. This module provides APIs for components to report and retrieve telemetry records.
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

/// Struct representing a telemetry record with key-value pairs.
pub struct TelemetryRecord {
    pub source: String,
    pub timestamp: u64,
    pub data: HashMap<String, String>,
}

/// Trait for components that produce telemetry data.
pub trait TelemetrySource {
    fn collect(&self) -> TelemetryRecord;
}

/// Telemetry emitter for pushing new records to the telemetry bus or log sink.
pub fn emit(record: TelemetryRecord) {
    println!(
        "[telemetry] from {} @ {} → {:?}",
        record.source, record.timestamp, record.data
    );
    fs::create_dir_all("/srv/telemetry").ok();
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/srv/telemetry/telemetry.log")
    {
        let _ = writeln!(f, "[{}] {:?}", record.source, record.data);
    }
}

/// Returns a placeholder timestamp.
fn get_current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Convenience method for emitting a basic telemetry record.
pub fn emit_kv(source: &str, kv: &[(&str, &str)]) {
    let data = kv
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let record = TelemetryRecord {
        source: source.to_string(),
        timestamp: get_current_timestamp(),
        data,
    };
    emit(record);
}

/// Snapshot of GPU telemetry metrics.
#[derive(Debug, Default, Clone)]
pub struct GpuTelemetry {
    pub cuda_present: bool,
    pub driver_version: String,
    pub mem_total: u64,
    pub mem_free: u64,
    pub exec_time_ns: u64,
    pub fallback_reason: String,
    pub temperature: Option<f32>,
    pub gpu_utilization: Option<u32>,
}
