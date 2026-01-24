// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Synthetic namespace builder backing the NineDoor Secure9P server.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};

use gpu_bridge_host::{GpuModelCatalog, TelemetrySchema};
use serde::Deserialize;
use sidecar_bus::{LinkState, OfflineSpool, SpoolConfig, SpoolError};
use sha2::{Digest, Sha256};
use secure9p_codec::{ErrorCode, Qid, QidType, MAX_MSIZE};
use secure9p_core::append_only_write_bounds;
use trace_model::TraceLevel;
use worker_lora::{DutyCycleConfig, DutyCycleGuard, TamperEntry, TamperLog, TamperReason};

use super::cas::{
    parse_sha256, validate_epoch, CasConfig, CasStore, ModelFileKind, UpdateStatusPayloads,
};
use super::observe::ObserveConfig;
use super::telemetry::{
    ingest::{
        TelemetryIngestError, TelemetryIngestErrorKind, TelemetryIngestState,
        MAX_TELEMETRY_RECORD_BYTES,
    },
    TelemetryAudit, TelemetryAuditLevel, TelemetryConfig, TelemetryFile, TelemetryIngestConfig,
    TelemetryManifestStore,
};
use super::tracefs::TraceFs;
use super::ui::{match_ui_provider, UiProviderConfig, UiProviderKind, UiVariant};
use crate::NineDoorError;

const SELFTEST_QUICK_SCRIPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../resources/proc_tests/selftest_quick.coh"
));
const SELFTEST_FULL_SCRIPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../resources/proc_tests/selftest_full.coh"
));
const SELFTEST_NEGATIVE_SCRIPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../resources/proc_tests/selftest_negative.coh"
));

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TelemetryCtlCommand {
    new: String,
    #[serde(default)]
    mime: Option<String>,
}

impl TelemetryCtlCommand {
    fn parse(line: &str) -> Result<Self, NineDoorError> {
        serde_json::from_str(line).map_err(|err| {
            NineDoorError::protocol(
                ErrorCode::Invalid,
                format!("invalid telemetry ctl command: {err}"),
            )
        })
    }
}

/// Host providers that may be mirrored into `/host`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostProvider {
    /// systemd provider nodes.
    Systemd,
    /// Kubernetes provider nodes.
    K8s,
    /// NVIDIA GPU provider nodes.
    Nvidia,
    /// Jetson provider nodes.
    Jetson,
    /// Network provider nodes.
    Net,
}

impl HostProvider {
    /// Return the canonical provider label.
    pub fn as_str(self) -> &'static str {
        match self {
            HostProvider::Systemd => "systemd",
            HostProvider::K8s => "k8s",
            HostProvider::Nvidia => "nvidia",
            HostProvider::Jetson => "jetson",
            HostProvider::Net => "net",
        }
    }
}

/// Manifest-driven shard layout for worker namespaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShardLayout {
    enabled: bool,
    shard_bits: u8,
    legacy_worker_alias: bool,
}

impl ShardLayout {
    /// Construct a disabled shard layout (legacy `/worker/<id>`).
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            shard_bits: 0,
            legacy_worker_alias: false,
        }
    }

    /// Construct an enabled shard layout.
    pub fn enabled(shard_bits: u8, legacy_worker_alias: bool) -> Self {
        debug_assert!(shard_bits <= 8, "shard_bits must be <= 8");
        Self {
            enabled: true,
            shard_bits,
            legacy_worker_alias,
        }
    }

    /// Return true when sharding is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Return the configured shard bits.
    pub fn shard_bits(&self) -> u8 {
        self.shard_bits
    }

    /// Return true when legacy `/worker/<id>` aliases are enabled.
    pub fn legacy_worker_alias(&self) -> bool {
        self.legacy_worker_alias
    }

    /// Return true when legacy aliases are active in sharded mode.
    pub fn legacy_worker_alias_enabled(&self) -> bool {
        self.enabled && self.legacy_worker_alias
    }

    /// Return the number of shards implied by the shard bits.
    pub fn shard_count(&self) -> usize {
        if self.enabled {
            1usize << self.shard_bits
        } else {
            1
        }
    }

    /// Render a two-digit shard label.
    pub fn shard_label(&self, shard: u8) -> String {
        format!("{:02x}", shard)
    }

    /// Compute the shard/provider index for a worker identifier.
    pub fn worker_shard(&self, worker_id: &str) -> u8 {
        if !self.enabled || self.shard_bits == 0 {
            return 0;
        }
        let digest = Sha256::digest(worker_id.as_bytes());
        let mut shard = digest[0];
        if self.shard_bits < 8 {
            shard >>= 8 - self.shard_bits;
        }
        shard
    }

    /// Compute the shard label for a worker identifier.
    pub fn worker_shard_label(&self, worker_id: &str) -> String {
        self.shard_label(self.worker_shard(worker_id))
    }

    /// Return shard labels in deterministic order.
    pub fn shard_labels(&self) -> Vec<String> {
        (0..self.shard_count())
            .map(|idx| self.shard_label(idx as u8))
            .collect()
    }

    /// Return the canonical parent path for a worker directory.
    pub fn worker_parent(&self, worker_id: &str) -> Vec<String> {
        if self.enabled {
            vec![
                "shard".to_owned(),
                self.worker_shard_label(worker_id),
                "worker".to_owned(),
            ]
        } else {
            vec!["worker".to_owned()]
        }
    }

    /// Return the canonical path to a worker directory.
    pub fn worker_root(&self, worker_id: &str) -> Vec<String> {
        let mut path = self.worker_parent(worker_id);
        path.push(worker_id.to_owned());
        path
    }

    /// Return the canonical path to a worker telemetry file.
    pub fn worker_telemetry_path(&self, worker_id: &str) -> Vec<String> {
        let mut path = self.worker_root(worker_id);
        path.push("telemetry".to_owned());
        path
    }

    /// Return true when the supplied path is a worker telemetry file.
    pub fn is_worker_telemetry_path(&self, path: &[String], worker_id: &str) -> bool {
        if self.enabled {
            if matches!(
                path,
                [first, shard, mid, id, leaf]
                    if first == "shard"
                        && mid == "worker"
                        && leaf == "telemetry"
                        && id == worker_id
                        && shard == self.worker_shard_label(worker_id).as_str()
            ) {
                return true;
            }
            if self.legacy_worker_alias_enabled() {
                return matches!(path, [first, id, leaf] if first == "worker" && id == worker_id && leaf == "telemetry");
            }
            return false;
        }
        matches!(path, [first, id, leaf] if first == "worker" && id == worker_id && leaf == "telemetry")
    }

    /// Return a worker id when the path resolves to a telemetry file.
    pub fn worker_id_from_telemetry_path<'a>(&self, path: &'a [String]) -> Option<&'a str> {
        if self.enabled {
            if let [first, shard, mid, id, leaf] = path {
                if first == "shard"
                    && mid == "worker"
                    && leaf == "telemetry"
                    && shard == self.worker_shard_label(id).as_str()
                {
                    return Some(id.as_str());
                }
            }
            if self.legacy_worker_alias_enabled() {
                if let [first, id, leaf] = path {
                    if first == "worker" && leaf == "telemetry" {
                        return Some(id.as_str());
                    }
                }
            }
            return None;
        }
        if let [first, id, leaf] = path {
            if first == "worker" && leaf == "telemetry" {
                return Some(id.as_str());
            }
        }
        None
    }

    /// Return the maximum worker telemetry path depth implied by this layout.
    pub fn max_worker_path_depth(&self) -> usize {
        if self.enabled {
            5
        } else {
            3
        }
    }
}

impl Default for ShardLayout {
    fn default() -> Self {
        Self::enabled(8, true)
    }
}

/// Configuration describing whether `/host` should be mounted.
#[derive(Debug, Clone)]
pub struct HostNamespaceConfig {
    enabled: bool,
    mount_path: Vec<String>,
    providers: Vec<HostProvider>,
}

impl HostNamespaceConfig {
    /// Construct a disabled host configuration.
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            mount_path: vec!["host".to_owned()],
            providers: Vec::new(),
        }
    }

    /// Construct an enabled host configuration.
    pub fn enabled(mount_at: &str, providers: &[HostProvider]) -> Result<Self, NineDoorError> {
        let mount_path = parse_host_mount(mount_at)?;
        Ok(Self {
            enabled: true,
            mount_path,
            providers: providers.to_vec(),
        })
    }
}

/// Configuration for a single bus-sidecar adapter.
#[derive(Debug, Clone)]
pub struct SidecarBusAdapterConfig {
    mount: String,
    scope: String,
    spool: SpoolConfig,
}

impl SidecarBusAdapterConfig {
    /// Construct a bus adapter configuration.
    pub fn new(mount: impl Into<String>, scope: impl Into<String>, spool: SpoolConfig) -> Self {
        Self {
            mount: mount.into(),
            scope: scope.into(),
            spool,
        }
    }
}

/// Configuration describing a bus sidecar namespace.
#[derive(Debug, Clone)]
pub struct SidecarBusConfig {
    enabled: bool,
    mount_path: Vec<String>,
    adapters: Vec<SidecarBusAdapterConfig>,
}

impl SidecarBusConfig {
    /// Construct a disabled bus sidecar configuration.
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            mount_path: vec!["bus".to_owned()],
            adapters: Vec::new(),
        }
    }

    /// Construct an enabled bus sidecar configuration.
    pub fn enabled(
        mount_at: &str,
        adapters: &[SidecarBusAdapterConfig],
    ) -> Result<Self, NineDoorError> {
        let mount_path = parse_sidecar_mount("bus", mount_at)?;
        Ok(Self {
            enabled: true,
            mount_path,
            adapters: adapters.to_vec(),
        })
    }
}

/// Configuration for a single LoRa sidecar adapter.
#[derive(Debug, Clone)]
pub struct SidecarLoraAdapterConfig {
    mount: String,
    scope: String,
    duty_cycle: DutyCycleConfig,
    tamper_log_max_entries: usize,
}

impl SidecarLoraAdapterConfig {
    /// Construct a LoRa adapter configuration.
    pub fn new(
        mount: impl Into<String>,
        scope: impl Into<String>,
        duty_cycle: DutyCycleConfig,
        tamper_log_max_entries: usize,
    ) -> Self {
        Self {
            mount: mount.into(),
            scope: scope.into(),
            duty_cycle,
            tamper_log_max_entries,
        }
    }
}

/// Configuration describing a LoRa sidecar namespace.
#[derive(Debug, Clone)]
pub struct SidecarLoraConfig {
    enabled: bool,
    mount_path: Vec<String>,
    adapters: Vec<SidecarLoraAdapterConfig>,
}

impl SidecarLoraConfig {
    /// Construct a disabled LoRa sidecar configuration.
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            mount_path: vec!["lora".to_owned()],
            adapters: Vec::new(),
        }
    }

    /// Construct an enabled LoRa sidecar configuration.
    pub fn enabled(
        mount_at: &str,
        adapters: &[SidecarLoraAdapterConfig],
    ) -> Result<Self, NineDoorError> {
        let mount_path = parse_sidecar_mount("lora", mount_at)?;
        Ok(Self {
            enabled: true,
            mount_path,
            adapters: adapters.to_vec(),
        })
    }
}

/// Namespace configuration for bus and LoRa sidecars.
#[derive(Debug, Clone)]
pub struct SidecarNamespaceConfig {
    modbus: SidecarBusConfig,
    dnp3: SidecarBusConfig,
    lora: SidecarLoraConfig,
}

impl SidecarNamespaceConfig {
    /// Construct a disabled sidecar configuration.
    pub fn disabled() -> Self {
        Self {
            modbus: SidecarBusConfig::disabled(),
            dnp3: SidecarBusConfig::disabled(),
            lora: SidecarLoraConfig::disabled(),
        }
    }
}

/// Capability scope entry tied to a sidecar mount.
#[derive(Debug, Clone)]
pub struct SidecarScope {
    scope: String,
    mount_root: Vec<String>,
}

impl SidecarScope {
    pub fn scope(&self) -> &str {
        self.scope.as_str()
    }

    #[allow(dead_code)]
    pub fn mount_root(&self) -> &[String] {
        &self.mount_root
    }

    #[allow(dead_code)]
    fn matches_path(&self, path: &[String]) -> bool {
        self.matches_prefix(path)
    }

    pub(crate) fn matches_prefix(&self, path: &[String]) -> bool {
        path.starts_with(&self.mount_root) || self.mount_root.starts_with(path)
    }

    pub(crate) fn contains_path(&self, path: &[String]) -> bool {
        path.starts_with(&self.mount_root)
    }
}

/// Configuration describing whether `/policy` and `/actions` should be mounted.
#[derive(Debug, Clone)]
pub struct PolicyNamespaceConfig {
    enabled: bool,
    rules_snapshot: Vec<u8>,
}

impl PolicyNamespaceConfig {
    /// Construct a disabled policy configuration.
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            rules_snapshot: Vec::new(),
        }
    }

    /// Construct an enabled policy configuration with the rules snapshot payload.
    pub fn enabled(rules_snapshot: Vec<u8>) -> Self {
        Self {
            enabled: true,
            rules_snapshot,
        }
    }
}

/// Configuration describing whether `/audit` should be mounted.
#[derive(Debug, Clone)]
pub struct AuditNamespaceConfig {
    enabled: bool,
    export_snapshot: Vec<u8>,
}

impl AuditNamespaceConfig {
    /// Construct a disabled audit configuration.
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            export_snapshot: Vec::new(),
        }
    }

    /// Construct an enabled audit configuration with the export snapshot payload.
    pub fn enabled(export_snapshot: Vec<u8>) -> Self {
        Self {
            enabled: true,
            export_snapshot,
        }
    }
}

