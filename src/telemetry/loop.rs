// CLASSIFICATION: COMMUNITY
// Filename: loop.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-01

//! Telemetry loops coordinating metric exposure and simulation state.

use log::trace;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::thread;
use std::time::Duration;

use super::router::{BasicTelemetryRouter, TelemetryRouter};

/// Run the main telemetry feedback loop and spawn the sync helper.
pub fn run() {
    TelemetrySyncLoop::spawn();
    let mut router = BasicTelemetryRouter::default();
    loop {
        let metrics = router.gather_metrics();
        router.expose_metrics(&metrics);
        thread::sleep(Duration::from_millis(100));
    }
}

/// Loop that syncs simulation state to the telemetry service.
pub struct TelemetrySyncLoop;

impl TelemetrySyncLoop {
    /// Spawn the synchronization thread.
    pub fn spawn() {
        thread::spawn(Self::run);
    }

    fn run() {
        fs::create_dir_all("/srv").ok();
        let mut last = String::new();
        loop {
            let state = fs::read_to_string("/sim/state").unwrap_or_default();
            if state != last {
                last = state.clone();
                let _ = fs::write("/srv/telemetry", &state);
                if let Ok(mut f) = OpenOptions::new().append(true).open("/srv/devlog") {
                    let _ = writeln!(f, "[telemetry_loop] updated state");
                }
                trace!("telemetry updated");
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}

