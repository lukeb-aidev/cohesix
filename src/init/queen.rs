// CLASSIFICATION: COMMUNITY
// Filename: queen.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-06-18

//! seL4 root task hook for the Queen role.
//! Loads the boot namespace and registers core services.

use std::fs::OpenOptions;
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

/// Entry point for the Queen root task.
pub fn start() {
    match NamespaceLoader::load() {
        Ok(ns) => {
            let _ = NamespaceLoader::apply(&ns);
            log(&format!("[queen] loaded {} namespace ops", ns.ops.len()));
        }
        Err(e) => log(&format!("[queen] failed to load namespace: {e}")),
    }

    let fs = InMemoryFs::new();
    fs.mount("/srv/telemetry");
    fs.mount("/srv/sim");
    fs.mount("/srv/p9mux");
}
