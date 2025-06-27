// CLASSIFICATION: COMMUNITY
// Filename: service_registry.rs v0.6
// Author: Lukas Bower
// Date Modified: 2026-09-30
#![cfg(not(target_os = "uefi"))]

//! Runtime service registry for Cohesix.
//!
//! Allows services under `/srv/` to be dynamically registered and
//! looked up by name. Lookups are filtered by the caller's role
//! as exposed via `/srv/cohrole`.

use std::collections::HashMap;
use std::sync::Mutex;

use thiserror::Error;

use once_cell::sync::Lazy;
use log::info;

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

/// Errors returned by [`ServiceRegistry`] operations.
#[derive(Debug, Error)]
pub enum ServiceRegistryError {
    #[error("service registry lock poisoned")]
    LockPoisoned,
}

type RegistryResult<T> = Result<T, ServiceRegistryError>;

/// Registry for runtime services.
pub struct ServiceRegistry;

impl ServiceRegistry {
    /// Register a service path for the current role.
    pub fn register_service(name: &str, path: &str) -> RegistryResult<()> {
        let role = RoleManifest::current_role();
        let handle = ServiceHandle {
            path: path.into(),
            role: role.clone(),
        };
        REGISTRY
            .lock()
            .map_err(|_| ServiceRegistryError::LockPoisoned)?
            .insert(name.into(), handle);
        info!("Service {:?} registered by {:?}", name, role);
        Ok(())
    }

    /// Remove a previously registered service.
    pub fn unregister_service(name: &str) -> RegistryResult<()> {
        REGISTRY
            .lock()
            .map_err(|_| ServiceRegistryError::LockPoisoned)?
            .remove(name);
        info!("Service {:?} unregistered", name);
        Ok(())
    }

    /// Lookup a service handle if visible to the current role.
    pub fn lookup(name: &str) -> RegistryResult<Option<ServiceHandle>> {
        let role = RoleManifest::current_role();
        let opt = REGISTRY
            .lock()
            .map_err(|_| ServiceRegistryError::LockPoisoned)?
            .get(name)
            .cloned()
            .filter(|h| h.role == role || matches!(role, Role::QueenPrimary));
        info!("Lookup for service {:?} by {:?}: {}", name, role, opt.is_some());
        Ok(opt)
    }

    /// Clear all registered services. Only used in tests.
    pub fn reset() -> RegistryResult<()> {
        REGISTRY
            .lock()
            .map_err(|_| ServiceRegistryError::LockPoisoned)?
            .clear();
        Ok(())
    }

    /// Return the names of all registered services.
    pub fn list_services() -> RegistryResult<Vec<String>> {
        let list = REGISTRY
            .lock()
            .map_err(|_| ServiceRegistryError::LockPoisoned)?
            .keys()
            .cloned()
            .collect();
        Ok(list)
    }
}

pub struct TestRegistryGuard;

impl TestRegistryGuard {
    pub fn new() -> Self {
        let _ = ServiceRegistry::reset();
        TestRegistryGuard
    }
}

impl Drop for TestRegistryGuard {
    fn drop(&mut self) {
        let _ = ServiceRegistry::reset();
    }
}
