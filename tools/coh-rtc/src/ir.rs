// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Define and validate the root-task manifest IR.
// Author: Lukas Bower

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const SCHEMA_VERSION: &str = "1.5";
const MAX_WALK_DEPTH: usize = 8;
const MAX_MSIZE: u32 = 8192;
const MAX_SHARD_BITS: u8 = 8;
const SHARDED_WORKER_PATH_DEPTH: usize = 5;
const LEGACY_WORKER_PATH_DEPTH: usize = 3;
const EVENT_PUMP_TELEMETRY_BUDGET_BYTES: u32 = 32 * 1024;
const EVENT_PUMP_MAX_TELEMETRY_WORKERS: u32 = 8;
const EVENT_PUMP_CAS_BUDGET_BYTES: u32 = 32 * 1024;
const EVENT_PUMP_SIDECAR_BUDGET_BYTES: u32 = 16 * 1024;
const CAS_MAX_CHUNKS: u32 = 8;
const MAX_POLICY_QUEUE_ENTRIES: u16 = 64;
const MAX_POLICY_RULE_ID_LEN: usize = 64;
const MAX_REPLAY_ENTRIES: u16 = 256;
const MAX_OBSERVE_LATENCY_SAMPLES: u16 = 64;
const MAX_OBSERVE_WATCH_ENTRIES: u16 = 64;
const MAX_SIDECAR_SCOPE_LEN: usize = 64;
const MAX_SIDECAR_ID_LEN: usize = 64;
const MAX_SIDECAR_MOUNT_LEN: usize = 64;
const MAX_SPOOL_ENTRIES: u16 = 256;
const LORA_TAMPER_ENTRY_BYTES: u32 = 128;
const MAX_U64_DIGITS: usize = 20;
const MAX_U32_DIGITS: usize = 10;
const MAX_U8_DIGITS: usize = 3;
const SHARD_LABEL_BYTES: usize = 2;
const SHARD_COUNT_DIGITS: usize = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    #[serde(default)]
    pub meta: ManifestMeta,
    pub root_task: RootTaskSection,
    pub profile: Profile,
    pub event_pump: EventPump,
    pub secure9p: Secure9pLimits,
    pub features: FeatureToggles,
    #[serde(default)]
    pub cache: CacheConfig,
    pub tickets: Vec<TicketSpec>,
    #[serde(default)]
    pub namespaces: Namespaces,
    #[serde(default)]
    pub sharding: Sharding,
    #[serde(default)]
    pub ecosystem: Ecosystem,
    #[serde(default)]
    pub sidecars: Sidecars,
    #[serde(default)]
    pub telemetry: Telemetry,
    #[serde(default)]
    pub observability: Observability,
    #[serde(default)]
    pub ui_providers: UiProviders,
    #[serde(default)]
    pub client_policies: ClientPolicies,
    #[serde(default)]
    pub client_paths: ClientPaths,
    #[serde(default)]
    pub swarmui: SwarmUiConfig,
    #[serde(default)]
    pub cas: CasConfig,
}

impl Manifest {
    pub fn validate(&self) -> Result<()> {
        self.validate_with_base(None)
    }

    pub fn validate_with_base(&self, base_dir: Option<&Path>) -> Result<()> {
        if self.root_task.schema != SCHEMA_VERSION {
            bail!(
                "unsupported root_task.schema {} (expected {})",
                self.root_task.schema,
                SCHEMA_VERSION
            );
        }
        if self.secure9p.msize > MAX_MSIZE {
            bail!(
                "secure9p.msize {} exceeds maximum {}",
                self.secure9p.msize,
                MAX_MSIZE
            );
        }
        if self.secure9p.walk_depth as usize > MAX_WALK_DEPTH {
            bail!(
                "secure9p.walk_depth {} exceeds maximum {}",
                self.secure9p.walk_depth,
                MAX_WALK_DEPTH
            );
        }
        if self.secure9p.tags_per_session < 1 {
            bail!("secure9p.tags_per_session must be >= 1");
        }
        if self.secure9p.batch_frames < 1 {
            bail!("secure9p.batch_frames must be >= 1");
        }
        if self.profile.kernel {
            if self.features.std_console {
                bail!("std_console requires profile.kernel = false");
            }
            if self.features.std_host_tools {
                bail!("std_host_tools requires profile.kernel = false");
            }
        }
        self.validate_cache()?;
        self.validate_namespace_mounts()?;
        self.validate_sharding()?;
        self.validate_tickets()?;
        self.validate_ecosystem()?;
        self.validate_sidecars()?;
        self.validate_telemetry()?;
        self.validate_observability()?;
        self.validate_ui_providers()?;
        self.validate_client_policies()?;
        self.validate_client_paths()?;
        self.validate_swarmui()?;
        self.validate_cas(base_dir)?;
        Ok(())
    }

    fn validate_namespace_mounts(&self) -> Result<()> {
        for mount in &self.namespaces.mounts {
            if mount.target.len() > MAX_WALK_DEPTH {
                bail!(
                    "namespace mount {} exceeds walk depth {}",
                    mount.service,
                    MAX_WALK_DEPTH
                );
            }
            for component in &mount.target {
                if component == ".." {
                    bail!("namespace mount {} contains disallowed '..'", mount.service);
                }
                if component.is_empty() {
                    bail!(
                        "namespace mount {} contains empty path component",
                        mount.service
                    );
                }
            }
        }
        Ok(())
    }

    fn validate_tickets(&self) -> Result<()> {
        let mut seen = BTreeSet::new();
        for ticket in &self.tickets {
            let key = (ticket.role.as_str(), ticket.secret.as_str());
            if !seen.insert(key) {
                bail!("duplicate ticket entry for role {}", ticket.role.as_str());
            }
        }
        Ok(())
    }

    fn validate_sharding(&self) -> Result<()> {
        if self.sharding.shard_bits > MAX_SHARD_BITS {
            bail!(
                "sharding.shard_bits {} exceeds max {}",
                self.sharding.shard_bits,
                MAX_SHARD_BITS
            );
        }
        if self.sharding.enabled {
            if (self.secure9p.walk_depth as usize) < SHARDED_WORKER_PATH_DEPTH {
                bail!(
                    "sharding.enabled requires secure9p.walk_depth >= {}",
                    SHARDED_WORKER_PATH_DEPTH
                );
            }
            if self.sharding.legacy_worker_alias
                && (self.secure9p.walk_depth as usize) < LEGACY_WORKER_PATH_DEPTH
            {
                bail!(
                    "sharding.legacy_worker_alias requires secure9p.walk_depth >= {}",
                    LEGACY_WORKER_PATH_DEPTH
                );
            }
            if !self.sharding.legacy_worker_alias {
                self.reject_legacy_worker_paths()?;
            }
        }
        Ok(())
    }

