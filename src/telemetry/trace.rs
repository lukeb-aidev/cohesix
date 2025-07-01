// CLASSIFICATION: COMMUNITY
// Filename: trace.rs v1.2
// Author: Lukas Bower
// Date Modified: 2025-07-21

use crate::prelude::*;
//! Trace Logging for Cohesix
//!
//! The trace module provides structured and timestamped logging support for
//! system events, service behavior, and rule validation results.
//! This supports validation agents, debugging, and runtime observability.

use alloc::boxed::Box;

/// Enum representing the level of a trace event.
#[derive(Debug)]
pub enum TraceLevel {
    Debug,
    Info,
    Warn,
    Error,
}

/// Struct representing a trace event entry.
pub struct TraceEntry {
    pub timestamp: u64,
    pub level: TraceLevel,
    pub source: String,
    pub message: String,
}

/// Emits a trace entry to the system trace log.
pub fn emit(entry: TraceEntry) {
    println!(
        "[trace][{:?}] [{}] {}",
        entry.level, entry.source, entry.message
    );
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    fs::create_dir_all("/srv/telemetry").ok();
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/srv/telemetry/trace.log")
    {
        let _ = writeln!(f, "[{}][{:?}] {}", entry.source, entry.level, entry.message);
    }
    fs::create_dir_all("/log").ok();
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open("/log/trace.log") {
        let _ = writeln!(f, "[{}][{:?}] {}", entry.source, entry.level, entry.message);
    }
}

/// Helper function to emit a quick trace from inline values.
pub fn trace(level: TraceLevel, source: &str, message: &str) {
    let entry = TraceEntry {
        timestamp: get_timestamp(),
        level,
        source: source.to_string(),
        message: message.to_string(),
    };
    emit(entry);
}

use std::time::{SystemTime, UNIX_EPOCH};
/// Returns a simple UNIX timestamp.
fn get_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Install a panic hook that logs to `/log/trace.log`.
pub fn init_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        trace(TraceLevel::Error, "panic", &format!("{}", info));
    }));
}
