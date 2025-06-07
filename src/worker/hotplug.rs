// CLASSIFICATION: COMMUNITY
// Filename: hotplug.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-01

//! Worker discovery and retirement hooks.
//!
//! When a worker joins it receives the current boot namespace and registers its
//! services with the global service mesh. On exit the worker's services are
//! unregistered and active agents should be reassigned by higher level logic.

use crate::swarm::mesh::ServiceMeshRegistry;
use crate::runtime::ServiceRegistry;
use std::fs::{self, OpenOptions};
use std::io::Write;

pub struct WorkerHotplug;

impl WorkerHotplug {
    /// Called when a worker node becomes available.
    pub fn join(node_id: &str) {
        if let Ok(ns) = fs::read_to_string("/srv/bootns") {
            let url = format!("http://{node_id}/sync_bootns");
            let _ = ureq::post(&url).send_string(&ns);
        }
        ServiceMeshRegistry::register(node_id, "bootns", "/srv/bootns", "QueenPrimary", 60);
    }

    /// Called when a worker node leaves the cluster.
    pub fn retire(node_id: &str) {
        for entry in ServiceMeshRegistry::list() {
            if entry.node == node_id {
                ServiceRegistry::unregister_service(&entry.name);
            }
        }
        // trigger agent migration by touching audit log
        let _ = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/srv/orchestration.log")
            .and_then(|mut f| writeln!(f, "worker_retired {node_id}"));
    }
}

