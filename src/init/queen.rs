// CLASSIFICATION: COMMUNITY
// Filename: queen.rs v0.6
// Author: Lukas Bower
// Date Modified: 2026-10-11

use crate::prelude::*;
/// seL4 root task hook for the Queen role.
/// Loads the boot namespace and registers core services.

use std::fs::{self, OpenOptions};
use std::io::Write;
use ureq::Agent;

use crate::boot::plan9_ns::load_namespace;
use crate::cloud::orchestrator::CloudOrchestrator;
use crate::plan9::namespace::NamespaceLoader;
use crate::runtime::ServiceRegistry;
use cohesix_9p::fs::InMemoryFs;
use serde_json;

fn log(msg: &str) {
    match OpenOptions::new().append(true).open("/srv/devlog") {
        Ok(mut f) => {
            let _ = writeln!(f, "{}", msg);
        }
        Err(_) => println!("{msg}"),
    }
}

/// Entry point for the Queen root task.
pub fn start() {
    match NamespaceLoader::load() {
        Ok(mut ns) => {
            let _ = NamespaceLoader::apply(&mut ns);
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
    // initialize world state summary
    fs::create_dir_all("/srv/world_state").ok();
    let world = serde_json::json!({"workers": [], "active_traces": [], "telemetry": {}});
    let _ = fs::write(
        "/srv/world_state/world.json",
        serde_json::to_string(&world).unwrap(),
    );
    let _ = ServiceRegistry::register_service("telemetry", "/srv/telemetry");
    let _ = ServiceRegistry::register_service("sim", "/sim");
    let _ = ServiceRegistry::register_service("p9mux", "/srv/p9mux");

    if let Ok(url) = std::env::var("CLOUD_HOOK_URL") {
        match CloudOrchestrator::start(&url) {
            Ok(_) => log("Cloud orchestrator started"),
            Err(e) => log(&format!("cloud orchestration failed: {e}")),
        }
    }
    // Spawning of initial processes will be added in a future update
    // FIXME(cohesix): spawn initial processes under this namespace
}