    fn reject_legacy_worker_paths(&self) -> Result<()> {
        for mount in &self.namespaces.mounts {
            if matches!(mount.target.first(), Some(component) if component == "worker") {
                bail!(
                    "namespace mount {} references legacy /worker paths while sharding.legacy_worker_alias is false",
                    mount.service
                );
            }
        }
        for rule in &self.ecosystem.policy.rules {
            let target = rule.target.trim();
            let components: Vec<&str> =
                target.split('/').filter(|seg| !seg.is_empty()).collect();
            if matches!(components.first(), Some(component) if *component == "worker") {
                bail!(
                    "ecosystem.policy.rules[].target references legacy /worker paths while sharding.legacy_worker_alias is false"
                );
            }
        }
        Ok(())
    }

    fn validate_ecosystem(&self) -> Result<()> {
        self.validate_policy()?;
        self.validate_audit()?;
        if !self.ecosystem.host.enable {
            return Ok(());
        }
        self.validate_host_mount()?;
        if self.secure9p.msize > MAX_MSIZE {
            bail!("ecosystem.host.enable requires secure9p.msize <= {MAX_MSIZE}");
        }
        if self.secure9p.walk_depth as usize > MAX_WALK_DEPTH {
            bail!("ecosystem.host.enable requires secure9p.walk_depth <= {MAX_WALK_DEPTH}");
        }
        if !self.namespaces.role_isolation {
            bail!("ecosystem.host.enable requires namespaces.role_isolation = true");
        }
        Ok(())
    }

    fn validate_sidecars(&self) -> Result<()> {
        self.validate_sidecar_bus("sidecars.modbus", &self.sidecars.modbus)?;
        self.validate_sidecar_bus("sidecars.dnp3", &self.sidecars.dnp3)?;
        self.validate_sidecar_lora(&self.sidecars.lora)?;
        self.validate_sidecar_scopes()?;
        self.validate_sidecar_budget()?;
        Ok(())
    }

    fn validate_sidecar_bus(&self, label: &str, config: &SidecarBusConfig) -> Result<()> {
        if !config.enable {
            return Ok(());
        }
        self.validate_sidecar_mount_at(&format!("{label}.mount_at"), &config.mount_at)?;
        if config.adapters.is_empty() {
            bail!("{label}.enable requires at least one adapter");
        }
        let mut scopes = BTreeSet::new();
        for adapter in &config.adapters {
            self.validate_sidecar_adapter(label, adapter)?;
            if !scopes.insert(adapter.scope.as_str()) {
                bail!("{label}.adapters scope '{}' is duplicated", adapter.scope);
            }
        }
        Ok(())
    }

    fn validate_sidecar_lora(&self, config: &SidecarLoraConfig) -> Result<()> {
        if !config.enable {
            return Ok(());
        }
        self.validate_sidecar_mount_at("sidecars.lora.mount_at", &config.mount_at)?;
        if config.adapters.is_empty() {
            bail!("sidecars.lora.enable requires at least one adapter");
        }
        let mut scopes = BTreeSet::new();
        for adapter in &config.adapters {
            self.validate_sidecar_id("sidecars.lora.adapters[].id", &adapter.id)?;
            self.validate_sidecar_mount("sidecars.lora.adapters[].mount", &adapter.mount)?;
            self.validate_sidecar_scope("sidecars.lora.adapters[].scope", &adapter.scope)?;
            if adapter.region.trim().is_empty() {
                bail!("sidecars.lora.adapters[].region must not be empty");
            }
            if adapter.duty_cycle_percent == 0 || adapter.duty_cycle_percent > 100 {
                bail!("sidecars.lora.adapters[].duty_cycle_percent must be 1..=100");
            }
            if adapter.window_ms == 0 {
                bail!("sidecars.lora.adapters[].window_ms must be >= 1");
            }
            if adapter.max_payload_bytes == 0 {
                bail!("sidecars.lora.adapters[].max_payload_bytes must be >= 1");
            }
            if adapter.max_payload_bytes > self.secure9p.msize {
                bail!(
                    "sidecars.lora.adapters[].max_payload_bytes {} exceeds secure9p.msize {}",
                    adapter.max_payload_bytes,
                    self.secure9p.msize
                );
            }
            if adapter.tamper_log_max_entries == 0 {
                bail!("sidecars.lora.adapters[].tamper_log_max_entries must be >= 1");
            }
            if !scopes.insert(adapter.scope.as_str()) {
                bail!(
                    "sidecars.lora.adapters scope '{}' is duplicated",
                    adapter.scope
                );
            }
        }
        Ok(())
    }

    fn validate_sidecar_adapter(
        &self,
        label: &str,
        adapter: &SidecarBusAdapter,
    ) -> Result<()> {
        self.validate_sidecar_id(&format!("{label}.adapters[].id"), &adapter.id)?;
        self.validate_sidecar_mount(&format!("{label}.adapters[].mount"), &adapter.mount)?;
        self.validate_sidecar_scope(&format!("{label}.adapters[].scope"), &adapter.scope)?;
        match adapter.link {
            SidecarLink::Serial => {
                if adapter.baud == 0 {
                    bail!("{label}.adapters[].baud must be >= 1 for serial links");
                }
            }
            SidecarLink::Tcp => {}
        }
        self.validate_spool(&format!("{label}.adapters[].spool"), &adapter.spool)?;
        Ok(())
    }

    fn validate_spool(&self, label: &str, spool: &SpoolConfig) -> Result<()> {
        if spool.max_entries == 0 {
            bail!("{label}.max_entries must be >= 1");
        }
        if spool.max_entries > MAX_SPOOL_ENTRIES {
            bail!(
                "{label}.max_entries {} exceeds max {}",
                spool.max_entries,
                MAX_SPOOL_ENTRIES
            );
        }
        if spool.max_bytes == 0 {
            bail!("{label}.max_bytes must be >= 1");
        }
        if spool.max_bytes > self.secure9p.msize {
            bail!(
                "{label}.max_bytes {} exceeds secure9p.msize {}",
                spool.max_bytes,
                self.secure9p.msize
            );
        }
        Ok(())
    }

    fn validate_sidecar_mount_at(&self, label: &str, mount_at: &str) -> Result<()> {
        let trimmed = mount_at.trim();
        if !trimmed.starts_with('/') {
            bail!("{label} must be an absolute path");
        }
        let components: Vec<&str> = trimmed.split('/').filter(|seg| !seg.is_empty()).collect();
        if components.is_empty() {
            bail!("{label} must not be root");
        }
        if components.len() > self.secure9p.walk_depth as usize {
            bail!(
                "{label} exceeds secure9p.walk_depth {}",
                self.secure9p.walk_depth
            );
        }
        if components.len() + 1 > self.secure9p.walk_depth as usize {
            bail!(
                "{label} requires secure9p.walk_depth >= {}",
                components.len() + 1
            );
        }
        for component in components {
            if component == ".." {
                bail!("{label} contains disallowed '..'");
            }
            if component.is_empty() {
                bail!("{label} contains empty path component");
            }
        }
        Ok(())
    }

