// CLASSIFICATION: COMMUNITY
// Filename: validator.rs v0.3
// Author: Lukas Bower
// Date Modified: 2025-07-22

use crate::queen::trust;
use crate::{new_err, CohError};
/// Simple simulation trace validator run on the Queen.
use hex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

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
pub fn validate_trace(path: &str, worker: &str) -> Result<(), CohError> {
    let data = fs::read_to_string(path)?;
    let trace: TiltTrace = serde_json::from_str(&data)?;
    println!(
        "[validator] Loaded trace: offset={} angle={}",
        trace._offset, trace.angle
    );
    let angle_ok = trace.angle.abs() < 1.0;
    if !angle_ok {
        println!(
            "[validator] Angle check failed for worker {worker}, angle={}",
            trace.angle
        );
        trust::record_failure(worker);
    }
    let report = ValidationReport {
        angle_ok,
        drift: trace.angle,
    };
    let base = std::env::var("COHESIX_TRACE_REPORT_DIR")
        .or_else(|_| std::env::var("TMPDIR"))
        .unwrap_or_else(|_| "/srv".to_string());
    fs::create_dir_all(format!("{}/trace/reports", base)).ok();
    let out = format!("{}/trace/reports/{worker}.report.json", base);
    fs::write(&out, serde_json::to_string(&report)?)?;
    println!(
        "[validator] ValidationReport -> angle_ok={} drift={}",
        angle_ok, trace.angle
    );
    println!("[validator] report stored at {out}");
    Ok(())
}

/// Return a SHA-256 digest of the active security policy for audit logs.
pub fn security_policy_digest() -> Result<String, CohError> {
    let configured = std::env::var("COHESIX_SECURITY_POLICY").ok();
    let candidates = if let Some(path) = configured {
        vec![path]
    } else {
        vec![
            "workspace/docs/security/SECURITY_POLICY.md".into(),
            "../docs/security/SECURITY_POLICY.md".into(),
            "docs/security/SECURITY_POLICY.md".into(),
        ]
    };

    for candidate in candidates {
        let path = Path::new(&candidate);
        if let Ok(data) = fs::read(path) {
            let digest = Sha256::digest(&data);
            return Ok(hex::encode(digest));
        }
    }

    Err(new_err("security policy digest unavailable"))
}
