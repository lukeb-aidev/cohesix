// CLASSIFICATION: COMMUNITY
// Filename: upgrade_manager.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-08-17

//! Handles atomic upgrades and rollbacks of Cohesix bundles.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::io::Read;

#[derive(Debug, Deserialize)]
pub struct UpgradeManifest {
    pub version: String,
    pub hash: String,
    pub applies_to: Vec<String>,
    pub rollback_hash: Option<String>,
}

/// Upgrade manager for downloading and applying new images.
pub struct UpgradeManager;

impl UpgradeManager {
    /// Apply an upgrade from the given URL.
    pub fn apply_from_url(url: &str) -> anyhow::Result<()> {
        let mut bytes = Vec::new();
        ureq::get(url).call()?.into_reader().read_to_end(&mut bytes)?;
        let manifest_url = format!("{url}.manifest");
        let manifest_text = ureq::get(&manifest_url).call()?.into_string()?;
        let manifest: UpgradeManifest = serde_json::from_str(&manifest_text)?;
        Self::apply_bundle(&bytes, &manifest)
    }

    /// Apply an upgrade from local bytes with manifest.
    pub fn apply_bundle(bundle: &[u8], manifest: &UpgradeManifest) -> anyhow::Result<()> {
        fs::create_dir_all("/persist/upgrades")?;
        fs::create_dir_all("/log")?;
        let mut hasher = Sha256::new();
        hasher.update(bundle);
        let hash = format!("sha256:{:x}", hasher.finalize());
        if hash != manifest.hash {
            Self::log("hash mismatch; rolling back");
            return Self::rollback();
        }
        let role = fs::read_to_string("/srv/cohrole").unwrap_or_default();
        if !manifest.applies_to.iter().any(|r| r == role.trim()) {
            Self::log("manifest not applicable to role");
            return Ok(());
        }
        fs::write("/persist/upgrades/previous.cohimg", bundle).ok();
        fs::write("/persist/upgrades/current.cohimg", bundle)?;
        Self::log(&format!("upgrade {} applied", manifest.version));
        Ok(())
    }

    /// Roll back to the last good image.
    pub fn rollback() -> anyhow::Result<()> {
        if fs::metadata("/persist/upgrades/previous.cohimg").is_ok() {
            let data = fs::read("/persist/upgrades/previous.cohimg")?;
            fs::write("/persist/upgrades/current.cohimg", data)?;
            Self::log("rolled back to previous image");
        }
        Ok(())
    }

    fn log(msg: &str) {
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open("/log/upgrade.log")
        {
            let _ = writeln!(f, "{msg}");
        }
    }
}