    fn validate_sidecar_id(&self, label: &str, id: &str) -> Result<()> {
        let trimmed = id.trim();
        if trimmed.is_empty() {
            bail!("{label} must not be empty");
        }
        if trimmed.len() > MAX_SIDECAR_ID_LEN {
            bail!(
                "{label} '{}' exceeds max length {}",
                trimmed,
                MAX_SIDECAR_ID_LEN
            );
        }
        if trimmed.contains('/') {
            bail!("{label} '{}' must not include '/'", trimmed);
        }
        if trimmed == ".." {
            bail!("{label} must not be '..'");
        }
        Ok(())
    }

    fn validate_sidecar_mount(&self, label: &str, mount: &str) -> Result<()> {
        let trimmed = mount.trim();
        if trimmed.is_empty() {
            bail!("{label} must not be empty");
        }
        if trimmed.len() > MAX_SIDECAR_MOUNT_LEN {
            bail!(
                "{label} '{}' exceeds max length {}",
                trimmed,
                MAX_SIDECAR_MOUNT_LEN
            );
        }
        if trimmed.contains('/') {
            bail!("{label} '{}' must not include '/'", trimmed);
        }
        if trimmed == ".." {
            bail!("{label} must not be '..'");
        }
        Ok(())
    }

    fn validate_sidecar_scope(&self, label: &str, scope: &str) -> Result<()> {
        let trimmed = scope.trim();
        if trimmed.is_empty() {
            bail!("{label} must not be empty");
        }
        if trimmed.len() > MAX_SIDECAR_SCOPE_LEN {
            bail!(
                "{label} '{}' exceeds max length {}",
                trimmed,
                MAX_SIDECAR_SCOPE_LEN
            );
        }
        if trimmed.contains('/') {
            bail!("{label} '{}' must not include '/'", trimmed);
        }
        if trimmed == ".." {
            bail!("{label} must not be '..'");
        }
        Ok(())
    }

    fn validate_sidecar_budget(&self) -> Result<()> {
        let mut bytes = 0u32;
        if self.sidecars.modbus.enable {
            for adapter in &self.sidecars.modbus.adapters {
                bytes = bytes.saturating_add(adapter.spool.max_bytes);
            }
        }
        if self.sidecars.dnp3.enable {
            for adapter in &self.sidecars.dnp3.adapters {
                bytes = bytes.saturating_add(adapter.spool.max_bytes);
            }
        }
        if self.sidecars.lora.enable {
            for adapter in &self.sidecars.lora.adapters {
                let entries = u32::from(adapter.tamper_log_max_entries);
                bytes = bytes.saturating_add(entries.saturating_mul(LORA_TAMPER_ENTRY_BYTES));
            }
        }
        if bytes > EVENT_PUMP_SIDECAR_BUDGET_BYTES {
            bail!(
                "sidecar budgets {} bytes exceed event-pump budget {} bytes",
                bytes,
                EVENT_PUMP_SIDECAR_BUDGET_BYTES
            );
        }
        Ok(())
    }

    fn validate_sidecar_scopes(&self) -> Result<()> {
        let mut scopes = BTreeSet::new();
        if self.sidecars.modbus.enable {
            for adapter in &self.sidecars.modbus.adapters {
                if !scopes.insert(adapter.scope.as_str()) {
                    bail!("sidecar scope '{}' is duplicated", adapter.scope);
                }
            }
        }
        if self.sidecars.dnp3.enable {
            for adapter in &self.sidecars.dnp3.adapters {
                if !scopes.insert(adapter.scope.as_str()) {
                    bail!("sidecar scope '{}' is duplicated", adapter.scope);
                }
            }
        }
        if self.sidecars.lora.enable {
            for adapter in &self.sidecars.lora.adapters {
                if !scopes.insert(adapter.scope.as_str()) {
                    bail!("sidecar scope '{}' is duplicated", adapter.scope);
                }
            }
        }
        Ok(())
    }

    fn validate_policy(&self) -> Result<()> {
        let policy = &self.ecosystem.policy;
        if policy.queue_max_entries == 0 {
            bail!("ecosystem.policy.queue_max_entries must be >= 1");
        }
        if policy.queue_max_entries > MAX_POLICY_QUEUE_ENTRIES {
            bail!(
                "ecosystem.policy.queue_max_entries {} exceeds max {}",
                policy.queue_max_entries,
                MAX_POLICY_QUEUE_ENTRIES
            );
        }
        let msize = self.secure9p.msize;
        if policy.queue_max_bytes == 0 {
            bail!("ecosystem.policy.queue_max_bytes must be >= 1");
        }
        if policy.queue_max_bytes > msize {
            bail!(
                "ecosystem.policy.queue_max_bytes {} exceeds secure9p.msize {}",
                policy.queue_max_bytes,
                msize
            );
        }
        if policy.ctl_max_bytes == 0 {
            bail!("ecosystem.policy.ctl_max_bytes must be >= 1");
        }
        if policy.ctl_max_bytes > msize {
            bail!(
                "ecosystem.policy.ctl_max_bytes {} exceeds secure9p.msize {}",
                policy.ctl_max_bytes,
                msize
            );
        }
        if policy.status_max_bytes == 0 {
            bail!("ecosystem.policy.status_max_bytes must be >= 1");
        }
        if policy.status_max_bytes > msize {
            bail!(
                "ecosystem.policy.status_max_bytes {} exceeds secure9p.msize {}",
                policy.status_max_bytes,
                msize
            );
        }
        for rule in &policy.rules {
            validate_policy_rule(rule)?;
        }
        Ok(())
    }

    fn validate_audit(&self) -> Result<()> {
        let audit = &self.ecosystem.audit;
        let msize = self.secure9p.msize;
        if audit.journal_max_bytes == 0 {
            bail!("ecosystem.audit.journal_max_bytes must be >= 1");
        }
        if audit.journal_max_bytes > msize {
            bail!(
                "ecosystem.audit.journal_max_bytes {} exceeds secure9p.msize {}",
                audit.journal_max_bytes,
                msize
            );
        }
        if audit.decisions_max_bytes == 0 {
            bail!("ecosystem.audit.decisions_max_bytes must be >= 1");
        }
        if audit.decisions_max_bytes > msize {
            bail!(
                "ecosystem.audit.decisions_max_bytes {} exceeds secure9p.msize {}",
                audit.decisions_max_bytes,
                msize
            );
        }
        if audit.replay_ctl_max_bytes == 0 {
            bail!("ecosystem.audit.replay_ctl_max_bytes must be >= 1");
        }
        if audit.replay_ctl_max_bytes > msize {
            bail!(
                "ecosystem.audit.replay_ctl_max_bytes {} exceeds secure9p.msize {}",
                audit.replay_ctl_max_bytes,
                msize
            );
        }
        if audit.replay_status_max_bytes == 0 {
            bail!("ecosystem.audit.replay_status_max_bytes must be >= 1");
        }
        if audit.replay_status_max_bytes > msize {
            bail!(
                "ecosystem.audit.replay_status_max_bytes {} exceeds secure9p.msize {}",
                audit.replay_status_max_bytes,
                msize
            );
        }
        if audit.replay_max_entries == 0 {
            bail!("ecosystem.audit.replay_max_entries must be >= 1");
        }
        if audit.replay_max_entries > MAX_REPLAY_ENTRIES {
            bail!(
                "ecosystem.audit.replay_max_entries {} exceeds max {}",
                audit.replay_max_entries,
                MAX_REPLAY_ENTRIES
            );
        }
        if audit.replay_enable && !audit.enable {
            bail!("ecosystem.audit.replay_enable requires ecosystem.audit.enable = true");
        }
        Ok(())
    }

