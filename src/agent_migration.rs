// CLASSIFICATION: COMMUNITY
// Filename: agent_migration.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-15

use crate::agent_transport::AgentTransport;
/// High level agent migration helpers used by the orchestrator.
//
/// Wraps the lower-level snapshot routines and federation
/// transfer helpers to move agents between workers.
use crate::agents::migration as snap;
use serde_json;

/// Status states for migration control.
#[derive(Debug, Clone)]
pub enum MigrationStatus {
    Init,
    SnapshotCreated,
    Transferred,
    Completed,
    Failed(String),
}

/// Export the agent state and send it via the provided transport.
pub fn migrate<T: AgentTransport>(
    agent_id: &str,
    peer: &str,
    transport: &T,
) -> Result<MigrationStatus, CohError> {
    let state = snap::serialize(agent_id)?;
    let path = format!("/mnt/snapshots/agent_{agent_id}.json");
    std::fs::create_dir_all("/mnt/snapshots").ok();
    std::fs::write(&path, serde_json::to_vec(&state)?)?;
    transport.send_state(agent_id, peer, &path)?;
    Ok(MigrationStatus::Completed)
}

/// Trait for structures that can initiate migration.
pub trait Migrateable {
    /// Migrate this agent to a peer via the given transport.
    fn migrate<T: AgentTransport>(&self, peer: &str, transport: &T) -> Result<MigrationStatus, CohError>;
}
