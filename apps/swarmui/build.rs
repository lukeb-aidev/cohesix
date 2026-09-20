// Author: Lukas Bower
// Purpose: Bind native UI acceptance to the exact desktop sources, offline assets and selected contracts.
// Copyright 2026 Lukas Bower
use sha2::{Digest, Sha256};
use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
};

fn files(path: &Path, output: &mut Vec<PathBuf>) -> Result<(), Box<dyn Error>> {
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            files(&entry?.path(), output)?;
        }
    } else if path.is_file() {
        output.push(path.to_owned());
    }
    Ok(())
}
fn main() -> Result<(), Box<dyn Error>> {
    let root = Path::new("../..");
    let mut paths = Vec::new();
    for path in [
        "apps/swarmui/src",
        "apps/swarmui/src-tauri",
        "apps/swarmui/frontend",
        "apps/swarmui/reference",
        "apps/swarmui/Cargo.toml",
        "apps/swarmui/build.rs",
        "Cargo.toml",
        "apps/swarmui/tauri.conf.json",
        "crates/coh-cli",
        "Cargo.lock",
        "configs/generated/cohsh_policy.toml",
        "configs/generated/swarmui_defaults.toml",
    ] {
        files(&root.join(path), &mut paths)?;
    }
    paths.sort();
    let mut digest = Sha256::new();
    for path in paths {
        println!("cargo:rerun-if-changed={}", path.display());
        digest.update(path.strip_prefix(root)?.to_string_lossy().as_bytes());
        digest.update([0]);
        digest.update(fs::read(path)?);
    }
    let hash: String = digest
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    println!("cargo:rustc-env=SWARMUI_SOURCE_SHA256={hash}");
    tauri_build::build();
    Ok(())
}
