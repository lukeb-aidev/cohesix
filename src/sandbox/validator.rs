// CLASSIFICATION: COMMUNITY
// Filename: validator.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-04

//! Runtime syscall validator for sandboxed agents.
//! Violations are logged to `/srv/violations/<agent>.json` and the
//! offending syscall is dropped.

use crate::cohesix_types::{Role, Syscall};
use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::Write;

#[derive(Serialize)]
struct Violation {
    syscall: String,
    role: String,
    detail: String,
}

/// Validate a syscall for the given role. Returns true if allowed.
pub fn validate(agent: &str, role: Role, sc: &Syscall) -> bool {
    let allowed = match role {
        Role::DroneWorker => true,
        Role::QueenPrimary => !matches!(sc, Syscall::Spawn { .. }),
        _ => false,
    };
    if !allowed {
        log_violation(agent, role, sc);
    }
    allowed
}

fn log_violation(agent: &str, role: Role, sc: &Syscall) {
    let path = format!("/srv/violations/{agent}.json");
    fs::create_dir_all("/srv/violations").ok();
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
