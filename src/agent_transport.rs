// CLASSIFICATION: COMMUNITY
// Filename: agent_transport.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-15

/// Agent transport helpers for migration.
//
/// Provides filesystem-based transfer of agent snapshots
/// and abstract trait for pluggable transport mechanisms.

use std::fs;
use anyhow::Result;

/// Interface for sending agent state to a remote peer.
pub trait AgentTransport {
    /// Send the given snapshot file to the named peer.
    fn send_state(&self, agent_id: &str, peer: &str, path: &str) -> Result<()>;
}

/// Simple filesystem transport used for local federation directories.
pub struct FilesystemTransport;

impl AgentTransport for FilesystemTransport {
    fn send_state(&self, agent_id: &str, peer: &str, path: &str) -> Result<()> {
        let data = fs::read(path)?;
        let dest_dir = format!("/srv/federation/state/{peer}/incoming");
        fs::create_dir_all(&dest_dir)?;
        fs::write(format!("{dest_dir}/{agent_id}.json"), data)?;
        Ok(())
    }
}
