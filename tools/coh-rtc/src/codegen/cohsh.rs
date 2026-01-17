// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Emit cohsh client policy artefacts derived from the root-task manifest.
// Author: Lukas Bower

use crate::codegen::hash_bytes;
use crate::ir::Manifest;
use anyhow::{Context, Result};
use cohsh_core::docs::{render_console_grammar_doc, render_ticket_policy_doc};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct CohshPolicyArtifacts {
    pub policy_toml: PathBuf,
    pub policy_hash: PathBuf,
    pub policy_rust: PathBuf,
    pub policy_doc: PathBuf,
}

#[derive(Debug)]
pub struct CohshDocArtifacts {
    pub grammar_doc: PathBuf,
    pub ticket_policy_doc: PathBuf,
}

#[derive(Debug)]
pub struct CohshClientArtifacts {
    pub client_rust: PathBuf,
    pub client_doc: PathBuf,
}

pub fn emit_cohsh_policy(
    manifest: &Manifest,
    manifest_hash: &str,
    policy_out: &Path,
    policy_rust_out: &Path,
    policy_doc_out: &Path,
) -> Result<CohshPolicyArtifacts> {
    let policy_toml = render_policy_toml(manifest, manifest_hash);
    fs::write(policy_out, &policy_toml)
        .with_context(|| format!("failed to write cohsh policy {}", policy_out.display()))?;

    let policy_hash_value = hash_bytes(policy_toml.as_bytes());
    let policy_hash_path = policy_out.with_extension("toml.sha256");
    let hash_contents = format!(
        "# Author: Lukas Bower\n# Purpose: SHA-256 fingerprint for cohsh_policy.toml.\n{}  {}\n",
        policy_hash_value,
        policy_out
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("cohsh_policy.toml")
    );
    fs::write(&policy_hash_path, hash_contents).with_context(|| {
        format!(
            "failed to write cohsh policy hash {}",
            policy_hash_path.display()
        )
    })?;

    let policy_doc = render_policy_doc(manifest, manifest_hash, &policy_hash_value);
    fs::write(policy_doc_out, policy_doc)
        .with_context(|| format!("failed to write cohsh policy doc {}", policy_doc_out.display()))?;

    let policy_rust = render_policy_rust(manifest, manifest_hash, &policy_hash_value);
    fs::write(policy_rust_out, policy_rust).with_context(|| {
        format!(
            "failed to write cohsh policy rust {}",
            policy_rust_out.display()
        )
    })?;

    Ok(CohshPolicyArtifacts {
        policy_toml: policy_out.to_path_buf(),
        policy_hash: policy_hash_path,
        policy_rust: policy_rust_out.to_path_buf(),
        policy_doc: policy_doc_out.to_path_buf(),
    })
}

pub fn emit_cohsh_client(
    manifest: &Manifest,
    manifest_hash: &str,
    client_rust_out: &Path,
    client_doc_out: &Path,
) -> Result<CohshClientArtifacts> {
    let client_rust = render_client_rust(manifest, manifest_hash);
    fs::write(client_rust_out, client_rust).with_context(|| {
        format!(
            "failed to write cohsh client rust {}",
            client_rust_out.display()
        )
    })?;

    let client_doc = render_client_doc(manifest, manifest_hash);
    fs::write(client_doc_out, client_doc).with_context(|| {
        format!(
            "failed to write cohsh client doc {}",
            client_doc_out.display()
        )
    })?;

    Ok(CohshClientArtifacts {
        client_rust: client_rust_out.to_path_buf(),
        client_doc: client_doc_out.to_path_buf(),
    })
}

pub fn emit_cohsh_docs(
    grammar_out: &Path,
    ticket_policy_out: &Path,
) -> Result<CohshDocArtifacts> {
    let grammar_doc = render_console_grammar_doc();
    fs::write(grammar_out, grammar_doc)
        .with_context(|| format!("failed to write cohsh grammar doc {}", grammar_out.display()))?;

    let ticket_policy_doc = render_ticket_policy_doc();
    fs::write(ticket_policy_out, ticket_policy_doc).with_context(|| {
        format!(
            "failed to write cohsh ticket policy doc {}",
            ticket_policy_out.display()
        )
    })?;

    Ok(CohshDocArtifacts {
        grammar_doc: grammar_out.to_path_buf(),
        ticket_policy_doc: ticket_policy_out.to_path_buf(),
    })
}