/// Configuration describing whether `/replay` should be mounted.
#[derive(Debug, Clone)]
pub struct ReplayNamespaceConfig {
    enabled: bool,
    status_snapshot: Vec<u8>,
}

impl ReplayNamespaceConfig {
    /// Construct a disabled replay configuration.
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            status_snapshot: Vec::new(),
        }
    }

    /// Construct an enabled replay configuration with the status payload.
    pub fn enabled(status_snapshot: Vec<u8>) -> Self {
        Self {
            enabled: true,
            status_snapshot,
        }
    }
}

/// Synthetic namespace backing the NineDoor Secure9P server.
#[derive(Debug)]
pub struct Namespace {
    root: Node,
    trace: TraceFs,
    shards: ShardLayout,
    telemetry: TelemetryConfig,
    telemetry_manifest: TelemetryManifestStore,
    telemetry_ingest: TelemetryIngestState,
    cas: CasStore,
    worker_ids: BTreeSet<String>,
    ui: UiProviderConfig,
    host: HostNamespaceConfig,
    sidecar_modbus: SidecarBusState,
    sidecar_dnp3: SidecarBusState,
    sidecar_lora: SidecarLoraState,
    sidecar_bus_scopes: Vec<SidecarScope>,
    sidecar_lora_scopes: Vec<SidecarScope>,
    policy: PolicyNamespaceConfig,
    audit: AuditNamespaceConfig,
    replay: ReplayNamespaceConfig,
}

/// Metadata for matched UI provider paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UiProviderInfo {
    /// Provider kind.
    pub kind: UiProviderKind,
    /// Variant requested for the provider.
    pub variant: UiVariant,
    /// Whether the provider is enabled for this namespace.
    pub enabled: bool,
}

