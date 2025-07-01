// CLASSIFICATION: COMMUNITY
// Filename: base.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-22

use crate::prelude::*;
//! Minimal base agent with introspection logging and self-diagnosis.

use crate::sim::introspect::{self, IntrospectionData};
use serde_json;

pub struct BaseAgent {
    id: String,
    error_history: Vec<f32>,
}

use crate::agent_transport::AgentTransport;
use crate::agent_migration::{Migrateable, MigrationStatus};

impl Migrateable for BaseAgent {
    fn migrate<T: AgentTransport>(&self, peer: &str, transport: &T) -> anyhow::Result<MigrationStatus> {
        let base_tmp = std::env::var("TMPDIR").unwrap_or_else(|_| "/srv".to_string());
        let tmp = format!("{}/{}_base.json", base_tmp, self.id);
        let data = serde_json::json!({"id": self.id});
        std::fs::write(&tmp, data.to_string())?;
        transport.send_state(&self.id, peer, &tmp)?;
        Ok(MigrationStatus::Completed)
    }
}

impl BaseAgent {
    /// Create a new agent handle.
    pub fn new(id: &str) -> Self {
        Self { id: id.into(), error_history: Vec::new() }
    }

    /// Run one tick with the provided action error and introspection data.
    /// Returns true if warning triggered.
    pub fn tick(&mut self, action_error: f32, data: &IntrospectionData) -> bool {
        introspect::log(&self.id, data);
        self.error_history.push(action_error);
        if self.error_history.len() > 10 {
            self.error_history.remove(0);
        }
        let avg: f32 = self.error_history.iter().copied().sum::<f32>() / self.error_history.len() as f32;
        avg > 1.0
    }
}
