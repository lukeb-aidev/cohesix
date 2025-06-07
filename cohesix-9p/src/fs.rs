// CLASSIFICATION: COMMUNITY
// Filename: fs.rs v0.2
// Date Modified: 2025-07-09
// Author: Lukas Bower

//! Minimal in-memory filesystem for Cohesix-9P.
//! Supports a few synthetic nodes and dynamic service registration
//! under `/srv` to mirror Plan 9 semantics.

/// Dummy filesystem handle.
use std::collections::HashMap;

/// Simple in-memory filesystem tree.
#[derive(Default)]
pub struct InMemoryFs {
    nodes: HashMap<String, Vec<u8>>, // path -> contents
    validator_hook: Option<Box<dyn Fn(&'static str, String, String, u64) + Send + Sync>>,
}

impl InMemoryFs {
    /// Create a new filesystem instance with default `/srv` nodes.
    pub fn new() -> Self {
        let mut fs = Self::default();
        fs.nodes.insert("/srv/cohrole".into(), b"Unknown".to_vec());
        fs.nodes.insert("/srv/telemetry".into(), Vec::new());
        fs.validator_hook = None;
        fs
    }

    /// Mount the filesystem at the provided path. This is a no-op stub.
    pub fn mount(&self, mountpoint: &str) {
        println!("[fs] mounting at {} (stub)", mountpoint);
    }

    /// Register a new service under `/srv`.
    pub fn register_service(&mut self, name: &str, info: &[u8]) {
        let path = format!("/srv/{}", name);
        self.nodes.insert(path, info.to_vec());
    }

    /// Install a validator hook for access violations.
    pub fn set_validator_hook<F>(&mut self, hook: F)
    where
        F: Fn(&'static str, String, String, u64) + Send + Sync + 'static,
    {
        self.validator_hook = Some(Box::new(hook));
    }

    /// Retrieve contents of a file if present.
    pub fn read(&self, path: &str) -> Option<&[u8]> {
        self.nodes.get(path).map(|v| v.as_slice())
    }

    /// Write contents to a file, emitting violations if path is restricted.
    pub fn write(&mut self, path: &str, data: &[u8], agent: &str) {
        if path.starts_with("/persist") || path.starts_with("/srv/secure") {
            if let Some(hook) = &self.validator_hook {
                hook("9p_access", path.to_string(), agent.to_string(), current_ts());
            }
            return;
        }
        self.nodes.insert(path.into(), data.to_vec());
    }
}

fn current_ts() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
