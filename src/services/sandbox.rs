// CLASSIFICATION: COMMUNITY
// Filename: sandbox.rs v1.0
// Date Modified: 2025-06-02
// Author: Lukas Bower

//! Sandbox enforcement service

use super::Service;

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
    /// Simple check that prints the command to be executed.
    pub fn check_command(&self, cmd: &str) {
        if self.active {
            println!("[sandbox] checking command: {}", cmd);
        }
    }
}
