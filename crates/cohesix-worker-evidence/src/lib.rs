// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Parse and validate bounded, hash-bound Cohesix Worker evidence.
// Author: Lukas Bower

#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Host-side validation for Milestone 26e Worker evidence.
//!
//! The types deliberately keep declaration, runtime lifecycle, artifact state,
//! receipt state, integration mode, and execution proof independent.  A valid
//! record can therefore describe an unavailable provider or verified package
//! without accidentally promoting it to Worker READY or target acceptance.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Schema for one host-integration dependency-row result.
pub const WORKER_INTEGRATION_SCHEMA: &str = "cohesix-worker-integration-evidence/v1";
/// Schema for one target Worker component result.
pub const WORKER_COMPONENT_SCHEMA: &str = "cohesix-worker-task-evidence/v1";
/// Schema for one target root-TCB containment result.
pub const ROOT_TCB_SCHEMA: &str = "cohesix-root-tcb-acceptance/v1";
/// Schema for one target full-system MCS result.
pub const MCS_SYSTEM_SCHEMA: &str = "cohesix-mcs-smp-system-acceptance/v1";
/// Schema for the two-target Worker-runtime promotion record.
pub const WORKER_RELEASE_SCHEMA: &str = "cohesix-worker-release-acceptance/v1";

const SHA256_HEX_BYTES: usize = 64;
const MAX_RECORD_BYTES: usize = 256 * 1024;
const MAX_ID_BYTES: usize = 128;
const MAX_LABEL_BYTES: usize = 256;
const MAX_BLOCKERS: usize = 64;
const MAX_OUTCOMES: usize = 128;
const MAX_RAW_ARTIFACTS: usize = 128;
const MAX_INTEGRATION_REFERENCES: usize = 128;
const REQUIRED_WORKER_INTEGRATIONS: [&str; 3] =
    ["gpu-receipt-path", "peft-receipt-path", "worker-control"];
const REQUIRED_COMPONENT_OUTCOMES: [&str; 25] = [
    "bounded-control-path",
    "bounded-receipt-path",
    "budget-exhaustion-attributed",
    "combined-notification",
    "driver-liveness",
    "durable-completion-order",
    "fault-before-ready",
    "fault-during-ipc",
    "forbidden-blocking-send-refused",
    "fresh-supervisor-generation",
    "gpu-grant-confirmed-rejected-stale",
    "gpu-release-confirmed-rejected-stale",
    "gpu-renew-confirmed-rejected-stale",
    "heartbeat-progress",
    "lora-activate-confirmed-rejected-stale",
    "lora-export-confirmed-rejected-stale",
    "lora-import-confirmed-rejected-stale",
    "lora-rollback-confirmed-rejected-stale",
    "maximum-slot-refused",
    "no-post-revoke-activity",
    "operator-liveness",
    "same-role-sequential-instances",
    "stale-record-revoked",
    "teardown-zero-leak",
    "timeout-attributed",
];
const REQUIRED_ROOT_OUTCOMES: [&str; 15] = [
    "console-network-fault-contained",
    "donated-time-returned-or-revoked",
    "driver-supervisor-progress",
    "emergency-progress",
    "fault-supervisor-progress",
    "ninedoor-fault-contained",
    "operator-liveness",
    "pressure-bounded",
    "root-control-progress",
    "shutdown-contained",
    "stale-authority-revoked",
    "worker-gpu-fault-contained",
    "worker-heartbeat-fault-contained",
    "worker-lora-fault-contained",
    "worker-supervisor-progress",
];
const REQUIRED_SYSTEM_OUTCOMES: [&str; 18] = [
    "artifact-freeze",
    "budget-exhaustion-attributed",
    "cold-warm-boot",
    "cyw43-coexistence-record-bound",
    "driver-call-recovered",
    "fault-contained",
    "four-core-mcs-topology",
    "fresh-supervisor-generation",
    "gpu-receipt-path",
    "no-classic-scheduler",
    "normal-load-liveness",
    "operator-liveness",
    "overload-bounded",
    "peft-receipt-path",
    "protocol-regression",
    "same-harness-performance",
    "timeout-contained",
    "worker-teardown-zero-leak",
];

/// Typed validation failure.  Callers must not downgrade these errors to a
/// weaker proof class.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum EvidenceError {
    /// Input exceeded the bounded evidence-record size.
    #[error("evidence record exceeds the {MAX_RECORD_BYTES}-byte bound")]
    RecordTooLarge,
    /// Input was not canonical typed JSON for a supported schema.
    #[error("invalid evidence JSON: {0}")]
    InvalidJson(String),
    /// The schema and record-kind pair is unsupported or inconsistent.
    #[error("unsupported or inconsistent schema/record kind")]
    WrongRecordKind,
    /// A required identifier or label is absent, oversized, or malformed.
    #[error("invalid bounded identifier: {0}")]
    InvalidIdentifier(&'static str),
    /// A hash is not exactly lowercase SHA-256 hexadecimal.
    #[error("invalid SHA-256 field: {0}")]
    InvalidHash(&'static str),
    /// A list is too large, duplicated, or not in canonical sorted order.
    #[error("invalid canonical list: {0}")]
    InvalidList(&'static str),
    /// A field combination violates the proof-class contract.
    #[error("invalid evidence field matrix: {0}")]
    InvalidFieldMatrix(&'static str),
    /// The record contains a likely secret or a raw capability identifier.
    #[error("evidence contains prohibited sensitive material")]
    SensitiveMaterial,
    /// A Worker identity is zero, model-only, or generation-inconsistent.
    #[error("invalid Worker identity")]
    InvalidIdentity,
    /// PASS/FAIL does not agree with blockers or required outcomes.
    #[error("verdict does not agree with record contents")]
    InvalidVerdict,
    /// A referenced record's bytes do not match its declared digest.
    #[error("referenced evidence digest mismatch")]
    DigestMismatch,
}

/// Fixed lowercase SHA-256 hexadecimal.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Sha256Hex(pub String);

impl Sha256Hex {
    /// Validate exact lowercase hexadecimal form.
    pub fn validate(&self, field: &'static str) -> Result<(), EvidenceError> {
        if self.0.len() != SHA256_HEX_BYTES
            || !self
                .0
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err(EvidenceError::InvalidHash(field));
        }
        Ok(())
    }

    /// Hash exact bytes into the canonical representation.
    #[must_use]
    pub fn digest(bytes: &[u8]) -> Self {
        Self(hex::encode(Sha256::digest(bytes)))
    }

    /// Compare a declared digest against exact bytes.
    pub fn verify(&self, bytes: &[u8]) -> Result<(), EvidenceError> {
        self.validate("reference")?;
        if *self == Self::digest(bytes) {
            Ok(())
        } else {
            Err(EvidenceError::DigestMismatch)
        }
    }
}

/// Evidence record kind.  Each schema permits exactly one value except the
/// release validator, which consumes references rather than a target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecordKind {
    /// One generated host-integration dependency row.
    WorkerIntegration,
    /// One direct target Worker component result.
    TargetComponent,
    /// One direct target root authority/containment result.
    RootTcb,
    /// One direct target full-system MCS result.
    FullSystem,
    /// Two-target Worker-runtime release promotion.
    Release,
}

/// Static implementation declaration; it is not runtime state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeclarationState {
    /// Compiler-selected executable child contract.
    Executable,
    /// Model/session behavior with no executable target task.
    ModelOnly,
}

