// CLASSIFICATION: COMMUNITY
// Filename: drone_worker.rs v0.4
// Author: Lukas Bower
// Date Modified: 2026-10-25

use crate::prelude::*;
//! Initialization routines for the DroneWorker role.

use std::fs::{self, OpenOptions};
use std::io::Write;
use crate::runtime::env::init::detect_cohrole;
use crate::runtime::ServiceRegistry;
use ureq::Agent;

fn log(msg: &str) {
    if let Ok(mut f) = OpenOptions::new().append(true).open("/srv/devlog") {
        let _ = writeln!(f, "{}", msg);
    } else {
        println!("{msg}");
    }
}

/// Start the DroneWorker environment.
pub fn start() {
    if detect_cohrole() != "DroneWorker" {
        log("access denied: wrong role");
        return;
    }
    let _ = ServiceRegistry::register_service("drone", "/srv/drone");
    let mut source = "env var";
    let mut url = std::env::var("CLOUD_HOOK_URL").unwrap_or_default();
    if url.trim().is_empty() {
        url = fs::read_to_string("/srv/cloud/url").unwrap_or_default();
        source = "/srv/cloud/url";
    }
    let url = url.trim().to_string();
    if !url.is_empty() {
        let endpoint = format!("{}/worker_ping", url.trim_end_matches('/'));
        println!("Worker sending status=ready to {}", endpoint);
        std::io::stdout().flush().ok();
        let _ = Agent::new().post(&endpoint).send_string("status=ready");
        log(&format!(
            "Worker registered to Queen cloud endpoint at {} (source: {})",
            url, source
        ));
    }
    log("[drone_worker] startup complete");
}

