// CLASSIFICATION: COMMUNITY
// Filename: health.rs v1.0
// Date Modified: 2025-06-02
// Author: Lukas Bower

/// Health monitoring service
use super::Service;

#[derive(Default)]
pub struct HealthService {
    ready: bool,
}

impl Service for HealthService {
    fn name(&self) -> &'static str {
        "HealthService"
    }

    fn init(&mut self) {
        self.ready = true;
        println!("[health] service ready");
    }

    fn shutdown(&mut self) {
        if self.ready {
            println!("[health] service stopped");
            self.ready = false;
        }
    }
}

impl HealthService {
    /// Basic health check returning readiness state.
    pub fn is_ready(&self) -> bool {
        self.ready
    }
}