fn render_policy_toml(manifest: &Manifest, manifest_hash: &str) -> String {
    let mut contents = String::new();
    writeln!(contents, "# Author: Lukas Bower").ok();
    writeln!(
        contents,
        "# Purpose: Generated cohsh client policy derived from configs/root_task.toml."
    )
    .ok();
    writeln!(contents, "[meta]").ok();
    writeln!(contents, "manifest_sha256 = \"{}\"", manifest_hash).ok();
    writeln!(contents).ok();
    writeln!(contents, "[cohsh.pool]").ok();
    writeln!(
        contents,
        "control_sessions = {}",
        manifest.client_policies.cohsh.pool.control_sessions
    )
    .ok();
    writeln!(
        contents,
        "telemetry_sessions = {}",
        manifest.client_policies.cohsh.pool.telemetry_sessions
    )
    .ok();
    writeln!(contents).ok();
    writeln!(contents, "[retry]").ok();
    writeln!(
        contents,
        "max_attempts = {}",
        manifest.client_policies.retry.max_attempts
    )
    .ok();
    writeln!(
        contents,
        "backoff_ms = {}",
        manifest.client_policies.retry.backoff_ms
    )
    .ok();
    writeln!(
        contents,
        "ceiling_ms = {}",
        manifest.client_policies.retry.ceiling_ms
    )
    .ok();
    writeln!(
        contents,
        "timeout_ms = {}",
        manifest.client_policies.retry.timeout_ms
    )
    .ok();
    writeln!(contents).ok();
    writeln!(contents, "[heartbeat]").ok();
    writeln!(
        contents,
        "interval_ms = {}",
        manifest.client_policies.heartbeat.interval_ms
    )
    .ok();
    contents
}

fn render_policy_doc(manifest: &Manifest, manifest_hash: &str, policy_hash: &str) -> String {
    let mut contents = String::new();
    writeln!(contents, "<!-- Author: Lukas Bower -->").ok();
    writeln!(
        contents,
        "<!-- Purpose: Generated cohsh policy snippet consumed by docs/USERLAND_AND_CLI.md. -->"
    )
    .ok();
    writeln!(contents).ok();
    writeln!(contents, "### cohsh client policy (generated)").ok();
    writeln!(contents, "- `manifest.sha256`: `{}`", manifest_hash).ok();
    writeln!(contents, "- `policy.sha256`: `{}`", policy_hash).ok();
    writeln!(
        contents,
        "- `cohsh.pool.control_sessions`: `{}`",
        manifest.client_policies.cohsh.pool.control_sessions
    )
    .ok();
    writeln!(
        contents,
        "- `cohsh.pool.telemetry_sessions`: `{}`",
        manifest.client_policies.cohsh.pool.telemetry_sessions
    )
    .ok();
    writeln!(
        contents,
        "- `retry.max_attempts`: `{}`",
        manifest.client_policies.retry.max_attempts
    )
    .ok();
    writeln!(
        contents,
        "- `retry.backoff_ms`: `{}`",
        manifest.client_policies.retry.backoff_ms
    )
    .ok();
    writeln!(
        contents,
        "- `retry.ceiling_ms`: `{}`",
        manifest.client_policies.retry.ceiling_ms
    )
    .ok();
    writeln!(
        contents,
        "- `retry.timeout_ms`: `{}`",
        manifest.client_policies.retry.timeout_ms
    )
    .ok();
    writeln!(
        contents,
        "- `heartbeat.interval_ms`: `{}`",
        manifest.client_policies.heartbeat.interval_ms
    )
    .ok();
    writeln!(contents).ok();
    writeln!(
        contents,
        "_Generated from `configs/root_task.toml` (sha256: `{}`)._",
        manifest_hash
    )
    .ok();
    contents
}

