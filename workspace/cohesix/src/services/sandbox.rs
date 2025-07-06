// CLASSIFICATION: COMMUNITY
// Filename: sandbox.rs v1.1
// Date Modified: 2025-07-23
// Author: Lukas Bower

/// Sandbox enforcement service
use super::Service;
use crate::security::capabilities;
use crate::validator::{self, RuleViolation};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::process;

#[derive(Default)]
pub struct SandboxService {
    active: bool,
}

impl Service for SandboxService {
    fn name(&self) -> &'static str {
        "SandboxService"
    }

    fn init(&mut self) {
        self.active = true;
        println!("[sandbox] enforcement active");
    }

    fn shutdown(&mut self) {
        if self.active {
            println!("[sandbox] enforcement stopped");
            self.active = false;
        }
    }
}

impl SandboxService {
    fn log_violation(&self, verb: &str, path: &str, role: &str) {
        fs::create_dir_all("/log").ok();
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open("/log/sandbox.log")
        {
            let _ = writeln!(
                f,
                "blocked action={verb} path={path} pid={} role={role}",
                process::id()
            );
        }
        validator::log_violation(RuleViolation {
            type_: "ns_violation",
            file: path.to_string(),
            agent: role.to_string(),
            time: validator::timestamp(),
        });
    }

    /// Enforce a syscall verb/path for the given role.
    pub fn enforce(&self, verb: &str, path: &str, role: &str) -> bool {
        if !self.active {
            return true;
        }
        let allowed = capabilities::role_allows(role, verb, path);
        if !allowed {
            self.log_violation(verb, path, role);
        }
        allowed
    }
}
