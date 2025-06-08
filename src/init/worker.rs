// CLASSIFICATION: COMMUNITY
// Filename: worker.rs v0.3
// Author: Lukas Bower
// Date Modified: 2025-07-11

//! DroneWorker role initialisation.

use std::fs::{self, OpenOptions};
use std::io::Write;
use rand::random;
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

    let mut ns = NamespaceLoader::load().unwrap_or_default();
    let _ = NamespaceLoader::apply(&mut ns);

    let fs = InMemoryFs::new();
    fs.mount("/srv/cuda");
    fs.mount("/srv/shell");
    fs.mount("/srv/diag");

    fs::create_dir_all("/srv").ok();
    for p in ["/srv/cuda", "/srv/sim", "/srv/shell", "/srv/telemetry"] {
        let _ = fs::write(p, "ready");
    }

    // expose runtime metadata to other agents
    let role = std::env::var("COH_ROLE").unwrap_or_else(|_| "unknown".into());
    fs::create_dir_all("/srv/agent_meta").ok();
    fs::write("/srv/agent_meta/role.txt", &role).ok();
    fs::write("/srv/agent_meta/uptime.txt", "0").ok();
    fs::write("/srv/agent_meta/last_goal.json", "null").ok();
    let trace_id = format!("{:08x}", rand::random::<u32>());
    fs::write("/srv/agent_meta/trace_id.txt", trace_id).ok();

    log("[worker] services ready");
}
