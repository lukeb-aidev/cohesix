// CLASSIFICATION: COMMUNITY
// Filename: config.rs v0.1
// Author: Lukas Bower
// Date Modified: 2028-12-31

use cohesix_9p::policy::SandboxPolicy;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Clone)]
pub struct NamespaceEntry {
    pub agent: String,
    pub root: PathBuf,
    #[serde(default)]
    pub read_only: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PolicyEntry {
    pub agent: String,
    #[serde(default)]
    pub allow: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Secure9pConfig {
    #[serde(default)]
    pub namespace: Vec<NamespaceEntry>,
    #[serde(default)]
    pub policy: Vec<PolicyEntry>,
    pub port: u16,
    pub cert: PathBuf,
    pub key: PathBuf,
    #[serde(default)]
    pub ca_cert: Option<PathBuf>,
    #[serde(default)]
    pub require_client_auth: bool,
}

impl Secure9pConfig {
    pub fn load(path: &Path) -> io::Result<Self> {
        let text = fs::read_to_string(path)?;
        let mut cfg: Secure9pConfig =
            toml::from_str(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        cfg.rebase_paths(path);
        Ok(cfg)
    }

    fn rebase_paths(&mut self, path: &Path) {
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        self.cert = Self::join_base(base, &self.cert);
        self.key = Self::join_base(base, &self.key);
        if let Some(ca) = &mut self.ca_cert {
            *ca = Self::join_base(base, ca);
        }
        for ns in &mut self.namespace {
            ns.root = Self::join_base(base, &ns.root);
        }
    }

    fn join_base(base: &Path, p: &Path) -> PathBuf {
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            base.join(p)
        }
    }

    pub fn agent_policies(&self) -> HashMap<String, SandboxPolicy> {
        let mut map = HashMap::new();
        for entry in &self.policy {
            if let Some(policy) = entry.to_policy() {
                map.insert(entry.agent.clone(), policy);
            }
        }
        map
    }
}

impl PolicyEntry {
    fn to_policy(&self) -> Option<SandboxPolicy> {
        let mut read = Vec::new();
        let mut write = Vec::new();
        for rule in &self.allow {
            let mut parts = rule.splitn(2, ':');
            let verb = parts.next().unwrap_or_default();
            let path = match parts.next() {
                Some(p) if !p.is_empty() => p.trim().to_string(),
                _ => continue,
            };
            match verb {
                "read" => read.push(path),
                "write" => write.push(path),
                _ => continue,
            }
        }
        Some(SandboxPolicy { read, write })
    }
}
