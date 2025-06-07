// CLASSIFICATION: COMMUNITY
// Filename: overlay.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-04

//! Overlay filesystem for combining remote /srv paths.
//!
//! This is a lightweight shadow-mount implementation used by the
//! Queen and Worker nodes to expose remote namespaces under
//! `/srv/worker/<id>` or `/srv/queen`.

use std::collections::HashMap;
use std::fs;

/// Simple overlay mapping of local → remote paths.
#[derive(Default)]
pub struct OverlayFS {
    mounts: HashMap<String, String>,
}

impl OverlayFS {
    /// Create a new overlay manager.
    pub fn new() -> Self {
        Self { mounts: HashMap::new() }
    }

    /// Mount a remote path over a local prefix.
    pub fn mount(&mut self, local: &str, remote: &str) {
        self.mounts.insert(local.to_string(), remote.to_string());
        let _ = fs::create_dir_all(local);
    }

    /// Resolve a local path to its remote counterpart if mounted.
    pub fn resolve(&self, local_path: &str) -> Option<String> {
        for (local, remote) in &self.mounts {
            if local_path.starts_with(local) {
                let suffix = local_path.trim_start_matches(local).trim_start_matches('/') ;
                return Some(format!("{}/{}", remote.trim_end_matches('/'), suffix));
            }
        }
        None
    }
}