    fn validate_host_mount(&self) -> Result<()> {
        let mount_at = self.ecosystem.host.mount_at.trim();
        if !mount_at.starts_with('/') {
            bail!("ecosystem.host.mount_at must be an absolute path");
        }
        let components: Vec<&str> = mount_at.split('/').filter(|seg| !seg.is_empty()).collect();
        if components.is_empty() {
            bail!("ecosystem.host.mount_at must not be root");
        }
        if components.len() > MAX_WALK_DEPTH {
            bail!(
                "ecosystem.host.mount_at exceeds walk depth {}",
                MAX_WALK_DEPTH
            );
        }
        for component in components {
            if component == ".." {
                bail!("ecosystem.host.mount_at contains disallowed '..'");
            }
            if component.is_empty() {
                bail!("ecosystem.host.mount_at contains empty path component");
            }
        }
        Ok(())
    }

    fn validate_cache(&self) -> Result<()> {
        let requested =
            self.cache.dma_clean || self.cache.dma_invalidate || self.cache.unify_instructions;
        if requested && !self.cache.kernel_ops {
            bail!("cache.kernel_ops must be true when cache maintenance is requested");
        }
        Ok(())
    }

    fn validate_telemetry(&self) -> Result<()> {
        if self.telemetry.ring_bytes_per_worker == 0 {
            bail!("telemetry.ring_bytes_per_worker must be > 0");
        }
        let aggregate = self
            .telemetry
            .ring_bytes_per_worker
            .saturating_mul(EVENT_PUMP_MAX_TELEMETRY_WORKERS);
        if aggregate > EVENT_PUMP_TELEMETRY_BUDGET_BYTES {
            bail!(
                "telemetry rings {} bytes exceed event-pump budget {} bytes",
                aggregate,
                EVENT_PUMP_TELEMETRY_BUDGET_BYTES
            );
        }
        Ok(())
    }

    fn validate_observability(&self) -> Result<()> {
        let proc_9p = &self.observability.proc_9p;
        let shard_count = self.proc_9p_shard_count();
        if proc_9p.sessions {
            let required = required_proc_9p_sessions_bytes(shard_count);
            ensure_buffer_bytes(
                "observability.proc_9p.sessions_bytes",
                proc_9p.sessions_bytes,
                required,
            )?;
        }
        if proc_9p.outstanding {
            let required = required_proc_9p_outstanding_bytes();
            ensure_buffer_bytes(
                "observability.proc_9p.outstanding_bytes",
                proc_9p.outstanding_bytes,
                required,
            )?;
        }
        if proc_9p.short_writes {
            let required = required_proc_9p_short_writes_bytes();
            ensure_buffer_bytes(
                "observability.proc_9p.short_writes_bytes",
                proc_9p.short_writes_bytes,
                required,
            )?;
        }

        let proc_ingest = &self.observability.proc_ingest;
        let ingest_enabled = proc_ingest.p50_ms
            || proc_ingest.p95_ms
            || proc_ingest.backpressure
            || proc_ingest.dropped
            || proc_ingest.queued
            || proc_ingest.watch;

        if ingest_enabled {
            if proc_ingest.latency_samples == 0 {
                bail!("observability.proc_ingest.latency_samples must be >= 1");
            }
            if proc_ingest.latency_samples > MAX_OBSERVE_LATENCY_SAMPLES {
                bail!(
                    "observability.proc_ingest.latency_samples {} exceeds max {}",
                    proc_ingest.latency_samples,
                    MAX_OBSERVE_LATENCY_SAMPLES
                );
            }
        }

        if proc_ingest.p50_ms {
            let required = required_proc_ingest_p50_bytes();
            ensure_buffer_bytes(
                "observability.proc_ingest.p50_ms_bytes",
                proc_ingest.p50_ms_bytes,
                required,
            )?;
        }
        if proc_ingest.p95_ms {
            let required = required_proc_ingest_p95_bytes();
            ensure_buffer_bytes(
                "observability.proc_ingest.p95_ms_bytes",
                proc_ingest.p95_ms_bytes,
                required,
            )?;
        }
        if proc_ingest.backpressure {
            let required = required_proc_ingest_backpressure_bytes();
            ensure_buffer_bytes(
                "observability.proc_ingest.backpressure_bytes",
                proc_ingest.backpressure_bytes,
                required,
            )?;
        }
        if proc_ingest.dropped {
            let required = required_proc_ingest_dropped_bytes();
            ensure_buffer_bytes(
                "observability.proc_ingest.dropped_bytes",
                proc_ingest.dropped_bytes,
                required,
            )?;
        }
        if proc_ingest.queued {
            let required = required_proc_ingest_queued_bytes();
            ensure_buffer_bytes(
                "observability.proc_ingest.queued_bytes",
                proc_ingest.queued_bytes,
                required,
            )?;
        }
        if proc_ingest.watch {
            if !proc_ingest.p50_ms
                || !proc_ingest.p95_ms
                || !proc_ingest.backpressure
                || !proc_ingest.dropped
                || !proc_ingest.queued
            {
                bail!("observability.proc_ingest.watch requires p50_ms, p95_ms, backpressure, dropped, and queued to be enabled");
            }
            if proc_ingest.watch_max_entries == 0 {
                bail!("observability.proc_ingest.watch_max_entries must be >= 1");
            }
            if proc_ingest.watch_max_entries > MAX_OBSERVE_WATCH_ENTRIES {
                bail!(
                    "observability.proc_ingest.watch_max_entries {} exceeds max {}",
                    proc_ingest.watch_max_entries,
                    MAX_OBSERVE_WATCH_ENTRIES
                );
            }
            if proc_ingest.watch_min_interval_ms == 0 {
                bail!("observability.proc_ingest.watch_min_interval_ms must be >= 1");
            }
            let required = required_proc_ingest_watch_line_bytes();
            ensure_buffer_bytes(
                "observability.proc_ingest.watch_line_bytes",
                proc_ingest.watch_line_bytes,
                required,
            )?;
        }
        Ok(())
    }

