// CLASSIFICATION: COMMUNITY
// Filename: config.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-08-17

use once_cell::sync::Lazy;
use std::path::PathBuf;
use std::sync::RwLock;
use thiserror::Error;

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

/// Errors produced by validator config operations.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("validator config lock poisoned")]
    LockPoisoned,
}

/// Replace the global validator configuration.
pub fn set_config(cfg: ValidatorConfig) -> Result<(), ConfigError> {
    let mut guard = CONFIG
        .write()
        .map_err(|_| ConfigError::LockPoisoned)?;
    *guard = cfg;
    Ok(())
}

/// Get a clone of the current configuration.
pub fn get_config() -> Result<ValidatorConfig, ConfigError> {
    CONFIG
        .read()
        .map_err(|_| ConfigError::LockPoisoned)
        .map(|g| g.clone())
}