/// Public Worker lifecycle independent of control acknowledgement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LifecycleState {
    /// No instance occupies the slot.
    Absent,
    /// Bounded control queue admission succeeded.
    Queued,
    /// Construction/resume succeeded but READY has not been validated.
    Starting,
    /// An exact durable READY record was validated.
    Ready,
    /// New authority is closed and teardown is underway.
    Closing,
    /// A protocol or kernel fault is being contained.
    Faulted,
    /// Teardown completed and the retained record is observational only.
    Terminal,
}

/// Content-addressed package state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArtifactState {
    /// A required image or manifest is absent.
    Missing,
    /// Every selected artifact matches its declared digest.
    Verified,
    /// An artifact is present but differs from its bound digest.
    Mismatch,
}

/// Role-specific durable receipt state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReceiptState {
    /// This observation carries no receipt.
    None,
    /// An admitted operation is awaiting a terminal Worker record.
    Pending,
    /// The exact current generation confirmed the operation.
    Confirmed,
    /// The exact current generation rejected the operation.
    Rejected,
    /// A late or old-generation receipt was rejected.
    Stale,
}

/// Generated integration obligation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationObligation {
    /// Required for the executable role path.
    RoleRequired,
    /// Required for the selected release package.
    ReleaseRequired,
    /// Required only before a named use case can be promoted.
    UseCaseRequired,
    /// Supported when present but not a release blocker.
    Optional,
    /// Owned by a later milestone and never selectable as live now.
    Future,
}

/// Observed host/provider mode, separate from implementation class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ObservedMode {
    /// No observation was possible.
    Unknown,
    /// Required implementation/package was absent.
    Missing,
    /// Explicitly disabled by the selected profile.
    Disabled,
    /// Deterministic test fixture.
    Fixture,
    /// Host model with no target authority.
    Mock,
    /// Real adapter validation without external side effects.
    DryRun,
    /// Real target/provider session.
    Live,
}

/// Execution proof axis.  Packaging and connectivity never set this field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExecutionProof {
    /// No execution proof.
    None,
    /// Host-only model execution.
    HostModel,
    /// Direct exact-image QEMU execution.
    Qemu,
    /// Direct fresh exact-image Pi 4 execution.
    FreshPi,
}

/// Direct target named by an acceptance or target-session record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TargetClass {
    /// Four-core AArch64 QEMU virt/GICv3.
    Qemu,
    /// Fresh Raspberry Pi 4 hardware.
    Pi4,
}

/// Exact executable Worker role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkerRole {
    /// Heartbeat telemetry Worker.
    WorkerHeartbeat,
    /// GPU lease-result receipt Worker.
    WorkerGpu,
    /// PEFT lifecycle-result receipt Worker.
    WorkerLora,
    /// Model/session-only field-bus role; never executable in 26e.
    WorkerBus,
}

/// Immutable five-part Worker identity with no raw capability values.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerIdentity {
    /// Exact role.
    pub role: WorkerRole,
    /// Compiler-admitted per-role slot.
    pub slot: u16,
    /// Root-resolved logical lease epoch.
    pub lease_epoch: u64,
    /// Root Worker-supervisor generation.
    pub supervisor_generation: u64,
    /// Revocable capability-bundle generation.
    pub cap_generation: u64,
}

impl WorkerIdentity {
    fn validate(&self) -> Result<(), EvidenceError> {
        if self.role == WorkerRole::WorkerBus
            || self.lease_epoch == 0
            || self.supervisor_generation == 0
            || self.cap_generation == 0
        {
            return Err(EvidenceError::InvalidIdentity);
        }
        Ok(())
    }
}

/// Independent Worker state axes exposed by host projections.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerState {
    /// Static compiler declaration.
    pub declaration: DeclarationState,
    /// Current runtime lifecycle.
    pub lifecycle: LifecycleState,
    /// Content-addressed artifact state.
    pub artifact: ArtifactState,
    /// Role-specific receipt state.
    pub receipt: ReceiptState,
    /// Direct execution proof.
    pub execution_proof: ExecutionProof,
}

impl WorkerState {
    fn validate(&self, identity: Option<&WorkerIdentity>) -> Result<(), EvidenceError> {
        if self.declaration == DeclarationState::ModelOnly {
            if identity.is_some()
                || self.lifecycle != LifecycleState::Absent
                || self.execution_proof != ExecutionProof::HostModel
                || self.receipt != ReceiptState::None
            {
                return Err(EvidenceError::InvalidFieldMatrix("model-only Worker state"));
            }
            return Ok(());
        }
        if self.lifecycle == LifecycleState::Absent && identity.is_some() {
            return Err(EvidenceError::InvalidFieldMatrix("absent identity"));
        }
        if self.lifecycle != LifecycleState::Absent && identity.is_none() {
            return Err(EvidenceError::InvalidFieldMatrix("live lifecycle identity"));
        }
        if self.execution_proof != ExecutionProof::None && self.artifact != ArtifactState::Verified
        {
            return Err(EvidenceError::InvalidFieldMatrix(
                "proof without verified artifact",
            ));
        }
        Ok(())
    }
}

/// PASS or FAIL; partial/degraded proof is represented by an explicit FAIL and
/// blockers, never a third promotion state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Verdict {
    /// Every obligation for this record passed.
    Pass,
    /// One or more exact blockers remain.
    Fail,
}

/// Host environment bound into one integration observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostEnvironment {
    /// Generated host profile id.
    pub profile: String,
    /// Operating-system label.
    pub os: String,
    /// Architecture label.
    pub architecture: String,
    /// Optional provider/package version; no credentials are permitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_version: Option<String>,
}

/// Optional exact target session for a live integration row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetSession {
    /// One proof target only.
    pub target: TargetClass,
    /// Exact source tree identity used for the target build.
    pub source_sha256: Sha256Hex,
    /// Exact resolved manifest.
    pub manifest_sha256: Sha256Hex,
    /// Exact seL4 kernel image/config identity.
    pub kernel_sha256: Sha256Hex,
    /// Exact root task image.
    pub root_image_sha256: Sha256Hex,
    /// Exact separate MCS driver archive.
    pub driver_archive_sha256: Sha256Hex,
    /// Exact separate MCS driver manifest.
    pub driver_manifest_sha256: Sha256Hex,
    /// Exact target-qualified CYW43 coexistence record, including an explicit
    /// not-applicable QEMU record rather than an omitted field.
    pub cyw43_coexistence_record_sha256: Sha256Hex,
    /// Exact separate Worker archive.
    pub worker_archive_sha256: Sha256Hex,
    /// Exact Worker image manifest.
    pub worker_image_manifest_sha256: Sha256Hex,
    /// Exact Worker task ABI/schema bundle.
    pub worker_abi_sha256: Sha256Hex,
}