    fn validate_ui_providers(&self) -> Result<()> {
        let ui = &self.ui_providers;
        let proc_9p = &self.observability.proc_9p;
        let proc_ingest = &self.observability.proc_ingest;
        if ui.proc_9p.sessions && !proc_9p.sessions {
            bail!("ui_providers.proc_9p.sessions requires observability.proc_9p.sessions = true");
        }
        if ui.proc_9p.outstanding && !proc_9p.outstanding {
            bail!(
                "ui_providers.proc_9p.outstanding requires observability.proc_9p.outstanding = true"
            );
        }
        if ui.proc_9p.short_writes && !proc_9p.short_writes {
            bail!(
                "ui_providers.proc_9p.short_writes requires observability.proc_9p.short_writes = true"
            );
        }
        if ui.proc_ingest.p50_ms && !proc_ingest.p50_ms {
            bail!(
                "ui_providers.proc_ingest.p50_ms requires observability.proc_ingest.p50_ms = true"
            );
        }
        if ui.proc_ingest.p95_ms && !proc_ingest.p95_ms {
            bail!(
                "ui_providers.proc_ingest.p95_ms requires observability.proc_ingest.p95_ms = true"
            );
        }
        if ui.proc_ingest.backpressure && !proc_ingest.backpressure {
            bail!(
                "ui_providers.proc_ingest.backpressure requires observability.proc_ingest.backpressure = true"
            );
        }
        if (ui.policy_preflight.req || ui.policy_preflight.diff) && !self.ecosystem.policy.enable {
            bail!("ui_providers.policy_preflight requires ecosystem.policy.enable = true");
        }
        if (ui.updates.manifest || ui.updates.status) && !self.cas.enable {
            bail!("ui_providers.updates requires cas.enable = true");
        }
        if self.cas.enable && !ui.updates.manifest {
            bail!("ui_providers.updates.manifest must be true when cas.enable = true");
        }
        Ok(())
    }

    fn proc_9p_shard_count(&self) -> usize {
        if self.sharding.enabled {
            1usize << self.sharding.shard_bits
        } else {
            1
        }
    }

    fn validate_client_policies(&self) -> Result<()> {
        let pool = &self.client_policies.cohsh.pool;
        if pool.control_sessions == 0 {
            bail!("client_policies.cohsh.pool.control_sessions must be >= 1");
        }
        if pool.telemetry_sessions == 0 {
            bail!("client_policies.cohsh.pool.telemetry_sessions must be >= 1");
        }
        let retry = &self.client_policies.retry;
        if retry.max_attempts == 0 {
            bail!("client_policies.retry.max_attempts must be >= 1");
        }
        if retry.backoff_ms == 0 {
            bail!("client_policies.retry.backoff_ms must be >= 1");
        }
        if retry.ceiling_ms < retry.backoff_ms {
            bail!(
                "client_policies.retry.ceiling_ms {} must be >= backoff_ms {}",
                retry.ceiling_ms,
                retry.backoff_ms
            );
        }
        if retry.timeout_ms == 0 {
            bail!("client_policies.retry.timeout_ms must be >= 1");
        }
        let heartbeat = &self.client_policies.heartbeat;
        if heartbeat.interval_ms == 0 {
            bail!("client_policies.heartbeat.interval_ms must be >= 1");
        }
        Ok(())
    }

    fn validate_client_paths(&self) -> Result<()> {
        self.validate_client_path("client_paths.queen_ctl", &self.client_paths.queen_ctl)?;
        self.validate_client_path("client_paths.log", &self.client_paths.log)?;
        Ok(())
    }

    fn validate_swarmui(&self) -> Result<()> {
        let swarmui = &self.swarmui;
        if swarmui.cache.max_bytes == 0 {
            bail!("swarmui.cache.max_bytes must be > 0");
        }
        if swarmui.cache.ttl_s == 0 {
            bail!("swarmui.cache.ttl_s must be > 0");
        }
        let hive = &swarmui.hive;
        if hive.frame_cap_fps < 30 || hive.frame_cap_fps > 60 {
            bail!("swarmui.hive.frame_cap_fps must be within 30..=60");
        }
        if hive.step_ms == 0 {
            bail!("swarmui.hive.step_ms must be > 0");
        }
        if hive.lod_zoom_out <= 0.0 || hive.lod_zoom_out >= 1.0 {
            bail!("swarmui.hive.lod_zoom_out must be within (0.0, 1.0)");
        }
        if hive.lod_zoom_in <= hive.lod_zoom_out {
            bail!("swarmui.hive.lod_zoom_in must be > lod_zoom_out");
        }
        if hive.lod_event_budget == 0 {
            bail!("swarmui.hive.lod_event_budget must be > 0");
        }
        if hive.snapshot_max_events == 0 {
            bail!("swarmui.hive.snapshot_max_events must be > 0");
        }
        self.validate_client_path(
            "swarmui.paths.telemetry_root",
            &swarmui.paths.telemetry_root,
        )?;
        self.validate_client_path(
            "swarmui.paths.proc_ingest_root",
            &swarmui.paths.proc_ingest_root,
        )?;
        self.validate_client_path("swarmui.paths.worker_root", &swarmui.paths.worker_root)?;
        if swarmui.paths.namespace_roots.is_empty() {
            bail!("swarmui.paths.namespace_roots must not be empty");
        }
        for (idx, path) in swarmui.paths.namespace_roots.iter().enumerate() {
            let label = format!("swarmui.paths.namespace_roots[{idx}]");
            self.validate_client_path(&label, path)?;
        }
        Ok(())
    }

    fn validate_client_path(&self, label: &str, path: &str) -> Result<()> {
        if !path.starts_with('/') {
            bail!("{label} must be an absolute path");
        }
        let mut depth = 0usize;
        for component in path.split('/').skip(1) {
            if component.is_empty() {
                continue;
            }
            if component == "." || component == ".." {
                bail!("{label} contains disallowed path component '{component}'");
            }
            if component.as_bytes().contains(&0) {
                bail!("{label} contains NUL byte");
            }
            depth = depth.saturating_add(1);
            if depth > self.secure9p.walk_depth as usize {
                bail!(
                    "{label} exceeds secure9p.walk_depth {}",
                    self.secure9p.walk_depth
                );
            }
        }
        if depth == 0 {
            bail!("{label} must not be empty");
        }
        Ok(())
    }