fn render_policy_rust(manifest: &Manifest, manifest_hash: &str, policy_hash: &str) -> String {
    let mut contents = String::new();
    writeln!(contents, "// Author: Lukas Bower").ok();
    writeln!(
        contents,
        "// Purpose: Generated cohsh policy defaults derived from configs/root_task.toml."
    )
    .ok();
    writeln!(contents, "// @generated by coh-rtc; do not edit.").ok();
    writeln!(contents).ok();
    writeln!(
        contents,
        "pub const MANIFEST_SHA256: &str = \"{}\";",
        manifest_hash
    )
    .ok();
    writeln!(
        contents,
        "pub const POLICY_SHA256: &str = \"{}\";",
        policy_hash
    )
    .ok();
    writeln!(
        contents,
        "pub const COHSH_POOL_CONTROL_SESSIONS: u16 = {};",
        manifest.client_policies.cohsh.pool.control_sessions
    )
    .ok();
    writeln!(
        contents,
        "pub const COHSH_POOL_TELEMETRY_SESSIONS: u16 = {};",
        manifest.client_policies.cohsh.pool.telemetry_sessions
    )
    .ok();
    writeln!(
        contents,
        "pub const COHSH_RETRY_MAX_ATTEMPTS: u8 = {};",
        manifest.client_policies.retry.max_attempts
    )
    .ok();
    writeln!(
        contents,
        "pub const COHSH_RETRY_BACKOFF_MS: u64 = {};",
        manifest.client_policies.retry.backoff_ms
    )
    .ok();
    writeln!(
        contents,
        "pub const COHSH_RETRY_CEILING_MS: u64 = {};",
        manifest.client_policies.retry.ceiling_ms
    )
    .ok();
    writeln!(
        contents,
        "pub const COHSH_RETRY_TIMEOUT_MS: u64 = {};",
        manifest.client_policies.retry.timeout_ms
    )
    .ok();
    writeln!(
        contents,
        "pub const COHSH_HEARTBEAT_INTERVAL_MS: u64 = {};",
        manifest.client_policies.heartbeat.interval_ms
    )
    .ok();
    contents
}

fn render_client_rust(manifest: &Manifest, manifest_hash: &str) -> String {
    let mut contents = String::new();
    writeln!(contents, "// Author: Lukas Bower").ok();
    writeln!(
        contents,
        "// Purpose: Generated cohsh client defaults derived from configs/root_task.toml."
    )
    .ok();
    writeln!(contents, "// @generated by coh-rtc; do not edit.").ok();
    writeln!(contents).ok();
    writeln!(
        contents,
        "pub const MANIFEST_SHA256: &str = \"{}\";",
        manifest_hash
    )
    .ok();
    writeln!(
        contents,
        "pub const SECURE9P_MSIZE: u32 = {};",
        manifest.secure9p.msize
    )
    .ok();
    writeln!(
        contents,
        "pub const SECURE9P_WALK_DEPTH: u8 = {};",
        manifest.secure9p.walk_depth
    )
    .ok();
    writeln!(
        contents,
        "pub const CLIENT_QUEEN_CTL_PATH: &str = {:?};",
        manifest.client_paths.queen_ctl
    )
    .ok();
    writeln!(
        contents,
        "pub const CLIENT_LOG_PATH: &str = {:?};",
        manifest.client_paths.log
    )
    .ok();
    contents
}

fn render_client_doc(manifest: &Manifest, manifest_hash: &str) -> String {
    let mut contents = String::new();
    writeln!(contents, "<!-- Author: Lukas Bower -->").ok();
    writeln!(
        contents,
        "<!-- Purpose: Generated cohsh client snippet consumed by docs/USERLAND_AND_CLI.md. -->"
    )
    .ok();
    writeln!(contents).ok();
    writeln!(contents, "### cohsh client defaults (generated)").ok();
    writeln!(contents, "- `manifest.sha256`: `{}`", manifest_hash).ok();
    writeln!(
        contents,
        "- `secure9p.msize`: `{}`",
        manifest.secure9p.msize
    )
    .ok();
    writeln!(
        contents,
        "- `secure9p.walk_depth`: `{}`",
        manifest.secure9p.walk_depth
    )
    .ok();
    writeln!(
        contents,
        "- `client_paths.queen_ctl`: `{}`",
        manifest.client_paths.queen_ctl
    )
    .ok();
    writeln!(
        contents,
        "- `client_paths.log`: `{}`",
        manifest.client_paths.log
    )
    .ok();
    writeln!(contents).ok();
    writeln!(
        contents,
        "_Generated from `configs/root_task.toml` (sha256: `{}`)._",
        manifest_hash
    )
    .ok();
    contents
}
