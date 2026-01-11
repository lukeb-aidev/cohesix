// Author: Lukas Bower
// Purpose: Emit manifest-derived Markdown snippets for documentation.

use crate::ir::Manifest;
use anyhow::{Context, Result};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct DocFragments {
    pub schema_md: String,
    pub namespace_md: String,
    pub ecosystem_md: String,
}

impl DocFragments {
    pub fn from_manifest(manifest: &Manifest, manifest_hash: &str) -> Self {
        let mut schema_md = String::new();
        writeln!(schema_md, "### Root-task manifest schema (generated)").ok();
        writeln!(
            schema_md,
            "- `meta.author`: `{}`",
            manifest.meta.author
        )
        .ok();
        writeln!(
            schema_md,
            "- `meta.purpose`: `{}`",
            manifest.meta.purpose
        )
        .ok();
        writeln!(schema_md, "- `root_task.schema`: `{}`", manifest.root_task.schema).ok();
        writeln!(schema_md, "- `profile.name`: `{}`", manifest.profile.name).ok();
        writeln!(schema_md, "- `profile.kernel`: `{}`", manifest.profile.kernel).ok();
        writeln!(schema_md, "- `event_pump.tick_ms`: `{}`", manifest.event_pump.tick_ms).ok();
        writeln!(schema_md, "- `secure9p.msize`: `{}`", manifest.secure9p.msize).ok();
        writeln!(schema_md, "- `secure9p.walk_depth`: `{}`", manifest.secure9p.walk_depth).ok();
        writeln!(schema_md, "- `features.net_console`: `{}`", manifest.features.net_console).ok();
        writeln!(schema_md, "- `features.serial_console`: `{}`", manifest.features.serial_console).ok();
        writeln!(schema_md, "- `features.std_console`: `{}`", manifest.features.std_console).ok();
        writeln!(schema_md, "- `features.std_host_tools`: `{}`", manifest.features.std_host_tools).ok();
        writeln!(schema_md, "- `namespaces.role_isolation`: `{}`", manifest.namespaces.role_isolation).ok();
        writeln!(schema_md, "- `tickets`: {} entries", manifest.tickets.len()).ok();
        writeln!(schema_md, "- `manifest.sha256`: `{}`", manifest_hash).ok();

        let mut namespace_md = String::new();
        writeln!(namespace_md, "### Namespace mounts (generated)").ok();
        if manifest.namespaces.mounts.is_empty() {
            writeln!(namespace_md, "- (none)").ok();
        } else {
            for mount in &manifest.namespaces.mounts {
                let target = if mount.target.is_empty() {
                    "/".to_owned()
                } else {
                    format!("/{}", mount.target.join("/"))
                };
                writeln!(
                    namespace_md,
                    "- service `{}` → `{}`",
                    mount.service,
                    target
                )
                .ok();
            }
        }

        let mut ecosystem_md = String::new();
        writeln!(ecosystem_md, "### Ecosystem section (generated)").ok();
        writeln!(
            ecosystem_md,
            "- `ecosystem.host.enable`: `{}`",
            manifest.ecosystem.host.enable
        )
        .ok();
        writeln!(
            ecosystem_md,
            "- `ecosystem.host.mount_at`: `{}`",
            manifest.ecosystem.host.mount_at
        )
        .ok();
        if manifest.ecosystem.host.providers.is_empty() {
            writeln!(ecosystem_md, "- `ecosystem.host.providers`: `(none)`").ok();
        } else {
            let providers = manifest
                .ecosystem
                .host
                .providers
                .iter()
                .map(|provider| format!("`{}`", format_provider(provider)))
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(ecosystem_md, "- `ecosystem.host.providers`: {providers}").ok();
        }
        writeln!(
            ecosystem_md,
            "- `ecosystem.audit.enable`: `{}`",
            manifest.ecosystem.audit.enable
        )
        .ok();
        writeln!(
            ecosystem_md,
            "- `ecosystem.policy.enable`: `{}`",
            manifest.ecosystem.policy.enable
        )
        .ok();
        writeln!(
            ecosystem_md,
            "- `ecosystem.models.enable`: `{}`",
            manifest.ecosystem.models.enable
        )
        .ok();
        writeln!(ecosystem_md, "- Nodes appear only when enabled.").ok();

        Self {
            schema_md,
            namespace_md,
            ecosystem_md,
        }
    }
}

pub fn emit_doc_snippet(
    manifest_hash: &str,
    docs: &DocFragments,
    path: &Path,
) -> Result<()> {
    let mut contents = String::new();
    writeln!(contents, "<!-- Author: Lukas Bower -->")?;
    writeln!(
        contents,
        "<!-- Purpose: Generated manifest snippet consumed by docs/ARCHITECTURE.md. -->"
    )?;
    writeln!(contents)?;
    writeln!(contents, "{}", docs.schema_md.trim_end())?;
    writeln!(contents)?;
    writeln!(contents, "{}", docs.namespace_md.trim_end())?;
    writeln!(contents)?;
    writeln!(contents, "{}", docs.ecosystem_md.trim_end())?;
    writeln!(contents)?;
    writeln!(
        contents,
        "_Generated from `configs/root_task.toml` (sha256: `{}`)._",
        manifest_hash
    )?;
    fs::write(path, contents)
        .with_context(|| format!("failed to write docs snippet {}", path.display()))?;
    Ok(())
}

fn format_provider(provider: &crate::ir::HostProvider) -> &'static str {
    match provider {
        crate::ir::HostProvider::Systemd => "systemd",
        crate::ir::HostProvider::K8s => "k8s",
        crate::ir::HostProvider::Nvidia => "nvidia",
        crate::ir::HostProvider::Jetson => "jetson",
        crate::ir::HostProvider::Net => "net",
    }
}
