// CLASSIFICATION: COMMUNITY
// Filename: queen.rs v1.1
// Author: Lukas Bower
// Date Modified: 2025-06-08

use crate::prelude::*;
//! Role module for the Cohesix `QueenPrimary`.
//! The queen node governs distributed orchestration, manages metadata, dispatches workloads, and performs sandbox validation.

/// Trait representing the queen role's governance responsibilities.
pub trait QueenRole {
    fn initialize_governance(&mut self) -> Result<(), String>;
    fn dispatch_workload(&self, node_id: &str, workload: &str) -> Result<(), String>;
    fn validate_sandbox(&self, node_id: &str) -> bool;
}

use std::collections::HashSet;

/// Minimal Queen implementation tracking initialization and known nodes.
pub struct QueenPrimary {
    initialized: bool,
    known_nodes: HashSet<String>,
}

impl Default for QueenPrimary {
    fn default() -> Self {
        Self {
            initialized: false,
            known_nodes: HashSet::new(),
        }
    }
}

impl QueenRole for QueenPrimary {
    fn initialize_governance(&mut self) -> Result<(), String> {
        println!("[queen] initializing governance protocols...");
        self.initialized = true;
        Ok(())
    }

    fn dispatch_workload(&self, node_id: &str, workload: &str) -> Result<(), String> {
        println!(
            "[queen] dispatching workload '{}' to node '{}'",
            workload, node_id
        );
        if !self.initialized {
            return Err("governance not initialized".into());
        }
        self.known_nodes.insert(node_id.to_string());
        println!("[queen] queued '{}' for node '{}'", workload, node_id);
        Ok(())
    }

    fn validate_sandbox(&self, node_id: &str) -> bool {
        println!(
            "[queen] validating sandbox integrity for node '{}'",
            node_id
        );
        self.known_nodes.contains(node_id)
    }
}
