// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Dispatch host ticket actions to bounded executor adapters.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::fmt;
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use coh::CohAccess;
use cohsh::{Session, Transport};
use serde_json::Value;

use crate::HostTicketSpec;

/// Docker remediation executor.
pub mod docker;
/// Generated field-bus maps with remote ACK and durable replay fencing.
pub mod field_bus;
/// GPU lease executor.
pub mod gpu;
/// Kubernetes coexistence executor.
pub mod k8s;
/// Compiler-selected macOS service lifecycle.
pub mod launchd;
/// Compiler-selected macOS build, release and endpoint observations.
pub mod mac_release;
/// Durable bounded native evidence objects and compact ticket-result references.
pub mod observation;
/// PEFT lifecycle executor.
pub mod peft;
/// systemd remediation executor.
pub mod systemd;
/// Authenticated GPU workload IPC and root lease supervision.
pub mod workload;

/// Runtime configuration used by executors.
#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    /// Host namespace mount root.
    pub mount: String,
    /// Default PEFT registry root.
    pub registry_root: PathBuf,
    /// Host root containing bounded exported LoRA job records.
    pub export_root: PathBuf,
    /// Host root containing provider-produced adapter bundles.
    pub adapter_root: PathBuf,
    /// Private host-owned content-addressed native observation store.
    pub provider_evidence_root: PathBuf,
    /// Explicit private field-bus WAL; absent disables bus ticket execution.
    pub field_bus_state_root: Option<PathBuf>,
    /// Optional independently enrolled signed operation bindings and custodian keys.
    pub evidence_enrollment_dir: Option<PathBuf>,
    /// Separate Worker witness key enrollment; cannot sign native execution or grants.
    pub worker_evidence_enrollment_dir: Option<PathBuf>,
    /// Startup measurement of this native agent, required for signed operations.
    pub evidence_executable_sha256: Option<String>,
    /// Explicit private host-local GPU executor socket; absent disables workloads.
    pub gpu_executor_socket: Option<PathBuf>,
    /// Secret reference held only by the ticket agent and GPU bridge.
    pub gpu_executor_credential_ref: Option<String>,
    /// Bounded immutable workload request CAS; paths never come from tickets.
    pub gpu_request_root: Option<PathBuf>,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            mount: "/host".to_owned(),
            registry_root: PathBuf::from("out/model_registry"),
            export_root: PathBuf::from("out/peft_exports"),
            adapter_root: PathBuf::from("out/peft_adapters"),
            provider_evidence_root: PathBuf::from("out/provider-evidence"),
            field_bus_state_root: None,
            evidence_enrollment_dir: None,
            worker_evidence_enrollment_dir: None,
            evidence_executable_sha256: None,
            gpu_executor_socket: None,
            gpu_executor_credential_ref: None,
            gpu_request_root: None,
        }
    }
}

/// Action-specific restart observation for an operation left in `executing`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconcileOutcome {
    /// The exact operation is observed committed and may publish success.
    Committed(String),
    /// The exact operation is observed rejected or terminally failed.
    Rejected(String),
    /// State is insufficient to decide; provider execution must not be repeated.
    Ambiguous,
}

#[derive(Debug)]
pub(crate) struct ProviderPending {
    detail: String,
}

