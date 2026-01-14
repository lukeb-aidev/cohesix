// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Emit manifest-derived Markdown snippets for documentation.
// Author: Lukas Bower

use crate::ir::Manifest;
use anyhow::{Context, Result};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct DocFragments {
    pub schema_md: String,
    pub namespace_md: String,
    pub sharding_md: String,
    pub ecosystem_md: String,
}

impl DocFragments {
    pub fn from_manifest(manifest: &Manifest, manifest_hash: &str) -> Self {
        let mut schema_md = String::new();
        writeln!(schema_md, "### Root-task manifest schema (generated)").ok();
        writeln!(schema_md, "- `meta.author`: `{}`", manifest.meta.author).ok();
        writeln!(schema_md, "- `meta.purpose`: `{}`", manifest.meta.purpose).ok();
        writeln!(
            schema_md,
            "- `root_task.schema`: `{}`",
            manifest.root_task.schema
        )
        .ok();
        writeln!(schema_md, "- `profile.name`: `{}`", manifest.profile.name).ok();
        writeln!(
            schema_md,
            "- `profile.kernel`: `{}`",
            manifest.profile.kernel
        )
        .ok();
        writeln!(
            schema_md,
            "- `event_pump.tick_ms`: `{}`",
            manifest.event_pump.tick_ms
        )
        .ok();
        writeln!(
            schema_md,
            "- `secure9p.msize`: `{}`",
            manifest.secure9p.msize
        )
        .ok();
        writeln!(
            schema_md,
            "- `secure9p.walk_depth`: `{}`",
            manifest.secure9p.walk_depth
        )
        .ok();
        writeln!(
            schema_md,
            "- `secure9p.tags_per_session`: `{}`",
            manifest.secure9p.tags_per_session
        )
        .ok();
        writeln!(
            schema_md,
            "- `secure9p.batch_frames`: `{}`",
            manifest.secure9p.batch_frames
        )
        .ok();
        writeln!(
            schema_md,
            "- `secure9p.short_write.policy`: `{}`",
            format_short_write_policy(&manifest.secure9p.short_write.policy)
        )
        .ok();
        writeln!(
            schema_md,
            "- `telemetry.ring_bytes_per_worker`: `{}`",
            manifest.telemetry.ring_bytes_per_worker
        )
        .ok();
        writeln!(
            schema_md,
            "- `telemetry.frame_schema`: `{}`",
            format_telemetry_schema(&manifest.telemetry.frame_schema)
        )
        .ok();
        writeln!(
            schema_md,
            "- `telemetry.cursor.retain_on_boot`: `{}`",
            manifest.telemetry.cursor.retain_on_boot
        )
        .ok();
        writeln!(
            schema_md,
            "- `client_policies.cohsh.pool.control_sessions`: `{}`",
            manifest.client_policies.cohsh.pool.control_sessions
        )
        .ok();
        writeln!(
            schema_md,
            "- `client_policies.cohsh.pool.telemetry_sessions`: `{}`",
            manifest.client_policies.cohsh.pool.telemetry_sessions
        )
        .ok();
        writeln!(
            schema_md,
            "- `client_policies.retry.max_attempts`: `{}`",
            manifest.client_policies.retry.max_attempts
        )
        .ok();
        writeln!(
            schema_md,
            "- `client_policies.retry.backoff_ms`: `{}`",
            manifest.client_policies.retry.backoff_ms
        )
        .ok();
        writeln!(
            schema_md,
            "- `client_policies.retry.ceiling_ms`: `{}`",
            manifest.client_policies.retry.ceiling_ms
        )
        .ok();
        writeln!(
            schema_md,
            "- `client_policies.retry.timeout_ms`: `{}`",
            manifest.client_policies.retry.timeout_ms
        )
        .ok();
        writeln!(
            schema_md,
            "- `client_policies.heartbeat.interval_ms`: `{}`",
            manifest.client_policies.heartbeat.interval_ms
        )
        .ok();
        writeln!(
            schema_md,
            "- `cache.kernel_ops`: `{}`",
            manifest.cache.kernel_ops
        )
        .ok();
        writeln!(
            schema_md,
            "- `cache.dma_clean`: `{}`",
            manifest.cache.dma_clean
        )
        .ok();
        writeln!(
            schema_md,
            "- `cache.dma_invalidate`: `{}`",
            manifest.cache.dma_invalidate
        )
        .ok();
        writeln!(
            schema_md,
            "- `cache.unify_instructions`: `{}`",
            manifest.cache.unify_instructions
        )
        .ok();
        writeln!(
            schema_md,
            "- `features.net_console`: `{}`",
            manifest.features.net_console
        )
        .ok();
        writeln!(
            schema_md,
            "- `features.serial_console`: `{}`",
            manifest.features.serial_console
        )
        .ok();
        writeln!(
            schema_md,
            "- `features.std_console`: `{}`",
            manifest.features.std_console
        )
        .ok();
        writeln!(
            schema_md,
            "- `features.std_host_tools`: `{}`",
            manifest.features.std_host_tools
        )
        .ok();
        writeln!(
            schema_md,
            "- `namespaces.role_isolation`: `{}`",
            manifest.namespaces.role_isolation
        )
        .ok();
        writeln!(
            schema_md,
            "- `sharding.enabled`: `{}`",
            manifest.sharding.enabled
        )
        .ok();
        writeln!(
            schema_md,
            "- `sharding.shard_bits`: `{}`",
            manifest.sharding.shard_bits
        )
        .ok();
        writeln!(
            schema_md,
            "- `sharding.legacy_worker_alias`: `{}`",
            manifest.sharding.legacy_worker_alias
        )
        .ok();
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
                writeln!(namespace_md, "- service `{}` → `{}`", mount.service, target).ok();
            }
        }

        let mut sharding_md = String::new();
        writeln!(sharding_md, "### Sharded worker namespace (generated)").ok();
        writeln!(
            sharding_md,
            "- `sharding.enabled`: `{}`",
            manifest.sharding.enabled
        )
        .ok();
        writeln!(
            sharding_md,
            "- `sharding.shard_bits`: `{}`",
            manifest.sharding.shard_bits
        )
        .ok();
        writeln!(
            sharding_md,
            "- `sharding.legacy_worker_alias`: `{}`",
            manifest.sharding.legacy_worker_alias
        )
        .ok();
        let shard_labels = build_shard_labels(manifest);
        if manifest.sharding.enabled {
            let range = render_shard_range(&shard_labels);
            writeln!(
                sharding_md,
                "- shard labels: `{range}` (count: {})",
                shard_labels.len()
            )
            .ok();
            writeln!(
                sharding_md,
                "- canonical worker path: `/shard/<label>/worker/<id>/telemetry`"
            )
            .ok();
            if manifest.sharding.legacy_worker_alias {
                writeln!(sharding_md, "- legacy alias: `/worker/<id>/telemetry`").ok();
            } else {
                writeln!(sharding_md, "- legacy alias: `(disabled)`").ok();
            }
        } else {
            writeln!(
                sharding_md,
                "- sharding disabled; worker path: `/worker/<id>/telemetry`"
            )
            .ok();
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
        if manifest.ecosystem.host.enable {
            writeln!(
                ecosystem_md,
                "- `/host` namespace mounted at `{}` when enabled.",
                manifest.ecosystem.host.mount_at
            )
            .ok();
        }
        writeln!(
            ecosystem_md,
            "- `ecosystem.audit.enable`: `{}`",
            manifest.ecosystem.audit.enable
        )
        .ok();
        writeln!(
            ecosystem_md,
            "- `ecosystem.audit.journal_max_bytes`: `{}`",
            manifest.ecosystem.audit.journal_max_bytes
        )
        .ok();
        writeln!(
            ecosystem_md,
            "- `ecosystem.audit.decisions_max_bytes`: `{}`",
            manifest.ecosystem.audit.decisions_max_bytes
        )
        .ok();
        writeln!(
            ecosystem_md,
            "- `ecosystem.audit.replay_enable`: `{}`",
            manifest.ecosystem.audit.replay_enable
        )
        .ok();
        writeln!(
            ecosystem_md,
            "- `ecosystem.audit.replay_max_entries`: `{}`",
            manifest.ecosystem.audit.replay_max_entries
        )
        .ok();
        writeln!(
            ecosystem_md,
            "- `ecosystem.audit.replay_ctl_max_bytes`: `{}`",
            manifest.ecosystem.audit.replay_ctl_max_bytes
        )
        .ok();
        writeln!(
            ecosystem_md,
            "- `ecosystem.audit.replay_status_max_bytes`: `{}`",
            manifest.ecosystem.audit.replay_status_max_bytes
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
            "- `ecosystem.policy.queue_max_entries`: `{}`",
            manifest.ecosystem.policy.queue_max_entries
        )
        .ok();
        writeln!(
            ecosystem_md,
            "- `ecosystem.policy.queue_max_bytes`: `{}`",
            manifest.ecosystem.policy.queue_max_bytes
        )
        .ok();
        writeln!(
            ecosystem_md,
            "- `ecosystem.policy.ctl_max_bytes`: `{}`",
            manifest.ecosystem.policy.ctl_max_bytes
        )
        .ok();
        writeln!(
            ecosystem_md,
            "- `ecosystem.policy.status_max_bytes`: `{}`",
            manifest.ecosystem.policy.status_max_bytes
        )
        .ok();
        if manifest.ecosystem.policy.rules.is_empty() {
            writeln!(ecosystem_md, "- `ecosystem.policy.rules`: `(none)`").ok();
        } else {
            for rule in &manifest.ecosystem.policy.rules {
                writeln!(
                    ecosystem_md,
                    "- `ecosystem.policy.rules`: `{}` → `{}`",
                    rule.id, rule.target
                )
                .ok();
            }
        }
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
            sharding_md,
            ecosystem_md,
        }
    }
}