impl TargetSession {
    /// Validate the complete target-session artifact identity graph.
    pub fn validate(&self) -> Result<(), EvidenceError> {
        self.source_sha256.validate("source")?;
        self.manifest_sha256.validate("target manifest")?;
        self.kernel_sha256.validate("kernel")?;
        self.root_image_sha256.validate("root image")?;
        self.driver_archive_sha256.validate("driver archive")?;
        self.driver_manifest_sha256.validate("driver manifest")?;
        self.cyw43_coexistence_record_sha256
            .validate("CYW43 coexistence record")?;
        self.worker_archive_sha256.validate("Worker archive")?;
        self.worker_image_manifest_sha256
            .validate("Worker image manifest")?;
        self.worker_abi_sha256.validate("Worker ABI")
    }
}

/// Parse and validate one bounded standalone target-session record.
///
/// Gateways use this for the independently supplied identity of the target
/// currently behind their console transport.  Equality with the target session
/// embedded in accepted component evidence is checked by the gateway; merely
/// parsing this record never establishes execution proof.
pub fn parse_target_session(bytes: &[u8]) -> Result<TargetSession, EvidenceError> {
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(EvidenceError::RecordTooLarge);
    }
    let session: TargetSession = serde_json::from_slice(bytes)
        .map_err(|error| EvidenceError::InvalidJson(error.to_string()))?;
    session.validate()?;
    Ok(session)
}

/// Redacted action/observation/receipt outcome.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntegrationOutcome {
    /// Generated outcome id.
    pub id: String,
    /// `action`, `observation`, or `receipt`.
    pub class: String,
    /// Bounded canonical result label.
    pub result: String,
}

/// Hash-only raw evidence reference.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceArtifact {
    /// Stable logical artifact id, not an untrusted absolute path.
    pub id: String,
    /// Exact bytes digest.
    pub sha256: Sha256Hex,
    /// Exact byte length.
    pub bytes: u64,
}

/// One strict dependency-row evidence record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerIntegrationEvidence {
    /// Exact [`WORKER_INTEGRATION_SCHEMA`].
    pub schema: String,
    /// Exact [`RecordKind::WorkerIntegration`].
    pub record_kind: RecordKind,
    /// Generated dependency-row id.
    pub dependency_id: String,
    /// Generated owning milestone id.
    pub owner_milestone: String,
    /// Generated obligation.
    pub obligation: IntegrationObligation,
    /// Mode actually observed by this run.
    pub observed_mode: ObservedMode,
    /// Exact generated dependency graph.
    pub dependency_graph_sha256: Sha256Hex,
    /// Exact selected resolved manifest.
    pub manifest_sha256: Sha256Hex,
    /// Exact implementation/config component.
    pub component_sha256: Sha256Hex,
    /// Exact selected configuration.
    pub config_sha256: Sha256Hex,
    /// Optional packaged artifact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_sha256: Option<Sha256Hex>,
    /// Host/provider identity with no credentials.
    pub host: HostEnvironment,
    /// Present only for a direct target-session lane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_session: Option<TargetSession>,
    /// Direct target proof, never inferred from mode or session presence.
    pub execution_proof: ExecutionProof,
    /// Sorted action/observation/receipt results.
    #[serde(default)]
    pub outcomes: Vec<IntegrationOutcome>,
    /// Sorted redacted raw-artifact references.
    #[serde(default)]
    pub raw_evidence: Vec<EvidenceArtifact>,
    /// Exact verdict.
    pub verdict: Verdict,
    /// Sorted explicit blockers; empty only on PASS.
    #[serde(default)]
    pub blockers: Vec<String>,
}

impl WorkerIntegrationEvidence {
    /// Validate all bounds, hashes, mode/proof separation, and verdict rules.
    pub fn validate(&self) -> Result<(), EvidenceError> {
        if self.schema != WORKER_INTEGRATION_SCHEMA
            || self.record_kind != RecordKind::WorkerIntegration
        {
            return Err(EvidenceError::WrongRecordKind);
        }
        validate_identifier(&self.dependency_id, "dependency_id")?;
        validate_identifier(&self.owner_milestone, "owner_milestone")?;
        validate_identifier(&self.host.profile, "host profile")?;
        validate_label(&self.host.os, "host os")?;
        validate_label(&self.host.architecture, "host architecture")?;
        if let Some(version) = &self.host.provider_version {
            validate_label(version, "provider version")?;
        }
        self.dependency_graph_sha256.validate("dependency graph")?;
        self.manifest_sha256.validate("manifest")?;
        self.component_sha256.validate("component")?;
        self.config_sha256.validate("config")?;
        if let Some(hash) = &self.artifact_sha256 {
            hash.validate("artifact")?;
        }
        if self.obligation == IntegrationObligation::Future
            && self.observed_mode == ObservedMode::Live
        {
            return Err(EvidenceError::InvalidFieldMatrix("future integration live"));
        }
        match (self.execution_proof, self.target_session.as_ref()) {
            (ExecutionProof::Qemu, Some(session)) if session.target == TargetClass::Qemu => {
                session.validate()?;
            }
            (ExecutionProof::FreshPi, Some(session)) if session.target == TargetClass::Pi4 => {
                session.validate()?;
            }
            (ExecutionProof::None | ExecutionProof::HostModel, None) => {}
            _ => return Err(EvidenceError::InvalidFieldMatrix("target session/proof")),
        }
        if matches!(
            self.execution_proof,
            ExecutionProof::Qemu | ExecutionProof::FreshPi
        ) && self.observed_mode != ObservedMode::Live
        {
            return Err(EvidenceError::InvalidFieldMatrix("target proof mode"));
        }
        validate_sorted_unique(&self.outcomes, MAX_OUTCOMES, "outcomes")?;
        for outcome in &self.outcomes {
            validate_identifier(&outcome.id, "outcome id")?;
            if !matches!(outcome.class.as_str(), "action" | "observation" | "receipt") {
                return Err(EvidenceError::InvalidIdentifier("outcome class"));
            }
            validate_label(&outcome.result, "outcome result")?;
        }
        validate_sorted_unique(&self.raw_evidence, MAX_RAW_ARTIFACTS, "raw evidence")?;
        for artifact in &self.raw_evidence {
            validate_identifier(&artifact.id, "raw evidence id")?;
            artifact.sha256.validate("raw evidence")?;
            if artifact.bytes == 0 {
                return Err(EvidenceError::InvalidFieldMatrix("empty raw evidence"));
            }
        }
        validate_blockers(self.verdict, &self.blockers)?;
        scan_sensitive(self)?;
        Ok(())
    }
}

/// Hash reference to another canonical evidence record.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceReference {
    /// Exact logical id.
    pub id: String,
    /// Referenced record kind.
    pub record_kind: RecordKind,
    /// Exact referenced bytes digest.
    pub sha256: Sha256Hex,
}

