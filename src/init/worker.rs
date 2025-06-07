// CLASSIFICATION: COMMUNITY
// Filename: worker.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-06-18

//! seL4 root task hook for the DroneWorker role.
//! Mounts worker specific services like CUDA and BusyBox shell output.

use crate::plan9::namespace::NamespaceLoader;
use cohesix_9p::fs::InMemoryFs;

/// Entry point for the Worker root task.
pub fn start() {
    let ns = NamespaceLoader::load().unwrap_or_default();
    let _ = NamespaceLoader::apply(&ns);

    let mut fs = InMemoryFs::new();
    fs.mount("/srv/cuda");
    fs.mount("/srv/shell");
    fs.mount("/srv/diag");
}
