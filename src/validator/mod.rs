// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-22

//! Runtime validator utilities for rule violations.

pub mod config;

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::validator::config::{get_config, ConfigError};
use log::error;

/// Structured rule violation alert.
pub struct RuleViolation {
    pub type_: &'static str,
    pub file: String,
    pub agent: String,
    pub time: u64,
}

/// Log a rule violation to the runtime validator log.
pub fn log_violation(v: RuleViolation) {
    let cfg = match get_config() {
        Ok(c) => c,
        Err(e) => {
            error!("validator config error: {e}");
            return;
        }
    };
    fs::create_dir_all(&cfg.log_dir).ok();
    let path = cfg.log_dir.join("validator_runtime.log");
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
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
