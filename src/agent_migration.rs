// CLASSIFICATION: COMMUNITY
// Filename: agent_migration.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-07

//! High level agent migration helpers used by the orchestrator.
//!
//! Wraps the lower-level snapshot routines and federation
//! transfer helpers to move agents between workers.

use crate::agents::migration as snap;
use crate::federation::migration as fed_mig;
use serde_json;

/// Export the agent state and send it to the target peer.
pub fn migrate(agent_id: &str, peer: &str) -> anyhow::Result<()> {
    let state = snap::serialize(agent_id)?;
    let path = format!("/mnt/snapshots/agent_{agent_id}.json");
    std::fs::create_dir_all("/mnt/snapshots").ok();
    std::fs::write(&path, serde_json::to_vec(&state)?)?;
    fed_mig::migrate_agent(agent_id, peer)?;
    Ok(())
}
