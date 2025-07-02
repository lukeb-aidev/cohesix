// CLASSIFICATION: COMMUNITY
// Filename: directory.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-04

use crate::prelude::*;
/// Agent directory table maintained under `/srv/agents/agent_table.json`.
use serde::{Deserialize, Serialize};
use serde_json;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

/// Record for a running agent.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AgentRecord {
    pub id: String,
    pub location: String,
    pub role: String,
    pub status: String,
    pub last_heartbeat: u64,
}

use crate::agent_migration::{Migrateable, MigrationStatus};
use crate::agent_transport::AgentTransport;
use crate::CohError;

impl Migrateable for AgentRecord {
    fn migrate<T: AgentTransport>(
        &self,
        peer: &str,
        transport: &T,
    ) -> Result<MigrationStatus, CohError> {
        let tmpdir = std::env::var("TMPDIR").unwrap_or("/srv".to_string());
        let tmp = format!("{}/record_{}.json", tmpdir, self.id);
        let data = serde_json::to_vec(self)?;
        std::fs::write(&tmp, data)?;
        transport.send_state(&self.id, peer, &tmp)?;
        Ok(MigrationStatus::Completed)
    }
}

/// Directory management utilities.
pub struct AgentDirectory;

impl AgentDirectory {
    /// Load existing table or return empty list.
    fn load() -> Vec<AgentRecord> {
        if let Ok(data) = fs::read_to_string("/srv/agents/agent_table.json") {
            serde_json::from_str(&data).unwrap_or_default()
        } else {
            Vec::new()
        }
    }

    fn save(recs: &[AgentRecord]) {
        if let Ok(data) = serde_json::to_string_pretty(recs) {
            let _ = fs::create_dir_all("/srv/agents");
            let _ = fs::write("/srv/agents/agent_table.json", data);
        }
    }

    /// Update or insert a record.
    pub fn update(rec: AgentRecord) {
        let mut table = Self::load();
        if let Some(existing) = table.iter_mut().find(|r| r.id == rec.id) {
            *existing = rec;
        } else {
            table.push(rec);
        }
        Self::save(&table);
    }

    /// Mark a heartbeat for an agent.
    pub fn heartbeat(id: &str) {
        let mut table = Self::load();
        if let Some(r) = table.iter_mut().find(|r| r.id == id) {
            r.last_heartbeat = timestamp();
        }
        Self::save(&table);
    }

    /// Remove an agent record.
    pub fn remove(id: &str) {
        let mut table = Self::load();
        table.retain(|r| r.id != id);
        Self::save(&table);
    }

    /// Return stale agent records based on timeout.
    pub fn stale(timeout_secs: u64) -> Vec<AgentRecord> {
        let now = timestamp();
        Self::load()
            .into_iter()
            .filter(|r| now.saturating_sub(r.last_heartbeat) > timeout_secs)
            .collect()
    }
}

fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
