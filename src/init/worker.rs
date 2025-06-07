// CLASSIFICATION: COMMUNITY
// Filename: worker.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-06-20

//! DroneWorker role initialisation.

use std::fs::{self, OpenOptions};
use std::io::Write;
use crate::plan9::namespace::NamespaceLoader;
use cohesix_9p::fs::InMemoryFs;

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

/// Entry point for the DroneWorker role.
pub fn start() {
    init_physics();

    let ns = NamespaceLoader::load().unwrap_or_default();
    let _ = NamespaceLoader::apply(&ns);

    let mut fs = InMemoryFs::new();
    fs.mount("/srv/cuda");
    fs.mount("/srv/shell");
    fs.mount("/srv/diag");

    fs::create_dir_all("/srv").ok();
    for p in ["/srv/cuda", "/srv/sim", "/srv/shell", "/srv/telemetry"] {
        let _ = fs::write(p, "ready");
    }

    log("[worker] services ready");
}