impl Namespace {
    /// Construct the namespace with the predefined synthetic tree.
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::new_with_telemetry(TelemetryConfig::default())
    }

    /// Construct the namespace with explicit telemetry configuration.
    pub fn new_with_telemetry(telemetry: TelemetryConfig) -> Self {
        Self::new_with_telemetry_manifest_host_policy(
            telemetry,
            TelemetryIngestConfig::default(),
            TelemetryManifestStore::default(),
            CasConfig::disabled(),
            ShardLayout::default(),
            UiProviderConfig::default(),
            HostNamespaceConfig::disabled(),
            SidecarNamespaceConfig::disabled(),
            PolicyNamespaceConfig::disabled(),
            AuditNamespaceConfig::disabled(),
            ReplayNamespaceConfig::disabled(),
        )
    }

    /// Construct the namespace with explicit telemetry configuration and manifest store.
    #[allow(dead_code)]
    pub fn new_with_telemetry_and_manifest(
        telemetry: TelemetryConfig,
        telemetry_manifest: TelemetryManifestStore,
    ) -> Self {
        Self::new_with_telemetry_manifest_host_policy(
            telemetry,
            TelemetryIngestConfig::default(),
            telemetry_manifest,
            CasConfig::disabled(),
            ShardLayout::default(),
            UiProviderConfig::default(),
            HostNamespaceConfig::disabled(),
            SidecarNamespaceConfig::disabled(),
            PolicyNamespaceConfig::disabled(),
            AuditNamespaceConfig::disabled(),
            ReplayNamespaceConfig::disabled(),
        )
    }

    /// Construct the namespace with telemetry, manifest storage, host provider config, and policy.
    pub fn new_with_telemetry_manifest_host_policy(
        telemetry: TelemetryConfig,
        telemetry_ingest: TelemetryIngestConfig,
        telemetry_manifest: TelemetryManifestStore,
        cas: CasConfig,
        shards: ShardLayout,
        ui: UiProviderConfig,
        host: HostNamespaceConfig,
        sidecars: SidecarNamespaceConfig,
        policy: PolicyNamespaceConfig,
        audit: AuditNamespaceConfig,
        replay: ReplayNamespaceConfig,
    ) -> Self {
        let sidecar_modbus = SidecarBusState::new(sidecars.modbus);
        let sidecar_dnp3 = SidecarBusState::new(sidecars.dnp3);
        let sidecar_lora = SidecarLoraState::new(sidecars.lora);
        let mut sidecar_bus_scopes = Vec::new();
        sidecar_bus_scopes.extend(sidecar_modbus.scopes());
        sidecar_bus_scopes.extend(sidecar_dnp3.scopes());
        let sidecar_lora_scopes = sidecar_lora.scopes();
        let mut namespace = Self {
            root: Node::directory(Vec::new()),
            trace: TraceFs::new(),
            shards,
            telemetry,
            telemetry_manifest,
            telemetry_ingest: TelemetryIngestState::new(telemetry_ingest),
            cas: CasStore::new(cas),
            worker_ids: BTreeSet::new(),
            ui,
            host,
            sidecar_modbus,
            sidecar_dnp3,
            sidecar_lora,
            sidecar_bus_scopes,
            sidecar_lora_scopes,
            policy,
            audit,
            replay,
        };
        namespace.bootstrap();
        namespace
    }

    /// Construct the namespace with telemetry, manifest storage, and host provider config.
    #[allow(dead_code)]
    pub fn new_with_telemetry_manifest_and_host(
        telemetry: TelemetryConfig,
        telemetry_manifest: TelemetryManifestStore,
        host: HostNamespaceConfig,
    ) -> Self {
        Self::new_with_telemetry_manifest_host_policy(
            telemetry,
            TelemetryIngestConfig::default(),
            telemetry_manifest,
            CasConfig::disabled(),
            ShardLayout::default(),
            UiProviderConfig::default(),
            host,
            SidecarNamespaceConfig::disabled(),
            PolicyNamespaceConfig::disabled(),
            AuditNamespaceConfig::disabled(),
            ReplayNamespaceConfig::disabled(),
        )
    }

    /// Retrieve the root Qid.
    pub fn root_qid(&self) -> Qid {
        self.root.qid
    }

    /// Return the manifest-driven shard layout.
    pub fn shard_layout(&self) -> &ShardLayout {
        &self.shards
    }

    /// Return UI provider metadata for the supplied canonical path.
    pub fn ui_provider_info(&self, path: &[String]) -> Option<UiProviderInfo> {
        let matched = match_ui_provider(path)?;
        let enabled = match matched.kind {
            UiProviderKind::Proc9pSessions => self.ui.proc_9p.sessions,
            UiProviderKind::Proc9pOutstanding => self.ui.proc_9p.outstanding,
            UiProviderKind::Proc9pShortWrites => self.ui.proc_9p.short_writes,
            UiProviderKind::ProcIngestP50 => self.ui.proc_ingest.p50_ms,
            UiProviderKind::ProcIngestP95 => self.ui.proc_ingest.p95_ms,
            UiProviderKind::ProcIngestBackpressure => self.ui.proc_ingest.backpressure,
            UiProviderKind::PolicyPreflightReq => self.ui.policy_preflight.req && self.policy.enabled,
            UiProviderKind::PolicyPreflightDiff => {
                self.ui.policy_preflight.diff && self.policy.enabled
            }
            UiProviderKind::UpdatesManifest => self.ui.updates.manifest && self.cas.enabled(),
            UiProviderKind::UpdatesStatus => self.ui.updates.status && self.cas.enabled(),
        };
        Some(UiProviderInfo {
            kind: matched.kind,
            variant: matched.variant,
            enabled,
        })
    }

    /// Return the configured host mount path, if enabled.
    pub fn host_mount_path(&self) -> Option<&[String]> {
        self.host.enabled.then_some(self.host.mount_path.as_slice())
    }

    /// Return the configured bus sidecar scopes.
    pub(crate) fn sidecar_bus_scopes(&self) -> &[SidecarScope] {
        &self.sidecar_bus_scopes
    }

    /// Return the configured LoRa sidecar scopes.
    pub(crate) fn sidecar_lora_scopes(&self) -> &[SidecarScope] {
        &self.sidecar_lora_scopes
    }

    /// Return true if a bus scope is declared.
    pub(crate) fn bus_scope_exists(&self, scope: &str) -> bool {
        self.sidecar_bus_scopes
            .iter()
            .any(|entry| entry.scope() == scope)
    }

    /// Return true if a LoRa scope is declared.
    pub(crate) fn lora_scope_exists(&self, scope: &str) -> bool {
        self.sidecar_lora_scopes
            .iter()
            .any(|entry| entry.scope() == scope)
    }

    /// Return the sidecar kind if the path is within a sidecar mount.
    pub(crate) fn sidecar_kind_for_path(&self, path: &[String]) -> Option<SidecarKind> {
        if self.sidecar_modbus.matches_path(path) || self.sidecar_dnp3.matches_path(path) {
            return Some(SidecarKind::Bus);
        }
        if self.sidecar_lora.matches_path(path) {
            return Some(SidecarKind::Lora);
        }
        None
    }

    /// Return true when policy namespaces are enabled.
    #[allow(dead_code)]
    pub fn policy_enabled(&self) -> bool {
        self.policy.enabled
    }

    /// Read bytes from the supplied path.
    pub fn read(
        &mut self,
        path: &[String],
        offset: u64,
        count: u32,
    ) -> Result<Vec<u8>, NineDoorError> {
        enum ReadAction {
            Data(Vec<u8>, Option<TelemetryAudit>),
            TraceControl,
            TraceEvents,
            KernelMessages,
            TaskTrace(String),
            CasManifest(String),
            CasChunk([u8; 32]),
            CasModel { digest: [u8; 32], kind: ModelFileKind },
        }

        let retain_on_boot = self.telemetry.cursor.retain_on_boot;
        if let Some(data) = self.read_sidecar(path, offset, count)? {
            return Ok(data);
        }
        let worker_id = self
            .shards
            .worker_id_from_telemetry_path(path)
            .map(str::to_owned);
        let mut audit = None;
        let mut manifest_snapshot = None;
        let action = {
            if self.shards.legacy_worker_alias_enabled()
                && matches!(path, [single] if single == "worker")
            {
                let listing = render_directory_listing(self.worker_alias_listing());
                return Ok(read_slice(&listing, offset, count));
            }
            if let Some(listing) = self.cas_directory_listing(path)? {
                let listing = render_directory_listing(listing);
                return Ok(read_slice(&listing, offset, count));
            }
            let node = self.lookup_mut(path)?;
            match node.node.kind_mut() {
                NodeKind::Directory { .. } => {
                    let listing = render_directory_listing(node.list_children());
                    ReadAction::Data(read_slice(&listing, offset, count), None)
                }
                NodeKind::File(FileNode::ReadOnly(data))
                | NodeKind::File(FileNode::AppendOnly(data)) => {
                    ReadAction::Data(read_slice(data, offset, count), None)
                }
                NodeKind::File(FileNode::Telemetry(file)) => match file.read(offset, count) {
                    Ok(outcome) => {
                        if retain_on_boot && worker_id.is_some() {
                            manifest_snapshot = Some(file.snapshot());
                        }
                        ReadAction::Data(outcome.data, outcome.audit)
                    }
                    Err(err) => {
                        if let Some(audit) = err.audit {
                            self.record_telemetry_audit(audit)?;
                        }
                        return Err(NineDoorError::protocol(ErrorCode::Invalid, err.message));
                    }
                },
                NodeKind::File(FileNode::TraceControl) => ReadAction::TraceControl,
                NodeKind::File(FileNode::TraceEvents) => ReadAction::TraceEvents,
                NodeKind::File(FileNode::KernelMessages) => ReadAction::KernelMessages,
                NodeKind::File(FileNode::TaskTrace(task)) => ReadAction::TaskTrace(task.clone()),
                NodeKind::File(FileNode::CasManifest { epoch }) => {
                    ReadAction::CasManifest(epoch.clone())
                }
                NodeKind::File(FileNode::CasChunk { digest, .. }) => ReadAction::CasChunk(*digest),
                NodeKind::File(FileNode::CasModel { digest, kind }) => ReadAction::CasModel {
                    digest: *digest,
                    kind: *kind,
                },
            }
        };
        let data = match action {
            ReadAction::Data(data, read_audit) => {
                audit = read_audit;
                data
            }
            ReadAction::TraceControl => self.trace.read_ctl(offset, count),
            ReadAction::TraceEvents => self.trace.read_events(offset, count),
            ReadAction::KernelMessages => self.trace.read_kmesg(offset, count),
            ReadAction::TaskTrace(task) => self.trace.read_task(&task, offset, count),
            ReadAction::CasManifest(epoch) => self.cas.read_manifest(&epoch, offset, count)?,
            ReadAction::CasChunk(digest) => self.cas.read_chunk(&digest, offset, count)?,
            ReadAction::CasModel { digest, kind } => {
                self.cas.read_model_file(&digest, kind, offset, count)?
            }
        };
        if let Some(audit) = audit {
            self.record_telemetry_audit(audit)?;
        }
        if retain_on_boot {
            if let (Some(worker_id), Some(snapshot)) = (worker_id, manifest_snapshot) {
                self.telemetry_manifest
                    .persist_snapshot(&worker_id, snapshot);
            }
        }
        Ok(data)
    }

    /// Append bytes to the supplied path.
    pub fn write_append(
        &mut self,
        path: &[String],
        offset: u64,
        data: &[u8],
    ) -> Result<u32, NineDoorError> {
        enum WriteAction {
            Result(Result<u32, NineDoorError>),
            CasManifest(String),
            CasChunk { epoch: String, digest: [u8; 32] },
            CasModel { digest: [u8; 32], kind: ModelFileKind },
        }

        let retain_on_boot = self.telemetry.cursor.retain_on_boot;
        if let Some(count) = self.write_sidecar(path, offset, data)? {
            return Ok(count);
        }
        if self.telemetry_ingest.enabled() {
            if let Some(device_id) = telemetry_ingest_ctl_device(path) {
                return self.write_telemetry_ingest_ctl(device_id, data);
            }
            if let Some((device_id, seg_id)) = telemetry_ingest_segment_parts(path) {
                return self.write_telemetry_ingest_segment(device_id, seg_id, offset, data);
            }
        }
        let worker_id = self
            .shards
            .worker_id_from_telemetry_path(path)
            .map(str::to_owned);
        let mut audit = None;
        let mut manifest_snapshot = None;
        let action = {
            let node = self.lookup_mut(path)?;
            match node.node.kind_mut() {
                NodeKind::File(FileNode::AppendOnly(buffer)) => {
                    buffer.extend_from_slice(data);
                    WriteAction::Result(Ok(data.len() as u32))
                }
                NodeKind::File(FileNode::Telemetry(file)) => match file.append(offset, data) {
                    Ok(outcome) => {
                        audit = outcome.audit;
                        if retain_on_boot && worker_id.is_some() {
                            manifest_snapshot = Some(file.snapshot());
                        }
                        WriteAction::Result(Ok(outcome.count))
                    }
                    Err(err) => {
                        audit = err.audit;
                        let code = match err.kind {
                            super::telemetry::TelemetryErrorKind::QuotaExceeded => {
                                ErrorCode::TooBig
                            }
                            super::telemetry::TelemetryErrorKind::InvalidOffset
                            | super::telemetry::TelemetryErrorKind::InvalidFrame
                            | super::telemetry::TelemetryErrorKind::CursorStale => {
                                ErrorCode::Invalid
                            }
                        };
                        WriteAction::Result(Err(NineDoorError::protocol(code, err.message)))
                    }
                },
                NodeKind::File(FileNode::ReadOnly(_)) => WriteAction::Result(Err(
                    NineDoorError::protocol(
                        ErrorCode::Permission,
                        format!("cannot write read-only file /{}", join_path(path)),
                    ),
                )),
                NodeKind::File(FileNode::TraceControl) => {
                    WriteAction::Result(self.trace.write_ctl(data))
                }
                NodeKind::File(FileNode::TraceEvents)
                | NodeKind::File(FileNode::KernelMessages)
                | NodeKind::File(FileNode::TaskTrace(_)) => WriteAction::Result(Err(
                    NineDoorError::protocol(
                        ErrorCode::Permission,
                        format!("cannot write read-only file /{}", join_path(path)),
                    ),
                )),
                NodeKind::File(FileNode::CasManifest { epoch }) => {
                    WriteAction::CasManifest(epoch.clone())
                }
                NodeKind::File(FileNode::CasChunk { epoch, digest }) => WriteAction::CasChunk {
                    epoch: epoch.clone(),
                    digest: *digest,
                },
                NodeKind::File(FileNode::CasModel { digest, kind }) => WriteAction::CasModel {
                    digest: *digest,
                    kind: *kind,
                },
                NodeKind::Directory { .. } => WriteAction::Result(Err(NineDoorError::protocol(
                    ErrorCode::Permission,
                    format!("cannot write directory /{}", join_path(path)),
                ))),
            }
        };
        let update_epoch = match &action {
            WriteAction::CasManifest(epoch) => Some(epoch.clone()),
            WriteAction::CasChunk { epoch, .. } => Some(epoch.clone()),
            _ => None,
        };
        let result = match action {
            WriteAction::Result(result) => result,
            WriteAction::CasManifest(epoch) => {
                self.cas.append_manifest(epoch.as_str(), offset, data)
            }
            WriteAction::CasChunk { epoch, digest } => {
                self.cas.append_chunk(epoch.as_str(), &digest, offset, data)
            }
            WriteAction::CasModel { digest, kind } => {
                self.cas.append_model_file(&digest, kind, offset, data)
            }
        };
        if let Some(epoch) = update_epoch {
            if let Err(err) = self.refresh_update_status(&epoch) {
                if result.is_ok() {
                    return Err(err);
                }
            }
        }
        if let Some(audit) = audit {
            self.record_telemetry_audit(audit)?;
        }
        if retain_on_boot {
            if let (Some(worker_id), Some(snapshot)) = (worker_id, manifest_snapshot) {
                self.telemetry_manifest
                    .persist_snapshot(&worker_id, snapshot);
            }
        }
        self.flush_cas_events()?;
        result
    }

    fn read_sidecar(
        &mut self,
        path: &[String],
        offset: u64,
        count: u32,
    ) -> Result<Option<Vec<u8>>, NineDoorError> {
        if let Some(data) = self.sidecar_modbus.read(path, offset, count) {
            return Ok(Some(data));
        }
        if let Some(data) = self.sidecar_dnp3.read(path, offset, count) {
            return Ok(Some(data));
        }
        if let Some(data) = self.sidecar_lora.read(path, offset, count) {
            return Ok(Some(data));
        }
        Ok(None)
    }

    fn write_sidecar(
        &mut self,
        path: &[String],
        offset: u64,
        data: &[u8],
    ) -> Result<Option<u32>, NineDoorError> {
        let max_log_bytes = MAX_MSIZE as usize;
        if let Some(count) = self.sidecar_modbus.write(path, offset, data, max_log_bytes)? {
            return Ok(Some(count));
        }
        if let Some(count) = self.sidecar_dnp3.write(path, offset, data, max_log_bytes)? {
            return Ok(Some(count));
        }
        if let Some(count) = self.sidecar_lora.write(path, offset, data, max_log_bytes)? {
            return Ok(Some(count));
        }
        Ok(None)
    }

    /// Create namespace entries for a spawned worker.
    pub fn create_worker(&mut self, worker_id: &str) -> Result<(), NineDoorError> {
        if worker_id.is_empty() || worker_id.contains('/') {
            return Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                format!("invalid worker id '{worker_id}'"),
            ));
        }
        let telemetry_config = self.telemetry;
        let worker_parent = self.shards.worker_parent(worker_id);
        {
            let node = self.lookup_mut(&worker_parent)?;
            if node.has_child(worker_id) {
                return Err(NineDoorError::protocol(
                    ErrorCode::Busy,
                    format!("worker {worker_id} already exists"),
                ));
            }
        }
        let telemetry_file = if telemetry_config.cursor.retain_on_boot {
            self.telemetry_manifest
                .restore_file(worker_id, telemetry_config)
                .unwrap_or_else(|| TelemetryFile::new(telemetry_config))
        } else {
            self.telemetry_manifest.clear_worker(worker_id);
            TelemetryFile::new(telemetry_config)
        };
        let mut node = self.lookup_mut(&worker_parent)?;
        let worker_dir = node.ensure_directory(worker_id);
        worker_dir.ensure_file("telemetry", FileNode::Telemetry(telemetry_file));
        self.worker_ids.insert(worker_id.to_owned());
        let proc_root = vec!["proc".to_owned()];
        self.ensure_dir(&proc_root, worker_id)?;
        let proc_path = vec!["proc".to_owned(), worker_id.to_owned()];
        let mut proc_node = self.lookup_mut(&proc_path)?;
        proc_node.ensure_file("trace", FileNode::TaskTrace(worker_id.to_owned()));
        Ok(())
    }

    /// Remove namespace entries for a killed worker.
    pub fn remove_worker(&mut self, worker_id: &str) -> Result<(), NineDoorError> {
        if !self.worker_ids.remove(worker_id) {
            return Err(NineDoorError::protocol(
                ErrorCode::NotFound,
                format!("worker {worker_id} not found"),
            ));
        }
        let worker_parent = self.shards.worker_parent(worker_id);
        let mut node = self.lookup_mut(&worker_parent)?;
        let _ = node.remove_child(worker_id);
        let proc_root = vec!["proc".to_owned()];
        if let Ok(mut proc_dir) = self.lookup_mut(&proc_root) {
            let _ = proc_dir.remove_child(worker_id);
        }
        Ok(())
    }

    /// Borrow the trace filesystem for mutation.
    pub fn tracefs_mut(&mut self) -> &mut TraceFs {
        &mut self.trace
    }

    /// Emit an audit notice through the telemetry audit sink.
    pub fn emit_audit_notice(
        &mut self,
        level: TelemetryAuditLevel,
        message: impl Into<String>,
    ) -> Result<(), NineDoorError> {
        self.record_telemetry_audit(TelemetryAudit::audit_notice(level, message))
    }

    fn record_telemetry_audit(&mut self, audit: TelemetryAudit) -> Result<(), NineDoorError> {
        let level = match audit.level {
            TelemetryAuditLevel::Info => TraceLevel::Info,
            TelemetryAuditLevel::Warn => TraceLevel::Warn,
        };
        self.trace.record(level, "telemetry", None, &audit.message);
        let log_path = vec!["log".to_owned(), "queen.log".to_owned()];
        let log_node = self.lookup_mut(&log_path)?;
        let line = format!("[audit] {}\n", audit.message);
        match log_node.node.kind_mut() {
            NodeKind::File(FileNode::AppendOnly(buffer)) => {
                buffer.extend_from_slice(line.as_bytes());
                Ok(())
            }
            _ => Err(NineDoorError::protocol(
                ErrorCode::Permission,
                "cannot append telemetry audit line",
            )),
        }
    }

    fn worker_alias_listing(&self) -> Vec<String> {
        self.worker_ids.iter().cloned().collect()
    }

    fn resolve_path(&self, path: &[String]) -> Vec<String> {
        if self.shards.legacy_worker_alias_enabled() {
            if let [first, worker_id, rest @ ..] = path {
                if first == "worker" {
                    let mut resolved = self.shards.worker_root(worker_id);
                    resolved.extend_from_slice(rest);
                    return resolved;
                }
            }
        }
        path.to_vec()
    }

    /// Lookup a node by path.
    pub fn lookup(&mut self, path: &[String]) -> Result<NodeView<'_>, NineDoorError> {
        let resolved = self.resolve_path(path);
        self.ensure_telemetry_ingest_path(&resolved)?;
        self.ensure_cas_path(&resolved)?;
        let mut node = &self.root;
        for component in &resolved {
            node = node.child(component).ok_or_else(|| {
                NineDoorError::protocol(
                    ErrorCode::NotFound,
                    format!("path /{} not found", join_path(path)),
                )
            })?;
        }
        Ok(NodeView { node })
    }

    fn lookup_mut(&mut self, path: &[String]) -> Result<NodeViewMut<'_>, NineDoorError> {
        let resolved = self.resolve_path(path);
        self.ensure_telemetry_ingest_path(&resolved)?;
        self.ensure_cas_path(&resolved)?;
        self.lookup_mut_raw(&resolved)
    }

    fn lookup_mut_raw(&mut self, path: &[String]) -> Result<NodeViewMut<'_>, NineDoorError> {
        let mut node = &mut self.root;
        for component in path {
            node = node.child_mut(component).ok_or_else(|| {
                NineDoorError::protocol(
                    ErrorCode::NotFound,
                    format!("path /{} not found", join_path(path)),
                )
            })?;
        }
        Ok(NodeViewMut { node })
    }

    fn cas_directory_listing(
        &mut self,
        path: &[String],
    ) -> Result<Option<Vec<String>>, NineDoorError> {
        let Some(cas_path) = parse_cas_path(path)? else {
            return Ok(None);
        };
        match cas_path {
            CasPath::UpdatesRoot => {
                self.ensure_cas_path(path)?;
                Ok(Some(self.cas.list_updates()))
            }
            CasPath::UpdateEpoch { .. } => {
                self.ensure_cas_path(path)?;
                let mut entries = vec!["chunks".to_owned()];
                if self.ui.updates.manifest {
                    entries.push("manifest.cbor".to_owned());
                }
                if self.ui.updates.status {
                    entries.push("status".to_owned());
                    entries.push("status.cbor".to_owned());
                }
                Ok(Some(entries))
            }
            CasPath::UpdateChunks { epoch } => {
                self.ensure_cas_path(path)?;
                Ok(Some(self.cas.list_update_chunks(&epoch)))
            }
            CasPath::ModelsRoot => {
                self.ensure_cas_path(path)?;
                Ok(Some(self.cas.list_models()))
            }
            CasPath::ModelRoot { digest } => {
                self.ensure_cas_path(path)?;
                Ok(Some(self.cas.list_model_entries(&digest)))
            }
            _ => Ok(None),
        }
    }

    fn ensure_telemetry_ingest_path(&mut self, path: &[String]) -> Result<(), NineDoorError> {
        if !self.telemetry_ingest.enabled() {
            return Ok(());
        }
        if let Some(device_id) = telemetry_ingest_device_root(path) {
            self.ensure_telemetry_ingest_device(device_id)?;
        }
        Ok(())
    }

    fn ensure_cas_path(&mut self, path: &[String]) -> Result<(), NineDoorError> {
        let Some(cas_path) = parse_cas_path(path)? else {
            return Ok(());
        };
        match cas_path {
            CasPath::UpdatesRoot => {
                if !self.cas.enabled() {
                    return Err(NineDoorError::protocol(ErrorCode::NotFound, "cas disabled"));
                }
                self.ensure_dir_raw(&[], "updates")?;
            }
            CasPath::UpdateEpoch { epoch }
            | CasPath::UpdateManifest { epoch }
            | CasPath::UpdateStatus { epoch, .. }
            | CasPath::UpdateChunks { epoch } => {
                self.cas.ensure_update(&epoch)?;
                self.ensure_cas_update_nodes(&epoch)?;
            }
            CasPath::UpdateChunk { epoch, digest } => {
                self.cas.ensure_update(&epoch)?;
                self.ensure_cas_update_nodes(&epoch)?;
                self.ensure_cas_chunk_node(&epoch, &digest)?;
            }
            CasPath::ModelsRoot => {
                if !self.cas.models_enabled() {
                    return Err(NineDoorError::protocol(
                        ErrorCode::NotFound,
                        "models disabled",
                    ));
                }
                self.ensure_dir_raw(&[], "models")?;
            }
            CasPath::ModelRoot { digest } | CasPath::ModelFile { digest, .. } => {
                self.cas.ensure_model_entry(&digest)?;
                self.ensure_cas_model_nodes(&digest)?;
            }
        }
        Ok(())
    }

    fn ensure_cas_update_nodes(&mut self, epoch: &str) -> Result<(), NineDoorError> {
        self.ensure_dir_raw(&[], "updates")?;
        let updates_root = vec!["updates".to_owned()];
        self.ensure_dir_raw(&updates_root, epoch)?;
        let update_path = vec!["updates".to_owned(), epoch.to_owned()];
        if self.ui.updates.manifest {
            self.ensure_file_raw(
                &update_path,
                "manifest.cbor",
                FileNode::CasManifest {
                    epoch: epoch.to_owned(),
                },
            )?;
        }
        if self.ui.updates.status {
            self.ensure_read_only_file(&update_path, "status", b"")?;
            self.ensure_read_only_file(&update_path, "status.cbor", b"")?;
        }
        self.ensure_dir_raw(&update_path, "chunks")?;
        Ok(())
    }

    fn ensure_cas_chunk_node(
        &mut self,
        epoch: &str,
        digest: &[u8; 32],
    ) -> Result<(), NineDoorError> {
        let update_path = vec!["updates".to_owned(), epoch.to_owned(), "chunks".to_owned()];
        self.ensure_file_raw(
            &update_path,
            &hex::encode(digest),
            FileNode::CasChunk {
                epoch: epoch.to_owned(),
                digest: *digest,
            },
        )?;
        Ok(())
    }

    fn ensure_cas_model_nodes(&mut self, digest: &[u8; 32]) -> Result<(), NineDoorError> {
        self.ensure_dir_raw(&[], "models")?;
        let models_root = vec!["models".to_owned()];
        let hex_digest = hex::encode(digest);
        self.ensure_dir_raw(&models_root, &hex_digest)?;
        let model_path = vec!["models".to_owned(), hex_digest];
        self.ensure_file_raw(
            &model_path,
            "weights",
            FileNode::CasModel {
                digest: *digest,
                kind: ModelFileKind::Weights,
            },
        )?;
        self.ensure_file_raw(
            &model_path,
            "schema",
            FileNode::CasModel {
                digest: *digest,
                kind: ModelFileKind::Schema,
            },
        )?;
        self.ensure_file_raw(
            &model_path,
            "signature",
            FileNode::CasModel {
                digest: *digest,
                kind: ModelFileKind::Signature,
            },
        )?;
        Ok(())
    }

    fn flush_cas_events(&mut self) -> Result<(), NineDoorError> {
        let events = self.cas.drain_events();
        if events.is_empty() {
            return Ok(());
        }
        for event in events {
            self.trace.record(event.level, "cas", None, &event.message);
            self.append_cas_log(&event.message)?;
        }
        Ok(())
    }

    fn refresh_update_status(&mut self, epoch: &str) -> Result<(), NineDoorError> {
        if !self.ui.updates.status || !self.cas.enabled() {
            return Ok(());
        }
        let payloads = self.cas.update_status_payloads(epoch)?;
        self.write_update_status_payloads(epoch, payloads)
    }

    fn write_update_status_payloads(
        &mut self,
        epoch: &str,
        payloads: UpdateStatusPayloads,
    ) -> Result<(), NineDoorError> {
        self.ensure_cas_update_nodes(epoch)?;
        self.set_update_status_payload(epoch, &payloads.text)?;
        self.set_update_status_cbor_payload(epoch, &payloads.cbor)?;
        Ok(())
    }

    fn append_cas_log(&mut self, message: &str) -> Result<(), NineDoorError> {
        let log_path = vec!["log".to_owned(), "queen.log".to_owned()];
        let log_node = self.lookup_mut(&log_path)?;
        let mut line = message.as_bytes().to_vec();
        line.push(b'\n');
        match log_node.node.kind_mut() {
            NodeKind::File(FileNode::AppendOnly(buffer)) => {
                buffer.extend_from_slice(&line);
                Ok(())
            }
            _ => Err(NineDoorError::protocol(
                ErrorCode::Permission,
                "cannot append cas log line",
            )),
        }
    }

    fn bootstrap(&mut self) {
        self.ensure_dir(&[], "proc").expect("create /proc");
        let proc_path = vec!["proc".to_owned()];
        let boot_text = b"Cohesix boot: root-task online\nspawned user-component endpoint 1\ntick 1\nPING 1\nPONG 1\ntick 2\ntick 3\nroot-task shutdown\n".to_vec();
        self.ensure_read_only_file(&proc_path, "boot", &boot_text)
            .expect("create /proc/boot");
        self.ensure_dir(&proc_path, "tests")
            .expect("create /proc/tests");
        let tests_path = vec!["proc".to_owned(), "tests".to_owned()];
        self.ensure_read_only_file(
            &tests_path,
            "selftest_quick.coh",
            SELFTEST_QUICK_SCRIPT.as_bytes(),
        )
        .expect("create /proc/tests/selftest_quick.coh");
        self.ensure_read_only_file(
            &tests_path,
            "selftest_full.coh",
            SELFTEST_FULL_SCRIPT.as_bytes(),
        )
        .expect("create /proc/tests/selftest_full.coh");
        self.ensure_read_only_file(
            &tests_path,
            "selftest_negative.coh",
            SELFTEST_NEGATIVE_SCRIPT.as_bytes(),
        )
        .expect("create /proc/tests/selftest_negative.coh");

        self.ensure_dir(&[], "log").expect("create /log");
        let log_path = vec!["log".to_owned()];
        self.ensure_append_only_file(&log_path, "queen.log", &boot_text)
            .expect("create /log/queen.log");

        self.ensure_dir(&[], "queen").expect("create /queen");
        let queen_path = vec!["queen".to_owned()];
        self.ensure_append_only_file(&queen_path, "ctl", b"")
            .expect("create /queen/ctl");
        if self.telemetry_ingest.enabled() {
            self.ensure_dir(&queen_path, "telemetry")
                .expect("create /queen/telemetry");
        }
        if self.shards.is_enabled() {
            self.ensure_dir(&[], "shard").expect("create /shard");
            let shard_root = vec!["shard".to_owned()];
            for label in self.shards.shard_labels() {
                self.ensure_dir(&shard_root, &label)
                    .expect("create /shard/<id>");
                let shard_path = vec!["shard".to_owned(), label];
                self.ensure_dir(&shard_path, "worker")
                    .expect("create /shard/<id>/worker");
            }
            if self.shards.legacy_worker_alias_enabled() {
                self.ensure_dir(&[], "worker").expect("create /worker alias");
            }
        } else {
            self.ensure_dir(&[], "worker").expect("create /worker");
        }
        if self.cas.enabled() {
            self.ensure_dir(&[], "updates").expect("create /updates");
            if self.cas.models_enabled() {
                self.ensure_dir(&[], "models").expect("create /models");
            }
        }
        self.ensure_dir(&[], "gpu").expect("create /gpu");
        self.ensure_dir(&[], "trace").expect("create /trace");
        let trace_path = vec!["trace".to_owned()];
        self.ensure_trace_control(&trace_path, "ctl")
            .expect("create /trace/ctl");
        self.ensure_trace_events(&trace_path, "events")
            .expect("create /trace/events");
        self.ensure_kernel_messages().expect("create /kmesg");
        self.bootstrap_sidecars()
            .expect("create /bus and /lora namespaces");
        if self.host.enabled {
            self.bootstrap_host().expect("create /host namespace");
        }
        if self.policy.enabled {
            self.bootstrap_policy()
                .expect("create /policy namespace");
        }
        if self.audit.enabled {
            self.bootstrap_audit()
                .expect("create /audit namespace");
        }
        if self.replay.enabled {
            self.bootstrap_replay()
                .expect("create /replay namespace");
        }
    }

    /// Install manifest-driven /proc observability nodes.
    pub fn install_observability(&mut self, config: ObserveConfig) -> Result<(), NineDoorError> {
        if !config.enabled() {
            return Ok(());
        }
        let proc_path = vec!["proc".to_owned()];
        if config.proc_9p.enabled() {
            self.ensure_dir(&proc_path, "9p")?;
            let proc_9p_path = vec!["proc".to_owned(), "9p".to_owned()];
            if config.proc_9p.sessions && self.ui.proc_9p.sessions {
                self.ensure_read_only_file(&proc_9p_path, "sessions", b"")?;
                self.ensure_read_only_file(&proc_9p_path, "sessions.cbor", b"")?;
            }
            if config.proc_9p.outstanding && self.ui.proc_9p.outstanding {
                self.ensure_read_only_file(&proc_9p_path, "outstanding", b"")?;
                self.ensure_read_only_file(&proc_9p_path, "outstanding.cbor", b"")?;
            }
            if config.proc_9p.short_writes && self.ui.proc_9p.short_writes {
                self.ensure_read_only_file(&proc_9p_path, "short_writes", b"")?;
                self.ensure_read_only_file(&proc_9p_path, "short_writes.cbor", b"")?;
            }
        }
        if config.proc_ingest.enabled() {
            self.ensure_dir(&proc_path, "ingest")?;
            let ingest_path = vec!["proc".to_owned(), "ingest".to_owned()];
            if config.proc_ingest.p50_ms && self.ui.proc_ingest.p50_ms {
                self.ensure_read_only_file(&ingest_path, "p50_ms", b"")?;
                self.ensure_read_only_file(&ingest_path, "p50_ms.cbor", b"")?;
            }
            if config.proc_ingest.p95_ms && self.ui.proc_ingest.p95_ms {
                self.ensure_read_only_file(&ingest_path, "p95_ms", b"")?;
                self.ensure_read_only_file(&ingest_path, "p95_ms.cbor", b"")?;
            }
            if config.proc_ingest.backpressure && self.ui.proc_ingest.backpressure {
                self.ensure_read_only_file(&ingest_path, "backpressure", b"")?;
                self.ensure_read_only_file(&ingest_path, "backpressure.cbor", b"")?;
            }
            if config.proc_ingest.dropped {
                self.ensure_read_only_file(&ingest_path, "dropped", b"")?;
            }
            if config.proc_ingest.queued {
                self.ensure_read_only_file(&ingest_path, "queued", b"")?;
            }
            if config.proc_ingest.watch {
                self.ensure_append_only_file(&ingest_path, "watch", b"")?;
            }
        }
        Ok(())
    }

    fn bootstrap_sidecars(&mut self) -> Result<(), NineDoorError> {
        let modbus = self.sidecar_modbus.clone();
        let dnp3 = self.sidecar_dnp3.clone();
        let lora = self.sidecar_lora.clone();
        modbus.bootstrap(self)?;
        dnp3.bootstrap(self)?;
        lora.bootstrap(self)?;
        Ok(())
    }

    fn bootstrap_host(&mut self) -> Result<(), NineDoorError> {
        let host_root = self.host.mount_path.clone();
        self.ensure_dir_path(&host_root)?;
        let providers = self.host.providers.clone();
        for provider in providers {
            match provider {
                HostProvider::Systemd => self.install_host_systemd(&host_root)?,
                HostProvider::K8s => self.install_host_k8s(&host_root)?,
                HostProvider::Nvidia => self.install_host_nvidia(&host_root)?,
                HostProvider::Jetson => self.ensure_dir(&host_root, "jetson")?,
                HostProvider::Net => self.ensure_dir(&host_root, "net")?,
            }
        }
        Ok(())
    }

    fn bootstrap_policy(&mut self) -> Result<(), NineDoorError> {
        self.ensure_dir(&[], "policy")?;
        let policy_root = vec!["policy".to_owned()];
        let rules_snapshot = self.policy.rules_snapshot.clone();
        self.ensure_append_only_file(&policy_root, "ctl", b"")?;
        self.ensure_read_only_file(&policy_root, "rules", &rules_snapshot)?;
        if self.ui.policy_preflight.req || self.ui.policy_preflight.diff {
            self.ensure_dir(&policy_root, "preflight")?;
            let preflight_root = vec!["policy".to_owned(), "preflight".to_owned()];
            if self.ui.policy_preflight.req {
                self.ensure_read_only_file(&preflight_root, "req", b"")?;
                self.ensure_read_only_file(&preflight_root, "req.cbor", b"")?;
            }
            if self.ui.policy_preflight.diff {
                self.ensure_read_only_file(&preflight_root, "diff", b"")?;
                self.ensure_read_only_file(&preflight_root, "diff.cbor", b"")?;
            }
        }
        self.ensure_dir(&[], "actions")?;
        let actions_root = vec!["actions".to_owned()];
        self.ensure_append_only_file(&actions_root, "queue", b"")?;
        Ok(())
    }

    fn bootstrap_audit(&mut self) -> Result<(), NineDoorError> {
        self.ensure_dir(&[], "audit")?;
        let audit_root = vec!["audit".to_owned()];
        self.ensure_append_only_file(&audit_root, "journal", b"")?;
        self.ensure_append_only_file(&audit_root, "decisions", b"")?;
        let export_snapshot = self.audit.export_snapshot.clone();
        self.ensure_read_only_file(&audit_root, "export", &export_snapshot)?;
        Ok(())
    }

    fn bootstrap_replay(&mut self) -> Result<(), NineDoorError> {
        self.ensure_dir(&[], "replay")?;
        let replay_root = vec!["replay".to_owned()];
        self.ensure_append_only_file(&replay_root, "ctl", b"")?;
        let status_snapshot = self.replay.status_snapshot.clone();
        self.ensure_read_only_file(&replay_root, "status", &status_snapshot)?;
        Ok(())
    }

    fn install_host_systemd(&mut self, host_root: &[String]) -> Result<(), NineDoorError> {
        self.ensure_dir(host_root, "systemd")?;
        let systemd_root = {
            let mut path = host_root.to_vec();
            path.push("systemd".to_owned());
            path
        };
        for unit in ["cohesix-agent.service", "ssh.service"] {
            self.ensure_dir(&systemd_root, unit)?;
            let unit_path = {
                let mut path = systemd_root.clone();
                path.push(unit.to_owned());
                path
            };
            self.ensure_append_only_file(&unit_path, "status", b"active")?;
            self.ensure_append_only_file(&unit_path, "restart", b"")?;
        }
        Ok(())
    }

    fn install_host_k8s(&mut self, host_root: &[String]) -> Result<(), NineDoorError> {
        self.ensure_dir(host_root, "k8s")?;
        let k8s_root = {
            let mut path = host_root.to_vec();
            path.push("k8s".to_owned());
            path
        };
        self.ensure_dir(&k8s_root, "node")?;
        let nodes_root = {
            let mut path = k8s_root.clone();
            path.push("node".to_owned());
            path
        };
        for node in ["node-1"] {
            self.ensure_dir(&nodes_root, node)?;
            let node_path = {
                let mut path = nodes_root.clone();
                path.push(node.to_owned());
                path
            };
            self.ensure_append_only_file(&node_path, "cordon", b"")?;
            self.ensure_append_only_file(&node_path, "drain", b"")?;
        }
        Ok(())
    }

    fn install_host_nvidia(&mut self, host_root: &[String]) -> Result<(), NineDoorError> {
        self.ensure_dir(host_root, "nvidia")?;
        let nvidia_root = {
            let mut path = host_root.to_vec();
            path.push("nvidia".to_owned());
            path
        };
        self.ensure_dir(&nvidia_root, "gpu")?;
        let gpu_root = {
            let mut path = nvidia_root.clone();
            path.push("gpu".to_owned());
            path
        };
        for gpu in ["0"] {
            self.ensure_dir(&gpu_root, gpu)?;
            let gpu_path = {
                let mut path = gpu_root.clone();
                path.push(gpu.to_owned());
                path
            };
            self.ensure_append_only_file(&gpu_path, "status", b"ok")?;
            self.ensure_append_only_file(&gpu_path, "power_cap", b"")?;
            self.ensure_append_only_file(&gpu_path, "thermal", b"42C")?;
        }
        Ok(())
    }

    fn ensure_dir_path(&mut self, path: &[String]) -> Result<(), NineDoorError> {
        let mut current = Vec::new();
        for component in path {
            self.ensure_dir(&current, component)?;
            current.push(component.clone());
        }
        Ok(())
    }

    fn ensure_dir_raw(&mut self, parent: &[String], name: &str) -> Result<(), NineDoorError> {
        let mut node = self.lookup_mut_raw(parent)?;
        node.ensure_directory(name);
        Ok(())
    }

    fn ensure_file_raw(
        &mut self,
        parent: &[String],
        name: &str,
        file: FileNode,
    ) -> Result<(), NineDoorError> {
        let mut node = self.lookup_mut_raw(parent)?;
        node.ensure_file(name, file);
        Ok(())
    }

    fn ensure_dir(&mut self, parent: &[String], name: &str) -> Result<(), NineDoorError> {
        let mut node = self.lookup_mut(parent)?;
        node.ensure_directory(name);
        Ok(())
    }

    fn ensure_read_only_file(
        &mut self,
        parent: &[String],
        name: &str,
        data: &[u8],
    ) -> Result<(), NineDoorError> {
        let mut node = self.lookup_mut(parent)?;
        node.ensure_file(name, FileNode::ReadOnly(data.to_vec()));
        Ok(())
    }

    fn ensure_append_only_file(
        &mut self,
        parent: &[String],
        name: &str,
        data: &[u8],
    ) -> Result<(), NineDoorError> {
        let mut node = self.lookup_mut(parent)?;
        node.ensure_file(name, FileNode::AppendOnly(data.to_vec()));
        Ok(())
    }

    fn ensure_telemetry_ingest_device(&mut self, device_id: &str) -> Result<(), NineDoorError> {
        if !self.telemetry_ingest.enabled() {
            return Err(NineDoorError::protocol(
                ErrorCode::NotFound,
                "telemetry ingest disabled",
            ));
        }
        let queen_path = vec!["queen".to_owned()];
        let telemetry_root = vec!["queen".to_owned(), "telemetry".to_owned()];
        let device_path = vec![
            "queen".to_owned(),
            "telemetry".to_owned(),
            device_id.to_owned(),
        ];
        self.ensure_dir_raw(&[], "queen")?;
        self.ensure_dir_raw(&queen_path, "telemetry")?;
        self.ensure_dir_raw(&telemetry_root, device_id)?;
        self.ensure_file_raw(&device_path, "ctl", FileNode::AppendOnly(Vec::new()))?;
        self.ensure_dir_raw(&device_path, "seg")?;
        self.ensure_file_raw(&device_path, "latest", FileNode::ReadOnly(Vec::new()))?;
        self.telemetry_ingest.ensure_device(device_id);
        Ok(())
    }

    fn set_telemetry_ingest_latest(
        &mut self,
        device_id: &str,
        seg_id: &str,
    ) -> Result<(), NineDoorError> {
        let device_path = vec![
            "queen".to_owned(),
            "telemetry".to_owned(),
            device_id.to_owned(),
        ];
        let payload = format!("{seg_id}\n");
        self.set_read_only_file(&device_path, "latest", payload.as_bytes())
    }

    fn ensure_telemetry_ingest_segment(
        &mut self,
        device_id: &str,
        seg_id: &str,
    ) -> Result<(), NineDoorError> {
        let seg_root = vec![
            "queen".to_owned(),
            "telemetry".to_owned(),
            device_id.to_owned(),
            "seg".to_owned(),
        ];
        self.ensure_append_only_file(&seg_root, seg_id, b"")
    }

    fn remove_telemetry_ingest_segment(
        &mut self,
        device_id: &str,
        seg_id: &str,
    ) -> Result<(), NineDoorError> {
        let seg_root = vec![
            "queen".to_owned(),
            "telemetry".to_owned(),
            device_id.to_owned(),
            "seg".to_owned(),
        ];
        let mut node = self.lookup_mut(&seg_root)?;
        if node.remove_child(seg_id).is_some() {
            Ok(())
        } else {
            Err(NineDoorError::protocol(
                ErrorCode::NotFound,
                format!("telemetry segment {seg_id} not found"),
            ))
        }
    }

    fn write_telemetry_ingest_ctl(
        &mut self,
        device_id: &str,
        data: &[u8],
    ) -> Result<u32, NineDoorError> {
        self.ensure_telemetry_ingest_device(device_id)?;
        let ctl_path = vec![
            "queen".to_owned(),
            "telemetry".to_owned(),
            device_id.to_owned(),
            "ctl".to_owned(),
        ];
        {
            let node = self.lookup_mut(&ctl_path)?;
            match node.node.kind_mut() {
                NodeKind::File(FileNode::AppendOnly(buffer)) => buffer.extend_from_slice(data),
                _ => {
                    return Err(NineDoorError::protocol(
                        ErrorCode::Permission,
                        "telemetry ctl is not append-only",
                    ))
                }
            }
        }
        let text = std::str::from_utf8(data).map_err(|err| {
            NineDoorError::protocol(
                ErrorCode::Invalid,
                format!("telemetry ctl must be UTF-8: {err}"),
            )
        })?;
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let command = TelemetryCtlCommand::parse(trimmed)?;
            if let Some(mime) = command.mime.as_deref() {
                let mime = mime.trim();
                if mime.is_empty()
                    || mime.chars().any(|ch| ch == '\n' || ch == '\r' || ch == '\0')
                {
                    return Err(NineDoorError::protocol(
                        ErrorCode::Invalid,
                        "telemetry ctl mime is invalid",
                    ));
                }
            }
            if command.new != "segment" {
                return Err(NineDoorError::protocol(
                    ErrorCode::Invalid,
                    format!("unsupported telemetry ctl verb {}", command.new),
                ));
            }
            let outcome = self
                .telemetry_ingest
                .create_segment(device_id)
                .map_err(map_telemetry_ingest_error)?;
            for seg_id in &outcome.evicted {
                self.remove_telemetry_ingest_segment(device_id, seg_id)?;
            }
            self.ensure_telemetry_ingest_segment(device_id, &outcome.seg_id)?;
            self.set_telemetry_ingest_latest(device_id, &outcome.seg_id)?;
        }
        Ok(data.len() as u32)
    }

    fn write_telemetry_ingest_segment(
        &mut self,
        device_id: &str,
        seg_id: &str,
        offset: u64,
        data: &[u8],
    ) -> Result<u32, NineDoorError> {
        if data.len() > MAX_TELEMETRY_RECORD_BYTES {
            return Err(NineDoorError::protocol(
                ErrorCode::TooBig,
                format!(
                    "telemetry record exceeds max_record_bytes {}",
                    MAX_TELEMETRY_RECORD_BYTES
                ),
            ));
        }
        let seg_path = vec![
            "queen".to_owned(),
            "telemetry".to_owned(),
            device_id.to_owned(),
            "seg".to_owned(),
            seg_id.to_owned(),
        ];
        let max_bytes_per_segment = self.telemetry_ingest.config().max_bytes_per_segment;
        let (expected_offset, remaining) = {
            let node = self.lookup_mut(&seg_path)?;
            match node.node.kind_mut() {
                NodeKind::File(FileNode::AppendOnly(buffer)) => {
                    let remaining = max_bytes_per_segment.saturating_sub(buffer.len());
                    (buffer.len() as u64, remaining)
                }
                _ => {
                    return Err(NineDoorError::protocol(
                        ErrorCode::Permission,
                        format!("cannot write /{}", join_path(&seg_path)),
                    ))
                }
            }
        };
        let provided_offset = if offset == u64::MAX { expected_offset } else { offset };
        let bounds = append_only_write_bounds(expected_offset, provided_offset, remaining, data.len())
            .map_err(|err| {
                NineDoorError::protocol(
                    ErrorCode::Invalid,
                    format!("telemetry append offset rejected: {err}"),
                )
            })?;
        if bounds.short {
            return Err(NineDoorError::protocol(
                ErrorCode::TooBig,
                "telemetry segment quota exceeded",
            ));
        }
        let outcome = self
            .telemetry_ingest
            .append_record(device_id, seg_id, data.len())
            .map_err(map_telemetry_ingest_error)?;
        for evicted in &outcome.evicted {
            self.remove_telemetry_ingest_segment(device_id, evicted)?;
        }
        let node = self.lookup_mut(&seg_path)?;
        match node.node.kind_mut() {
            NodeKind::File(FileNode::AppendOnly(buffer)) => {
                buffer.extend_from_slice(data);
                Ok(data.len() as u32)
            }
            _ => Err(NineDoorError::protocol(
                ErrorCode::Permission,
                format!("cannot write /{}", join_path(&seg_path)),
            )),
        }
    }

    fn ensure_trace_control(&mut self, parent: &[String], name: &str) -> Result<(), NineDoorError> {
        let mut node = self.lookup_mut(parent)?;
        node.ensure_file(name, FileNode::TraceControl);
        Ok(())
    }

    fn ensure_trace_events(&mut self, parent: &[String], name: &str) -> Result<(), NineDoorError> {
        let mut node = self.lookup_mut(parent)?;
        node.ensure_file(name, FileNode::TraceEvents);
        Ok(())
    }

    fn ensure_kernel_messages(&mut self) -> Result<(), NineDoorError> {
        let mut node = self.lookup_mut(&[])?;
        node.ensure_file("kmesg", FileNode::KernelMessages);
        Ok(())
    }

    pub fn set_gpu_node(
        &mut self,
        gpu_id: &str,
        info: &[u8],
        ctl: &[u8],
        status: &[u8],
    ) -> Result<(), NineDoorError> {
        let root = vec!["gpu".to_owned()];
        let base = vec!["gpu".to_owned(), gpu_id.to_owned()];
        self.ensure_dir(&root, gpu_id)?;
        self.set_read_only_file(&base, "info", info)?;
        self.set_append_only_file(&base, "ctl", ctl)?;
        self.set_append_only_file(&base, "status", status)?;
        self.set_append_only_file(&base, "job", b"")?;
        Ok(())
    }

    /// Install the GPU model lifecycle namespace.
    pub fn set_gpu_models(&mut self, catalog: &GpuModelCatalog) -> Result<(), NineDoorError> {
        let root = vec!["gpu".to_owned()];
        self.ensure_dir(&root, "models")?;
        let models_root = vec!["gpu".to_owned(), "models".to_owned()];
        self.ensure_dir(&models_root, "available")?;
        let available_root = vec![
            "gpu".to_owned(),
            "models".to_owned(),
            "available".to_owned(),
        ];
        for manifest in &catalog.available {
            self.ensure_dir(&available_root, &manifest.model_id)?;
            let model_path = vec![
                "gpu".to_owned(),
                "models".to_owned(),
                "available".to_owned(),
                manifest.model_id.clone(),
            ];
            self.set_read_only_file(
                &model_path,
                "manifest.toml",
                manifest.manifest_toml.as_bytes(),
            )?;
        }
        self.set_append_only_file(
            &models_root,
            "active",
            catalog.active_pointer_payload().as_bytes(),
        )?;
        Ok(())
    }

    /// Install the telemetry schema descriptor under `/gpu/telemetry/schema.json`.
    pub fn set_gpu_telemetry_schema(
        &mut self,
        schema: &TelemetrySchema,
    ) -> Result<(), NineDoorError> {
        let root = vec!["gpu".to_owned()];
        self.ensure_dir(&root, "telemetry")?;
        let telemetry_root = vec!["gpu".to_owned(), "telemetry".to_owned()];
        self.set_read_only_file(
            &telemetry_root,
            "schema.json",
            schema.descriptor_json().as_bytes(),
        )?;
        Ok(())
    }

    pub fn append_gpu_status(&mut self, gpu_id: &str, payload: &[u8]) -> Result<(), NineDoorError> {
        let path = vec!["gpu".to_owned(), gpu_id.to_owned(), "status".to_owned()];
        self.write_append(&path, u64::MAX, payload)?;
        Ok(())
    }

    /// Replace the `/proc/9p/sessions` contents.
    pub fn set_proc_sessions_payload(&mut self, data: &[u8]) -> Result<(), NineDoorError> {
        self.ensure_ui_provider_enabled(self.ui.proc_9p.sessions, "proc/9p/sessions")?;
        let parent = vec!["proc".to_owned(), "9p".to_owned()];
        self.set_read_only_file(&parent, "sessions", data)
    }

    /// Replace the `/proc/9p/sessions.cbor` contents.
    pub fn set_proc_sessions_cbor_payload(&mut self, data: &[u8]) -> Result<(), NineDoorError> {
        self.ensure_ui_provider_enabled(self.ui.proc_9p.sessions, "proc/9p/sessions.cbor")?;
        let parent = vec!["proc".to_owned(), "9p".to_owned()];
        self.set_read_only_file(&parent, "sessions.cbor", data)
    }

    /// Replace the `/proc/9p/outstanding` contents.
    pub fn set_proc_outstanding_payload(&mut self, data: &[u8]) -> Result<(), NineDoorError> {
        self.ensure_ui_provider_enabled(self.ui.proc_9p.outstanding, "proc/9p/outstanding")?;
        let parent = vec!["proc".to_owned(), "9p".to_owned()];
        self.set_read_only_file(&parent, "outstanding", data)
    }

    /// Replace the `/proc/9p/outstanding.cbor` contents.
    pub fn set_proc_outstanding_cbor_payload(&mut self, data: &[u8]) -> Result<(), NineDoorError> {
        self.ensure_ui_provider_enabled(
            self.ui.proc_9p.outstanding,
            "proc/9p/outstanding.cbor",
        )?;
        let parent = vec!["proc".to_owned(), "9p".to_owned()];
        self.set_read_only_file(&parent, "outstanding.cbor", data)
    }

    /// Replace the `/proc/9p/short_writes` contents.
    pub fn set_proc_short_writes_payload(&mut self, data: &[u8]) -> Result<(), NineDoorError> {
        self.ensure_ui_provider_enabled(self.ui.proc_9p.short_writes, "proc/9p/short_writes")?;
        let parent = vec!["proc".to_owned(), "9p".to_owned()];
        self.set_read_only_file(&parent, "short_writes", data)
    }

    /// Replace the `/proc/9p/short_writes.cbor` contents.
    pub fn set_proc_short_writes_cbor_payload(&mut self, data: &[u8]) -> Result<(), NineDoorError> {
        self.ensure_ui_provider_enabled(
            self.ui.proc_9p.short_writes,
            "proc/9p/short_writes.cbor",
        )?;
        let parent = vec!["proc".to_owned(), "9p".to_owned()];
        self.set_read_only_file(&parent, "short_writes.cbor", data)
    }

    /// Replace the `/proc/ingest/p50_ms` contents.
    pub fn set_proc_ingest_p50_payload(&mut self, data: &[u8]) -> Result<(), NineDoorError> {
        self.ensure_ui_provider_enabled(self.ui.proc_ingest.p50_ms, "proc/ingest/p50_ms")?;
        let parent = vec!["proc".to_owned(), "ingest".to_owned()];
        self.set_read_only_file(&parent, "p50_ms", data)
    }

    /// Replace the `/proc/ingest/p50_ms.cbor` contents.
    pub fn set_proc_ingest_p50_cbor_payload(&mut self, data: &[u8]) -> Result<(), NineDoorError> {
        self.ensure_ui_provider_enabled(
            self.ui.proc_ingest.p50_ms,
            "proc/ingest/p50_ms.cbor",
        )?;
        let parent = vec!["proc".to_owned(), "ingest".to_owned()];
        self.set_read_only_file(&parent, "p50_ms.cbor", data)
    }

    /// Replace the `/proc/ingest/p95_ms` contents.
    pub fn set_proc_ingest_p95_payload(&mut self, data: &[u8]) -> Result<(), NineDoorError> {
        self.ensure_ui_provider_enabled(self.ui.proc_ingest.p95_ms, "proc/ingest/p95_ms")?;
        let parent = vec!["proc".to_owned(), "ingest".to_owned()];
        self.set_read_only_file(&parent, "p95_ms", data)
    }

    /// Replace the `/proc/ingest/p95_ms.cbor` contents.
    pub fn set_proc_ingest_p95_cbor_payload(&mut self, data: &[u8]) -> Result<(), NineDoorError> {
        self.ensure_ui_provider_enabled(
            self.ui.proc_ingest.p95_ms,
            "proc/ingest/p95_ms.cbor",
        )?;
        let parent = vec!["proc".to_owned(), "ingest".to_owned()];
        self.set_read_only_file(&parent, "p95_ms.cbor", data)
    }

    /// Replace the `/proc/ingest/backpressure` contents.
    pub fn set_proc_ingest_backpressure_payload(
        &mut self,
        data: &[u8],
    ) -> Result<(), NineDoorError> {
        self.ensure_ui_provider_enabled(
            self.ui.proc_ingest.backpressure,
            "proc/ingest/backpressure",
        )?;
        let parent = vec!["proc".to_owned(), "ingest".to_owned()];
        self.set_read_only_file(&parent, "backpressure", data)
    }

    /// Replace the `/proc/ingest/backpressure.cbor` contents.
    pub fn set_proc_ingest_backpressure_cbor_payload(
        &mut self,
        data: &[u8],
    ) -> Result<(), NineDoorError> {
        self.ensure_ui_provider_enabled(
            self.ui.proc_ingest.backpressure,
            "proc/ingest/backpressure.cbor",
        )?;
        let parent = vec!["proc".to_owned(), "ingest".to_owned()];
        self.set_read_only_file(&parent, "backpressure.cbor", data)
    }

    /// Replace the `/proc/ingest/dropped` contents.
    pub fn set_proc_ingest_dropped_payload(&mut self, data: &[u8]) -> Result<(), NineDoorError> {
        let parent = vec!["proc".to_owned(), "ingest".to_owned()];
        self.set_read_only_file(&parent, "dropped", data)
    }

    /// Replace the `/proc/ingest/queued` contents.
    pub fn set_proc_ingest_queued_payload(&mut self, data: &[u8]) -> Result<(), NineDoorError> {
        let parent = vec!["proc".to_owned(), "ingest".to_owned()];
        self.set_read_only_file(&parent, "queued", data)
    }

    /// Replace the `/proc/ingest/watch` contents.
    pub fn set_proc_ingest_watch_payload(&mut self, data: &[u8]) -> Result<(), NineDoorError> {
        let parent = vec!["proc".to_owned(), "ingest".to_owned()];
        self.set_append_only_file(&parent, "watch", data)
    }

    /// Replace the `/policy/preflight/req` contents.
    pub fn set_policy_preflight_req_payload(&mut self, data: &[u8]) -> Result<(), NineDoorError> {
        self.ensure_ui_provider_enabled(self.ui.policy_preflight.req, "policy/preflight/req")?;
        let parent = vec!["policy".to_owned(), "preflight".to_owned()];
        self.set_read_only_file(&parent, "req", data)
    }

    /// Replace the `/policy/preflight/req.cbor` contents.
    pub fn set_policy_preflight_req_cbor_payload(
        &mut self,
        data: &[u8],
    ) -> Result<(), NineDoorError> {
        self.ensure_ui_provider_enabled(
            self.ui.policy_preflight.req,
            "policy/preflight/req.cbor",
        )?;
        let parent = vec!["policy".to_owned(), "preflight".to_owned()];
        self.set_read_only_file(&parent, "req.cbor", data)
    }

    /// Replace the `/policy/preflight/diff` contents.
    pub fn set_policy_preflight_diff_payload(&mut self, data: &[u8]) -> Result<(), NineDoorError> {
        self.ensure_ui_provider_enabled(self.ui.policy_preflight.diff, "policy/preflight/diff")?;
        let parent = vec!["policy".to_owned(), "preflight".to_owned()];
        self.set_read_only_file(&parent, "diff", data)
    }

    /// Replace the `/policy/preflight/diff.cbor` contents.
    pub fn set_policy_preflight_diff_cbor_payload(
        &mut self,
        data: &[u8],
    ) -> Result<(), NineDoorError> {
        self.ensure_ui_provider_enabled(
            self.ui.policy_preflight.diff,
            "policy/preflight/diff.cbor",
        )?;
        let parent = vec!["policy".to_owned(), "preflight".to_owned()];
        self.set_read_only_file(&parent, "diff.cbor", data)
    }

    /// Replace the `/updates/<epoch>/status` contents.
    pub fn set_update_status_payload(
        &mut self,
        epoch: &str,
        data: &[u8],
    ) -> Result<(), NineDoorError> {
        self.ensure_ui_provider_enabled(self.ui.updates.status, "updates/<epoch>/status")?;
        let parent = vec!["updates".to_owned(), epoch.to_owned()];
        self.set_read_only_file(&parent, "status", data)
    }

    /// Replace the `/updates/<epoch>/status.cbor` contents.
    pub fn set_update_status_cbor_payload(
        &mut self,
        epoch: &str,
        data: &[u8],
    ) -> Result<(), NineDoorError> {
        self.ensure_ui_provider_enabled(self.ui.updates.status, "updates/<epoch>/status.cbor")?;
        let parent = vec!["updates".to_owned(), epoch.to_owned()];
        self.set_read_only_file(&parent, "status.cbor", data)
    }

    fn set_read_only_file(
        &mut self,
        parent: &[String],
        name: &str,
        data: &[u8],
    ) -> Result<(), NineDoorError> {
        let mut node = self.lookup_mut(parent)?;
        node.remove_child(name);
        node.ensure_file(name, FileNode::ReadOnly(data.to_vec()));
        Ok(())
    }

    fn ensure_ui_provider_enabled(
        &self,
        enabled: bool,
        label: &str,
    ) -> Result<(), NineDoorError> {
        if !enabled {
            return Err(NineDoorError::protocol(
                ErrorCode::NotFound,
                format!("ui provider {label} disabled"),
            ));
        }
        Ok(())
    }

    fn set_append_only_file(
        &mut self,
        parent: &[String],
        name: &str,
        data: &[u8],
    ) -> Result<(), NineDoorError> {
        let mut node = self.lookup_mut(parent)?;
        node.remove_child(name);
        node.ensure_file(name, FileNode::AppendOnly(data.to_vec()));
        Ok(())
    }

    /// Replace the `/policy/ctl` contents.
    pub fn set_policy_ctl_payload(&mut self, data: &[u8]) -> Result<(), NineDoorError> {
        let parent = vec!["policy".to_owned()];
        self.set_append_only_file(&parent, "ctl", data)
    }

    /// Replace the `/actions/queue` contents.
    pub fn set_action_queue_payload(&mut self, data: &[u8]) -> Result<(), NineDoorError> {
        let parent = vec!["actions".to_owned()];
        self.set_append_only_file(&parent, "queue", data)
    }

    /// Replace the `/actions/<id>/status` contents.
    pub fn set_action_status_payload(
        &mut self,
        action_id: &str,
        data: &[u8],
    ) -> Result<(), NineDoorError> {
        let root = vec!["actions".to_owned()];
        self.ensure_dir(&root, action_id)?;
        let status_root = vec!["actions".to_owned(), action_id.to_owned()];
        self.set_read_only_file(&status_root, "status", data)
    }

    /// Replace the `/audit/journal` contents.
    pub fn set_audit_journal_payload(&mut self, data: &[u8]) -> Result<(), NineDoorError> {
        let parent = vec!["audit".to_owned()];
        self.set_append_only_file(&parent, "journal", data)
    }

    /// Replace the `/audit/decisions` contents.
    pub fn set_audit_decisions_payload(&mut self, data: &[u8]) -> Result<(), NineDoorError> {
        let parent = vec!["audit".to_owned()];
        self.set_append_only_file(&parent, "decisions", data)
    }

    /// Replace the `/audit/export` contents.
    pub fn set_audit_export_payload(&mut self, data: &[u8]) -> Result<(), NineDoorError> {
        let parent = vec!["audit".to_owned()];
        self.set_read_only_file(&parent, "export", data)
    }

    /// Replace the `/replay/ctl` contents.
    pub fn set_replay_ctl_payload(&mut self, data: &[u8]) -> Result<(), NineDoorError> {
        let parent = vec!["replay".to_owned()];
        self.set_append_only_file(&parent, "ctl", data)
    }

    /// Replace the `/replay/status` contents.
    pub fn set_replay_status_payload(&mut self, data: &[u8]) -> Result<(), NineDoorError> {
        let parent = vec!["replay".to_owned()];
        self.set_read_only_file(&parent, "status", data)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SidecarKind {
    Bus,
    Lora,
}

impl SidecarKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Bus => "bus",
            Self::Lora => "lora",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidecarBusFile {
    Ctl,
    Telemetry,
    Link,
    Replay,
    Spool,
}

#[derive(Debug, Clone)]
struct SidecarBusAdapterState {
    mount_root: Vec<String>,
    mount_label: String,
    scope: String,
    spool: OfflineSpool,
    link_state: LinkState,
    telemetry: Vec<u8>,
    ctl: Vec<u8>,
    link: Vec<u8>,
    replay: Vec<u8>,
}

impl SidecarBusAdapterState {
    fn match_file(&self, path: &[String]) -> Option<SidecarBusFile> {
        let rel = path.strip_prefix(self.mount_root.as_slice())?;
        match rel {
            [leaf] if leaf == "ctl" => Some(SidecarBusFile::Ctl),
            [leaf] if leaf == "telemetry" => Some(SidecarBusFile::Telemetry),
            [leaf] if leaf == "link" => Some(SidecarBusFile::Link),
            [leaf] if leaf == "replay" => Some(SidecarBusFile::Replay),
            [leaf] if leaf == "spool" => Some(SidecarBusFile::Spool),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
struct SidecarBusState {
    enabled: bool,
    mount_path: Vec<String>,
    adapters: Vec<SidecarBusAdapterState>,
}

impl SidecarBusState {
    fn new(config: SidecarBusConfig) -> Self {
        if !config.enabled {
            return Self {
                enabled: false,
                mount_path: config.mount_path,
                adapters: Vec::new(),
            };
        }
        let mut adapters = Vec::new();
        for adapter in config.adapters {
            let mut mount_root = config.mount_path.clone();
            mount_root.push(adapter.mount.clone());
            adapters.push(SidecarBusAdapterState {
                mount_root,
                mount_label: adapter.mount.clone(),
                scope: adapter.scope.clone(),
                spool: OfflineSpool::new(adapter.spool),
                link_state: LinkState::Offline,
                telemetry: Vec::new(),
                ctl: Vec::new(),
                link: Vec::new(),
                replay: Vec::new(),
            });
        }
        Self {
            enabled: true,
            mount_path: config.mount_path,
            adapters,
        }
    }

    #[allow(dead_code)]
    fn enabled(&self) -> bool {
        self.enabled
    }

    fn scopes(&self) -> Vec<SidecarScope> {
        self.adapters
            .iter()
            .map(|adapter| SidecarScope {
                scope: adapter.scope.clone(),
                mount_root: adapter.mount_root.clone(),
            })
            .collect()
    }

    fn matches_path(&self, path: &[String]) -> bool {
        if !self.enabled {
            return false;
        }
        self.adapters.iter().any(|adapter| {
            path.starts_with(&adapter.mount_root) || adapter.mount_root.starts_with(path)
        })
    }

    fn adapter_for_path(&self, path: &[String]) -> Option<(&SidecarBusAdapterState, SidecarBusFile)> {
        self.adapters
            .iter()
            .find_map(|adapter| adapter.match_file(path).map(|file| (adapter, file)))
    }

    fn adapter_for_path_mut(
        &mut self,
        path: &[String],
    ) -> Option<(&mut SidecarBusAdapterState, SidecarBusFile)> {
        self.adapters
            .iter_mut()
            .find_map(|adapter| adapter.match_file(path).map(|file| (adapter, file)))
    }

    fn bootstrap(&self, namespace: &mut Namespace) -> Result<(), NineDoorError> {
        if !self.enabled {
            return Ok(());
        }
        namespace.ensure_dir_path(&self.mount_path)?;
        for adapter in &self.adapters {
            let mut adapter_path = self.mount_path.clone();
            adapter_path.push(adapter.mount_label.clone());
            namespace.ensure_dir_path(&adapter_path)?;
            namespace.ensure_append_only_file(&adapter_path, "ctl", b"")?;
            namespace.ensure_append_only_file(&adapter_path, "telemetry", b"")?;
            namespace.ensure_append_only_file(&adapter_path, "link", b"")?;
            namespace.ensure_append_only_file(&adapter_path, "replay", b"")?;
            namespace.ensure_read_only_file(&adapter_path, "spool", b"")?;
        }
        Ok(())
    }

    fn read(&self, path: &[String], offset: u64, count: u32) -> Option<Vec<u8>> {
        let (adapter, file) = self.adapter_for_path(path)?;
        match file {
            SidecarBusFile::Ctl => Some(read_slice(&adapter.ctl, offset, count)),
            SidecarBusFile::Telemetry => Some(read_slice(&adapter.telemetry, offset, count)),
            SidecarBusFile::Link => Some(read_slice(&adapter.link, offset, count)),
            SidecarBusFile::Replay => Some(read_slice(&adapter.replay, offset, count)),
            SidecarBusFile::Spool => {
                let data = render_spool_status(&adapter.spool);
                Some(read_slice(&data, offset, count))
            }
        }
    }

    fn write(
        &mut self,
        path: &[String],
        offset: u64,
        data: &[u8],
        max_log_bytes: usize,
    ) -> Result<Option<u32>, NineDoorError> {
        let (adapter, file) = match self.adapter_for_path_mut(path) {
            Some(found) => found,
            None => return Ok(None),
        };
        if offset != u64::MAX {
            return Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                "sidecar writes must use append-only offsets",
            ));
        }
        match file {
            SidecarBusFile::Ctl => {
                let count = append_bounded(&mut adapter.ctl, data, max_log_bytes)?;
                Ok(Some(count))
            }
            SidecarBusFile::Link => {
                let text = std::str::from_utf8(data)
                    .map_err(|_| NineDoorError::protocol(ErrorCode::Invalid, "invalid link payload"))?
                    .trim();
                match text {
                    "online" => adapter.link_state = LinkState::Online,
                    "offline" => adapter.link_state = LinkState::Offline,
                    _ => {
                        return Err(NineDoorError::protocol(
                            ErrorCode::Invalid,
                            "link state must be 'online' or 'offline'",
                        ))
                    }
                }
                let count = append_bounded(&mut adapter.link, data, max_log_bytes)?;
                Ok(Some(count))
            }
            SidecarBusFile::Telemetry => match adapter.link_state {
                LinkState::Online => {
                    let count = append_bounded(&mut adapter.telemetry, data, max_log_bytes)?;
                    Ok(Some(count))
                }
                LinkState::Offline => match adapter.spool.push(data) {
                    Ok(_) => Ok(Some(data.len() as u32)),
                    Err(SpoolError::Full) => Err(NineDoorError::protocol(
                        ErrorCode::TooBig,
                        "sidecar spool full",
                    )),
                    Err(SpoolError::Oversize { .. }) => Err(NineDoorError::protocol(
                        ErrorCode::TooBig,
                        "sidecar payload exceeds spool limit",
                    )),
                },
            },
            SidecarBusFile::Replay => {
                let snapshot = adapter.spool.snapshot();
                let total_bytes: usize = snapshot.iter().map(|frame| frame.payload.len()).sum();
                if adapter
                    .telemetry
                    .len()
                    .saturating_add(total_bytes)
                    > max_log_bytes
                {
                    return Err(NineDoorError::protocol(
                        ErrorCode::TooBig,
                        "sidecar telemetry buffer full",
                    ));
                }
                let drained = adapter.spool.drain();
                for frame in drained {
                    adapter.telemetry.extend_from_slice(&frame.payload);
                }
                let summary = format!("replay entries={} bytes={}\n", snapshot.len(), total_bytes);
                let count = append_bounded(&mut adapter.replay, summary.as_bytes(), max_log_bytes)?;
                Ok(Some(count))
            }
            SidecarBusFile::Spool => Err(NineDoorError::protocol(
                ErrorCode::Permission,
                "sidecar spool is read-only",
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidecarLoraFile {
    Ctl,
    Telemetry,
    Tamper,
}

#[derive(Debug, Clone)]
struct SidecarLoraAdapterState {
    mount_root: Vec<String>,
    mount_label: String,
    scope: String,
    guard: DutyCycleGuard,
    tamper: TamperLog,
    telemetry: Vec<u8>,
    ctl: Vec<u8>,
}

impl SidecarLoraAdapterState {
    fn match_file(&self, path: &[String]) -> Option<SidecarLoraFile> {
        let rel = path.strip_prefix(self.mount_root.as_slice())?;
        match rel {
            [leaf] if leaf == "ctl" => Some(SidecarLoraFile::Ctl),
            [leaf] if leaf == "telemetry" => Some(SidecarLoraFile::Telemetry),
            [leaf] if leaf == "tamper" => Some(SidecarLoraFile::Tamper),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
struct SidecarLoraState {
    enabled: bool,
    mount_path: Vec<String>,
    adapters: Vec<SidecarLoraAdapterState>,
    clock_ms: u64,
}

impl SidecarLoraState {
    fn new(config: SidecarLoraConfig) -> Self {
        if !config.enabled {
            return Self {
                enabled: false,
                mount_path: config.mount_path,
                adapters: Vec::new(),
                clock_ms: 0,
            };
        }
        let mut adapters = Vec::new();
        for adapter in config.adapters {
            let mut mount_root = config.mount_path.clone();
            mount_root.push(adapter.mount.clone());
            adapters.push(SidecarLoraAdapterState {
                mount_root,
                mount_label: adapter.mount.clone(),
                scope: adapter.scope.clone(),
                guard: DutyCycleGuard::new(adapter.duty_cycle),
                tamper: TamperLog::new(adapter.tamper_log_max_entries),
                telemetry: Vec::new(),
                ctl: Vec::new(),
            });
        }
        Self {
            enabled: true,
            mount_path: config.mount_path,
            adapters,
            clock_ms: 0,
        }
    }

    #[allow(dead_code)]
    fn enabled(&self) -> bool {
        self.enabled
    }

    fn scopes(&self) -> Vec<SidecarScope> {
        self.adapters
            .iter()
            .map(|adapter| SidecarScope {
                scope: adapter.scope.clone(),
                mount_root: adapter.mount_root.clone(),
            })
            .collect()
    }

    fn matches_path(&self, path: &[String]) -> bool {
        if !self.enabled {
            return false;
        }
        self.adapters.iter().any(|adapter| {
            path.starts_with(&adapter.mount_root) || adapter.mount_root.starts_with(path)
        })
    }

    fn adapter_for_path(
        &self,
        path: &[String],
    ) -> Option<(&SidecarLoraAdapterState, SidecarLoraFile)> {
        self.adapters
            .iter()
            .find_map(|adapter| adapter.match_file(path).map(|file| (adapter, file)))
    }

    fn adapter_index_for_path(&self, path: &[String]) -> Option<(usize, SidecarLoraFile)> {
        self.adapters
            .iter()
            .enumerate()
            .find_map(|(idx, adapter)| adapter.match_file(path).map(|file| (idx, file)))
    }

    #[allow(dead_code)]
    fn adapter_for_path_mut(
        &mut self,
        path: &[String],
    ) -> Option<(&mut SidecarLoraAdapterState, SidecarLoraFile)> {
        self.adapters
            .iter_mut()
            .find_map(|adapter| adapter.match_file(path).map(|file| (adapter, file)))
    }

    fn bootstrap(&self, namespace: &mut Namespace) -> Result<(), NineDoorError> {
        if !self.enabled {
            return Ok(());
        }
        namespace.ensure_dir_path(&self.mount_path)?;
        for adapter in &self.adapters {
            let mut adapter_path = self.mount_path.clone();
            adapter_path.push(adapter.mount_label.clone());
            namespace.ensure_dir_path(&adapter_path)?;
            namespace.ensure_append_only_file(&adapter_path, "ctl", b"")?;
            namespace.ensure_append_only_file(&adapter_path, "telemetry", b"")?;
            namespace.ensure_read_only_file(&adapter_path, "tamper", b"")?;
        }
        Ok(())
    }

    fn read(&self, path: &[String], offset: u64, count: u32) -> Option<Vec<u8>> {
        let (adapter, file) = self.adapter_for_path(path)?;
        match file {
            SidecarLoraFile::Ctl => Some(read_slice(&adapter.ctl, offset, count)),
            SidecarLoraFile::Telemetry => Some(read_slice(&adapter.telemetry, offset, count)),
            SidecarLoraFile::Tamper => {
                let data = render_tamper_log(adapter.tamper.snapshot());
                Some(read_slice(&data, offset, count))
            }
        }
    }

    fn write(
        &mut self,
        path: &[String],
        offset: u64,
        data: &[u8],
        max_log_bytes: usize,
    ) -> Result<Option<u32>, NineDoorError> {
        let (index, file) = match self.adapter_index_for_path(path) {
            Some(found) => found,
            None => return Ok(None),
        };
        if offset != u64::MAX {
            return Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                "sidecar writes must use append-only offsets",
            ));
        }
        match file {
            SidecarLoraFile::Ctl => {
                let count = {
                    let adapter = &mut self.adapters[index];
                    append_bounded(&mut adapter.ctl, data, max_log_bytes)?
                };
                let now_ms = self.next_clock();
                let adapter = &mut self.adapters[index];
                match adapter.guard.attempt(now_ms, data.len() as u32) {
                    Ok(()) => {
                        append_bounded(&mut adapter.telemetry, data, max_log_bytes)?;
                        Ok(Some(count))
                    }
                    Err(reason) => {
                        adapter.tamper.push(TamperEntry {
                            timestamp_ms: now_ms,
                            reason,
                            payload_bytes: data.len() as u32,
                        });
                        let (code, message) = match reason {
                            TamperReason::PayloadOversize => {
                                (ErrorCode::TooBig, "lora payload exceeds max bytes")
                            }
                            TamperReason::DutyCycleExceeded => {
                                (ErrorCode::Busy, "lora duty cycle exceeded")
                            }
                        };
                        Err(NineDoorError::protocol(code, message))
                    }
                }
            }
            SidecarLoraFile::Telemetry => Err(NineDoorError::protocol(
                ErrorCode::Permission,
                "lora telemetry is read-only",
            )),
            SidecarLoraFile::Tamper => Err(NineDoorError::protocol(
                ErrorCode::Permission,
                "lora tamper log is read-only",
            )),
        }
    }

    fn next_clock(&mut self) -> u64 {
        self.clock_ms = self.clock_ms.saturating_add(1);
        self.clock_ms
    }
}

#[derive(Debug, Clone)]
enum CasPath {
    UpdatesRoot,
    UpdateEpoch { epoch: String },
    UpdateManifest { epoch: String },
    UpdateStatus { epoch: String, _variant: UiVariant },
    UpdateChunks { epoch: String },
    UpdateChunk { epoch: String, digest: [u8; 32] },
    ModelsRoot,
    ModelRoot { digest: [u8; 32] },
    ModelFile { digest: [u8; 32], _kind: ModelFileKind },
}

fn parse_cas_path(path: &[String]) -> Result<Option<CasPath>, NineDoorError> {
    match path {
        [root] if root == "updates" => return Ok(Some(CasPath::UpdatesRoot)),
        [root] if root == "models" => return Ok(Some(CasPath::ModelsRoot)),
        _ => {}
    }
    if let [root, epoch, rest @ ..] = path {
        if root == "updates" {
            validate_epoch(epoch)?;
            return Ok(match rest {
                [] => Some(CasPath::UpdateEpoch {
                    epoch: epoch.to_owned(),
                }),
                [leaf] if leaf == "manifest.cbor" => Some(CasPath::UpdateManifest {
                    epoch: epoch.to_owned(),
                }),
                [leaf] if leaf == "status" => Some(CasPath::UpdateStatus {
                    epoch: epoch.to_owned(),
                    _variant: UiVariant::Text,
                }),
                [leaf] if leaf == "status.cbor" => Some(CasPath::UpdateStatus {
                    epoch: epoch.to_owned(),
                    _variant: UiVariant::Cbor,
                }),
                [leaf] if leaf == "chunks" => Some(CasPath::UpdateChunks {
                    epoch: epoch.to_owned(),
                }),
                [leaf, digest] if leaf == "chunks" => Some(CasPath::UpdateChunk {
                    epoch: epoch.to_owned(),
                    digest: parse_sha256(digest)?,
                }),
                _ => None,
            });
        }
    }
    if let [root, digest, rest @ ..] = path {
        if root == "models" {
            let digest = parse_sha256(digest)?;
            return Ok(match rest {
                [] => Some(CasPath::ModelRoot { digest }),
                [leaf] if leaf == "weights" => Some(CasPath::ModelFile {
                    digest,
                    _kind: ModelFileKind::Weights,
                }),
                [leaf] if leaf == "schema" => Some(CasPath::ModelFile {
                    digest,
                    _kind: ModelFileKind::Schema,
                }),
                [leaf] if leaf == "signature" => Some(CasPath::ModelFile {
                    digest,
                    _kind: ModelFileKind::Signature,
                }),
                _ => None,
            });
        }
    }
    Ok(None)
}

#[derive(Debug, Clone)]
struct Node {
    path: Vec<String>,
    qid: Qid,
    kind: NodeKind,
}

impl Node {
    fn directory(path: Vec<String>) -> Self {
        Self {
            qid: Qid::new(QidType::DIRECTORY, 0, hash_path(&path)),
            path,
            kind: NodeKind::Directory {
                children: BTreeMap::new(),
            },
        }
    }

    fn file(path: Vec<String>, file: FileNode) -> Self {
        let ty = match file {
            FileNode::ReadOnly(_) => QidType::FILE,
            FileNode::AppendOnly(_) => QidType::APPEND_ONLY,
            FileNode::Telemetry(_) => QidType::APPEND_ONLY,
            FileNode::TraceControl => QidType::APPEND_ONLY,
            FileNode::TraceEvents | FileNode::KernelMessages | FileNode::TaskTrace(_) => {
                QidType::FILE
            }
            FileNode::CasManifest { .. }
            | FileNode::CasChunk { .. }
            | FileNode::CasModel { .. } => QidType::APPEND_ONLY,
        };
        Self {
            qid: Qid::new(ty, 0, hash_path(&path)),
            path,
            kind: NodeKind::File(file),
        }
    }

    fn child(&self, name: &str) -> Option<&Node> {
        match &self.kind {
            NodeKind::Directory { children } => children.get(name),
            NodeKind::File(_) => None,
        }
    }

    fn child_mut(&mut self, name: &str) -> Option<&mut Node> {
        match &mut self.kind {
            NodeKind::Directory { children } => children.get_mut(name),
            NodeKind::File(_) => None,
        }
    }

    fn remove_child(&mut self, name: &str) -> Option<Node> {
        match &mut self.kind {
            NodeKind::Directory { children } => children.remove(name),
            NodeKind::File(_) => None,
        }
    }

    fn ensure_directory(&mut self, name: &str) -> &mut Node {
        match &mut self.kind {
            NodeKind::Directory { children } => {
                let mut path = self.path.clone();
                path.push(name.to_owned());
                children
                    .entry(name.to_owned())
                    .or_insert_with(|| Node::directory(path))
            }
            NodeKind::File(_) => panic!("cannot create directory under file"),
        }
    }

    fn ensure_file(&mut self, name: &str, file: FileNode) -> &mut Node {
        match &mut self.kind {
            NodeKind::Directory { children } => {
                let mut path = self.path.clone();
                path.push(name.to_owned());
                children
                    .entry(name.to_owned())
                    .or_insert_with(|| Node::file(path, file.clone()))
            }
            NodeKind::File(_) => panic!("cannot create file under file"),
        }
    }

    fn qid(&self) -> Qid {
        self.qid
    }

    fn is_directory(&self) -> bool {
        matches!(self.kind, NodeKind::Directory { .. })
    }

    fn kind_mut(&mut self) -> &mut NodeKind {
        &mut self.kind
    }
}

#[derive(Debug, Clone)]
enum NodeKind {
    Directory { children: BTreeMap<String, Node> },
    File(FileNode),
}

#[derive(Debug, Clone)]
enum FileNode {
    ReadOnly(Vec<u8>),
    AppendOnly(Vec<u8>),
    Telemetry(TelemetryFile),
    TraceControl,
    TraceEvents,
    KernelMessages,
    TaskTrace(String),
    CasManifest { epoch: String },
    CasChunk { epoch: String, digest: [u8; 32] },
    CasModel { digest: [u8; 32], kind: ModelFileKind },
}

/// Borrowed node view used by callers.
pub struct NodeView<'a> {
    node: &'a Node,
}

impl<'a> NodeView<'a> {
    pub fn qid(&self) -> Qid {
        self.node.qid()
    }

    pub fn is_directory(&self) -> bool {
        self.node.is_directory()
    }
}

struct NodeViewMut<'a> {
    node: &'a mut Node,
}

impl<'a> NodeViewMut<'a> {
    fn ensure_directory(&mut self, name: &str) -> &mut Node {
        self.node.ensure_directory(name)
    }

    fn ensure_file(&mut self, name: &str, file: FileNode) -> &mut Node {
        self.node.ensure_file(name, file)
    }

    fn has_child(&self, name: &str) -> bool {
        self.node.child(name).is_some()
    }

    fn remove_child(&mut self, name: &str) -> Option<Node> {
        self.node.remove_child(name)
    }

    fn list_children(&self) -> Vec<String> {
        match &self.node.kind {
            NodeKind::Directory { children } => children.keys().cloned().collect(),
            NodeKind::File(_) => Vec::new(),
        }
    }
}

fn hash_path(path: &[String]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for component in path {
        component.hash(&mut hasher);
    }
    hasher.finish()
}

fn join_path(path: &[String]) -> String {
    if path.is_empty() {
        String::new()
    } else {
        path.join("/")
    }
}

fn map_telemetry_ingest_error(err: TelemetryIngestError) -> NineDoorError {
    let code = match err.kind {
        TelemetryIngestErrorKind::Disabled | TelemetryIngestErrorKind::SegmentMissing => {
            ErrorCode::NotFound
        }
        TelemetryIngestErrorKind::QuotaExceeded => ErrorCode::TooBig,
    };
    NineDoorError::protocol(code, err.message)
}

fn telemetry_ingest_ctl_device(path: &[String]) -> Option<&str> {
    match path {
        [first, second, device_id, leaf]
            if first == "queen" && second == "telemetry" && leaf == "ctl" =>
        {
            Some(device_id.as_str())
        }
        _ => None,
    }
}

fn telemetry_ingest_segment_parts(path: &[String]) -> Option<(&str, &str)> {
    match path {
        [first, second, device_id, seg_dir, seg_id]
            if first == "queen" && second == "telemetry" && seg_dir == "seg" =>
        {
            Some((device_id.as_str(), seg_id.as_str()))
        }
        _ => None,
    }
}

fn telemetry_ingest_device_root(path: &[String]) -> Option<&str> {
    match path {
        [first, second, device_id] if first == "queen" && second == "telemetry" => {
            Some(device_id.as_str())
        }
        [first, second, device_id, leaf]
            if first == "queen"
                && second == "telemetry"
                && matches!(leaf.as_str(), "ctl" | "latest" | "seg") =>
        {
            Some(device_id.as_str())
        }
        _ => None,
    }
}

fn parse_host_mount(mount_at: &str) -> Result<Vec<String>, NineDoorError> {
    let trimmed = mount_at.trim();
    if trimmed.is_empty() || !trimmed.starts_with('/') {
        return Err(NineDoorError::protocol(
            ErrorCode::Invalid,
            "host mount must be an absolute path",
        ));
    }
    let components: Vec<String> = trimmed
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(str::to_owned)
        .collect();
    if components.is_empty() {
        return Err(NineDoorError::protocol(
            ErrorCode::Invalid,
            "host mount must not be root",
        ));
    }
    for component in &components {
        if component == ".." {
            return Err(NineDoorError::protocol(
                ErrorCode::Invalid,
                "host mount contains disallowed '..'",
            ));
        }
    }
    Ok(components)
}

fn parse_sidecar_mount(label: &str, mount_at: &str) -> Result<Vec<String>, NineDoorError> {
    let trimmed = mount_at.trim();
    if trimmed.is_empty() || !trimmed.starts_with('/') {
        return Err(NineDoorError::protocol(
            ErrorCode::Invalid,
            format!("sidecar {label} mount must be an absolute path"),
        ));
    }
    let components: Vec<String> = trimmed
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(str::to_owned)
        .collect();
    if components.is_empty() {
        return Err(NineDoorError::protocol(
            ErrorCode::Invalid,
            format!("sidecar {label} mount must not be root"),
        ));
    }
    Ok(components)
}

fn append_bounded(
    buffer: &mut Vec<u8>,
    data: &[u8],
    max_bytes: usize,
) -> Result<u32, NineDoorError> {
    if buffer.len().saturating_add(data.len()) > max_bytes {
        return Err(NineDoorError::protocol(
            ErrorCode::TooBig,
            "sidecar buffer capacity exceeded",
        ));
    }
    buffer.extend_from_slice(data);
    Ok(data.len() as u32)
}

fn render_spool_status(spool: &OfflineSpool) -> Vec<u8> {
    let config = spool.config();
    let entries = spool.snapshot();
    let mut out = String::new();
    out.push_str(&format!(
        "entries={} bytes={} max_entries={} max_bytes={}\n",
        entries.len(),
        spool.buffered_bytes(),
        config.max_entries,
        config.max_bytes
    ));
    for frame in entries {
        let payload = String::from_utf8_lossy(&frame.payload);
        out.push_str(&format!(
            "seq={} bytes={} payload={}\n",
            frame.seq,
            frame.payload.len(),
            payload
        ));
    }
    out.into_bytes()
}

fn render_tamper_log(entries: Vec<TamperEntry>) -> Vec<u8> {
    let mut out = String::new();
    for entry in entries {
        let reason = match entry.reason {
            TamperReason::PayloadOversize => "payload-oversize",
            TamperReason::DutyCycleExceeded => "duty-cycle",
        };
        out.push_str(&format!(
            "tamper ts_ms={} reason={} bytes={}\n",
            entry.timestamp_ms, reason, entry.payload_bytes
        ));
    }
    out.into_bytes()
}

fn read_slice(data: &[u8], offset: u64, count: u32) -> Vec<u8> {
    let start = offset as usize;
    if start >= data.len() {
        return Vec::new();
    }
    let end = start.saturating_add(count as usize).min(data.len());
    data[start..end].to_vec()
}

fn render_directory_listing(entries: Vec<String>) -> Vec<u8> {
    if entries.is_empty() {
        return Vec::new();
    }
    let mut output = String::new();
    for (idx, entry) in entries.iter().enumerate() {
        if idx > 0 {
            output.push('\n');
        }
        output.push_str(entry);
    }
    output.push('\n');
    output.into_bytes()
}
