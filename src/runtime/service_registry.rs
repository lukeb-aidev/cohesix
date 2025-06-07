// CLASSIFICATION: COMMUNITY
// Filename: service_registry.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-01

//! Runtime service registry for Cohesix.
//!
//! Allows services under `/srv/` to be dynamically registered and
//! looked up by name. Lookups are filtered by the caller's role
//! as exposed via `/srv/cohrole`.

use std::collections::HashMap;
use std::sync::Mutex;

use once_cell::sync::Lazy;

use crate::cohesix_types::{Role, RoleManifest};

/// Handle referencing a registered service.
#[derive(Clone, Debug)]
pub struct ServiceHandle {
    /// Filesystem path of the service mount point.
    pub path: String,
    /// Role that registered the service.
    pub role: Role,
}

static REGISTRY: Lazy<Mutex<HashMap<String, ServiceHandle>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Registry for runtime services.
pub struct ServiceRegistry;

impl ServiceRegistry {
    /// Register a service path for the current role.
    pub fn register_service(name: &str, path: &str) {
        let role = RoleManifest::current_role();
        let handle = ServiceHandle {
            path: path.into(),
            role,
        };
        REGISTRY.lock().unwrap().insert(name.into(), handle);
    }

    /// Remove a previously registered service.
    pub fn unregister_service(name: &str) {
        REGISTRY.lock().unwrap().remove(name);
    }

    /// Lookup a service handle if visible to the current role.
    pub fn lookup(name: &str) -> Option<ServiceHandle> {
        let role = RoleManifest::current_role();
        REGISTRY
            .lock()
            .unwrap()
            .get(name)
            .cloned()
            .filter(|h| h.role == role || role == Role::QueenPrimary)
    }

    /// Clear all registered services. Only used in tests.
    pub fn reset() {
        REGISTRY.lock().unwrap().clear();
    }
}