/// One role/identity observation inside a target component record.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerRoleObservation {
    /// Exact immutable identity.
    pub identity: WorkerIdentity,
    /// Independent public state axes.
    pub state: WorkerState,
    /// Exact role image digest.
    pub image_sha256: Sha256Hex,
    /// Last durable READY sequence, or zero before READY.
    pub ready_sequence: u64,
    /// Last durable completion sequence, or zero when absent.
    pub completion_sequence: u64,
    /// Generated endpoint badge for this Worker instance. This is an
    /// evidence-domain selector, never a capability address.
    pub endpoint_badge: u64,
    /// Generated fault badge for this Worker instance. This is an
    /// evidence-domain selector, never a capability address.
    pub fault_badge: u64,
    /// Generated zero-based CPU core assignment.
    pub core: u8,
    /// Exact active scheduling-context budget and period.
    pub scheduling_context: WorkerSchedulingContext,
    /// Exact generated per-instance kernel-object inventory.
    pub object_inventory: KernelObjectInventory,
}

/// Exact active MCS scheduling-context parameters for one Worker instance.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerSchedulingContext {
    /// Configured execution budget in microseconds.
    pub budget_us: u32,
    /// Configured replenishment period in microseconds.
    pub period_us: u32,
}

impl WorkerSchedulingContext {
    fn validate(&self) -> Result<(), EvidenceError> {
        if self.budget_us == 0 || self.period_us == 0 || self.budget_us > self.period_us {
            return Err(EvidenceError::InvalidFieldMatrix(
                "Worker scheduling context",
            ));
        }
        Ok(())
    }
}

/// Direct target Worker-component evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerComponentEvidence {
    /// Exact [`WORKER_COMPONENT_SCHEMA`].
    pub schema: String,
    /// Exact [`RecordKind::TargetComponent`].
    pub record_kind: RecordKind,
    /// Exactly one direct target.
    pub target: TargetClass,
    /// Exact target-session artifact graph.
    pub target_session: TargetSession,
    /// Compiler-owned critical/service/Worker/driver topology digest.
    pub topology_sha256: Sha256Hex,
    /// Sorted mandatory role observations.
    pub workers: Vec<WorkerRoleObservation>,
    /// Sorted dependency-row references.
    pub integration_evidence: Vec<EvidenceReference>,
    /// Sorted direct runtime, receipt, fault, teardown, and liveness outcomes.
    pub outcomes: Vec<IntegrationOutcome>,
    /// Sorted raw target artifacts.
    pub raw_evidence: Vec<EvidenceArtifact>,
    /// Exact target verdict.
    pub verdict: Verdict,
    /// Sorted blockers; empty only on PASS.
    #[serde(default)]
    pub blockers: Vec<String>,
}

impl WorkerComponentEvidence {
    /// Validate one target-component record without promoting it to release.
    pub fn validate(&self) -> Result<(), EvidenceError> {
        if self.schema != WORKER_COMPONENT_SCHEMA
            || self.record_kind != RecordKind::TargetComponent
            || self.target_session.target != self.target
        {
            return Err(EvidenceError::WrongRecordKind);
        }
        self.target_session.validate()?;
        self.topology_sha256.validate("topology")?;
        validate_sorted_unique(&self.workers, 64, "workers")?;
        let mut mandatory = BTreeSet::new();
        let mut endpoint_badges = BTreeSet::new();
        let mut fault_badges = BTreeSet::new();
        for worker in &self.workers {
            worker.identity.validate()?;
            worker.state.validate(Some(&worker.identity))?;
            worker.image_sha256.validate("Worker image")?;
            if worker.endpoint_badge == 0
                || worker.fault_badge == 0
                || worker.endpoint_badge == worker.fault_badge
                || worker.core >= 4
                || !endpoint_badges.insert(worker.endpoint_badge)
                || !fault_badges.insert(worker.fault_badge)
            {
                return Err(EvidenceError::InvalidFieldMatrix(
                    "Worker badge/core inventory",
                ));
            }
            worker.scheduling_context.validate()?;
            worker.object_inventory.validate_nonempty()?;
            if worker.state.lifecycle == LifecycleState::Ready && worker.ready_sequence == 0 {
                return Err(EvidenceError::InvalidFieldMatrix("READY without sequence"));
            }
            mandatory.insert(worker.identity.role);
            if self.verdict == Verdict::Pass {
                let expected_proof = match self.target {
                    TargetClass::Qemu => ExecutionProof::Qemu,
                    TargetClass::Pi4 => ExecutionProof::FreshPi,
                };
                if worker.state.lifecycle != LifecycleState::Ready
                    || worker.state.artifact != ArtifactState::Verified
                    || worker.state.execution_proof != expected_proof
                    || worker.completion_sequence == 0
                    || match worker.identity.role {
                        WorkerRole::WorkerHeartbeat => worker.state.receipt != ReceiptState::None,
                        WorkerRole::WorkerGpu | WorkerRole::WorkerLora => {
                            worker.state.receipt != ReceiptState::Confirmed
                        }
                        WorkerRole::WorkerBus => true,
                    }
                {
                    return Err(EvidenceError::InvalidFieldMatrix(
                        "accepted Worker role state",
                    ));
                }
            }
        }
        if !endpoint_badges.is_disjoint(&fault_badges) {
            return Err(EvidenceError::InvalidFieldMatrix(
                "Worker badge/core inventory",
            ));
        }
        let expected = BTreeSet::from([
            WorkerRole::WorkerHeartbeat,
            WorkerRole::WorkerGpu,
            WorkerRole::WorkerLora,
        ]);
        if mandatory != expected {
            return Err(EvidenceError::InvalidFieldMatrix("mandatory role matrix"));
        }
        validate_sorted_unique(
            &self.integration_evidence,
            MAX_INTEGRATION_REFERENCES,
            "integration evidence",
        )?;
        for reference in &self.integration_evidence {
            validate_identifier(&reference.id, "integration reference")?;
            reference.sha256.validate("integration reference")?;
            if reference.record_kind != RecordKind::WorkerIntegration {
                return Err(EvidenceError::WrongRecordKind);
            }
        }
        let integration_ids = self
            .integration_evidence
            .iter()
            .map(|reference| reference.id.as_str())
            .collect::<Vec<_>>();
        if integration_ids != REQUIRED_WORKER_INTEGRATIONS {
            return Err(EvidenceError::InvalidFieldMatrix(
                "mandatory Worker integration graph",
            ));
        }
        validate_sorted_unique(&self.outcomes, MAX_OUTCOMES, "component outcomes")?;
        for outcome in &self.outcomes {
            validate_identifier(&outcome.id, "component outcome id")?;
            if !matches!(outcome.class.as_str(), "action" | "observation" | "receipt") {
                return Err(EvidenceError::InvalidIdentifier("component outcome class"));
            }
            validate_label(&outcome.result, "component outcome result")?;
        }
        if self.verdict == Verdict::Pass {
            validate_required_pass_outcomes(
                &self.outcomes,
                &REQUIRED_COMPONENT_OUTCOMES,
                "component outcome matrix",
            )?;
        }
        validate_sorted_unique(&self.raw_evidence, MAX_RAW_ARTIFACTS, "raw evidence")?;
        for artifact in &self.raw_evidence {
            artifact.sha256.validate("raw evidence")?;
            if artifact.bytes == 0 {
                return Err(EvidenceError::InvalidFieldMatrix("empty raw evidence"));
            }
        }
        if self.verdict == Verdict::Pass
            && (self.outcomes.is_empty() || self.raw_evidence.is_empty())
        {
            return Err(EvidenceError::InvalidFieldMatrix(
                "accepted component evidence",
            ));
        }
        validate_blockers(self.verdict, &self.blockers)?;
        scan_sensitive(self)?;
        Ok(())
    }
}

