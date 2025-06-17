// CLASSIFICATION: COMMUNITY
// Filename: role_config.rs v0.3
// Author: Lukas Bower
// Date Modified: 2025-09-06

use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, PartialEq)]
pub struct RoleConfig {
    pub telemetry_interval: Option<u64>,
    pub trace_policy: Option<String>,
    pub validator: Option<bool>,
}

impl Default for RoleConfig {
    fn default() -> Self {
        Self {
            telemetry_interval: Some(30),
            trace_policy: Some("/etc/trace/policy.yaml".into()),
            validator: Some(true),
        }
    }
}

fn load_config_file(path: &Path) -> std::io::Result<RoleConfig> {
    let data = fs::read_to_string(path)?;
    serde_yaml::from_str(&data).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

pub fn load_for_role(dir: &Path, role: &str) -> std::io::Result<RoleConfig> {
    load_config_file(&dir.join(format!("{role}.yaml")))
}

pub fn load_active() -> RoleConfig {
    let cohrole_path = std::env::var("COHROLE_PATH").unwrap_or_else(|_| "/srv/cohrole".into());
    let roles_dir: PathBuf = std::env::var("ROLE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/roles"));
    let role = fs::read_to_string(&cohrole_path).unwrap_or_else(|_| "default".into());
    match load_for_role(&roles_dir, role.trim()) {
        Ok(cfg) => cfg,
        Err(_) => match load_for_role(&roles_dir, "default") {
            Ok(cfg) => cfg,
            Err(e) => {
                eprintln!("[init] using default role config: {}", e);
                RoleConfig::default()
            }
        },
    }
}
