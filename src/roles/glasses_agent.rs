// CLASSIFICATION: COMMUNITY
// Filename: glasses_agent.rs v0.1
// Author: Lukas Bower
// Date Modified: 2026-09-30

//! Initialization routines for the GlassesAgent role.

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

/// Start the GlassesAgent environment.
pub fn start() {
    if detect_cohrole() != "GlassesAgent" {
        log("access denied: wrong role");
        return;
    }
    let _ = ServiceRegistry::register_service("glasses", "/srv/glasses");
    log("[glasses_agent] startup complete");
}