/// Exact compiler-admitted or directly observed kernel-object inventory.
/// Individual capability addresses and badges are intentionally absent.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KernelObjectInventory {
    /// Runnable or suspended TCB objects in the generated topology.
    pub tcbs: u32,
    /// Active MCS scheduling contexts.
    pub scheduling_contexts: u32,
    /// Explicit MCS Reply objects.
    pub reply_objects: u32,
    /// Child VSpace roots.
    pub vspaces: u32,
    /// Child CSpace roots.
    pub cnodes: u32,
    /// AArch64 page-table objects.
    pub page_tables: u32,
    /// AArch64 ASID assignments.
    pub asids: u32,
    /// Shared and private frames.
    pub frames: u32,
    /// Endpoints.
    pub endpoints: u32,
    /// Notifications.
    pub notifications: u32,
    /// Standard fault endpoint capabilities.
    pub fault_caps: u32,
    /// Timeout-fault endpoint capabilities.
    pub timeout_fault_caps: u32,
    /// Admitted CSpace slots.
    pub cspace_slots: u32,
    /// Retyped untyped-memory extent in bytes.
    pub untyped_bytes: u64,
}

impl KernelObjectInventory {
    fn validate_nonempty(&self) -> Result<(), EvidenceError> {
        if self.tcbs == 0
            || self.scheduling_contexts == 0
            || self.vspaces == 0
            || self.cnodes == 0
            || self.page_tables == 0
            || self.asids == 0
            || self.frames == 0
            || self.fault_caps == 0
            || self.timeout_fault_caps == 0
            || self.cspace_slots == 0
            || self.untyped_bytes == 0
        {
            return Err(EvidenceError::InvalidFieldMatrix("kernel inventory"));
        }
        Ok(())
    }
}

/// Direct target root-TCB containment evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootTcbEvidence {
    /// Exact [`ROOT_TCB_SCHEMA`].
    pub schema: String,
    /// Exact [`RecordKind::RootTcb`].
    pub record_kind: RecordKind,
    /// Exactly one direct target.
    pub target: TargetClass,
    /// Exact target-session artifact graph.
    pub target_session: TargetSession,
    /// Matching target Worker-component record.
    pub worker_component: EvidenceReference,
    /// Exact generated critical/service/Worker/driver topology.
    pub topology_sha256: Sha256Hex,
    /// Compiler-expected object inventory.
    pub generated_inventory: KernelObjectInventory,
    /// Directly observed object inventory.
    pub observed_inventory: KernelObjectInventory,
    /// Sorted containment and operator-liveness outcomes.
    pub outcomes: Vec<IntegrationOutcome>,
    /// Sorted raw target artifacts.
    pub raw_evidence: Vec<EvidenceArtifact>,
    /// Exact target verdict.
    pub verdict: Verdict,
    /// Sorted blockers; empty only on PASS.
    #[serde(default)]
    pub blockers: Vec<String>,
}

impl RootTcbEvidence {
    /// Validate one direct target root-TCB record.
    pub fn validate(&self) -> Result<(), EvidenceError> {
        if self.schema != ROOT_TCB_SCHEMA
            || self.record_kind != RecordKind::RootTcb
            || self.target_session.target != self.target
            || self.worker_component.record_kind != RecordKind::TargetComponent
        {
            return Err(EvidenceError::WrongRecordKind);
        }
        self.target_session.validate()?;
        self.worker_component.sha256.validate("Worker component")?;
        self.topology_sha256.validate("topology")?;
        self.generated_inventory.validate_nonempty()?;
        self.observed_inventory.validate_nonempty()?;
        if self.verdict == Verdict::Pass && self.generated_inventory != self.observed_inventory {
            return Err(EvidenceError::InvalidFieldMatrix(
                "generated/observed inventory mismatch",
            ));
        }
        validate_sorted_unique(&self.outcomes, MAX_OUTCOMES, "root outcomes")?;
        if self.verdict == Verdict::Pass {
            validate_required_pass_outcomes(
                &self.outcomes,
                &REQUIRED_ROOT_OUTCOMES,
                "root outcome matrix",
            )?;
        }
        validate_sorted_unique(&self.raw_evidence, MAX_RAW_ARTIFACTS, "raw evidence")?;
        if self.verdict == Verdict::Pass
            && (self.outcomes.is_empty() || self.raw_evidence.is_empty())
        {
            return Err(EvidenceError::InvalidFieldMatrix("accepted root evidence"));
        }
        validate_blockers(self.verdict, &self.blockers)?;
        scan_sensitive(self)
    }
}

/// Per-core MCS admission total recorded by a full-system target run.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreAdmissionEvidence {
    /// Exact core index.
    pub core: u8,
    /// Generated admission window in microseconds.
    pub capacity_us: u32,
    /// Reserved critical time in microseconds.
    pub reserve_us: u32,
    /// Sum of admitted task budgets in microseconds.
    pub admitted_us: u32,
}

impl CoreAdmissionEvidence {
    fn validate(&self) -> Result<(), EvidenceError> {
        if self.capacity_us == 0
            || self.reserve_us > self.capacity_us
            || self
                .admitted_us
                .checked_add(self.reserve_us)
                .is_none_or(|total| total > self.capacity_us)
        {
            return Err(EvidenceError::InvalidFieldMatrix("per-core admission"));
        }
        Ok(())
    }
}

/// Direct target full-system MCS acceptance evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McsSystemEvidence {
    /// Exact [`MCS_SYSTEM_SCHEMA`].
    pub schema: String,
    /// Exact [`RecordKind::FullSystem`].
    pub record_kind: RecordKind,
    /// Exactly one direct target.
    pub target: TargetClass,
    /// Exact target-session artifact graph.
    pub target_session: TargetSession,
    /// Matching target Worker-component record.
    pub worker_component: EvidenceReference,
    /// Matching target root-TCB record.
    pub root_tcb: EvidenceReference,
    /// Exact generated topology.
    pub topology_sha256: Sha256Hex,
    /// Exactly four sorted per-core admission rows.
    pub core_admission: Vec<CoreAdmissionEvidence>,
    /// Sorted timeout/fault/Reply/liveness/performance outcomes.
    pub outcomes: Vec<IntegrationOutcome>,
    /// Sorted raw target artifacts.
    pub raw_evidence: Vec<EvidenceArtifact>,
    /// Exact target verdict.
    pub verdict: Verdict,
    /// Sorted blockers; empty only on PASS.
    #[serde(default)]
    pub blockers: Vec<String>,
}

