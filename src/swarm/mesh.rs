// CLASSIFICATION: COMMUNITY
// Filename: mesh.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-01

//! Distributed service mesh registry tracking services across nodes.
//!
//! Each registered service is associated with a node id, a TTL and health
//! status. Remote lookups will attempt a light-weight HTTP fetch from the
//! requested node, acting as a simple stand-in for 9P federation.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;

/// Record describing a remotely mounted service.
#[derive(Clone, Debug)]
pub struct ServiceEntry {
    /// Node that registered the service.
    pub node: String,
    /// Logical service name.
    pub name: String,
    /// Filesystem path relative to the node.
    pub path: String,
    /// Time-to-live for the registration.
    pub ttl: Duration,
    /// Last successful health update.
    pub last_update: Instant,
    /// Whether the service is considered healthy.
    pub healthy: bool,
}

static MESH: Lazy<Mutex<HashMap<String, ServiceEntry>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Registry tracking service locations across the swarm.
pub struct ServiceMeshRegistry;

impl ServiceMeshRegistry {
    /// Register a service hosted on the given node.
    pub fn register(node: &str, name: &str, path: &str, ttl_secs: u64) {
        let entry = ServiceEntry {
            node: node.into(),
            name: name.into(),
            path: path.into(),
            ttl: Duration::from_secs(ttl_secs),
            last_update: Instant::now(),
            healthy: true,
        };
        MESH.lock().unwrap().insert(format!("{node}:{name}"), entry);
    }

    /// Update health information for a service.
    pub fn update_health(node: &str, name: &str, healthy: bool) {
        if let Some(entry) = MESH.lock().unwrap().get_mut(&format!("{node}:{name}")) {
            entry.healthy = healthy;
            entry.last_update = Instant::now();
        }
    }

    /// Fetch an entry if it exists and has not expired.
    pub fn get(node: &str, name: &str) -> Option<ServiceEntry> {
        Self::cleanup();
        MESH.lock().unwrap().get(&format!("{node}:{name}")).cloned()
    }

    /// List all currently valid entries.
    pub fn list() -> Vec<ServiceEntry> {
        Self::cleanup();
        MESH.lock().unwrap().values().cloned().collect()
    }

    fn cleanup() {
        let now = Instant::now();
        MESH.lock()
            .unwrap()
            .retain(|_, e| now.duration_since(e.last_update) <= e.ttl);
    }

    /// Attempt to query a remote node for a service path. If successful the
    /// result is cached and returned.
    pub fn federated_lookup(node: &str, name: &str) -> Option<ServiceEntry> {
        if let Some(e) = Self::get(node, name) {
            return Some(e);
        }
        let url = format!("http://{node}/srv_lookup/{name}");
        if let Ok(resp) = ureq::get(&url).call() {
            if let Ok(path) = resp.into_string() {
                let entry = ServiceEntry {
                    node: node.into(),
                    name: name.into(),
                    path: path.trim().into(),
                    ttl: Duration::from_secs(30),
                    last_update: Instant::now(),
                    healthy: true,
                };
                MESH.lock()
                    .unwrap()
                    .insert(format!("{node}:{name}"), entry.clone());
                return Some(entry);
            }
        }
        None
    }
}


