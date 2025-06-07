// CLASSIFICATION: COMMUNITY
// Filename: worker.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-06-17

//! DroneWorker role initialisation.

use std::fs::{self, OpenOptions};
use std::io::Write;

fn log(msg: &str) {
    match OpenOptions::new().append(true).open("/dev/log") {
        Ok(mut f) => {
            let _ = writeln!(f, "{}", msg);
        }
        Err(_) => println!("{msg}"),
    }
}

fn init_physics() {
    log("[worker] init physics engine");
    // placeholder for Rapier init; ignore failure
}

/// Entry point for DroneWorker role.
pub fn start() {
    init_physics();
    fs::create_dir_all("/srv").ok();
    for p in ["/srv/cuda", "/srv/sim", "/srv/shell", "/srv/telemetry"] {
        let _ = fs::write(p, "ready");
    }
    log("[worker] services ready");
}
