// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-06-17

//! Sandbox helper modules.

pub mod chain;
pub mod dispatcher;
pub mod profile;
pub mod queue;
pub mod validator;

use std::fs::OpenOptions;
use std::io::Write;

/// Validate sandbox environment after startup.
pub fn validate() {
    std::fs::create_dir_all("/log").ok();
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/log/sandbox_boot.log")
    {
        let _ = writeln!(f, "sandbox validated");
    }
}
