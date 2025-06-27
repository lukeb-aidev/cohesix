// CLASSIFICATION: COMMUNITY
// Filename: kiosk_interactive.rs v0.1
// Author: Lukas Bower
// Date Modified: 2026-09-30

//! Initialization routines for the KioskInteractive role.

use std::fs::OpenOptions;
use std::io::Write;
use crate::runtime::env::init::detect_cohrole;
use crate::runtime::ServiceRegistry;

fn log(msg: &str) {
    if let Ok(mut f) = OpenOptions::new().append(true).open("/dev/log") {
        let _ = writeln!(f, "{}", msg);
    } else {
        println!("{msg}");
    }
}

/// Start the KioskInteractive environment.
pub fn start() {
    if detect_cohrole() != "KioskInteractive" {
        log("access denied: wrong role");
        return;
    }
    let _ = ServiceRegistry::register_service("kiosk", "/srv/kiosk");
    log("[kiosk_interactive] startup complete");
}

