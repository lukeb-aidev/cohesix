// CLASSIFICATION: COMMUNITY
// Filename: fs.rs v0.1
// Date Modified: 2025-06-08
// Author: Lukas Bower

//! Minimal in-memory filesystem stub for Cohesix-9P.

/// Dummy filesystem handle.
#[derive(Default)]
pub struct InMemoryFs;

impl InMemoryFs {
    /// Mount the filesystem at the provided path. This is a no-op stub.
    pub fn mount(&self, mountpoint: &str) {
        println!("[fs] mounting at {} (stub)", mountpoint);
    }
}
