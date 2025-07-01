// CLASSIFICATION: COMMUNITY
// Filename: live_patch.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-05

use crate::prelude::*;
/// Live patching utilities for on-the-fly updates.

use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;

/// Runtime patching helper.
pub struct LivePatcher;

impl LivePatcher {
    /// Apply a binary patch to the given target path.
    pub fn apply(target: &str, binary: &[u8]) -> anyhow::Result<()> {
        let hash = Sha256::digest(binary);
        fs::create_dir_all("/srv/updates")?;
        let mut log = OpenOptions::new()
            .create(true)
            .append(true)
            .open("/srv/updates/patch.log")?;
        writeln!(log, "patch {} {}", target, hex::encode(hash))?;
        fs::write(target, binary)?;
        Ok(())
    }
}
