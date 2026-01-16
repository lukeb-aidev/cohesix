// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Emit deterministic artefacts from the root-task manifest.
// Author: Lukas Bower

mod cas;
mod cli;
mod cohsh;
pub mod cbor;
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
    pub cas_manifest_template: PathBuf,
    pub cas_manifest_template_hash: PathBuf,
    pub cli_script: PathBuf,
    pub doc_snippet: PathBuf,
    pub observability_interfaces_snippet: PathBuf,
    pub observability_security_snippet: PathBuf,
    pub cas_interfaces_snippet: PathBuf,
    pub cas_security_snippet: PathBuf,
    pub cbor_snippet: PathBuf,
    pub cohsh_policy: PathBuf,
    pub cohsh_policy_hash: PathBuf,
    pub cohsh_policy_rust: PathBuf,
    pub cohsh_policy_doc: PathBuf,
    pub cohsh_grammar_doc: PathBuf,
    pub cohsh_ticket_policy_doc: PathBuf,
}

impl GeneratedArtifacts {
    pub fn summary(&self) -> String {
        format!(
            "rust={}, manifest={}, cas_template={}, cas_hash={}, cli={}, docs={}, obs_interfaces={}, obs_security={}, cas_interfaces={}, cas_security={}, cbor={}, cohsh_policy={}, cohsh_hash={}, cohsh_rust={}, cohsh_docs={}, cohsh_grammar={}, cohsh_ticket_policy={}",
            self.rust_dir.display(),
            self.manifest_json.display(),
            self.cas_manifest_template.display(),
            self.cas_manifest_template_hash.display(),
            self.cli_script.display(),
            self.doc_snippet.display(),
            self.observability_interfaces_snippet.display(),
            self.observability_security_snippet.display(),
            self.cas_interfaces_snippet.display(),
            self.cas_security_snippet.display(),
            self.cbor_snippet.display(),
            self.cohsh_policy.display(),
            self.cohsh_policy_hash.display(),
            self.cohsh_policy_rust.display(),
            self.cohsh_policy_doc.display(),
            self.cohsh_grammar_doc.display(),
            self.cohsh_ticket_policy_doc.display()
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
    if let Some(parent) = options.cas_manifest_template_out.parent() {
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
    if let Some(parent) = options.observability_interfaces_snippet_out.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    if let Some(parent) = options.observability_security_snippet_out.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    if let Some(parent) = options.cas_interfaces_snippet_out.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    if let Some(parent) = options.cas_security_snippet_out.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    if let Some(parent) = options.cbor_snippet_out.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    if let Some(parent) = options.cohsh_policy_out.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    if let Some(parent) = options.cohsh_policy_rust_out.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    if let Some(parent) = options.cohsh_policy_doc_out.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    if let Some(parent) = options.cohsh_grammar_doc_out.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    if let Some(parent) = options.cohsh_ticket_policy_doc_out.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let manifest_dir = options.manifest_path.parent();
    rust::emit_rust(manifest, manifest_hash, &options.out_dir, manifest_dir)?;
    let cas_template = cas::build_cas_template(manifest);
    let cas_artifacts = cas::emit_cas_template(&cas_template, &options.cas_manifest_template_out)?;
    cli::emit_cli_script(manifest, &options.cli_script_out)?;
    docs::emit_doc_snippet(manifest_hash, docs, &options.doc_snippet_out)?;
    docs::emit_observability_interfaces_snippet(docs, &options.observability_interfaces_snippet_out)?;
    docs::emit_observability_security_snippet(docs, &options.observability_security_snippet_out)?;
    docs::emit_cas_interfaces_snippet(docs, &options.cas_interfaces_snippet_out)?;
    docs::emit_cas_security_snippet(docs, &options.cas_security_snippet_out)?;
    cbor::emit_cbor_snippet(&options.cbor_snippet_out)?;
    let cohsh_artifacts = cohsh::emit_cohsh_policy(
        manifest,
        manifest_hash,
        &options.cohsh_policy_out,
        &options.cohsh_policy_rust_out,
        &options.cohsh_policy_doc_out,
    )?;
    let cohsh_doc_artifacts = cohsh::emit_cohsh_docs(
        &options.cohsh_grammar_doc_out,
        &options.cohsh_ticket_policy_doc_out,
    )?;

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
        cas_manifest_template: cas_artifacts.template_json,
        cas_manifest_template_hash: cas_artifacts.template_hash,
        cli_script: options.cli_script_out.clone(),
        doc_snippet: options.doc_snippet_out.clone(),
        observability_interfaces_snippet: options.observability_interfaces_snippet_out.clone(),
        observability_security_snippet: options.observability_security_snippet_out.clone(),
        cas_interfaces_snippet: options.cas_interfaces_snippet_out.clone(),
        cas_security_snippet: options.cas_security_snippet_out.clone(),
        cbor_snippet: options.cbor_snippet_out.clone(),
        cohsh_policy: cohsh_artifacts.policy_toml,
        cohsh_policy_hash: cohsh_artifacts.policy_hash,
        cohsh_policy_rust: cohsh_artifacts.policy_rust,
        cohsh_policy_doc: cohsh_artifacts.policy_doc,
        cohsh_grammar_doc: cohsh_doc_artifacts.grammar_doc,
        cohsh_ticket_policy_doc: cohsh_doc_artifacts.ticket_policy_doc,
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
