// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-08

use crate::prelude::*;
//! World model snapshot and sync helpers.

use serde::{Deserialize, Serialize};
use std::fs;

/// Single entity in the world model.
#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq)]
pub struct Entity {
    pub id: String,
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub force: [f32; 3],
}

/// Snapshot of the entire world state.
#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq)]
pub struct WorldModelSnapshot {
    pub version: u64,
    pub entities: Vec<Entity>,
    pub agent_state: String,
    pub active_goals: Vec<String>,
    pub role: String,
    pub gpu_hash: Option<String>,
}

impl WorldModelSnapshot {
    /// Save snapshot to a JSON file.
    pub fn save(&self, path: &str) -> anyhow::Result<()> {
        let data = serde_json::to_vec_pretty(self)?;
        fs::write(path, data)?;
        Ok(())
    }

    /// Load snapshot from a JSON file.
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let data = fs::read(path)?;
        let snap = serde_json::from_slice(&data)?;
        Ok(snap)
    }
}


