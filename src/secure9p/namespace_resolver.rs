// CLASSIFICATION: COMMUNITY
// Filename: namespace_resolver.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-25

//! Map agent identifiers to mount namespaces.

#[cfg(feature = "secure9p")]
use anyhow::{anyhow, Result};
#[cfg(feature = "secure9p")]
use once_cell::sync::Lazy;
#[cfg(feature = "secure9p")]
use serde::Deserialize;
#[cfg(feature = "secure9p")]
use std::{collections::HashMap, path::{Path, PathBuf}, sync::RwLock};

#[cfg(feature = "secure9p")]
#[derive(Clone)]
pub struct MountNamespace {
    pub root: PathBuf,
    pub readonly: bool,
}

#[cfg(feature = "secure9p")]
#[derive(Deserialize)]
struct Config {
    namespace: Vec<NsEntry>,
}

#[cfg(feature = "secure9p")]
#[derive(Deserialize)]
struct NsEntry {
    agent: String,
    root: String,
    read_only: bool,
}

#[cfg(feature = "secure9p")]
static MAP: Lazy<RwLock<Option<HashMap<String, MountNamespace>>>> = Lazy::new(|| RwLock::new(None));

#[cfg(feature = "secure9p")]
fn load(path: &Path) -> Result<HashMap<String, MountNamespace>> {
    let text = std::fs::read_to_string(path)?;
    let cfg: Config = toml::from_str(&text)?;
    let mut map = HashMap::new();
    for n in cfg.namespace {
        map.insert(
            n.agent,
            MountNamespace { root: PathBuf::from(n.root), readonly: n.read_only },
        );
    }
    Ok(map)
}

/// Resolve the namespace for an agent using `config/secure9p.toml`.
#[cfg(feature = "secure9p")]
pub fn resolve_namespace(agent: &str) -> Result<MountNamespace> {
    let mut guard = MAP.write().unwrap();
    if guard.is_none() {
        *guard = Some(load(Path::new("config/secure9p.toml"))?);
    }
    let map = guard.as_ref().unwrap();
    map.get(agent).cloned().ok_or_else(|| anyhow!("unknown agent"))
}

#[cfg(all(test, feature = "secure9p"))]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn resolves_namespace() {
        let dir = std::env::temp_dir();
        let cfg_path = dir.join("secure9p.toml");
        fs::write(
            &cfg_path,
            "\n[[namespace]]\nagent='a'\nroot='/x'\nread_only=true\n",
        )
        .unwrap();
        {
            let mut map = MAP.write().unwrap();
            *map = None;
        }
        std::fs::create_dir_all("config").ok();
        std::fs::copy(&cfg_path, "config/secure9p.toml").unwrap();
        let ns = resolve_namespace("a").unwrap();
        assert!(ns.readonly);
    }
}
