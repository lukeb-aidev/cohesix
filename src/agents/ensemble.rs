// CLASSIFICATION: COMMUNITY
// Filename: ensemble.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-22

//! Cooperative ensemble agents with shared memory.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;

pub enum Arbitration {
    Voting,
    Weighted,
    Fallback,
}

pub struct SharedMemory {
    path: String,
}

impl SharedMemory {
    pub fn new(id: &str) -> Self {
        let tmpdir = std::env::var("TMPDIR").unwrap_or("/tmp".to_string());
        let root = std::env::var("COHESIX_ENS_TMP").unwrap_or_else(|_| format!("{}/ensemble", tmpdir));
        Self { path: format!("{root}/{id}/mem") }
    }
    pub fn read(&self) -> Option<String> { fs::read_to_string(&self.path).ok() }
    pub fn write(&self, data: &str) {
        let base = std::path::Path::new(&self.path).parent().unwrap_or_else(|| std::path::Path::new("/ensemble"));
        fs::create_dir_all(base).ok();
        fs::write(&self.path, data).ok();
    }
}

pub trait DecisionAgent {
    fn propose(&mut self, mem: &SharedMemory) -> (String, f32);
}

pub struct EnsembleAgent {
    pub id: String,
    pub members: Vec<Box<dyn DecisionAgent>>,
    pub memory: SharedMemory,
    pub strategy: Arbitration,
}

use crate::agent_transport::AgentTransport;
use crate::agent_migration::{Migrateable, MigrationStatus};
use serde_json;

impl Migrateable for EnsembleAgent {
    fn migrate<T: AgentTransport>(&self, peer: &str, transport: &T) -> anyhow::Result<MigrationStatus> {
        let tmpdir = std::env::var("TMPDIR").unwrap_or("/tmp".to_string());
        let tmp = format!("{}/{}_ensemble.json", tmpdir, self.id);
        let data = serde_json::to_string(&self.members.len()).unwrap_or_default();
        fs::write(&tmp, data)?;
        transport.send_state(&self.id, peer, &tmp)?;
        Ok(MigrationStatus::Completed)
    }
}

impl EnsembleAgent {
    pub fn new(id: &str, strategy: Arbitration) -> Self {
        Self { id: id.into(), members: Vec::new(), memory: SharedMemory::new(id), strategy }
    }

    pub fn add_agent(&mut self, a: Box<dyn DecisionAgent>) { self.members.push(a); }

    pub fn tick(&mut self) -> String {
        let mut proposals = Vec::new();
        for a in self.members.iter_mut() { proposals.push(a.propose(&self.memory)); }
        let action = match self.strategy {
            Arbitration::Voting => {
                let mut counts: HashMap<String, usize> = HashMap::new();
                for (act, _) in &proposals { *counts.entry(act.clone()).or_default() += 1; }
                counts.into_iter().max_by_key(|e| e.1).map(|(a, _)| a).unwrap_or_default()
            }
            Arbitration::Weighted => {
                proposals.iter().cloned().max_by(|a, b| a.1.partial_cmp(&b.1).unwrap()).map(|p| p.0).unwrap_or_default()
            }
            Arbitration::Fallback => proposals.get(0).map(|p| p.0.clone()).unwrap_or_default(),
        };
        let _ = self.log_scores(&proposals);
        action
    }

    fn log_scores(&self, scores: &[(String, f32)]) -> std::io::Result<()> {
        let tmpdir = std::env::var("TMPDIR").unwrap_or("/tmp".to_string());
        let root = std::env::var("COHESIX_ENS_TMP").unwrap_or_else(|_| format!("{}/ensemble", tmpdir));
        fs::create_dir_all(format!("{root}/{}/", self.id))?;
        let goals = serde_json::to_string(scores).unwrap_or_else(|_| "[]".into());
        fs::write(format!("{root}/{}/goals.json", self.id), goals)?;
        let trace_root = std::env::var("COHESIX_TRACE_TMP").unwrap_or_else(|_| format!("{}/trace", tmpdir));
        let mut f = OpenOptions::new().create(true).append(true)
            .open(format!("{trace_root}/ensemble_{}.log", self.id))?;
        writeln!(f, "tick")?;
        Ok(())
    }
}
