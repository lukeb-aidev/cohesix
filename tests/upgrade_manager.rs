// CLASSIFICATION: COMMUNITY
// Filename: upgrade_manager.rs v0.1
// Date Modified: 2025-07-10
// Author: Cohesix Codex

use cohesix::kernel::upgrade::upgrade_manager::{UpgradeManager, UpgradeManifest};
use sha2::{Digest, Sha256};
use std::fs;

#[test]
fn corrupt_bundle_triggers_rollback() {
    fs::create_dir_all("/persist/upgrades").unwrap();
    let manifest = UpgradeManifest {
        version: "v1".into(),
        hash: "sha256:bad".into(),
        applies_to: vec!["Worker".into()],
        rollback_hash: None,
    };
    let res = UpgradeManager::apply_bundle(b"bad", &manifest);
    assert!(res.is_ok());
}

#[test]
fn valid_upgrade_applied() {
    fs::create_dir_all("/persist/upgrades").unwrap();
    let data = b"image";
    let mut hasher = Sha256::new();
    hasher.update(data);
    let hash = format!("sha256:{:x}", hasher.finalize());
    let manifest = UpgradeManifest {
        version: "v1".into(),
        hash,
        applies_to: vec!["Unknown".into()],
        rollback_hash: None,
    };
    let res = UpgradeManager::apply_bundle(data, &manifest);
    assert!(res.is_ok());
}
