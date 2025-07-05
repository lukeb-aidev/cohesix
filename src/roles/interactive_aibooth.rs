// CLASSIFICATION: COMMUNITY
// Filename: interactive_aibooth.rs v0.3
// Author: Lukas Bower
// Date Modified: 2026-12-31

use crate::runtime::env::init::detect_cohrole;
use crate::runtime::ServiceRegistry;
/// Initialization routines for the InteractiveAiBooth role.
use std::fs::{self, OpenOptions};
use std::io::Write;

fn log(msg: &str) {
    match OpenOptions::new().append(true).open("/srv/devlog") {
        Ok(mut f) => {
            let _ = writeln!(f, "{}", msg);
        }
        Err(_) => println!("{msg}"),
    }
}

/// Start the Interactive AI Booth environment.
pub fn start() {
    if detect_cohrole() != "InteractiveAiBooth" {
        log("access denied: wrong role");
        return;
    }

    // Ensure Secure9P namespaces are mounted
    for p in [
        "/input/mic",
        "/input/cam",
        "/mnt/ui_in",
        "/mnt/speak",
        "/mnt/face_match",
    ] {
        fs::create_dir_all(p).ok();
    }

    // Initialize CUDA runtime if available
    log("[aibooth] CUDA pipeline uses remote dispatch");
    let _ = ServiceRegistry::register_service("aibooth", "/srv/aibooth");
    log("[aibooth] startup complete");
}
