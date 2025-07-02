// CLASSIFICATION: COMMUNITY
// Filename: role_hooks.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-11

use crate::prelude::*;
/// Role-specific boot hooks enabling demo services.
use std::fs::{self, OpenOptions};
use std::io::Write;

/// Setup demo services for the given role and log to `/trace/boot.log`.
pub fn setup(role: &str) {
    fs::create_dir_all("/srv").ok();
    fs::create_dir_all("/trace").ok();
    fs::write("/srv/cohrole", role).ok();
    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/trace/boot.log")
        .unwrap();
    writeln!(log, "role={role}").ok();

    fs::write("/srv/telemetry", "").ok();
    match role {
        "DroneWorker" | "SensorRelay" => {
            fs::write("/srv/webcam", "").ok();
            fs::write("/srv/gpuinfo", "").ok();
        }
        _ => {}
    }
}
