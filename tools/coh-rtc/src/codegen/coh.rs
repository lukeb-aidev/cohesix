// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Emit coh host tool policy artefacts derived from the root-task manifest.
// Author: Lukas Bower

use crate::codegen::hash_bytes;
use crate::ir::Manifest;
use anyhow::{Context, Result};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct CohPolicyArtifacts {
    pub policy_toml: PathBuf,
    pub policy_hash: PathBuf,
    pub policy_rust: PathBuf,
    pub policy_doc: PathBuf,
}

pub fn emit_coh_policy(
    manifest: &Manifest,
    manifest_hash: &str,
    policy_out: &Path,
    policy_rust_out: &Path,
    policy_doc_out: &Path,
) -> Result<CohPolicyArtifacts> {
    let policy_toml = render_policy_toml(manifest, manifest_hash);
    fs::write(policy_out, &policy_toml)
        .with_context(|| format!("failed to write coh policy {}", policy_out.display()))?;

    let policy_hash_value = hash_bytes(policy_toml.as_bytes());
    let policy_hash_path = policy_out.with_extension("toml.sha256");
    let hash_contents = format!(
        "# Author: Lukas Bower\n# Purpose: SHA-256 fingerprint for coh_policy.toml.\n{}  {}\n",
        policy_hash_value,
        policy_out
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("coh_policy.toml")
    );
    fs::write(&policy_hash_path, hash_contents).with_context(|| {
        format!(
            "failed to write coh policy hash {}",
            policy_hash_path.display()
        )
    })?;

    let policy_doc = render_policy_doc(manifest, manifest_hash, &policy_hash_value);
    fs::write(policy_doc_out, policy_doc)
        .with_context(|| format!("failed to write coh policy doc {}", policy_doc_out.display()))?;

    let policy_rust = render_policy_rust(manifest, manifest_hash, &policy_hash_value);
    fs::write(policy_rust_out, policy_rust)
        .with_context(|| format!("failed to write coh policy rust {}", policy_rust_out.display()))?;

    Ok(CohPolicyArtifacts {
        policy_toml: policy_out.to_path_buf(),
        policy_hash: policy_hash_path,
        policy_rust: policy_rust_out.to_path_buf(),
        policy_doc: policy_doc_out.to_path_buf(),
    })
}

