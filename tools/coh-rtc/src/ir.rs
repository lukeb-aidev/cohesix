// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Define and validate the root-task manifest IR.
// Author: Lukas Bower

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

const SCHEMA_VERSION: &str = "1.2";
const MAX_WALK_DEPTH: usize = 8;
const MAX_MSIZE: u32 = 8192;
const EVENT_PUMP_TELEMETRY_BUDGET_BYTES: u32 = 32 * 1024;
const EVENT_PUMP_MAX_TELEMETRY_WORKERS: u32 = 8;
const MAX_POLICY_QUEUE_ENTRIES: u16 = 64;
const MAX_POLICY_RULE_ID_LEN: usize = 64;
const MAX_REPLAY_ENTRIES: u16 = 256;

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
    pub ecosystem: Ecosystem,
    #[serde(default)]
    pub telemetry: Telemetry,
}

impl Manifest {
    pub fn validate(&self) -> Result<()> {
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
        self.validate_tickets()?;
        self.validate_ecosystem()?;
        self.validate_telemetry()?;
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
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queen => "queen",
            Self::WorkerHeartbeat => "worker-heartbeat",
            Self::WorkerGpu => "worker-gpu",
        }
    }
}

fn default_host_mount() -> String {
    "/host".to_owned()
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

pub fn serialize_manifest(manifest: &Manifest) -> Result<Vec<u8>> {
    let json = serde_json::to_vec_pretty(manifest)?;
    Ok(json)
}

pub fn schema_version() -> &'static str {
    SCHEMA_VERSION
}
