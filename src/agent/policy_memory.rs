// CLASSIFICATION: COMMUNITY
// Filename: policy_memory.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-12

//! Persistent policy memory utilities.

use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct PolicyMemory {
    pub decisions: Vec<String>,
    pub q_values: Vec<f32>,
    pub successes: Vec<bool>,
}

impl PolicyMemory {
    pub fn load(agent_id: &str) -> anyhow::Result<Self> {
        let path = format!("/persist/policy/agent_{agent_id}.policy.json");
        if let Ok(data) = fs::read(&path) {
            let mem = serde_json::from_slice(&data)?;
            Ok(mem)
        } else {
            Ok(Self::default())
        }
    }

    pub fn save(&self, agent_id: &str) -> anyhow::Result<()> {
        let path = format!("/persist/policy/agent_{agent_id}.policy.json");
        fs::create_dir_all("/persist/policy").ok();
        let data = serde_json::to_vec_pretty(self)?;
        fs::write(path, &data)?;
        Self::save_shared(self)?;
        Ok(())
    }

    /// Save the policy memory to a shared location for quick retrieval.
    pub fn save_shared(mem: &Self) -> anyhow::Result<()> {
        fs::create_dir_all("/srv").ok();
        let buf = serde_json::to_vec(mem)?;
        fs::write("/srv/policy_shared.json", buf)?;
        Ok(())
    }

    /// Load policy memory from the shared location if present.
    pub fn load_shared() -> anyhow::Result<Self> {
        if let Ok(buf) = fs::read("/srv/policy_shared.json") {
            let m = serde_json::from_slice(&buf)?;
            Ok(m)
        } else {
            Ok(Self::default())
        }
    }
}

