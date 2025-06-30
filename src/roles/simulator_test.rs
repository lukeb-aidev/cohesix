// CLASSIFICATION: COMMUNITY
// Filename: simulator_test.rs v0.1
// Author: Lukas Bower
// Date Modified: 2026-09-30

//! Initialization routines for the SimulatorTest role.

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

/// Start the SimulatorTest environment.
pub fn start() {
    if detect_cohrole() != "SimulatorTest" {
        log("access denied: wrong role");
        return;
    }
    let _ = ServiceRegistry::register_service("simtest", "/srv/simtest");
    log("[simulator_test] startup complete");
}