impl fmt::Display for ProviderPending {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for ProviderPending {}

pub(crate) fn provider_pending(detail: impl Into<String>) -> anyhow::Error {
    ProviderPending {
        detail: detail.into(),
    }
    .into()
}

pub(crate) fn is_provider_pending(error: &anyhow::Error) -> bool {
    error.downcast_ref::<ProviderPending>().is_some()
}

/// Execute one host ticket action.
pub fn execute_action(
    transport: &mut dyn Transport,
    session: &Session,
    spec: &HostTicketSpec,
    config: &ExecutorConfig,
) -> Result<String> {
    crate::provider::validate(spec)?;
    super::causal::preflight(config, spec)?;
    if spec.schema == crate::HOST_TICKET_V2_SCHEMA {
        crate::claim::validate_v2_action_args(spec)?;
    }
    if spec.action.starts_with("gpu.workload.") {
        return workload::execute(transport, session, spec, config);
    }
    if spec.action.starts_with("gpu.lease.") {
        return gpu::execute(transport, session, spec, config);
    }
    if spec.action.starts_with("peft.") {
        return peft::execute(transport, session, spec, config);
    }
    if spec.action.starts_with("systemd.") {
        return systemd::execute(transport, session, spec, config);
    }
    if spec.action.starts_with("mac_release.") || spec.action.starts_with("endpoint_compliance.") {
        return mac_release::execute(spec, config);
    }
    if spec.action.starts_with("launchd.") {
        return launchd::execute(spec, config);
    }
    if spec.action.starts_with("docker.") {
        return docker::execute(transport, session, spec, config);
    }
    if spec.action.starts_with("k8s.") {
        return k8s::execute(transport, session, spec, config);
    }
    if spec.action.starts_with("modbus.") || spec.action.starts_with("dnp3.") {
        return field_bus::execute(spec, config);
    }
    Err(anyhow!("unsupported ticket action {}", spec.action))
}

/// Observe/reconcile an action left in `executing` without blindly replaying it.
pub fn reconcile_action(
    transport: &mut dyn Transport,
    session: &Session,
    spec: &HostTicketSpec,
    config: &ExecutorConfig,
) -> Result<ReconcileOutcome> {
    if spec.action.starts_with("modbus.") || spec.action.starts_with("dnp3.") {
        return field_bus::reconcile(spec, config);
    }
    if spec.action.starts_with("gpu.workload.") {
        return workload::reconcile(spec, config);
    }
    if spec.action.starts_with("gpu.lease.") {
        return gpu::reconcile(transport, session, spec, config);
    }
    if spec.action.starts_with("peft.") {
        return peft::reconcile(transport, session, spec, config);
    }
    Ok(ReconcileOutcome::Ambiguous)
}

/// Adapter allowing `coh::peft` helpers to operate on a `cohsh::Transport` session.
pub struct TransportAccess<'a> {
    transport: &'a mut dyn Transport,
    session: &'a Session,
}

impl<'a> TransportAccess<'a> {
    /// Construct an access wrapper.
    pub fn new(transport: &'a mut dyn Transport, session: &'a Session) -> Self {
        Self { transport, session }
    }
}

impl CohAccess for TransportAccess<'_> {
    fn list_dir(&mut self, path: &str, _max_bytes: usize) -> Result<Vec<String>> {
        self.transport.list(self.session, path)
    }

    fn read_file(&mut self, path: &str, max_bytes: usize) -> Result<Vec<u8>> {
        let lines = self.transport.read(self.session, path)?;
        lines_to_bytes(&lines, max_bytes)
    }

    fn tail_file(&mut self, path: &str, max_bytes: usize) -> Result<Vec<u8>> {
        let lines = self.transport.tail(self.session, path, None)?;
        lines_to_bytes(&lines, max_bytes)
    }

    fn write_append(&mut self, path: &str, payload: &[u8]) -> Result<usize> {
        self.transport.write(self.session, path, payload)?;
        Ok(payload.len())
    }
}

/// Borrow a string argument from `spec.args`.
pub fn arg_str<'a>(spec: &'a HostTicketSpec, key: &str) -> Option<&'a str> {
    spec.args
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

/// Borrow an integer argument from `spec.args`.
pub fn arg_u64(spec: &HostTicketSpec, key: &str) -> Option<u64> {
    spec.args.get(key).and_then(Value::as_u64)
}

/// Borrow a boolean argument from `spec.args`.
pub fn arg_bool(spec: &HostTicketSpec, key: &str) -> Option<bool> {
    spec.args.get(key).and_then(Value::as_bool)
}

/// Parse a path-like target into non-empty components.
pub fn target_components(spec: &HostTicketSpec) -> Vec<&str> {
    spec.target
        .as_deref()
        .map(|target| {
            target
                .split('/')
                .filter(|segment| !segment.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

/// Read the last non-empty line from a path.
pub fn read_last_line(
    transport: &mut dyn Transport,
    session: &Session,
    path: &str,
) -> Result<Option<String>> {
    let lines = transport.tail(session, path, None)?;
    Ok(lines
        .iter()
        .rev()
        .map(|line| line.trim())
        .find(|line| !line.is_empty())
        .map(str::to_owned))
}

fn lines_to_bytes(lines: &[String], max_bytes: usize) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for line in lines {
        if !out.is_empty() {
            out.push(b'\n');
        }
        out.extend_from_slice(line.as_bytes());
        if out.len() > max_bytes {
            return Err(anyhow!("payload exceeds max bytes {max_bytes}"));
        }
    }
    if !out.is_empty() {
        out.push(b'\n');
    }
    Ok(out)
}
