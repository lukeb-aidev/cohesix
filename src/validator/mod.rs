// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v0.3
// Author: Lukas Bower
// Date Modified: 2026-09-30
#![cfg(feature = "std")]

#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
extern crate alloc;
use alloc::collections::VecDeque;
/// Runtime validator utilities for rule violations.
pub mod config;
pub mod syscall;

use once_cell::sync::Lazy;
use std::sync::Mutex;

use crate::cohesix_types::Syscall;

static TRACE_BUF: Lazy<Mutex<VecDeque<Syscall>>> = Lazy::new(|| Mutex::new(VecDeque::new()));

/// Record a validated syscall for later inspection.
pub fn record_syscall(sc: &Syscall) {
    let mut buf = TRACE_BUF.lock().unwrap_or_else(|p| p.into_inner());
    if buf.len() >= 32 {
        buf.pop_front();
    }
    buf.push_back(sc.clone());
}

/// Return the most recent validated syscalls, newest last.
pub fn recent_syscalls(limit: usize) -> Vec<Syscall> {
    let buf = TRACE_BUF.lock().unwrap_or_else(|p| p.into_inner());
    buf.iter().rev().take(limit).cloned().collect()
}

/// Check if the validator service appears active.
pub fn validator_running() -> bool {
    std::path::Path::new("/srv/validator/live.sock").exists()
}

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::validator::config::get_config;
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