impl McsSystemEvidence {
    /// Validate one direct target full-system record.
    pub fn validate(&self) -> Result<(), EvidenceError> {
        if self.schema != MCS_SYSTEM_SCHEMA
            || self.record_kind != RecordKind::FullSystem
            || self.target_session.target != self.target
            || self.worker_component.record_kind != RecordKind::TargetComponent
            || self.root_tcb.record_kind != RecordKind::RootTcb
        {
            return Err(EvidenceError::WrongRecordKind);
        }
        self.target_session.validate()?;
        self.worker_component.sha256.validate("Worker component")?;
        self.root_tcb.sha256.validate("root TCB")?;
        self.topology_sha256.validate("topology")?;
        validate_sorted_unique(&self.core_admission, 4, "core admission")?;
        if self.core_admission.len() != 4
            || self
                .core_admission
                .iter()
                .enumerate()
                .any(|(index, row)| usize::from(row.core) != index || row.validate().is_err())
        {
            return Err(EvidenceError::InvalidFieldMatrix("four-core MCS admission"));
        }
        validate_sorted_unique(&self.outcomes, MAX_OUTCOMES, "system outcomes")?;
        if self.verdict == Verdict::Pass {
            validate_required_pass_outcomes(
                &self.outcomes,
                &REQUIRED_SYSTEM_OUTCOMES,
                "system outcome matrix",
            )?;
        }
        validate_sorted_unique(&self.raw_evidence, MAX_RAW_ARTIFACTS, "raw evidence")?;
        if self.verdict == Verdict::Pass
            && (self.outcomes.is_empty() || self.raw_evidence.is_empty())
        {
            return Err(EvidenceError::InvalidFieldMatrix(
                "accepted system evidence",
            ));
        }
        validate_blockers(self.verdict, &self.blockers)?;
        scan_sensitive(self)
    }
}

/// Final Worker-runtime release evidence.  It contains no singular target and
/// references exactly the component/root/full-system record for QEMU and Pi 4.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerReleaseEvidence {
    /// Exact [`WORKER_RELEASE_SCHEMA`].
    pub schema: String,
    /// Exact [`RecordKind::Release`].
    pub record_kind: RecordKind,
    /// Must be exactly `worker-runtime`.
    pub scope: String,
    /// Exactly six sorted immutable acceptance references.
    pub acceptance_records: Vec<EvidenceReference>,
    /// Sorted release-required integration references.
    pub integration_evidence: Vec<EvidenceReference>,
    /// Exact release verdict.
    pub verdict: Verdict,
    /// Sorted blockers; empty only on PASS.
    #[serde(default)]
    pub blockers: Vec<String>,
}

impl WorkerReleaseEvidence {
    /// Validate the closed six-reference shape without inferring either target.
    pub fn validate(&self) -> Result<(), EvidenceError> {
        if self.schema != WORKER_RELEASE_SCHEMA
            || self.record_kind != RecordKind::Release
            || self.scope != "worker-runtime"
        {
            return Err(EvidenceError::WrongRecordKind);
        }
        validate_sorted_unique(&self.acceptance_records, 6, "acceptance records")?;
        if self.acceptance_records.len() != 6 {
            return Err(EvidenceError::InvalidFieldMatrix("six acceptance records"));
        }
        let component_count = self
            .acceptance_records
            .iter()
            .filter(|record| record.record_kind == RecordKind::TargetComponent)
            .count();
        let root_count = self
            .acceptance_records
            .iter()
            .filter(|record| record.record_kind == RecordKind::RootTcb)
            .count();
        let system_count = self
            .acceptance_records
            .iter()
            .filter(|record| record.record_kind == RecordKind::FullSystem)
            .count();
        if (component_count, root_count, system_count) != (2, 2, 2) {
            return Err(EvidenceError::InvalidFieldMatrix(
                "two records per acceptance layer",
            ));
        }
        for reference in &self.acceptance_records {
            validate_identifier(&reference.id, "acceptance reference")?;
            reference.sha256.validate("acceptance reference")?;
        }
        validate_sorted_unique(
            &self.integration_evidence,
            MAX_INTEGRATION_REFERENCES,
            "release integration evidence",
        )?;
        if self
            .integration_evidence
            .iter()
            .any(|record| record.record_kind != RecordKind::WorkerIntegration)
        {
            return Err(EvidenceError::WrongRecordKind);
        }
        validate_blockers(self.verdict, &self.blockers)?;
        scan_sensitive(self)
    }
}

/// Validate a full-system record against the exact component and root-TCB
/// bytes it names, including target and immutable artifact-graph equality.
pub fn validate_system_graph(
    system: &McsSystemEvidence,
    component_bytes: &[u8],
    root_bytes: &[u8],
) -> Result<(), EvidenceError> {
    system.validate()?;
    system.worker_component.sha256.verify(component_bytes)?;
    system.root_tcb.sha256.verify(root_bytes)?;
    let component: WorkerComponentEvidence = serde_json::from_slice(component_bytes)
        .map_err(|error| EvidenceError::InvalidJson(error.to_string()))?;
    let root: RootTcbEvidence = serde_json::from_slice(root_bytes)
        .map_err(|error| EvidenceError::InvalidJson(error.to_string()))?;
    component.validate()?;
    root.validate()?;
    if component.target != system.target
        || root.target != system.target
        || component.target_session != system.target_session
        || root.target_session != system.target_session
        || root.worker_component.sha256 != system.worker_component.sha256
        || component.topology_sha256 != system.topology_sha256
        || root.topology_sha256 != system.topology_sha256
    {
        return Err(EvidenceError::InvalidFieldMatrix(
            "cross-layer target/artifact graph",
        ));
    }
    Ok(())
}

/// Parsed supported evidence record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidatedEvidence {
    /// One host-integration row.
    Integration(Box<WorkerIntegrationEvidence>),
    /// One target Worker component record.
    Component(Box<WorkerComponentEvidence>),
    /// One target root-TCB record.
    RootTcb(Box<RootTcbEvidence>),
    /// One target full-system record.
    System(Box<McsSystemEvidence>),
    /// One two-target Worker-runtime release record.
    Release(Box<WorkerReleaseEvidence>),
}

