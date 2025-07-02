// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v1.1
// Author: Lukas Bower
// Date Modified: 2025-07-23

/// Sandbox helper modules.
pub mod chain;
pub mod dispatcher;
pub mod profile;
pub mod queue;
pub mod validator;

use std::fs::OpenOptions;
use std::io::Write;

/// Validate sandbox environment after startup.
pub fn validate() -> bool {
    let ok = validator::boot_must_succeed();
    std::fs::create_dir_all("/log").ok();
    if ok {
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open("/log/sandbox_boot.log")
        {
            let _ = writeln!(f, "sandbox validated");
        }
    }
    ok
}
