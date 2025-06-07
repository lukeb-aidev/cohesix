// CLASSIFICATION: COMMUNITY
// Filename: fs.rs v0.1
// Date Modified: 2025-06-08
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
}

impl InMemoryFs {
    /// Create a new filesystem instance with default `/srv` nodes.
    pub fn new() -> Self {
        let mut fs = Self::default();
        fs.nodes.insert("/srv/cohrole".into(), b"Unknown".to_vec());
        fs.nodes.insert("/srv/telemetry".into(), Vec::new());
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

    /// Retrieve contents of a file if present.
    pub fn read(&self, path: &str) -> Option<&[u8]> {
        self.nodes.get(path).map(|v| v.as_slice())
    }
}
