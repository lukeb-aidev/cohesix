// CLASSIFICATION: COMMUNITY
// Filename: queen.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-06-19

//! seL4 root task hook for the Queen role.
//! Loads the boot namespace and emits early log messages.

use std::fs::OpenOptions;
use std::io::Write;

use crate::boot::plan9_ns::load_namespace;
use crate::runtime::ServiceRegistry;

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
    match load_namespace("/srv/bootns") {
        Ok(ns) => log(&format!(
            "[queen] loaded {} namespace entries",
            ns.actions().len()
        )),
        Err(e) => log(&format!("[queen] failed to load namespace: {e}")),
    }
    ServiceRegistry::register_service("telemetry", "/srv/telemetry");
    ServiceRegistry::register_service("sim", "/sim");
    ServiceRegistry::register_service("p9mux", "/srv/p9mux");
    // TODO(cohesix): spawn initial processes under this namespace
}
