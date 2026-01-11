// Author: Lukas Bower
// Purpose: Emit deterministic artefacts from the root-task manifest.

mod cli;
mod docs;
mod rust;

use crate::ir::Manifest;
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

pub use docs::DocFragments;

#[derive(Debug)]
pub struct GeneratedArtifacts {
    pub rust_dir: PathBuf,
    pub manifest_json: PathBuf,
    pub manifest_hash: PathBuf,
    pub cli_script: PathBuf,
    pub doc_snippet: PathBuf,
}

impl GeneratedArtifacts {
    pub fn summary(&self) -> String {
        format!(
            "rust={}, manifest={}, cli={}, docs={}",
            self.rust_dir.display(),
            self.manifest_json.display(),
            self.cli_script.display(),
            self.doc_snippet.display()
        )
    }
}

pub fn emit_all(
    manifest: &Manifest,
    manifest_hash: &str,
    resolved_json: &[u8],
    options: &crate::CompileOptions,
    docs: &DocFragments,
) -> Result<GeneratedArtifacts> {
    fs::create_dir_all(&options.out_dir)
        .with_context(|| format!("failed to create {}", options.out_dir.display()))?;
    if let Some(parent) = options.manifest_out.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    if let Some(parent) = options.cli_script_out.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    if let Some(parent) = options.doc_snippet_out.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    rust::emit_rust(manifest, manifest_hash, &options.out_dir)?;
    cli::emit_cli_script(manifest, &options.cli_script_out)?;
    docs::emit_doc_snippet(manifest_hash, docs, &options.doc_snippet_out)?;

    fs::write(&options.manifest_out, resolved_json).with_context(|| {
        format!(
            "failed to write resolved manifest {}",
            options.manifest_out.display()
        )
    })?;

    let hash_path = options
        .manifest_out
        .with_extension("json.sha256");
    let hash_contents = format!(
        "# Author: Lukas Bower\n# Purpose: SHA-256 fingerprint for root_task_resolved.json.\n{}  {}\n",
        manifest_hash,
        options
            .manifest_out
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("root_task_resolved.json")
    );
    fs::write(&hash_path, hash_contents).with_context(|| {
        format!("failed to write manifest hash {}", hash_path.display())
    })?;

    Ok(GeneratedArtifacts {
        rust_dir: options.out_dir.clone(),
        manifest_json: options.manifest_out.clone(),
        manifest_hash: hash_path,
        cli_script: options.cli_script_out.clone(),
        doc_snippet: options.doc_snippet_out.clone(),
    })
}

pub fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    hex::encode(digest)
}

pub fn hash_path(path: &Path) -> Result<String> {
    let contents = fs::read(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    Ok(hash_bytes(&contents))
}
