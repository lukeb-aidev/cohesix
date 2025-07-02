// CLASSIFICATION: COMMUNITY
// Filename: fs.rs v0.5
// Author: Lukas Bower
// Date Modified: 2026-12-31

//! Minimal in-memory filesystem for Cohesix-9P.

use alloc::{collections::BTreeMap, sync::Arc, vec::Vec, string::{String, ToString}, format};
use crate::policy::{Access, SandboxPolicy};
use core::sync::atomic::{AtomicU64, Ordering};
use spin::RwLock;

/// Shared validator hook signature.
pub type ValidatorHook = dyn Fn(&'static str, String, String, u64) + Send + Sync;

/// Simple in-memory filesystem tree.
pub struct InMemoryFs {
    nodes: RwLock<BTreeMap<String, Vec<u8>>>, // path -> contents
    validator_hook: RwLock<Option<Arc<ValidatorHook>>>,
    policy: RwLock<Option<SandboxPolicy>>,
}

impl Default for InMemoryFs {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryFs {
    /// Create a new filesystem instance with default `/srv` nodes.
    pub fn new() -> Self {
        let mut nodes = BTreeMap::new();
        nodes.insert("/srv/cohrole".into(), b"Unknown".to_vec());
        nodes.insert("/srv/telemetry".into(), Vec::new());
        Self {
            nodes: RwLock::new(nodes),
            validator_hook: RwLock::new(None),
            policy: RwLock::new(None),
        }
    }

    /// Mount the filesystem at the provided path. No-op for inprocess builds.
    pub fn mount(&self, _mountpoint: &str) {}

    /// Register a new service under `/srv`.
    pub fn register_service(&self, name: &str, info: &[u8]) {
        let path = format!("/srv/{}", name);
        self.nodes.write().insert(path.clone(), info.to_vec());
        if let Some(hook) = &*self.validator_hook.read() {
            hook("srv_register", path, "kernel".into(), current_ts());
        }
    }

    /// Install a validator hook for access violations.
    pub fn set_validator_hook<F>(&self, hook: F)
    where
        F: Fn(&'static str, String, String, u64) + Send + Sync + 'static,
    {
        *self.validator_hook.write() = Some(Arc::new(hook));
    }

    /// Apply a sandbox policy controlling allowed paths.
    pub fn set_policy(&self, policy: SandboxPolicy) {
        *self.policy.write() = Some(policy);
    }

    /// Retrieve contents of a file if present.
    pub fn read(&self, path: &str, agent: &str) -> Option<Vec<u8>> {
        if let Some(pol) = &*self.policy.read() {
            if !pol.allows(path, Access::Read) {
                if let Some(hook) = &*self.validator_hook.read() {
                    hook("9p_access", path.to_string(), agent.to_string(), current_ts());
                }
                return None;
            }
        }
        self.nodes.read().get(path).cloned()
    }

    /// Write contents to a file, emitting violations if path is restricted.
    pub fn write(&self, path: &str, data: &[u8], agent: &str) {
        if let Some(pol) = &*self.policy.read() {
            if !pol.allows(path, Access::Write) {
                if let Some(hook) = &*self.validator_hook.read() {
                    hook("9p_access", path.to_string(), agent.to_string(), current_ts());
                }
                return;
            }
        }
        if path.starts_with("/persist") || path.starts_with("/srv/secure") {
            if let Some(hook) = &*self.validator_hook.read() {
                hook("9p_access", path.to_string(), agent.to_string(), current_ts());
            }
            return;
        }
        self.nodes.write().insert(path.into(), data.to_vec());
    }
}

static TS_COUNTER: AtomicU64 = AtomicU64::new(0);
fn current_ts() -> u64 {
    TS_COUNTER.fetch_add(1, Ordering::Relaxed)
}