    fn validate_cas(&self, base_dir: Option<&Path>) -> Result<()> {
        if self.ecosystem.models.enable && !self.cas.enable {
            bail!("ecosystem.models.enable requires cas.enable = true");
        }
        if !self.cas.enable {
            return Ok(());
        }
        if self.cas.store.chunk_bytes == 0 {
            bail!("cas.store.chunk_bytes must be > 0");
        }
        if self.cas.store.chunk_bytes > self.secure9p.msize {
            bail!(
                "cas.store.chunk_bytes {} exceeds secure9p.msize {}",
                self.cas.store.chunk_bytes,
                self.secure9p.msize
            );
        }
        let required = self
            .cas
            .store
            .chunk_bytes
            .saturating_mul(CAS_MAX_CHUNKS);
        if required > EVENT_PUMP_CAS_BUDGET_BYTES {
            bail!(
                "cas.store.chunk_bytes {} with max_chunks {} exceeds event-pump budget {}",
                self.cas.store.chunk_bytes,
                CAS_MAX_CHUNKS,
                EVENT_PUMP_CAS_BUDGET_BYTES
            );
        }
        if self.cas.delta.enable && !self.cas.enable {
            bail!("cas.delta.enable requires cas.enable = true");
        }
        let signing = self.cas.signing.as_ref().ok_or_else(|| {
            anyhow::anyhow!("cas.signing section required when cas.enable = true")
        })?;
        if signing.required {
            let key_path = signing
                .key_path
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow::anyhow!("cas.signing.key_path required when signing.required = true"))?;
            let resolved = resolve_manifest_relative_path(base_dir, key_path);
            let key_bytes = fs::read(&resolved).with_context(|| {
                format!("failed to read cas signing key {}", resolved.display())
            })?;
            let key_text = std::str::from_utf8(&key_bytes)
                .with_context(|| format!("cas signing key {} is not valid UTF-8", resolved.display()))?;
            let key_text = key_text.trim();
            if key_text.is_empty() {
                bail!("cas signing key {} is empty", resolved.display());
            }
            let raw = hex::decode(key_text).map_err(|err| {
                anyhow::anyhow!("cas signing key {} must be hex: {err}", resolved.display())
            })?;
            if raw.len() != 32 {
                bail!(
                    "cas signing key {} must be 32 bytes (got {})",
                    resolved.display(),
                    raw.len()
                );
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootTaskSection {
    pub schema: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ManifestMeta {
    pub author: String,
    pub purpose: String,
}

impl Default for ManifestMeta {
    fn default() -> Self {
        Self {
            author: "Lukas Bower".to_owned(),
            purpose: "Resolved root-task manifest.".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub name: String,
    pub kernel: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventPump {
    pub tick_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Secure9pLimits {
    pub msize: u32,
    pub walk_depth: u8,
    pub tags_per_session: u16,
    pub batch_frames: u16,
    pub short_write: ShortWriteConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShortWriteConfig {
    pub policy: ShortWritePolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ShortWritePolicy {
    Reject,
    Retry,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeatureToggles {
    pub net_console: bool,
    #[serde(default)]
    pub serial_console: bool,
    #[serde(default)]
    pub std_console: bool,
    #[serde(default)]
    pub std_host_tools: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct CacheConfig {
    pub kernel_ops: bool,
    pub dma_clean: bool,
    pub dma_invalidate: bool,
    pub unify_instructions: bool,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            kernel_ops: false,
            dma_clean: false,
            dma_invalidate: false,
            unify_instructions: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TicketSpec {
    pub role: Role,
    pub secret: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Namespaces {
    pub role_isolation: bool,
    pub mounts: Vec<NamespaceMount>,
}

impl Default for Namespaces {
    fn default() -> Self {
        Self {
            role_isolation: true,
            mounts: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Sharding {
    pub enabled: bool,
    pub shard_bits: u8,
    pub legacy_worker_alias: bool,
}

impl Default for Sharding {
    fn default() -> Self {
        Self {
            enabled: false,
            shard_bits: 0,
            legacy_worker_alias: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamespaceMount {
    pub service: String,
    pub target: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Ecosystem {
    pub host: EcosystemHost,
    pub audit: AuditConfig,
    pub policy: PolicyConfig,
    pub models: FeatureFlag,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Sidecars {
    pub modbus: SidecarBusConfig,
    pub dnp3: SidecarBusConfig,
    pub lora: SidecarLoraConfig,
}

impl Default for Sidecars {
    fn default() -> Self {
        Self {
            modbus: SidecarBusConfig::default(),
            dnp3: SidecarBusConfig::default(),
            lora: SidecarLoraConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SidecarBusConfig {
    pub enable: bool,
    #[serde(default = "default_bus_mount")]
    pub mount_at: String,
    #[serde(default)]
    pub adapters: Vec<SidecarBusAdapter>,
}

impl Default for SidecarBusConfig {
    fn default() -> Self {
        Self {
            enable: false,
            mount_at: default_bus_mount(),
            adapters: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SidecarBusAdapter {
    pub id: String,
    pub mount: String,
    pub scope: String,
    pub link: SidecarLink,
    pub baud: u32,
    #[serde(default)]
    pub spool: SpoolConfig,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SidecarLink {
    Serial,
    Tcp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SpoolConfig {
    pub max_entries: u16,
    pub max_bytes: u32,
}

impl Default for SpoolConfig {
    fn default() -> Self {
        Self {
            max_entries: 32,
            max_bytes: 4096,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SidecarLoraConfig {
    pub enable: bool,
    #[serde(default = "default_lora_mount")]
    pub mount_at: String,
    #[serde(default)]
    pub adapters: Vec<SidecarLoraAdapter>,
}

impl Default for SidecarLoraConfig {
    fn default() -> Self {
        Self {
            enable: false,
            mount_at: default_lora_mount(),
            adapters: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SidecarLoraAdapter {
    pub id: String,
    pub mount: String,
    pub scope: String,
    pub region: String,
    pub duty_cycle_percent: u8,
    pub window_ms: u64,
    pub max_payload_bytes: u32,
    pub tamper_log_max_entries: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Telemetry {
    pub ring_bytes_per_worker: u32,
    pub frame_schema: TelemetryFrameSchema,
    pub cursor: TelemetryCursor,
}

impl Default for Telemetry {
    fn default() -> Self {
        Self {
            ring_bytes_per_worker: 1024,
            frame_schema: TelemetryFrameSchema::LegacyPlaintext,
            cursor: TelemetryCursor::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Observability {
    pub proc_9p: Proc9pObservability,
    pub proc_ingest: ProcIngestObservability,
}

impl Default for Observability {
    fn default() -> Self {
        Self {
            proc_9p: Proc9pObservability::default(),
            proc_ingest: ProcIngestObservability::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Proc9pObservability {
    pub sessions: bool,
    pub outstanding: bool,
    pub short_writes: bool,
    pub sessions_bytes: u32,
    pub outstanding_bytes: u32,
    pub short_writes_bytes: u32,
}

impl Default for Proc9pObservability {
    fn default() -> Self {
        Self {
            sessions: false,
            outstanding: false,
            short_writes: false,
            sessions_bytes: 1024,
            outstanding_bytes: 128,
            short_writes_bytes: 128,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ProcIngestObservability {
    pub p50_ms: bool,
    pub p95_ms: bool,
    pub backpressure: bool,
    pub dropped: bool,
    pub queued: bool,
    pub watch: bool,
    pub p50_ms_bytes: u32,
    pub p95_ms_bytes: u32,
    pub backpressure_bytes: u32,
    pub dropped_bytes: u32,
    pub queued_bytes: u32,
    pub watch_max_entries: u16,
    pub watch_line_bytes: u32,
    pub watch_min_interval_ms: u64,
    pub latency_samples: u16,
    pub latency_tolerance_ms: u32,
    pub counter_tolerance: u32,
}

impl Default for ProcIngestObservability {
    fn default() -> Self {
        Self {
            p50_ms: false,
            p95_ms: false,
            backpressure: false,
            dropped: false,
            queued: false,
            watch: false,
            p50_ms_bytes: 64,
            p95_ms_bytes: 64,
            backpressure_bytes: 64,
            dropped_bytes: 64,
            queued_bytes: 64,
            watch_max_entries: 16,
            watch_line_bytes: 160,
            watch_min_interval_ms: 50,
            latency_samples: 16,
            latency_tolerance_ms: 5,
            counter_tolerance: 1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct UiProviders {
    pub proc_9p: UiProc9p,
    pub proc_ingest: UiProcIngest,
    pub policy_preflight: UiPolicyPreflight,
    pub updates: UiUpdates,
}

impl Default for UiProviders {
    fn default() -> Self {
        Self {
            proc_9p: UiProc9p::default(),
            proc_ingest: UiProcIngest::default(),
            policy_preflight: UiPolicyPreflight::default(),
            updates: UiUpdates::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct UiProc9p {
    pub sessions: bool,
    pub outstanding: bool,
    pub short_writes: bool,
}

impl Default for UiProc9p {
    fn default() -> Self {
        Self {
            sessions: false,
            outstanding: false,
            short_writes: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct UiProcIngest {
    pub p50_ms: bool,
    pub p95_ms: bool,
    pub backpressure: bool,
}

impl Default for UiProcIngest {
    fn default() -> Self {
        Self {
            p50_ms: false,
            p95_ms: false,
            backpressure: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct UiPolicyPreflight {
    pub req: bool,
    pub diff: bool,
}

impl Default for UiPolicyPreflight {
    fn default() -> Self {
        Self {
            req: false,
            diff: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct UiUpdates {
    pub manifest: bool,
    pub status: bool,
}

impl Default for UiUpdates {
    fn default() -> Self {
        Self {
            manifest: false,
            status: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ClientPolicies {
    pub cohsh: CohshClientPolicy,
    pub retry: ClientRetryPolicy,
    pub heartbeat: ClientHeartbeatPolicy,
}

impl Default for ClientPolicies {
    fn default() -> Self {
        Self {
            cohsh: CohshClientPolicy::default(),
            retry: ClientRetryPolicy::default(),
            heartbeat: ClientHeartbeatPolicy::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ClientPaths {
    pub queen_ctl: String,
    pub log: String,
}

impl Default for ClientPaths {
    fn default() -> Self {
        Self {
            queen_ctl: "/queen/ctl".to_owned(),
            log: "/log/queen.log".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SwarmUiTicketScope {
    PerTicket,
    PerRole,
}

impl SwarmUiTicketScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            SwarmUiTicketScope::PerTicket => "per-ticket",
            SwarmUiTicketScope::PerRole => "per-role",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SwarmUiConfig {
    pub ticket_scope: SwarmUiTicketScope,
    pub cache: SwarmUiCacheConfig,
    pub hive: SwarmUiHiveConfig,
    pub paths: SwarmUiPathsConfig,
}

impl Default for SwarmUiConfig {
    fn default() -> Self {
        Self {
            ticket_scope: SwarmUiTicketScope::PerTicket,
            cache: SwarmUiCacheConfig::default(),
            hive: SwarmUiHiveConfig::default(),
            paths: SwarmUiPathsConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SwarmUiCacheConfig {
    pub enabled: bool,
    pub max_bytes: u32,
    pub ttl_s: u64,
}

impl Default for SwarmUiCacheConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_bytes: 262_144,
            ttl_s: 3600,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SwarmUiHiveConfig {
    pub frame_cap_fps: u16,
    pub step_ms: u16,
    pub lod_zoom_out: f32,
    pub lod_zoom_in: f32,
    pub lod_event_budget: u32,
    pub snapshot_max_events: u32,
}

impl Default for SwarmUiHiveConfig {
    fn default() -> Self {
        Self {
            frame_cap_fps: 60,
            step_ms: 16,
            lod_zoom_out: 0.7,
            lod_zoom_in: 1.25,
            lod_event_budget: 512,
            snapshot_max_events: 4096,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SwarmUiPathsConfig {
    pub telemetry_root: String,
    pub proc_ingest_root: String,
    pub worker_root: String,
    pub namespace_roots: Vec<String>,
}

impl Default for SwarmUiPathsConfig {
    fn default() -> Self {
        Self {
            telemetry_root: "/worker".to_owned(),
            proc_ingest_root: "/proc/ingest".to_owned(),
            worker_root: "/worker".to_owned(),
            namespace_roots: vec![
                "/proc".to_owned(),
                "/queen".to_owned(),
                "/worker".to_owned(),
                "/log".to_owned(),
                "/gpu".to_owned(),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct CohshClientPolicy {
    pub pool: CohshPoolPolicy,
}

impl Default for CohshClientPolicy {
    fn default() -> Self {
        Self {
            pool: CohshPoolPolicy::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct CohshPoolPolicy {
    pub control_sessions: u16,
    pub telemetry_sessions: u16,
}

impl Default for CohshPoolPolicy {
    fn default() -> Self {
        Self {
            control_sessions: 2,
            telemetry_sessions: 4,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ClientRetryPolicy {
    pub max_attempts: u8,
    pub backoff_ms: u64,
    pub ceiling_ms: u64,
    pub timeout_ms: u64,
}

impl Default for ClientRetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            backoff_ms: 200,
            ceiling_ms: 2000,
            timeout_ms: 5000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ClientHeartbeatPolicy {
    pub interval_ms: u64,
}

impl Default for ClientHeartbeatPolicy {
    fn default() -> Self {
        Self { interval_ms: 15000 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct CasConfig {
    pub enable: bool,
    pub store: CasStoreConfig,
    pub delta: CasDeltaConfig,
    pub signing: Option<CasSigningConfig>,
}

impl Default for CasConfig {
    fn default() -> Self {
        Self {
            enable: false,
            store: CasStoreConfig::default(),
            delta: CasDeltaConfig::default(),
            signing: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct CasStoreConfig {
    pub chunk_bytes: u32,
}

impl Default for CasStoreConfig {
    fn default() -> Self {
        Self { chunk_bytes: 0 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct CasDeltaConfig {
    pub enable: bool,
}

impl Default for CasDeltaConfig {
    fn default() -> Self {
        Self { enable: false }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CasSigningConfig {
    pub required: bool,
    pub key_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct TelemetryCursor {
    pub retain_on_boot: bool,
}

impl Default for TelemetryCursor {
    fn default() -> Self {
        Self {
            retain_on_boot: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TelemetryFrameSchema {
    LegacyPlaintext,
    CborV1,
}

impl Default for Ecosystem {
    fn default() -> Self {
        Self {
            host: EcosystemHost::default(),
            audit: AuditConfig::default(),
            policy: PolicyConfig::default(),
            models: FeatureFlag::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EcosystemHost {
    pub enable: bool,
    #[serde(default)]
    pub providers: Vec<HostProvider>,
    #[serde(default = "default_host_mount")]
    pub mount_at: String,
}

impl Default for EcosystemHost {
    fn default() -> Self {
        Self {
            enable: false,
            providers: Vec::new(),
            mount_at: default_host_mount(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HostProvider {
    Systemd,
    K8s,
    Nvidia,
    Jetson,
    Net,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct FeatureFlag {
    pub enable: bool,
}

impl Default for FeatureFlag {
    fn default() -> Self {
        Self { enable: false }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct AuditConfig {
    pub enable: bool,
    pub journal_max_bytes: u32,
    pub decisions_max_bytes: u32,
    pub replay_enable: bool,
    pub replay_max_entries: u16,
    pub replay_ctl_max_bytes: u32,
    pub replay_status_max_bytes: u32,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            enable: false,
            journal_max_bytes: 8192,
            decisions_max_bytes: 4096,
            replay_enable: false,
            replay_max_entries: 64,
            replay_ctl_max_bytes: 1024,
            replay_status_max_bytes: 1024,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct PolicyConfig {
    pub enable: bool,
    pub queue_max_entries: u16,
    pub queue_max_bytes: u32,
    pub ctl_max_bytes: u32,
    pub status_max_bytes: u32,
    #[serde(default)]
    pub rules: Vec<PolicyRule>,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            enable: false,
            queue_max_entries: 32,
            queue_max_bytes: 4096,
            ctl_max_bytes: 2048,
            status_max_bytes: 512,
            rules: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyRule {
    pub id: String,
    pub target: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    Queen,
    WorkerHeartbeat,
    WorkerGpu,
    WorkerBus,
    WorkerLora,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queen => "queen",
            Self::WorkerHeartbeat => "worker-heartbeat",
            Self::WorkerGpu => "worker-gpu",
            Self::WorkerBus => "worker-bus",
            Self::WorkerLora => "worker-lora",
        }
    }
}

fn default_host_mount() -> String {
    "/host".to_owned()
}

fn default_bus_mount() -> String {
    "/bus".to_owned()
}

fn default_lora_mount() -> String {
    "/lora".to_owned()
}

fn ensure_buffer_bytes(label: &str, value: u32, required: usize) -> Result<()> {
    if required > MAX_MSIZE as usize {
        bail!(
            "{label} requires at least {required} bytes which exceeds max {MAX_MSIZE}"
        );
    }
    if value < required as u32 {
        bail!("{label} {value} is below required minimum {required}");
    }
    if value > MAX_MSIZE {
        bail!("{label} {value} exceeds max {MAX_MSIZE}");
    }
    Ok(())
}

fn required_proc_9p_sessions_bytes(shard_count: usize) -> usize {
    let header = "sessions total=".len()
        + MAX_U64_DIGITS
        + " worker=".len()
        + MAX_U64_DIGITS
        + " shard_bits=".len()
        + MAX_U8_DIGITS
        + " shard_count=".len()
        + SHARD_COUNT_DIGITS
        + 1;
    let shard_line = "shard ".len() + SHARD_LABEL_BYTES + 1 + MAX_U64_DIGITS + 1;
    header + shard_count.saturating_mul(shard_line)
}

fn required_proc_9p_outstanding_bytes() -> usize {
    "outstanding current=".len() + MAX_U64_DIGITS + " limit=".len() + MAX_U64_DIGITS + 1
}

fn required_proc_9p_short_writes_bytes() -> usize {
    "short_writes total=".len() + MAX_U64_DIGITS + " retries=".len() + MAX_U64_DIGITS + 1
}

fn required_proc_ingest_p50_bytes() -> usize {
    "p50_ms=".len() + MAX_U32_DIGITS + 1
}

fn required_proc_ingest_p95_bytes() -> usize {
    "p95_ms=".len() + MAX_U32_DIGITS + 1
}

fn required_proc_ingest_backpressure_bytes() -> usize {
    "backpressure=".len() + MAX_U64_DIGITS + 1
}

fn required_proc_ingest_dropped_bytes() -> usize {
    "dropped=".len() + MAX_U64_DIGITS + 1
}

fn required_proc_ingest_queued_bytes() -> usize {
    "queued=".len() + MAX_U32_DIGITS + 1
}

fn required_proc_ingest_watch_line_bytes() -> usize {
    "watch ts_ms=".len()
        + MAX_U64_DIGITS
        + " p50_ms=".len()
        + MAX_U32_DIGITS
        + " p95_ms=".len()
        + MAX_U32_DIGITS
        + " queued=".len()
        + MAX_U32_DIGITS
        + " backpressure=".len()
        + MAX_U64_DIGITS
        + " dropped=".len()
        + MAX_U64_DIGITS
        + 1
}

fn validate_policy_rule(rule: &PolicyRule) -> Result<()> {
    let id = rule.id.trim();
    if id.is_empty() {
        bail!("ecosystem.policy.rules[].id must not be empty");
    }
    if id.len() > MAX_POLICY_RULE_ID_LEN {
        bail!(
            "ecosystem.policy.rules[].id '{}' exceeds max length {}",
            id,
            MAX_POLICY_RULE_ID_LEN
        );
    }
    let target = rule.target.trim();
    if !target.starts_with('/') {
        bail!("ecosystem.policy.rules[].target must be absolute");
    }
    let components: Vec<&str> = target.split('/').filter(|seg| !seg.is_empty()).collect();
    if components.is_empty() {
        bail!("ecosystem.policy.rules[].target must not be root");
    }
    if components.len() > MAX_WALK_DEPTH {
        bail!(
            "ecosystem.policy.rules[].target exceeds walk depth {}",
            MAX_WALK_DEPTH
        );
    }
    for component in components {
        if component == ".." {
            bail!("ecosystem.policy.rules[].target contains disallowed '..'");
        }
        if component.is_empty() {
            bail!("ecosystem.policy.rules[].target contains empty component");
        }
        if component.contains('*') && component != "*" {
            bail!("ecosystem.policy.rules[].target wildcard must be '*'");
        }
    }
    Ok(())
}

pub fn load_manifest(path: &Path) -> Result<Manifest> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read manifest {}", path.display()))?;
    let manifest: Manifest = toml::from_str(&contents)
        .with_context(|| format!("invalid manifest TOML in {}", path.display()))?;
    Ok(manifest)
}

pub(crate) fn resolve_manifest_relative_path(base_dir: Option<&Path>, value: &str) -> PathBuf {
    let trimmed = value.trim();
    let candidate = Path::new(trimmed);
    if candidate.is_absolute() || base_dir.is_none() {
        return candidate.to_path_buf();
    }
    let base = base_dir.unwrap_or_else(|| Path::new("."));
    let primary = base.join(candidate);
    if primary.exists() {
        return primary;
    }
    if let Some(parent) = base.parent() {
        let secondary = parent.join(candidate);
        if secondary.exists() {
            return secondary;
        }
    }
    primary
}

pub fn serialize_manifest(manifest: &Manifest) -> Result<Vec<u8>> {
    let json = serde_json::to_vec_pretty(manifest)?;
    Ok(json)
}

pub fn schema_version() -> &'static str {
    SCHEMA_VERSION
}
