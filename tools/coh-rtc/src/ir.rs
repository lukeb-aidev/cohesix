// Author: Lukas Bower
// Purpose: Define and validate the root-task manifest IR.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

const SCHEMA_VERSION: &str = "1.1";
const MAX_WALK_DEPTH: usize = 8;
const MAX_MSIZE: u32 = 8192;

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
                    bail!("namespace mount {} contains empty path component", mount.service);
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
        if !self.ecosystem.host.enable {
            return Ok(());
        }
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

    fn validate_cache(&self) -> Result<()> {
        let requested = self.cache.dma_clean
            || self.cache.dma_invalidate
            || self.cache.unify_instructions;
        if requested && !self.cache.kernel_ops {
            bail!("cache.kernel_ops must be true when cache maintenance is requested");
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
    pub audit: FeatureFlag,
    pub policy: FeatureFlag,
    pub models: FeatureFlag,
}

impl Default for Ecosystem {
    fn default() -> Self {
        Self {
            host: EcosystemHost::default(),
            audit: FeatureFlag::default(),
            policy: FeatureFlag::default(),
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
