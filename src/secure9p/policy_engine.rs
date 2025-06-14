// CLASSIFICATION: COMMUNITY
// Filename: policy_engine.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-24

use std::collections::HashMap;

use super::cap_fid::Capability;

#[derive(Clone)]
pub struct PolicyEngine {
    policies: HashMap<String, Vec<Capability>>,
}

impl PolicyEngine {
    pub fn new() -> Self {
        Self { policies: HashMap::new() }
    }

    pub fn allow(&mut self, agent: String, cap: Capability) {
        self.policies.entry(agent).or_default().push(cap);
    }

    pub fn check(&self, agent: &str, cap: Capability) -> bool {
        self.policies.get(agent).map_or(false, |v| v.contains(&cap))
    }
}
