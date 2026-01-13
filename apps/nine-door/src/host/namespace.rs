// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Synthetic namespace builder backing the NineDoor Secure9P server.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

use gpu_bridge_host::{GpuModelCatalog, TelemetrySchema};
use secure9p_codec::{ErrorCode, Qid, QidType};
use trace_model::TraceLevel;

use super::telemetry::{
    TelemetryAudit, TelemetryAuditLevel, TelemetryConfig, TelemetryFile, TelemetryManifestStore,
};
use super::tracefs::TraceFs;
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
    telemetry: TelemetryConfig,
    telemetry_manifest: TelemetryManifestStore,
    host: HostNamespaceConfig,
    policy: PolicyNamespaceConfig,
    audit: AuditNamespaceConfig,
    replay: ReplayNamespaceConfig,
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
            TelemetryManifestStore::default(),
            HostNamespaceConfig::disabled(),
            PolicyNamespaceConfig::disabled(),
            AuditNamespaceConfig::disabled(),
            ReplayNamespaceConfig::disabled(),
        )
    }

    /// Construct the namespace with explicit telemetry configuration and manifest store.
    pub fn new_with_telemetry_and_manifest(
        telemetry: TelemetryConfig,
        telemetry_manifest: TelemetryManifestStore,
    ) -> Self {
        Self::new_with_telemetry_manifest_host_policy(
            telemetry,
            telemetry_manifest,
            HostNamespaceConfig::disabled(),
            PolicyNamespaceConfig::disabled(),
            AuditNamespaceConfig::disabled(),
            ReplayNamespaceConfig::disabled(),
        )
    }

    /// Construct the namespace with telemetry, manifest storage, host provider config, and policy.
    pub fn new_with_telemetry_manifest_host_policy(
        telemetry: TelemetryConfig,
        telemetry_manifest: TelemetryManifestStore,
        host: HostNamespaceConfig,
        policy: PolicyNamespaceConfig,
        audit: AuditNamespaceConfig,
        replay: ReplayNamespaceConfig,
    ) -> Self {
        let mut namespace = Self {
            root: Node::directory(Vec::new()),
            trace: TraceFs::new(),
            telemetry,
            telemetry_manifest,
            host,
            policy,
            audit,
            replay,
        };
        namespace.bootstrap();
        namespace
    }

    /// Construct the namespace with telemetry, manifest storage, and host provider config.
    pub fn new_with_telemetry_manifest_and_host(
        telemetry: TelemetryConfig,
        telemetry_manifest: TelemetryManifestStore,
        host: HostNamespaceConfig,
    ) -> Self {
        Self::new_with_telemetry_manifest_host_policy(
            telemetry,
            telemetry_manifest,
            host,
            PolicyNamespaceConfig::disabled(),
            AuditNamespaceConfig::disabled(),
            ReplayNamespaceConfig::disabled(),
        )
    }

    /// Retrieve the root Qid.
    pub fn root_qid(&self) -> Qid {
        self.root.qid
    }

    /// Return the configured host mount path, if enabled.
    pub fn host_mount_path(&self) -> Option<&[String]> {
        self.host.enabled.then_some(self.host.mount_path.as_slice())
    }

    /// Return true when policy namespaces are enabled.
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
        }

        let retain_on_boot = self.telemetry.cursor.retain_on_boot;
        let worker_id = telemetry_worker_id(path).map(str::to_owned);
        let mut audit = None;
        let mut manifest_snapshot = None;
        let action = {
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
        let retain_on_boot = self.telemetry.cursor.retain_on_boot;
        let worker_id = telemetry_worker_id(path).map(str::to_owned);
        let mut audit = None;
        let mut manifest_snapshot = None;
        let result = {
            let node = self.lookup_mut(path)?;
            match node.node.kind_mut() {
                NodeKind::File(FileNode::AppendOnly(buffer)) => {
                    buffer.extend_from_slice(data);
                    Ok(data.len() as u32)
                }
                NodeKind::File(FileNode::Telemetry(file)) => match file.append(offset, data) {
                    Ok(outcome) => {
                        audit = outcome.audit;
                        if retain_on_boot && worker_id.is_some() {
                            manifest_snapshot = Some(file.snapshot());
                        }
                        Ok(outcome.count)
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
                        Err(NineDoorError::protocol(code, err.message))
                    }
                },
                NodeKind::File(FileNode::ReadOnly(_)) => Err(NineDoorError::protocol(
                    ErrorCode::Permission,
                    format!("cannot write read-only file /{}", join_path(path)),
                )),
                NodeKind::File(FileNode::TraceControl) => self.trace.write_ctl(data),
                NodeKind::File(FileNode::TraceEvents)
                | NodeKind::File(FileNode::KernelMessages)
                | NodeKind::File(FileNode::TaskTrace(_)) => Err(NineDoorError::protocol(
                    ErrorCode::Permission,
                    format!("cannot write read-only file /{}", join_path(path)),
                )),
                NodeKind::Directory { .. } => Err(NineDoorError::protocol(
                    ErrorCode::Permission,
                    format!("cannot write directory /{}", join_path(path)),
                )),
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
        result
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
        let worker_root = vec!["worker".to_owned()];
        {
            let node = self.lookup_mut(&worker_root)?;
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
        let mut node = self.lookup_mut(&worker_root)?;
        let worker_dir = node.ensure_directory(worker_id);
        worker_dir.ensure_file("telemetry", FileNode::Telemetry(telemetry_file));
        let proc_root = vec!["proc".to_owned()];
        self.ensure_dir(&proc_root, worker_id)?;
        let proc_path = vec!["proc".to_owned(), worker_id.to_owned()];
        let mut proc_node = self.lookup_mut(&proc_path)?;
        proc_node.ensure_file("trace", FileNode::TaskTrace(worker_id.to_owned()));
        Ok(())
    }

    /// Remove namespace entries for a killed worker.
    pub fn remove_worker(&mut self, worker_id: &str) -> Result<(), NineDoorError> {
        let worker_root = vec!["worker".to_owned()];
        let mut node = self.lookup_mut(&worker_root)?;
        if node.remove_child(worker_id).is_none() {
            return Err(NineDoorError::protocol(
                ErrorCode::NotFound,
                format!("worker {worker_id} not found"),
            ));
        }
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

    /// Lookup a node by path.
    pub fn lookup(&self, path: &[String]) -> Result<NodeView<'_>, NineDoorError> {
        let mut node = &self.root;
        for component in path {
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

        self.ensure_dir(&[], "worker").expect("create /worker");
        self.ensure_dir(&[], "gpu").expect("create /gpu");
        self.ensure_dir(&[], "trace").expect("create /trace");
        let trace_path = vec!["trace".to_owned()];
        self.ensure_trace_control(&trace_path, "ctl")
            .expect("create /trace/ctl");
        self.ensure_trace_events(&trace_path, "events")
            .expect("create /trace/events");
        self.ensure_kernel_messages().expect("create /kmesg");
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

fn telemetry_worker_id(path: &[String]) -> Option<&str> {
    if path.len() != 3 {
        return None;
    }
    if path[0] != "worker" || path[2] != "telemetry" {
        return None;
    }
    Some(path[1].as_str())
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
