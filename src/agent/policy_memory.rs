// CLASSIFICATION: COMMUNITY
// Filename: policy_memory.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-08

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
        fs::write(path, data)?;
        Ok(())
    }
}

