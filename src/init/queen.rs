// CLASSIFICATION: COMMUNITY
// Filename: queen.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-06-07

//! seL4 root task hook for the Queen role.
//! Loads the boot namespace and registers core services.

use std::fs::{self, OpenOptions};
use std::io::Write;
use ureq::Agent;

use crate::plan9::namespace::NamespaceLoader;
use cohesix_9p::fs::InMemoryFs;
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
    match load_namespace("/srv/bootns") {
        Ok(ns) => log(&format!(
            "[queen] loaded {} namespace entries",
            ns.actions().len()
        )),
        Err(e) => log(&format!("[queen] failed to load namespace: {e}")),
    }

    if let Ok(url) = fs::read_to_string("/srv/cloudinit") {
        if let Ok(resp) = Agent::new().get(url.trim()).call() {
            if let Ok(body) = resp.into_string() {
                fs::create_dir_all("/srv/agents").ok();
                let _ = fs::write("/srv/agents/config.json", body);
            }
        }
    }

    fs::create_dir_all("/srv/bootstatus").ok();
    let _ = fs::write("/srv/bootstatus/queen", "ok");
    ServiceRegistry::register_service("telemetry", "/srv/telemetry");
    ServiceRegistry::register_service("sim", "/sim");
    ServiceRegistry::register_service("p9mux", "/srv/p9mux");
    // TODO(cohesix): spawn initial processes under this namespace
}
