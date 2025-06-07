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

// Date Modified: 2025-06-19

//! Telemetry synchronization loop.
//!
//! Periodically reads `/sim/state` and mirrors the data to `/srv/telemetry`.
//! When updates occur a log entry is appended to `/dev/log`.

use log::trace;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::thread;
use std::time::Duration;

/// Loop that syncs simulation state to the telemetry service.
pub struct TelemetrySyncLoop;

impl TelemetrySyncLoop {
    /// Spawn the synchronization thread.
    pub fn spawn() {
        thread::spawn(Self::run);
    }

    fn run() {
        fs::create_dir_all("srv").ok();
        let mut last = String::new();
        loop {
            let state = fs::read_to_string("sim/state").unwrap_or_default();
            if state != last {
                last = state.clone();
                let _ = fs::write("srv/telemetry", &state);
                if let Ok(mut f) = OpenOptions::new().append(true).open("/dev/log") {
                    let _ = writeln!(f, "[telemetry_loop] updated state");
                }
                trace!("telemetry updated");
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}