/// Parse and validate a supported evidence record from bounded JSON bytes.
pub fn parse_evidence(bytes: &[u8]) -> Result<ValidatedEvidence, EvidenceError> {
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(EvidenceError::RecordTooLarge);
    }
    let envelope: EvidenceEnvelope = serde_json::from_slice(bytes)
        .map_err(|error| EvidenceError::InvalidJson(error.to_string()))?;
    match (envelope.schema.as_str(), envelope.record_kind) {
        (WORKER_INTEGRATION_SCHEMA, RecordKind::WorkerIntegration) => {
            let record: WorkerIntegrationEvidence = serde_json::from_slice(bytes)
                .map_err(|error| EvidenceError::InvalidJson(error.to_string()))?;
            record.validate()?;
            Ok(ValidatedEvidence::Integration(Box::new(record)))
        }
        (WORKER_COMPONENT_SCHEMA, RecordKind::TargetComponent) => {
            let record: WorkerComponentEvidence = serde_json::from_slice(bytes)
                .map_err(|error| EvidenceError::InvalidJson(error.to_string()))?;
            record.validate()?;
            Ok(ValidatedEvidence::Component(Box::new(record)))
        }
        (ROOT_TCB_SCHEMA, RecordKind::RootTcb) => {
            let record: RootTcbEvidence = serde_json::from_slice(bytes)
                .map_err(|error| EvidenceError::InvalidJson(error.to_string()))?;
            record.validate()?;
            Ok(ValidatedEvidence::RootTcb(Box::new(record)))
        }
        (MCS_SYSTEM_SCHEMA, RecordKind::FullSystem) => {
            let record: McsSystemEvidence = serde_json::from_slice(bytes)
                .map_err(|error| EvidenceError::InvalidJson(error.to_string()))?;
            record.validate()?;
            Ok(ValidatedEvidence::System(Box::new(record)))
        }
        (WORKER_RELEASE_SCHEMA, RecordKind::Release) => {
            let record: WorkerReleaseEvidence = serde_json::from_slice(bytes)
                .map_err(|error| EvidenceError::InvalidJson(error.to_string()))?;
            record.validate()?;
            Ok(ValidatedEvidence::Release(Box::new(record)))
        }
        _ => Err(EvidenceError::WrongRecordKind),
    }
}

#[derive(Debug, Deserialize)]
struct EvidenceEnvelope {
    schema: String,
    record_kind: RecordKind,
}

fn validate_identifier(value: &str, field: &'static str) -> Result<(), EvidenceError> {
    if value.is_empty()
        || value.len() > MAX_ID_BYTES
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/' | b':')
        })
    {
        return Err(EvidenceError::InvalidIdentifier(field));
    }
    Ok(())
}

fn validate_label(value: &str, field: &'static str) -> Result<(), EvidenceError> {
    if value.is_empty()
        || value.len() > MAX_LABEL_BYTES
        || value.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(EvidenceError::InvalidIdentifier(field));
    }
    Ok(())
}

fn validate_sorted_unique<T: Ord>(
    values: &[T],
    maximum: usize,
    field: &'static str,
) -> Result<(), EvidenceError> {
    if values.len() > maximum || values.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(EvidenceError::InvalidList(field));
    }
    Ok(())
}

fn validate_required_pass_outcomes(
    outcomes: &[IntegrationOutcome],
    required: &[&str],
    field: &'static str,
) -> Result<(), EvidenceError> {
    if outcomes.len() != required.len()
        || outcomes
            .iter()
            .zip(required)
            .any(|(outcome, expected)| outcome.id != *expected || outcome.result != "pass")
    {
        return Err(EvidenceError::InvalidFieldMatrix(field));
    }
    Ok(())
}

fn validate_blockers(verdict: Verdict, blockers: &[String]) -> Result<(), EvidenceError> {
    if blockers.len() > MAX_BLOCKERS
        || blockers.iter().any(|blocker| {
            blocker.is_empty()
                || blocker.len() > MAX_LABEL_BYTES
                || blocker.bytes().any(|byte| byte.is_ascii_control())
        })
        || blockers.windows(2).any(|pair| pair[0] >= pair[1])
        || (verdict == Verdict::Pass) != blockers.is_empty()
    {
        return Err(EvidenceError::InvalidVerdict);
    }
    Ok(())
}

