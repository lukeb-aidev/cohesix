// CLASSIFICATION: COMMUNITY
// Filename: world_sync.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-08

use crate::prelude::*;
/// Worker side world model integration.

use crate::world_model::WorldModelSnapshot;
use std::fs;

/// Synchronise world model state from the Queen.
pub struct WorkerWorldSync;

impl WorkerWorldSync {
    /// Apply a snapshot received from the queen.
    pub fn apply(path: &str) -> Result<()> {
        let snap = WorldModelSnapshot::load(path)?;
        fs::create_dir_all("/sim").ok();
        snap.save("/sim/world.json")?;
        Ok(())
    }
}

