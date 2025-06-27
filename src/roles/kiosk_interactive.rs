// CLASSIFICATION: COMMUNITY
// Filename: kiosk_interactive.rs v0.2
// Author: Lukas Bower
// Date Modified: 2026-10-10

//! Initialization routines for the KioskInteractive role.

use std::fs::OpenOptions;
use std::io::Write;
use crate::runtime::env::init::detect_cohrole;
use crate::runtime::ServiceRegistry;
use ureq::Agent;

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
    if let Ok(url) = std::env::var("CLOUD_HOOK_URL") {
        let _ = Agent::new()
            .post(&format!("{}/worker_ping", url.trim_end_matches('/')))
            .send_string("status=ready");
        log(&format!("Worker registered to Queen cloud endpoint at {}", url));
    }
    log("[kiosk_interactive] startup complete");
}

