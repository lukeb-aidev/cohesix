// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-09

//! Runtime validator utilities for rule violations.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

/// Structured rule violation alert.
pub struct RuleViolation {
    pub type_: &'static str,
    pub file: String,
    pub agent: String,
    pub time: u64,
}

/// Log a rule violation to the runtime validator log.
pub fn log_violation(v: RuleViolation) {
    fs::create_dir_all("/log").ok();
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open("/log/validator_runtime.log") {
        let _ = writeln!(
            f,
            "rule_violation(type=\"{}\", file=\"{}\", agent=\"{}\", time={})",
            v.type_, v.file, v.agent, v.time
        );
    }
}

pub fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