pub fn emit_doc_snippet(manifest_hash: &str, docs: &DocFragments, path: &Path) -> Result<()> {
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
    writeln!(contents, "{}", docs.sharding_md.trim_end())?;
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

fn format_short_write_policy(policy: &crate::ir::ShortWritePolicy) -> &'static str {
    match policy {
        crate::ir::ShortWritePolicy::Reject => "reject",
        crate::ir::ShortWritePolicy::Retry => "retry",
    }
}

fn format_telemetry_schema(schema: &crate::ir::TelemetryFrameSchema) -> &'static str {
    match schema {
        crate::ir::TelemetryFrameSchema::LegacyPlaintext => "legacy-plaintext",
        crate::ir::TelemetryFrameSchema::CborV1 => "cbor-v1",
    }
}

fn build_shard_labels(manifest: &Manifest) -> Vec<String> {
    let count = if manifest.sharding.enabled {
        1usize << manifest.sharding.shard_bits
    } else {
        1
    };
    (0..count).map(|idx| format!("{:02x}", idx)).collect()
}

fn render_shard_range(labels: &[String]) -> String {
    match (labels.first(), labels.last()) {
        (Some(first), Some(last)) if first == last => first.clone(),
        (Some(first), Some(last)) => format!("{first}..{last}"),
        _ => "(none)".to_owned(),
    }
}
