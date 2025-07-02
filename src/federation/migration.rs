// CLASSIFICATION: COMMUNITY
// Filename: migration.rs v1.0
// Author: Codex
// Date Modified: 2025-06-07

use crate::prelude::*;
/// Agent migration utilities across federated clusters.
/// Uses snapshot files stored under `/mnt/snapshots/` and
/// replicates them to peer queens via the federation state
/// directory.

use std::fs;

/// Migrate an agent snapshot to the specified peer queen.
pub fn migrate_agent(agent_id: &str, peer: &str) -> Result<()> {
    let src = format!("/mnt/snapshots/agent_{agent_id}.json");
    let data = fs::read(&src)?;
    let dest_dir = format!("/srv/federation/state/{peer}/incoming");
    fs::create_dir_all(&dest_dir)?;
    let dest = format!("{dest_dir}/{agent_id}.json");
    fs::write(dest, data)?;
    Ok(())
}
