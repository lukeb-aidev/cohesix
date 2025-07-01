// CLASSIFICATION: PRIVATE
// Filename: verify.rs v1.0
// Author: Codex
// Date Modified: 2025-06-07

use crate::prelude::*;
/// Kernel and OS hash verification at boot.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;

#[derive(Deserialize)]
struct ManifestEntry {
    path: String,
    sha256: String,
}

#[derive(Deserialize)]
struct Manifest {
    files: Vec<ManifestEntry>,
}

/// Validate system files against the manifest located at `/srv/boot/hashes.json`.
/// Returns `true` if all hashes match.
pub fn verify_boot() -> anyhow::Result<bool> {
    let manifest: Manifest = serde_json::from_str(&fs::read_to_string("/srv/boot/hashes.json")?)?;
    fs::create_dir_all("/srv/boot")?;
    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/srv/boot/verify.log")?;
    let mut ok = true;
    for entry in manifest.files {
        match fs::read(&entry.path) {
            Ok(data) => {
                let digest = hex::encode(Sha256::digest(&data));
                if digest != entry.sha256 {
                    ok = false;
                    writeln!(log, "mismatch {} expected {} got {}", entry.path, entry.sha256, digest)?;
                }
            }
            Err(e) => {
                ok = false;
                writeln!(log, "error {} {}", entry.path, e)?;
            }
        }
    }
    Ok(ok)
}
