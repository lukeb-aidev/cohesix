// CLASSIFICATION: COMMUNITY
// Filename: base.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-09

//! Minimal base agent with introspection logging and self-diagnosis.

use crate::sim::introspect::{self, IntrospectionData};

pub struct BaseAgent {
    id: String,
    error_history: Vec<f32>,
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
