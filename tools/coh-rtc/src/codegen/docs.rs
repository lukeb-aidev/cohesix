// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Emit manifest-derived Markdown snippets for documentation.
// Author: Lukas Bower

use crate::codegen::{cas, hash_bytes};
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
    pub sidecar_md: String,
    pub ecosystem_md: String,
    pub observability_interfaces_md: String,
    pub observability_security_md: String,
    pub cas_interfaces_md: String,
    pub cas_security_md: String,
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
        writeln!(schema_md, "- `cas.enable`: `{}`", manifest.cas.enable).ok();
        writeln!(
            schema_md,
            "- `cas.store.chunk_bytes`: `{}`",
            manifest.cas.store.chunk_bytes
        )
        .ok();
        writeln!(
            schema_md,
            "- `cas.delta.enable`: `{}`",
            manifest.cas.delta.enable
        )
        .ok();
        if let Some(signing) = &manifest.cas.signing {
            writeln!(
                schema_md,
                "- `cas.signing.required`: `{}`",
                signing.required
            )
            .ok();
            if let Some(path) = signing.key_path.as_deref() {
                writeln!(schema_md, "- `cas.signing.key_path`: `{}`", path).ok();
            } else {
                writeln!(schema_md, "- `cas.signing.key_path`: `(none)`").ok();
            }
        } else {
            writeln!(schema_md, "- `cas.signing.required`: `(none)`").ok();
            writeln!(schema_md, "- `cas.signing.key_path`: `(none)`").ok();
        }
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
            "- `observability.proc_9p.sessions`: `{}`",
            manifest.observability.proc_9p.sessions
        )
        .ok();
        writeln!(
            schema_md,
            "- `observability.proc_9p.outstanding`: `{}`",
            manifest.observability.proc_9p.outstanding
        )
        .ok();
        writeln!(
            schema_md,
            "- `observability.proc_9p.short_writes`: `{}`",
            manifest.observability.proc_9p.short_writes
        )
        .ok();
        writeln!(
            schema_md,
            "- `observability.proc_9p.sessions_bytes`: `{}`",
            manifest.observability.proc_9p.sessions_bytes
        )
        .ok();
        writeln!(
            schema_md,
            "- `observability.proc_9p.outstanding_bytes`: `{}`",
            manifest.observability.proc_9p.outstanding_bytes
        )
        .ok();
        writeln!(
            schema_md,
            "- `observability.proc_9p.short_writes_bytes`: `{}`",
            manifest.observability.proc_9p.short_writes_bytes
        )
        .ok();
        writeln!(
            schema_md,
            "- `observability.proc_ingest.p50_ms`: `{}`",
            manifest.observability.proc_ingest.p50_ms
        )
        .ok();
        writeln!(
            schema_md,
            "- `observability.proc_ingest.p95_ms`: `{}`",
            manifest.observability.proc_ingest.p95_ms
        )
        .ok();
        writeln!(
            schema_md,
            "- `observability.proc_ingest.backpressure`: `{}`",
            manifest.observability.proc_ingest.backpressure
        )
        .ok();
        writeln!(
            schema_md,
            "- `observability.proc_ingest.dropped`: `{}`",
            manifest.observability.proc_ingest.dropped
        )
        .ok();
        writeln!(
            schema_md,
            "- `observability.proc_ingest.queued`: `{}`",
            manifest.observability.proc_ingest.queued
        )
        .ok();
        writeln!(
            schema_md,
            "- `observability.proc_ingest.watch`: `{}`",
            manifest.observability.proc_ingest.watch
        )
        .ok();
        writeln!(
            schema_md,
            "- `observability.proc_ingest.p50_ms_bytes`: `{}`",
            manifest.observability.proc_ingest.p50_ms_bytes
        )
        .ok();
        writeln!(
            schema_md,
            "- `observability.proc_ingest.p95_ms_bytes`: `{}`",
            manifest.observability.proc_ingest.p95_ms_bytes
        )
        .ok();
        writeln!(
            schema_md,
            "- `observability.proc_ingest.backpressure_bytes`: `{}`",
            manifest.observability.proc_ingest.backpressure_bytes
        )
        .ok();
        writeln!(
            schema_md,
            "- `observability.proc_ingest.dropped_bytes`: `{}`",
            manifest.observability.proc_ingest.dropped_bytes
        )
        .ok();
        writeln!(
            schema_md,
            "- `observability.proc_ingest.queued_bytes`: `{}`",
            manifest.observability.proc_ingest.queued_bytes
        )
        .ok();
        writeln!(
            schema_md,
            "- `observability.proc_ingest.watch_max_entries`: `{}`",
            manifest.observability.proc_ingest.watch_max_entries
        )
        .ok();
        writeln!(
            schema_md,
            "- `observability.proc_ingest.watch_line_bytes`: `{}`",
            manifest.observability.proc_ingest.watch_line_bytes
        )
        .ok();
        writeln!(
            schema_md,
            "- `observability.proc_ingest.watch_min_interval_ms`: `{}`",
            manifest.observability.proc_ingest.watch_min_interval_ms
        )
        .ok();
        writeln!(
            schema_md,
            "- `observability.proc_ingest.latency_samples`: `{}`",
            manifest.observability.proc_ingest.latency_samples
        )
        .ok();
        writeln!(
            schema_md,
            "- `observability.proc_ingest.latency_tolerance_ms`: `{}`",
            manifest.observability.proc_ingest.latency_tolerance_ms
        )
        .ok();
        writeln!(
            schema_md,
            "- `observability.proc_ingest.counter_tolerance`: `{}`",
            manifest.observability.proc_ingest.counter_tolerance
        )
        .ok();
        writeln!(
            schema_md,
            "- `ui_providers.proc_9p.sessions`: `{}`",
            manifest.ui_providers.proc_9p.sessions
        )
        .ok();
        writeln!(
            schema_md,
            "- `ui_providers.proc_9p.outstanding`: `{}`",
            manifest.ui_providers.proc_9p.outstanding
        )
        .ok();
        writeln!(
            schema_md,
            "- `ui_providers.proc_9p.short_writes`: `{}`",
            manifest.ui_providers.proc_9p.short_writes
        )
        .ok();
        writeln!(
            schema_md,
            "- `ui_providers.proc_ingest.p50_ms`: `{}`",
            manifest.ui_providers.proc_ingest.p50_ms
        )
        .ok();
        writeln!(
            schema_md,
            "- `ui_providers.proc_ingest.p95_ms`: `{}`",
            manifest.ui_providers.proc_ingest.p95_ms
        )
        .ok();
        writeln!(
            schema_md,
            "- `ui_providers.proc_ingest.backpressure`: `{}`",
            manifest.ui_providers.proc_ingest.backpressure
        )
        .ok();
        writeln!(
            schema_md,
            "- `ui_providers.policy_preflight.req`: `{}`",
            manifest.ui_providers.policy_preflight.req
        )
        .ok();
        writeln!(
            schema_md,
            "- `ui_providers.policy_preflight.diff`: `{}`",
            manifest.ui_providers.policy_preflight.diff
        )
        .ok();
        writeln!(
            schema_md,
            "- `ui_providers.updates.manifest`: `{}`",
            manifest.ui_providers.updates.manifest
        )
        .ok();
        writeln!(
            schema_md,
            "- `ui_providers.updates.status`: `{}`",
            manifest.ui_providers.updates.status
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
            "- `client_paths.queen_ctl`: `{}`",
            manifest.client_paths.queen_ctl
        )
        .ok();
        writeln!(
            schema_md,
            "- `client_paths.log`: `{}`",
            manifest.client_paths.log
        )
        .ok();
        writeln!(
            schema_md,
            "- `swarmui.ticket_scope`: `{}`",
            manifest.swarmui.ticket_scope.as_str()
        )
        .ok();
        writeln!(
            schema_md,
            "- `swarmui.cache.enabled`: `{}`",
            manifest.swarmui.cache.enabled
        )
        .ok();
        writeln!(
            schema_md,
            "- `swarmui.cache.max_bytes`: `{}`",
            manifest.swarmui.cache.max_bytes
        )
        .ok();
        writeln!(
            schema_md,
            "- `swarmui.cache.ttl_s`: `{}`",
            manifest.swarmui.cache.ttl_s
        )
        .ok();
        writeln!(
            schema_md,
            "- `swarmui.hive.frame_cap_fps`: `{}`",
            manifest.swarmui.hive.frame_cap_fps
        )
        .ok();
        writeln!(
            schema_md,
            "- `swarmui.hive.step_ms`: `{}`",
            manifest.swarmui.hive.step_ms
        )
        .ok();
        writeln!(
            schema_md,
            "- `swarmui.hive.lod_zoom_out`: `{}`",
            manifest.swarmui.hive.lod_zoom_out
        )
        .ok();
        writeln!(
            schema_md,
            "- `swarmui.hive.lod_zoom_in`: `{}`",
            manifest.swarmui.hive.lod_zoom_in
        )
        .ok();
        writeln!(
            schema_md,
            "- `swarmui.hive.lod_event_budget`: `{}`",
            manifest.swarmui.hive.lod_event_budget
        )
        .ok();
        writeln!(
            schema_md,
            "- `swarmui.hive.snapshot_max_events`: `{}`",
            manifest.swarmui.hive.snapshot_max_events
        )
        .ok();
        writeln!(
            schema_md,
            "- `swarmui.paths.telemetry_root`: `{}`",
            manifest.swarmui.paths.telemetry_root
        )
        .ok();
        writeln!(
            schema_md,
            "- `swarmui.paths.proc_ingest_root`: `{}`",
            manifest.swarmui.paths.proc_ingest_root
        )
        .ok();
        writeln!(
            schema_md,
            "- `swarmui.paths.worker_root`: `{}`",
            manifest.swarmui.paths.worker_root
        )
        .ok();
        writeln!(
            schema_md,
            "- `swarmui.paths.namespace_roots`: `{}`",
            manifest.swarmui.paths.namespace_roots.join(", ")
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

        let mut sidecar_md = String::new();
        writeln!(sidecar_md, "### Sidecars section (generated)").ok();
        let mut bus_used = sidecar_reserved_names();
        render_sidecar_bus_section(
            "modbus",
            &manifest.sidecars.modbus,
            &mut bus_used,
            &mut sidecar_md,
        );
        render_sidecar_bus_section(
            "dnp3",
            &manifest.sidecars.dnp3,
            &mut bus_used,
            &mut sidecar_md,
        );
        let mut lora_used = sidecar_reserved_names();
        render_sidecar_lora_section(&manifest.sidecars.lora, &mut lora_used, &mut sidecar_md);

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

        let proc_9p = &manifest.observability.proc_9p;
        let proc_ingest = &manifest.observability.proc_ingest;
        let proc_9p_enabled = proc_9p.sessions || proc_9p.outstanding || proc_9p.short_writes;
        let proc_ingest_enabled = proc_ingest.p50_ms
            || proc_ingest.p95_ms
            || proc_ingest.backpressure
            || proc_ingest.dropped
            || proc_ingest.queued
            || proc_ingest.watch;

        let mut observability_interfaces_md = String::new();
        writeln!(
            observability_interfaces_md,
            "### /proc observability nodes (generated)"
        )
        .ok();
        if !proc_9p_enabled && !proc_ingest_enabled {
            writeln!(observability_interfaces_md, "- (disabled)").ok();
        } else {
            if proc_9p.sessions {
                writeln!(
                    observability_interfaces_md,
                    "- `/proc/9p/sessions` (read-only, max {} bytes): `sessions total=<u64> worker=<u64> shard_bits=<u8> shard_count=<u16>` plus `shard <hex> <count>` lines.",
                    proc_9p.sessions_bytes
                )
                .ok();
            }
            if proc_9p.outstanding {
                writeln!(
                    observability_interfaces_md,
                    "- `/proc/9p/outstanding` (read-only, max {} bytes): `outstanding current=<u64> limit=<u64>`.",
                    proc_9p.outstanding_bytes
                )
                .ok();
            }
            if proc_9p.short_writes {
                writeln!(
                    observability_interfaces_md,
                    "- `/proc/9p/short_writes` (read-only, max {} bytes): `short_writes total=<u64> retries=<u64>`.",
                    proc_9p.short_writes_bytes
                )
                .ok();
            }
            if proc_ingest.p50_ms {
                writeln!(
                    observability_interfaces_md,
                    "- `/proc/ingest/p50_ms` (read-only, max {} bytes): `p50_ms=<u32>` (milliseconds).",
                    proc_ingest.p50_ms_bytes
                )
                .ok();
            }
            if proc_ingest.p95_ms {
                writeln!(
                    observability_interfaces_md,
                    "- `/proc/ingest/p95_ms` (read-only, max {} bytes): `p95_ms=<u32>` (milliseconds).",
                    proc_ingest.p95_ms_bytes
                )
                .ok();
            }
            if proc_ingest.backpressure {
                writeln!(
                    observability_interfaces_md,
                    "- `/proc/ingest/backpressure` (read-only, max {} bytes): `backpressure=<u64>`.",
                    proc_ingest.backpressure_bytes
                )
                .ok();
            }
            if proc_ingest.dropped {
                writeln!(
                    observability_interfaces_md,
                    "- `/proc/ingest/dropped` (read-only, max {} bytes): `dropped=<u64>`.",
                    proc_ingest.dropped_bytes
                )
                .ok();
            }
            if proc_ingest.queued {
                writeln!(
                    observability_interfaces_md,
                    "- `/proc/ingest/queued` (read-only, max {} bytes): `queued=<u32>`.",
                    proc_ingest.queued_bytes
                )
                .ok();
            }
            if proc_ingest.watch {
                writeln!(
                    observability_interfaces_md,
                    "- `/proc/ingest/watch` (append-only, max_entries={}, line_bytes={}, min_interval_ms={}): `watch ts_ms=<u64> p50_ms=<u32> p95_ms=<u32> queued=<u32> backpressure=<u64> dropped=<u64>`.",
                    proc_ingest.watch_max_entries,
                    proc_ingest.watch_line_bytes,
                    proc_ingest.watch_min_interval_ms
                )
                .ok();
            }
        }
        let interfaces_hash = hash_bytes(observability_interfaces_md.as_bytes());
        writeln!(
            observability_interfaces_md,
            "\n_Generated by coh-rtc (sha256: `{interfaces_hash}`)._"
        )
        .ok();

        let mut observability_security_md = String::new();
        writeln!(
            observability_security_md,
            "### Observability tolerances (generated)"
        )
        .ok();
        if !proc_ingest_enabled {
            writeln!(observability_security_md, "- (disabled)").ok();
        } else {
            writeln!(
                observability_security_md,
                "- `observability.proc_ingest.latency_samples`: `{}`",
                proc_ingest.latency_samples
            )
            .ok();
            writeln!(
                observability_security_md,
                "- `observability.proc_ingest.latency_tolerance_ms`: `{}`",
                proc_ingest.latency_tolerance_ms
            )
            .ok();
            writeln!(
                observability_security_md,
                "- `observability.proc_ingest.counter_tolerance`: `{}`",
                proc_ingest.counter_tolerance
            )
            .ok();
            if proc_ingest.watch {
                writeln!(
                    observability_security_md,
                    "- `observability.proc_ingest.watch_min_interval_ms`: `{}`",
                    proc_ingest.watch_min_interval_ms
                )
                .ok();
            }
        }
        let security_hash = hash_bytes(observability_security_md.as_bytes());
        writeln!(
            observability_security_md,
            "\n_Generated by coh-rtc (sha256: `{security_hash}`)._"
        )
        .ok();

        let cas_template = cas::build_cas_template(manifest);
        let mut cas_interfaces_md = String::new();
        writeln!(cas_interfaces_md, "### CAS update surfaces (generated)").ok();
        if !manifest.cas.enable {
            writeln!(cas_interfaces_md, "- (disabled)").ok();
        } else {
            writeln!(
                cas_interfaces_md,
                "- `cas.store.chunk_bytes`: `{}`",
                manifest.cas.store.chunk_bytes
            )
            .ok();
            writeln!(
                cas_interfaces_md,
                "- `cas.delta.enable`: `{}`",
                manifest.cas.delta.enable
            )
            .ok();
            let signing_required = manifest
                .cas
                .signing
                .as_ref()
                .map(|signing| signing.required)
                .unwrap_or(false);
            writeln!(
                cas_interfaces_md,
                "- `cas.signing.required`: `{}`",
                signing_required
            )
            .ok();
            writeln!(
                cas_interfaces_md,
                "- Base update layout: `/updates/<epoch>/manifest.cbor`, `/updates/<epoch>/chunks/<sha256>`."
            )
            .ok();
            writeln!(
                cas_interfaces_md,
                "- Model registry layout: `/models/<sha256>/weights`, `/models/<sha256>/schema`, `/models/<sha256>/signature`."
            )
            .ok();
            if manifest.cas.delta.enable {
                writeln!(
                    cas_interfaces_md,
                    "- Delta manifests supply `delta.base_epoch` and `delta.base_sha256`, referencing a non-delta base."
                )
                .ok();
            }
            writeln!(
                cas_interfaces_md,
                "- Payloads are appended as raw bytes or `b64:`-prefixed base64."
            )
            .ok();
            writeln!(cas_interfaces_md, "- CAS manifest template:").ok();
            writeln!(cas_interfaces_md, "```json").ok();
            writeln!(cas_interfaces_md, "{}", cas_template.json).ok();
            writeln!(cas_interfaces_md, "```").ok();
        }
        let cas_interfaces_hash = hash_bytes(cas_interfaces_md.as_bytes());
        writeln!(
            cas_interfaces_md,
            "\n_Generated by coh-rtc (sha256: `{cas_interfaces_hash}`)._"
        )
        .ok();

        let mut cas_security_md = String::new();
        writeln!(
            cas_security_md,
            "### CAS integrity stance (generated)"
        )
        .ok();
        if !manifest.cas.enable {
            writeln!(cas_security_md, "- (disabled)").ok();
        } else {
            let signing_required = manifest
                .cas
                .signing
                .as_ref()
                .map(|signing| signing.required)
                .unwrap_or(false);
            writeln!(
                cas_security_md,
                "- `cas.signing.required`: `{}`",
                signing_required
            )
            .ok();
            writeln!(
                cas_security_md,
                "- Hash mismatches are rejected, quarantined, and audited without side effects."
            )
            .ok();
            writeln!(
                cas_security_md,
                "- Signature failures emit deterministic ERR plus audit entries."
            )
            .ok();
            writeln!(
                cas_security_md,
                "- `/models` exposure remains gated by `ecosystem.models.enable`."
            )
            .ok();
        }
        let cas_security_hash = hash_bytes(cas_security_md.as_bytes());
        writeln!(
            cas_security_md,
            "\n_Generated by coh-rtc (sha256: `{cas_security_hash}`)._"
        )
        .ok();

        Self {
            schema_md,
            namespace_md,
            sharding_md,
            sidecar_md,
            ecosystem_md,
            observability_interfaces_md,
            observability_security_md,
            cas_interfaces_md,
            cas_security_md,
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
    writeln!(contents, "{}", docs.sidecar_md.trim_end())?;
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

pub fn emit_observability_interfaces_snippet(docs: &DocFragments, path: &Path) -> Result<()> {
    let mut contents = String::new();
    writeln!(contents, "<!-- Author: Lukas Bower -->")?;
    writeln!(
        contents,
        "<!-- Purpose: Generated observability snippet consumed by docs/INTERFACES.md. -->"
    )?;
    writeln!(contents)?;
    writeln!(contents, "{}", docs.observability_interfaces_md.trim_end())?;
    fs::write(path, contents).with_context(|| {
        format!(
            "failed to write observability interfaces snippet {}",
            path.display()
        )
    })?;
    Ok(())
}

pub fn emit_observability_security_snippet(docs: &DocFragments, path: &Path) -> Result<()> {
    let mut contents = String::new();
    writeln!(contents, "<!-- Author: Lukas Bower -->")?;
    writeln!(
        contents,
        "<!-- Purpose: Generated observability snippet consumed by docs/SECURITY.md. -->"
    )?;
    writeln!(contents)?;
    writeln!(contents, "{}", docs.observability_security_md.trim_end())?;
    fs::write(path, contents).with_context(|| {
        format!(
            "failed to write observability security snippet {}",
            path.display()
        )
    })?;
    Ok(())
}

pub fn emit_cas_interfaces_snippet(docs: &DocFragments, path: &Path) -> Result<()> {
    let mut contents = String::new();
    writeln!(contents, "<!-- Author: Lukas Bower -->")?;
    writeln!(
        contents,
        "<!-- Purpose: Generated CAS snippet consumed by docs/INTERFACES.md. -->"
    )?;
    writeln!(contents)?;
    writeln!(contents, "{}", docs.cas_interfaces_md.trim_end())?;
    fs::write(path, contents).with_context(|| {
        format!(
            "failed to write cas interfaces snippet {}",
            path.display()
        )
    })?;
    Ok(())
}

pub fn emit_cas_security_snippet(docs: &DocFragments, path: &Path) -> Result<()> {
    let mut contents = String::new();
    writeln!(contents, "<!-- Author: Lukas Bower -->")?;
    writeln!(
        contents,
        "<!-- Purpose: Generated CAS snippet consumed by docs/SECURITY.md. -->"
    )?;
    writeln!(contents)?;
    writeln!(contents, "{}", docs.cas_security_md.trim_end())?;
    fs::write(path, contents).with_context(|| {
        format!(
            "failed to write cas security snippet {}",
            path.display()
        )
    })?;
    Ok(())
}

fn render_sidecar_bus_section(
    label: &str,
    config: &crate::ir::SidecarBusConfig,
    used: &mut std::collections::BTreeSet<String>,
    output: &mut String,
) {
    writeln!(
        output,
        "- `sidecars.{label}.enable`: `{}`",
        config.enable
    )
    .ok();
    writeln!(
        output,
        "- `sidecars.{label}.mount_at`: `{}`",
        config.mount_at
    )
    .ok();
    if config.adapters.is_empty() {
        writeln!(output, "- `sidecars.{label}.adapters`: `(none)`").ok();
    } else {
        for adapter in &config.adapters {
            let resolved = resolve_sidecar_mount(label, &adapter.id, &adapter.mount, used);
            writeln!(
                output,
                "- `sidecars.{label}.adapters`: id=`{}` mount=`{}` scope=`{}` link=`{}` baud=`{}` spool.max_entries=`{}` spool.max_bytes=`{}`",
                adapter.id,
                resolved,
                adapter.scope,
                format_sidecar_link(adapter.link),
                adapter.baud,
                adapter.spool.max_entries,
                adapter.spool.max_bytes
            )
            .ok();
        }
    }
}

fn render_sidecar_lora_section(
    config: &crate::ir::SidecarLoraConfig,
    used: &mut std::collections::BTreeSet<String>,
    output: &mut String,
) {
    writeln!(output, "- `sidecars.lora.enable`: `{}`", config.enable).ok();
    writeln!(
        output,
        "- `sidecars.lora.mount_at`: `{}`",
        config.mount_at
    )
    .ok();
    if config.adapters.is_empty() {
        writeln!(output, "- `sidecars.lora.adapters`: `(none)`").ok();
    } else {
        for adapter in &config.adapters {
            let resolved = resolve_sidecar_mount("lora", &adapter.id, &adapter.mount, used);
            writeln!(
                output,
                "- `sidecars.lora.adapters`: id=`{}` mount=`{}` scope=`{}` region=`{}` duty_cycle_percent=`{}` window_ms=`{}` max_payload_bytes=`{}` tamper_log_max_entries=`{}`",
                adapter.id,
                resolved,
                adapter.scope,
                adapter.region,
                adapter.duty_cycle_percent,
                adapter.window_ms,
                adapter.max_payload_bytes,
                adapter.tamper_log_max_entries
            )
            .ok();
        }
    }
}

fn format_sidecar_link(link: crate::ir::SidecarLink) -> &'static str {
    match link {
        crate::ir::SidecarLink::Serial => "serial",
        crate::ir::SidecarLink::Tcp => "tcp",
    }
}

fn resolve_sidecar_mount(
    kind: &str,
    adapter_id: &str,
    mount: &str,
    used: &mut std::collections::BTreeSet<String>,
) -> String {
    let mut label = mount.to_owned();
    if used.contains(&label) {
        label = hashed_sidecar_label(kind, adapter_id, mount);
    }
    used.insert(label.clone());
    label
}

fn hashed_sidecar_label(kind: &str, adapter_id: &str, mount: &str) -> String {
    let seed = format!("{kind}:{adapter_id}:{mount}");
    let digest = hash_bytes(seed.as_bytes());
    let prefix = digest.get(0..8).unwrap_or("00000000");
    format!("{prefix}-{mount}")
}

fn sidecar_reserved_names() -> std::collections::BTreeSet<String> {
    [
        "proc", "log", "queen", "worker", "shard", "gpu", "host", "policy", "actions", "audit",
        "replay", "updates", "models", "trace", "kmesg", "bus", "lora",
    ]
    .iter()
    .map(|entry| entry.to_string())
    .collect()
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
