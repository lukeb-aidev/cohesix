// CLASSIFICATION: COMMUNITY
// Filename: validator.rs v0.5
// Author: Lukas Bower
// Date Modified: 2025-08-02

//! Runtime syscall validator for sandboxed agents.
//! Violations are logged to `/srv/violations/<agent>.json` and the
//! offending syscall is dropped.

use crate::cohesix_types::{Role, Syscall};
use crate::validator::config::get_config;
use log::error;
use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

#[derive(Serialize)]
struct Violation {
    syscall: String,
    role: String,
    detail: String,
}

/// Validate a syscall for the given role. Returns true if allowed.
pub fn validate(agent: &str, role: Role, sc: &Syscall) -> bool {
    let allowed = match role {
        Role::DroneWorker | Role::InteractiveAIBooth => true,
        Role::QueenPrimary => !matches!(sc, Syscall::Spawn { .. }),
        _ => false,
    };
    if !allowed {
        log_violation(agent, role, sc);
    }
    allowed
}

fn log_violation(agent: &str, role: Role, sc: &Syscall) {
    let cfg = match get_config() {
        Ok(c) => c,
        Err(e) => {
            error!("validator config error: {e}");
            return;
        }
    };
    fs::create_dir_all(&cfg.violations_dir).ok();
    let path = cfg.violations_dir.join(format!("{agent}.json"));
    let v = Violation {
        syscall: format!("{:?}", sc),
        role: format!("{:?}", role),
        detail: "denied".into(),
    };
    if let Ok(data) = serde_json::to_string(&v) {
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{}", data);
        }
    }
}

/// BootMustSucceed rule.
/// Wait for `/trace/boot_trace.json` and verify it contains a `boot_success` event.
pub fn boot_must_succeed() -> bool {
    let base = std::env::var("COHESIX_TRACE_TMP")
        .or_else(|_| std::env::var("TMPDIR"))
        .unwrap_or_else(|_| "/trace".to_string());
    let path = Path::new(&base).join("boot_trace.json");
    let start = Instant::now();
    while !path.exists() && start.elapsed() < Duration::from_secs(5) {
        sleep(Duration::from_millis(100));
    }
    if !path.exists() {
        println!("BOOT_FAIL:missing_trace");
        return false;
    }
    let data = match fs::read_to_string(&path) {
        Ok(d) => d,
        Err(_) => return false,
    };
    let events: serde_json::Value = match serde_json::from_str(&data) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if let Some(arr) = events.as_array() {
        for ev in arr {
            if ev.get("event") == Some(&serde_json::Value::String("boot_success".into())) {
                return true;
            }
        }
    }
    false
}