fn render_policy_toml(manifest: &Manifest, manifest_hash: &str) -> String {
    let mut contents = String::new();
    writeln!(contents, "# Author: Lukas Bower").ok();
    writeln!(
        contents,
        "# Purpose: Generated coh host policy derived from configs/root_task.toml."
    )
    .ok();
    writeln!(contents, "[meta]").ok();
    writeln!(contents, "manifest_sha256 = \"{}\"", manifest_hash).ok();
    writeln!(contents).ok();
    writeln!(contents, "[coh.mount]").ok();
    writeln!(contents, "root = \"{}\"", manifest.client_policies.coh.mount.root).ok();
    writeln!(contents, "allowlist = [").ok();
    for (idx, entry) in manifest.client_policies.coh.mount.allowlist.iter().enumerate() {
        let suffix = if idx + 1 == manifest.client_policies.coh.mount.allowlist.len() {
            ""
        } else {
            ","
        };
        writeln!(contents, "  \"{}\"{}", entry, suffix).ok();
    }
    writeln!(contents, "]").ok();
    writeln!(contents).ok();
    writeln!(contents, "[coh.telemetry]").ok();
    writeln!(
        contents,
        "root = \"{}\"",
        manifest.client_policies.coh.telemetry.root
    )
    .ok();
    writeln!(
        contents,
        "max_devices = {}",
        manifest.client_policies.coh.telemetry.max_devices
    )
    .ok();
    writeln!(
        contents,
        "max_segments_per_device = {}",
        manifest.client_policies.coh.telemetry.max_segments_per_device
    )
    .ok();
    writeln!(
        contents,
        "max_bytes_per_segment = {}",
        manifest.client_policies.coh.telemetry.max_bytes_per_segment
    )
    .ok();
    writeln!(
        contents,
        "max_total_bytes_per_device = {}",
        manifest.client_policies.coh.telemetry.max_total_bytes_per_device
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
    contents
}

fn render_policy_doc(manifest: &Manifest, manifest_hash: &str, policy_hash: &str) -> String {
    let mut contents = String::new();
    writeln!(contents, "<!-- Author: Lukas Bower -->").ok();
    writeln!(
        contents,
        "<!-- Purpose: Generated coh policy snippet consumed by docs/USERLAND_AND_CLI.md. -->"
    )
    .ok();
    writeln!(contents).ok();
    writeln!(contents, "### coh policy defaults (generated)").ok();
    writeln!(contents, "- `manifest.sha256`: `{}`", manifest_hash).ok();
    writeln!(contents, "- `policy.sha256`: `{}`", policy_hash).ok();
    writeln!(
        contents,
        "- `coh.mount.root`: `{}`",
        manifest.client_policies.coh.mount.root
    )
    .ok();
    writeln!(
        contents,
        "- `coh.mount.allowlist`: `{}`",
        manifest.client_policies.coh.mount.allowlist.join(", ")
    )
    .ok();
    writeln!(
        contents,
        "- `coh.telemetry.root`: `{}`",
        manifest.client_policies.coh.telemetry.root
    )
    .ok();
    writeln!(
        contents,
        "- `coh.telemetry.max_devices`: `{}`",
        manifest.client_policies.coh.telemetry.max_devices
    )
    .ok();
    writeln!(
        contents,
        "- `coh.telemetry.max_segments_per_device`: `{}`",
        manifest.client_policies.coh.telemetry.max_segments_per_device
    )
    .ok();
    writeln!(
        contents,
        "- `coh.telemetry.max_bytes_per_segment`: `{}`",
        manifest.client_policies.coh.telemetry.max_bytes_per_segment
    )
    .ok();
    writeln!(
        contents,
        "- `coh.telemetry.max_total_bytes_per_device`: `{}`",
        manifest.client_policies.coh.telemetry.max_total_bytes_per_device
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
    contents
}

fn render_policy_rust(manifest: &Manifest, manifest_hash: &str, policy_hash: &str) -> String {
    let mut contents = String::new();
    writeln!(
        contents,
        "// Purpose: Generated coh policy defaults derived from configs/root_task.toml."
    )
    .ok();
    writeln!(contents, "// @generated by coh-rtc; do not edit.").ok();
    writeln!(contents).ok();
    writeln!(contents, "pub const MANIFEST_SHA256: &str = \"{}\";", manifest_hash).ok();
    writeln!(contents, "pub const POLICY_SHA256: &str = \"{}\";", policy_hash).ok();
    writeln!(contents).ok();
    writeln!(
        contents,
        "pub const COH_MOUNT_ROOT: &str = \"{}\";",
        manifest.client_policies.coh.mount.root
    )
    .ok();
    writeln!(contents, "pub const COH_MOUNT_ALLOWLIST: &[&str] = &[").ok();
    for entry in &manifest.client_policies.coh.mount.allowlist {
        writeln!(contents, "    \"{}\",", entry).ok();
    }
    writeln!(contents, "];").ok();
    writeln!(contents).ok();
    writeln!(
        contents,
        "pub const COH_TELEMETRY_ROOT: &str = \"{}\";",
        manifest.client_policies.coh.telemetry.root
    )
    .ok();
    writeln!(
        contents,
        "pub const COH_TELEMETRY_MAX_DEVICES: u32 = {};",
        manifest.client_policies.coh.telemetry.max_devices
    )
    .ok();
    writeln!(
        contents,
        "pub const COH_TELEMETRY_MAX_SEGMENTS_PER_DEVICE: u32 = {};",
        manifest.client_policies.coh.telemetry.max_segments_per_device
    )
    .ok();
    writeln!(
        contents,
        "pub const COH_TELEMETRY_MAX_BYTES_PER_SEGMENT: u32 = {};",
        manifest.client_policies.coh.telemetry.max_bytes_per_segment
    )
    .ok();
    writeln!(
        contents,
        "pub const COH_TELEMETRY_MAX_TOTAL_BYTES_PER_DEVICE: u32 = {};",
        manifest.client_policies.coh.telemetry.max_total_bytes_per_device
    )
    .ok();
    writeln!(contents).ok();
    writeln!(
        contents,
        "pub const COH_RETRY_MAX_ATTEMPTS: u8 = {};",
        manifest.client_policies.retry.max_attempts
    )
    .ok();
    writeln!(
        contents,
        "pub const COH_RETRY_BACKOFF_MS: u64 = {};",
        manifest.client_policies.retry.backoff_ms
    )
    .ok();
    writeln!(
        contents,
        "pub const COH_RETRY_CEILING_MS: u64 = {};",
        manifest.client_policies.retry.ceiling_ms
    )
    .ok();
    writeln!(
        contents,
        "pub const COH_RETRY_TIMEOUT_MS: u64 = {};",
        manifest.client_policies.retry.timeout_ms
    )
    .ok();
    contents
}
