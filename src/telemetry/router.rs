// CLASSIFICATION: COMMUNITY
// Filename: router.rs v1.2
// Author: Lukas Bower
// Date Modified: 2026-12-31

/// Telemetry routing and collection utilities.
use log::debug;

use crate::cohesix_types::{Role, RoleManifest};
use cohesix_9p::fs::InMemoryFs;

/// Runtime telemetry metrics.
#[derive(Debug, Default)]
pub struct TelemetryMetrics {
    /// CPU usage percentage averaged over ~100ms.
    pub cpu_usage: f32,
    /// Temperature in degrees Celsius if available.
    pub temperature: Option<f32>,
}

/// Router trait for gathering and exposing telemetry.
pub trait TelemetryRouter {
    /// Gather metrics from the host system.
    fn gather_metrics(&mut self) -> TelemetryMetrics;
    /// Expose metrics under the 9P namespace.
    fn expose_metrics(&self, metrics: &TelemetryMetrics);
}

/// Basic implementation using `sysinfo` and a stub 9P server.
pub struct BasicTelemetryRouter {
    fs: InMemoryFs,
    role: Role,
}

impl Default for BasicTelemetryRouter {
    fn default() -> Self {
        Self {
            fs: InMemoryFs::default(),
            role: RoleManifest::current_role(),
        }
    }
}

impl BasicTelemetryRouter {
    fn read_temperature() -> Option<f32> {
        let candidates = ["/srv/ina226_mock"];
        for path in candidates.iter() {
            if let Ok(contents) = std::fs::read_to_string(path) {
                if let Ok(milli) = contents.trim().parse::<f32>() {
                    return Some(milli / 1000.0);
                }
            }
        }
        None
    }
}

impl TelemetryRouter for BasicTelemetryRouter {
    fn gather_metrics(&mut self) -> TelemetryMetrics {
        let cpu_usage = 0.0;
        let temperature = Self::read_temperature();
        TelemetryMetrics {
            cpu_usage,
            temperature,
        }
    }

    fn expose_metrics(&self, metrics: &TelemetryMetrics) {
        self.fs.mount("/srv/telemetry");
        debug!("role {:?} metrics: {:?}", self.role, metrics);
    }
}
