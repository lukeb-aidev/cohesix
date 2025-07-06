// CLASSIFICATION: COMMUNITY
// Filename: kernel_trace.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-22

/// Kernel boot tracing utilities.
use chrono::Utc;
use std::fs::{self, OpenOptions};
use std::io::Write;

/// Log a syscall invocation during boot.
pub fn log_syscall(name: &str) {
    fs::create_dir_all("/log").ok();
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/log/kernel_trace.log")
    {
        let _ = writeln!(f, "[{}] syscall {}", Utc::now().to_rfc3339(), name);
    }
}

/// Log an init call during boot.
pub fn log_init_call(name: &str) {
    fs::create_dir_all("/log").ok();
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/log/kernel_trace.log")
    {
        let _ = writeln!(f, "[{}] init {}", Utc::now().to_rfc3339(), name);
    }
}
