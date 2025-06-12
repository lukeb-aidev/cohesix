// CLASSIFICATION: COMMUNITY
// Filename: validator.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-11

//! Simple simulation trace validator run on the Queen.

use serde::{Deserialize, Serialize};
use std::fs;
use crate::queen::trust;

#[derive(Deserialize)]
struct TiltTrace {
    #[serde(rename = "offset")]
    _offset: f32,
    angle: f32,
}

#[derive(Serialize)]
struct ValidationReport {
    angle_ok: bool,
    drift: f32,
}

/// Validate a worker simulation trace and store a report.
pub fn validate_trace(path: &str, worker: &str) -> anyhow::Result<()> {
    let data = fs::read_to_string(path)?;
    let trace: TiltTrace = serde_json::from_str(&data)?;
    let angle_ok = trace.angle.abs() < 1.0;
    if !angle_ok {
        trust::record_failure(worker);
    }
    let report = ValidationReport {
        angle_ok,
        drift: trace.angle,
    };
    fs::create_dir_all("/trace/reports").ok();
    let out = format!("/trace/reports/{worker}.report.json");
    fs::write(&out, serde_json::to_string(&report)?)?;
    println!("[validator] report stored at {out}");
    Ok(())
}
