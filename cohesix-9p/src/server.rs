// CLASSIFICATION: COMMUNITY
// Filename: server.rs v0.4
// Author: Lukas Bower
// Date Modified: 2026-12-31

use crate::{fs::InMemoryFs, CohError, FsConfig};

/// Basic placeholder server for POSIX environments.
pub struct FsServer {
    cfg: FsConfig,
    fs: InMemoryFs,
}

impl FsServer {
    /// Create a new server with the provided configuration.
    pub fn new(cfg: FsConfig) -> Self {
        Self { cfg, fs: InMemoryFs::new() }
    }

    /// Start serving. This stub simply logs the mount point.
    pub fn start(&mut self) -> Result<(), CohError> {
        log::info!("Starting Cohesix 9P server on port {}", self.cfg.port);
        self.fs.mount("/srv");
        Ok(())
    }
}
