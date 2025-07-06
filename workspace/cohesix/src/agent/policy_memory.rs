// CLASSIFICATION: COMMUNITY
// Filename: policy_memory.rs v0.4
// Author: Lukas Bower
// Date Modified: 2025-08-16

#![cfg(feature = "std")]

#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
use crate::CohError;
/// Persistent policy memory utilities.
use serde::{Deserialize, Serialize};
#[cfg(feature = "std")]
use std::fs;
#[cfg(feature = "std")]
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct PolicyMemory {
    pub decisions: Vec<String>,
    pub q_values: Vec<f32>,
    pub successes: Vec<bool>,
}

impl PolicyMemory {
    pub fn load(agent_id: &str) -> Result<Self, CohError> {
        let primary = format!("/persist/policy/agent_{agent_id}.policy.json");
        if let Ok(data) = fs::read(&primary) {
            let mem = serde_json::from_slice(&data)?;
            Ok(mem)
        } else {
            // Fall back to a local path so tests can run without root perms
            let fallback = format!("persist/policy/agent_{agent_id}.policy.json");
            if let Ok(data) = fs::read(&fallback) {
                let mem = serde_json::from_slice(&data)?;
                Ok(mem)
            } else {
                Ok(Self::default())
            }
        }
    }

    pub fn save(&self, agent_id: &str) -> Result<(), CohError> {
        let data = serde_json::to_vec_pretty(self)?;
        let primary_path = format!("/persist/policy/agent_{agent_id}.policy.json");
        // Try to write to the standard location first
        let primary_res =
            fs::create_dir_all("/persist/policy").and_then(|_| fs::write(&primary_path, &data));

        if primary_res.is_err() {
            // Fallback for tests or sandboxed envs: write relative to cwd
            let local_dir = std::path::PathBuf::from("persist/policy");
            fs::create_dir_all(&local_dir)?;
            let fallback_path = local_dir.join(format!("agent_{agent_id}.policy.json"));
            fs::write(&fallback_path, &data)?;
        }

        Self::save_shared(self)?;
        Ok(())
    }

    /// Determine the shared policy path respecting environment overrides.
    fn shared_path() -> PathBuf {
        if let Ok(base) = std::env::var("COHESIX_POLICY_TMP").or_else(|_| std::env::var("TMPDIR")) {
            Path::new(&base).join("policy_shared.json")
        } else {
            Path::new("/srv").join("policy_shared.json")
        }
    }

    /// Save the policy memory to a shared location for quick retrieval.
    pub fn save_shared(mem: &Self) -> Result<(), CohError> {
        let path = Self::shared_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let buf = serde_json::to_vec(mem)?;
        fs::write(&path, buf)?;
        Ok(())
    }

    /// Load policy memory from the shared location if present.
    pub fn load_shared() -> Result<Self, CohError> {
        let path = Self::shared_path();
        if let Ok(buf) = fs::read(&path) {
            let m = serde_json::from_slice(&buf)?;
            Ok(m)
        } else {
            Ok(Self::default())
        }
    }
}
