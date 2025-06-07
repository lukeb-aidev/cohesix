// CLASSIFICATION: COMMUNITY
// Filename: worker.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-06-06

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
=======
// Date Modified: 2025-06-18

//! seL4 root task hook for the DroneWorker role.
//! Mounts worker specific services like CUDA and BusyBox shell output.

use crate::plan9::namespace::NamespaceLoader;
use cohesix_9p::fs::InMemoryFs;

/// Entry point for the Worker root task.
pub fn start() {
    let ns = NamespaceLoader::load().unwrap_or_default();
    let _ = NamespaceLoader::apply(&ns);

    let mut fs = InMemoryFs::new();
    fs.mount("/srv/cuda");
    fs.mount("/srv/shell");
    fs.mount("/srv/diag");
}
