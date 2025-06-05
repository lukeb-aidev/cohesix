// CLASSIFICATION: COMMUNITY
// Filename: telemetry.rs v1.0
// Date Modified: 2025-06-02
// Author: Lukas Bower

//! Telemetry service

use super::Service;

/// Basic telemetry service that logs events to stdout.
#[derive(Default)]
pub struct TelemetryService {
    initialized: bool,
}

impl Service for TelemetryService {
    fn name(&self) -> &'static str {
        "TelemetryService"
    }

    fn init(&mut self) {
        self.initialized = true;
        println!("[telemetry] initialized");
    }

    fn shutdown(&mut self) {
        if self.initialized {
            println!("[telemetry] shutting down");
            self.initialized = false;
        }
    }
}

impl TelemetryService {
    /// Record a simple event message.
    pub fn record(&self, msg: &str) {
        if self.initialized {
            println!("[telemetry] {}", msg);
        }
    }
}