fn scan_sensitive<T: Serialize>(record: &T) -> Result<(), EvidenceError> {
    let bytes = serde_json::to_vec(record)
        .map_err(|error| EvidenceError::InvalidJson(error.to_string()))?;
    let lowered = String::from_utf8_lossy(&bytes).to_ascii_lowercase();
    const FORBIDDEN: &[&str] = &[
        "auth_token",
        "authorization:",
        "bearer ",
        "private_key",
        "secret_key",
        "password",
        "cptr",
        "capability_value",
        "raw_badge",
    ];
    if FORBIDDEN.iter().any(|marker| lowered.contains(marker)) {
        return Err(EvidenceError::SensitiveMaterial);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(label: &str) -> Sha256Hex {
        Sha256Hex::digest(label.as_bytes())
    }

    fn target(target: TargetClass) -> TargetSession {
        TargetSession {
            target,
            source_sha256: hash("source"),
            manifest_sha256: hash("manifest"),
            kernel_sha256: hash("kernel"),
            root_image_sha256: hash("root"),
            driver_archive_sha256: hash("driver"),
            driver_manifest_sha256: hash("driver-manifest"),
            cyw43_coexistence_record_sha256: hash("cyw43-coexistence"),
            worker_archive_sha256: hash("workers"),
            worker_image_manifest_sha256: hash("worker-manifest"),
            worker_abi_sha256: hash("worker-abi"),
        }
    }

    fn integration_references() -> Vec<EvidenceReference> {
        REQUIRED_WORKER_INTEGRATIONS
            .iter()
            .map(|id| EvidenceReference {
                id: (*id).to_owned(),
                record_kind: RecordKind::WorkerIntegration,
                sha256: hash(id),
            })
            .collect()
    }

    #[test]
    fn standalone_target_session_is_strict_and_bounded() {
        let session = target(TargetClass::Qemu);
        let bytes = serde_json::to_vec(&session).expect("serialize target session");
        assert_eq!(parse_target_session(&bytes), Ok(session));

        let mut value: serde_json::Value =
            serde_json::from_slice(&bytes).expect("parse target session JSON");
        value["unexpected"] = serde_json::Value::Bool(true);
        assert!(matches!(
            parse_target_session(
                &serde_json::to_vec(&value).expect("serialize unknown-field session")
            ),
            Err(EvidenceError::InvalidJson(_))
        ));
        assert_eq!(
            parse_target_session(&vec![b'x'; MAX_RECORD_BYTES + 1]),
            Err(EvidenceError::RecordTooLarge)
        );
    }

    fn integration() -> WorkerIntegrationEvidence {
        WorkerIntegrationEvidence {
            schema: WORKER_INTEGRATION_SCHEMA.to_owned(),
            record_kind: RecordKind::WorkerIntegration,
            dependency_id: "worker-control".to_owned(),
            owner_milestone: "m26e-host-worker-integration".to_owned(),
            obligation: IntegrationObligation::RoleRequired,
            observed_mode: ObservedMode::Live,
            dependency_graph_sha256: hash("graph"),
            manifest_sha256: hash("manifest"),
            component_sha256: hash("component"),
            config_sha256: hash("config"),
            artifact_sha256: Some(hash("artifact")),
            host: HostEnvironment {
                profile: "macos-release".to_owned(),
                os: "macOS 26".to_owned(),
                architecture: "arm64".to_owned(),
                provider_version: None,
            },
            target_session: Some(target(TargetClass::Qemu)),
            execution_proof: ExecutionProof::Qemu,
            outcomes: vec![IntegrationOutcome {
                id: "ready".to_owned(),
                class: "observation".to_owned(),
                result: "ready".to_owned(),
            }],
            raw_evidence: vec![EvidenceArtifact {
                id: "qemu-serial".to_owned(),
                sha256: hash("serial"),
                bytes: 128,
            }],
            verdict: Verdict::Pass,
            blockers: Vec::new(),
        }
    }

    #[test]
    fn integration_record_round_trips_and_validates() {
        let bytes = serde_json::to_vec(&integration()).expect("serialize evidence");
        assert!(matches!(
            parse_evidence(&bytes).expect("valid evidence"),
            ValidatedEvidence::Integration(_)
        ));
    }

    #[test]
    fn mock_mode_cannot_claim_qemu_proof() {
        let mut record = integration();
        record.observed_mode = ObservedMode::Mock;
        assert_eq!(
            record.validate(),
            Err(EvidenceError::InvalidFieldMatrix("target proof mode"))
        );
    }

    #[test]
    fn future_row_cannot_be_promoted_live() {
        let mut record = integration();
        record.obligation = IntegrationObligation::Future;
        assert_eq!(
            record.validate(),
            Err(EvidenceError::InvalidFieldMatrix("future integration live"))
        );
    }

    #[test]
    fn pass_requires_no_blockers_and_fail_requires_one() {
        let mut record = integration();
        record.blockers.push("provider missing".to_owned());
        assert_eq!(record.validate(), Err(EvidenceError::InvalidVerdict));
        record.verdict = Verdict::Fail;
        assert!(record.validate().is_ok());
        record.blockers.clear();
        assert_eq!(record.validate(), Err(EvidenceError::InvalidVerdict));
    }

    #[test]
    fn secret_and_raw_capability_markers_are_rejected() {
        let mut record = integration();
        record.outcomes[0].result = "Bearer abc".to_owned();
        assert_eq!(record.validate(), Err(EvidenceError::SensitiveMaterial));
        record.outcomes[0].result = "raw_badge=1".to_owned();
        assert_eq!(record.validate(), Err(EvidenceError::SensitiveMaterial));
    }

    #[test]
    fn component_requires_exact_three_role_matrix() {
        let roles = [
            WorkerRole::WorkerGpu,
            WorkerRole::WorkerHeartbeat,
            WorkerRole::WorkerLora,
        ];
        let mut workers = roles
            .into_iter()
            .map(|role| WorkerRoleObservation {
                identity: WorkerIdentity {
                    role,
                    slot: 0,
                    lease_epoch: 1,
                    supervisor_generation: role as u64 + 1,
                    cap_generation: 1,
                },
                state: WorkerState {
                    declaration: DeclarationState::Executable,
                    lifecycle: LifecycleState::Ready,
                    artifact: ArtifactState::Verified,
                    receipt: match role {
                        WorkerRole::WorkerHeartbeat => ReceiptState::None,
                        WorkerRole::WorkerGpu | WorkerRole::WorkerLora => ReceiptState::Confirmed,
                        WorkerRole::WorkerBus => ReceiptState::None,
                    },
                    execution_proof: ExecutionProof::Qemu,
                },
                image_sha256: hash(match role {
                    WorkerRole::WorkerHeartbeat => "heart",
                    WorkerRole::WorkerGpu => "gpu",
                    WorkerRole::WorkerLora => "lora",
                    WorkerRole::WorkerBus => "bus",
                }),
                ready_sequence: 1,
                completion_sequence: 2,
                endpoint_badge: 1_u64 << (role as u8),
                fault_badge: 1_u64 << (8 + role as u8),
                core: role as u8,
                scheduling_context: WorkerSchedulingContext {
                    budget_us: 100,
                    period_us: 1_000,
                },
                object_inventory: KernelObjectInventory {
                    tcbs: 1,
                    scheduling_contexts: 1,
                    reply_objects: 0,
                    vspaces: 1,
                    cnodes: 1,
                    page_tables: 8,
                    asids: 1,
                    frames: 16,
                    endpoints: 0,
                    notifications: 1,
                    fault_caps: 1,
                    timeout_fault_caps: 1,
                    cspace_slots: 64,
                    untyped_bytes: 1_048_576,
                },
            })
            .collect::<Vec<_>>();
        workers.sort();
        let record = WorkerComponentEvidence {
            schema: WORKER_COMPONENT_SCHEMA.to_owned(),
            record_kind: RecordKind::TargetComponent,
            target: TargetClass::Qemu,
            target_session: target(TargetClass::Qemu),
            topology_sha256: hash("qemu-topology"),
            workers,
            integration_evidence: integration_references(),
            outcomes: REQUIRED_COMPONENT_OUTCOMES
                .iter()
                .map(|id| IntegrationOutcome {
                    id: (*id).to_owned(),
                    class: "observation".to_owned(),
                    result: "pass".to_owned(),
                })
                .collect(),
            raw_evidence: vec![EvidenceArtifact {
                id: "qemu-worker-transcript".to_owned(),
                sha256: hash("worker-transcript"),
                bytes: 128,
            }],
            verdict: Verdict::Pass,
            blockers: Vec::new(),
        };
        assert!(record.validate().is_ok());
        let mut missing = record.clone();
        missing.workers.pop();
        assert_eq!(
            missing.validate(),
            Err(EvidenceError::InvalidFieldMatrix("mandatory role matrix"))
        );

        let mut overlapping_badges = record.clone();
        overlapping_badges.workers[1].endpoint_badge = overlapping_badges.workers[0].fault_badge;
        assert_eq!(
            overlapping_badges.validate(),
            Err(EvidenceError::InvalidFieldMatrix(
                "Worker badge/core inventory"
            ))
        );

        let mut invalid_sc = record.clone();
        invalid_sc.workers[0].scheduling_context.budget_us = 1_001;
        invalid_sc.workers[0].scheduling_context.period_us = 1_000;
        assert_eq!(
            invalid_sc.validate(),
            Err(EvidenceError::InvalidFieldMatrix(
                "Worker scheduling context"
            ))
        );

        let mut partial_outcomes = record;
        partial_outcomes.outcomes.pop();
        assert_eq!(
            partial_outcomes.validate(),
            Err(EvidenceError::InvalidFieldMatrix(
                "component outcome matrix"
            ))
        );
    }

    #[test]
    fn sha256_reference_verification_detects_tampering() {
        let digest = Sha256Hex::digest(b"record");
        assert!(digest.verify(b"record").is_ok());
        assert_eq!(
            digest.verify(b"tampered"),
            Err(EvidenceError::DigestMismatch)
        );
    }
}
