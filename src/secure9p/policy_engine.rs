// CLASSIFICATION: COMMUNITY
// Filename: policy_engine.rs v0.5
// Author: Lukas Bower
// Date Modified: 2026-02-20

//! Policy evaluation for Secure9P operations.

#[cfg(feature = "secure9p")]
use serde::Deserialize;
#[cfg(feature = "secure9p")]
use std::collections::HashMap;
#[cfg(feature = "secure9p")]
use anyhow::{Result};

#[cfg(feature = "secure9p")]
#[derive(Deserialize)]
struct PolicyFile {
    policy: Vec<PolicyEntry>,
}

#[cfg(feature = "secure9p")]
#[derive(Clone, Deserialize)]
struct PolicyEntry {
    agent: String,
    allow: Vec<String>,
}

#[cfg(feature = "secure9p")]
#[derive(Clone, Default)]
pub struct PolicyEngine {
    rules: HashMap<String, Vec<(String, String)>>,
}

#[cfg(feature = "secure9p")]
impl PolicyEngine {
    /// Create a new empty policy engine.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an allow rule for `agent` in the form "verb:path".
    pub fn allow(&mut self, agent: String, rule: String) {
        if let Some((verb, path)) = rule.split_once(':') {
            self.rules
                .entry(agent)
                .or_default()
                .push((verb.to_string(), path.to_string()));
        }
    }

    pub fn load(path: &std::path::Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let pf: PolicyFile = if path.extension().and_then(|e| e.to_str()) == Some("json") {
            serde_json::from_str(&text)?
        } else {
            serde_yaml::from_str(&text)?
        };
        let mut rules = HashMap::new();
        for p in pf.policy {
            let parsed: Vec<(String, String)> = p
                .allow
                .into_iter()
                .filter_map(|s| s.split_once(':').map(|(a, b)| (a.to_string(), b.to_string())))
                .collect();
            rules.insert(p.agent, parsed);
        }
        Ok(Self { rules })
    }

    /// Verify that all policy entries reference valid roles.
    pub fn validate_roles(&self) -> Result<()> {
        for key in self.rules.keys() {
            if !crate::cohesix_types::RoleManifest::is_valid_role(key) {
                return Err(anyhow::anyhow!(format!("unknown role {key}")));
            }
        }
        Ok(())
    }

    pub fn allows(&self, agent: &str, verb: &str, path: &str) -> bool {
        self.rules
            .get(agent)
            .map(|v| v.iter().any(|(op, p)| op == verb && path.starts_with(p)))
            .unwrap_or(false)
    }

    pub fn check(&self, agent: &str, cap: crate::secure9p::cap_fid::Capability) -> bool {
        let verb = if cap.contains(crate::secure9p::cap_fid::Cap::READ) {
            "read"
        } else if cap.contains(crate::secure9p::cap_fid::Cap::WRITE) {
            "write"
        } else if cap.contains(crate::secure9p::cap_fid::Cap::REMOVE) {
            "remove"
        } else {
            return false;
        };
        self.rules
            .get(agent)
            .map(|v| v.iter().any(|(op, _)| op == verb))
            .unwrap_or(false)
    }

    pub fn policy_for(&self, agent: &str) -> Vec<(String, String)> {
        self.rules.get(agent).cloned().unwrap_or_default()
    }
}

#[cfg(all(test, feature = "secure9p"))]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn policy_check() {
        let tmp = std::env::temp_dir().join("p.json");
        fs::write(
            &tmp,
            "{\"policy\":[{\"agent\":\"lukas\",\"allow\":[\"read:/a\"]}]}",
        )
        .unwrap();
        let pe = PolicyEngine::load(&tmp).unwrap();
        assert!(pe.allows("lukas", "read", "/a/foo"));
        assert!(!pe.allows("lukas", "write", "/a/foo"));
    }
}
