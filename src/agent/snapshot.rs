// CLASSIFICATION: COMMUNITY
// Filename: snapshot.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-04

#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
/// Agent snapshot helpers for live migration.
//
/// Serializes an agent's policy, memory snapshot, and metrics
/// to a MessagePack blob which can be transferred to another
/// worker node.
use serde::{Deserialize, Serialize};
use std::fs;

/// Serializable snapshot structure.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct AgentSnapshot {
    /// Policy description or identifier.
    pub policy: String,
    /// Raw memory image (opaque bytes).
    pub memory: Vec<u8>,
    /// Metrics in JSON form.
    pub metrics: String,
}

use crate::agent_migration::{Migrateable, MigrationStatus};
use crate::agent_transport::AgentTransport;
use crate::CohError;

impl Migrateable for AgentSnapshot {
    fn migrate<T: AgentTransport>(
        &self,
        peer: &str,
        transport: &T,
    ) -> Result<MigrationStatus, CohError> {
        let tmpdir = std::env::var("TMPDIR").unwrap_or("/srv".to_string());
        let tmp = format!("{}/agent_snapshot.msgpack", tmpdir);
        SnapshotWriter::write(&tmp, self)?;
        transport.send_state("snapshot", peer, &tmp)?;
        Ok(MigrationStatus::Completed)
    }
}

/// Writer for agent snapshots.
pub struct SnapshotWriter;

impl SnapshotWriter {
    /// Write the snapshot to the specified path.
    pub fn write(path: &str, snapshot: &AgentSnapshot) -> Result<(), CohError> {
        let data = rmp_serde::to_vec(snapshot)?;
        fs::write(path, data)?;
        Ok(())
    }
}

/// Reader for agent snapshots.
pub struct SnapshotReader;

impl SnapshotReader {
    /// Read a snapshot from the specified file.
    pub fn read(path: &str) -> Result<AgentSnapshot, CohError> {
        let buf = fs::read(path)?;
        let snap = rmp_serde::from_slice(&buf)?;
        Ok(snap)
    }
}
