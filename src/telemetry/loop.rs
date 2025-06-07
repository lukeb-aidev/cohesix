// CLASSIFICATION: COMMUNITY
// Filename: loop.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-06-18

//! Periodic telemetry feedback loop combining physics and GPU state.

use std::thread;
use std::time::Duration;

use super::router::{BasicTelemetryRouter, TelemetryRouter};

/// Run the telemetry feedback loop.  Metrics are gathered every 100ms
/// and written to `/srv/telemetry` via the router's in-memory FS.
pub fn run() {
    let mut router = BasicTelemetryRouter::default();
    loop {
        let metrics = router.gather_metrics();
        router.expose_metrics(&metrics);
        thread::sleep(Duration::from_millis(100));
    }
}
