// CLASSIFICATION: COMMUNITY
// Filename: sensor_relay.rs v0.1
// Author: Lukas Bower
// Date Modified: 2026-09-30

use crate::prelude::*;
/// Initialization routines for the SensorRelay role.

use std::fs::OpenOptions;
use std::io::Write;
use crate::runtime::env::init::detect_cohrole;
use crate::runtime::ServiceRegistry;

fn log(msg: &str) {
    if let Ok(mut f) = OpenOptions::new().append(true).open("/srv/devlog") {
        let _ = writeln!(f, "{}", msg);
    } else {
        println!("{msg}");
    }
}

/// Start the SensorRelay environment.
pub fn start() {
    if detect_cohrole() != "SensorRelay" {
        log("access denied: wrong role");
        return;
    }
    let _ = ServiceRegistry::register_service("relay", "/srv/relay");
    log("[sensor_relay] startup complete");
}

