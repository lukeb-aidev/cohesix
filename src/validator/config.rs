// CLASSIFICATION: COMMUNITY
// Filename: config.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-22

use once_cell::sync::Lazy;
use std::path::{PathBuf};
use std::sync::RwLock;

/// Runtime configurable validator paths.
#[derive(Clone)]
pub struct ValidatorConfig {
    pub log_dir: PathBuf,
    pub violations_dir: PathBuf,
}

impl Default for ValidatorConfig {
    fn default() -> Self {
        let tmp = std::env::temp_dir();
        Self {
            log_dir: std::env::var("COHESIX_LOG_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| tmp.clone()),
            violations_dir: std::env::var("COHESIX_VIOLATIONS_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| tmp),
        }
    }
}

static CONFIG: Lazy<RwLock<ValidatorConfig>> = Lazy::new(|| RwLock::new(ValidatorConfig::default()));

/// Replace the global validator configuration.
pub fn set_config(cfg: ValidatorConfig) {
    let mut guard = CONFIG.write().expect("config lock");
    *guard = cfg;
}

/// Get a clone of the current configuration.
pub fn get_config() -> ValidatorConfig {
    CONFIG.read().expect("config lock").clone()
}
