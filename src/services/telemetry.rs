// CLASSIFICATION: COMMUNITY
// Filename: telemetry.rs v1.1
// Date Modified: 2025-06-19
// Author: Lukas Bower

//! Telemetry service

use super::Service;
use crate::runtime::ServiceRegistry;
use crate::telemetry::r#loop::TelemetrySyncLoop;

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
        ServiceRegistry::register_service("telemetry", "/srv/telemetry");
        TelemetrySyncLoop::spawn();
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
