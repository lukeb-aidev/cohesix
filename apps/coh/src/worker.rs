// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Project generated Worker bounds and render independent Worker state axes in coh.
// Author: Lukas Bower

//! Read-only Worker contract and observation helpers for `coh`.
//!
//! A control write and a provider result are deliberately absent from
//! [`WorkerProjection`]. They are operation outcomes, not evidence that a
//! Worker reached READY, emitted a receipt, or executed on a target.

use cohesix_worker_evidence::{
    ArtifactState, DeclarationState, ExecutionProof, LifecycleState, ReceiptState, WorkerState,
};

use crate::policy::generated;

/// One compiler-generated Worker role declaration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkerRoleContract {
    /// Canonical role label.
    pub role: &'static str,
    /// Static `executable` or `model-only` declaration.
    pub declaration: &'static str,
    /// Compiler-admitted simultaneous executable slots.
    pub executable_slots: u16,
    /// Ticket scope used for role attachment.
    pub ticket_scope: &'static str,
    /// Canonical sharded telemetry path template.
    pub telemetry_path_template: &'static str,
    /// Optional role-specific lease path template.
    pub lease_path_template: &'static str,
}

/// Complete compiler-generated Worker runtime bounds consumed by `coh`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkerRuntimeContract {
    /// Fixed Worker task ABI schema.
    pub task_abi_schema: &'static str,
    /// Fixed Worker task ABI version.
    pub task_abi_version: u16,
    /// Structured Worker observation schema.
    pub observation_schema: &'static str,
    /// Host-integration evidence schema.
    pub integration_evidence_schema: &'static str,
    /// Maximum simultaneously live executable Worker tasks.
    pub maximum_live_tasks: u16,
    /// Canonical sharded Worker telemetry template.
    pub canonical_telemetry_template: &'static str,
    /// Number of digest bits used for the shard label.
    pub shard_bits: u8,
    /// Whether the compatibility `/worker` alias is enabled.
    pub legacy_worker_alias: bool,
    /// Generated lifecycle vocabulary.
    pub lifecycle_vocabulary: &'static [&'static str],
    /// Generated receipt vocabulary.
    pub receipt_vocabulary: &'static [&'static str],
    /// Generated artifact vocabulary.
    pub artifact_vocabulary: &'static [&'static str],
    /// Generated execution-proof vocabulary.
    pub execution_proof_vocabulary: &'static [&'static str],
    /// Exact selected-profile role matrix.
    pub roles: Vec<WorkerRoleContract>,
}

/// Public Worker state axes rendered without collapsing them into health.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkerProjection<'a> {
    /// Canonical Worker role label.
    pub role: &'a str,
    /// Independent compiler/runtime/artifact/receipt/proof state.
    pub state: WorkerState,
}

impl WorkerProjection<'_> {
    /// Render declaration, lifecycle, artifact, receipt, and proof separately.
    #[must_use]
    pub fn render(&self) -> String {
        format!(
            "worker role={} declaration={} lifecycle={} artifact={} receipt={} proof={}",
            self.role,
            declaration_label(self.state.declaration),
            lifecycle_label(self.state.lifecycle),
            artifact_label(self.state.artifact),
            receipt_label(self.state.receipt),
            proof_label(self.state.execution_proof),
        )
    }
}

/// Return all compiler-generated Worker runtime bounds for the selected profile.
#[must_use]
pub fn runtime_contract() -> WorkerRuntimeContract {
    WorkerRuntimeContract {
        task_abi_schema: generated::COH_WORKER_TASK_ABI_SCHEMA,
        task_abi_version: generated::COH_WORKER_TASK_ABI_VERSION,
        observation_schema: generated::COH_WORKER_OBSERVATION_SCHEMA,
        integration_evidence_schema: generated::COH_WORKER_INTEGRATION_EVIDENCE_SCHEMA,
        maximum_live_tasks: generated::COH_WORKER_MAXIMUM_LIVE_TASKS,
        canonical_telemetry_template: generated::COH_WORKER_CANONICAL_TELEMETRY_TEMPLATE,
        shard_bits: generated::COH_WORKER_SHARD_BITS,
        legacy_worker_alias: generated::COH_WORKER_LEGACY_ALIAS,
        lifecycle_vocabulary: generated::COH_WORKER_LIFECYCLE_VOCABULARY,
        receipt_vocabulary: generated::COH_WORKER_RECEIPT_VOCABULARY,
        artifact_vocabulary: generated::COH_WORKER_ARTIFACT_VOCABULARY,
        execution_proof_vocabulary: generated::COH_WORKER_EXECUTION_PROOF_VOCABULARY,
        roles: generated::COH_WORKER_ROLE_CONTRACTS
            .iter()
            .map(|role| WorkerRoleContract {
                role: role.role,
                declaration: role.declaration,
                executable_slots: role.executable_slots,
                ticket_scope: role.ticket_scope,
                telemetry_path_template: role.telemetry_path_template,
                lease_path_template: role.lease_path_template,
            })
            .collect(),
    }
}

const fn declaration_label(state: DeclarationState) -> &'static str {
    match state {
        DeclarationState::Executable => "executable",
        DeclarationState::ModelOnly => "model-only",
    }
}

const fn lifecycle_label(state: LifecycleState) -> &'static str {
    match state {
        LifecycleState::Absent => "absent",
        LifecycleState::Queued => "queued",
        LifecycleState::Starting => "starting",
        LifecycleState::Ready => "ready",
        LifecycleState::Closing => "closing",
        LifecycleState::Faulted => "faulted",
        LifecycleState::Terminal => "terminal",
    }
}

const fn artifact_label(state: ArtifactState) -> &'static str {
    match state {
        ArtifactState::Missing => "missing",
        ArtifactState::Verified => "verified",
        ArtifactState::Mismatch => "mismatch",
    }
}

const fn receipt_label(state: ReceiptState) -> &'static str {
    match state {
        ReceiptState::None => "none",
        ReceiptState::Pending => "pending",
        ReceiptState::Confirmed => "confirmed",
        ReceiptState::Rejected => "rejected",
        ReceiptState::Stale => "stale",
    }
}

const fn proof_label(proof: ExecutionProof) -> &'static str {
    match proof {
        ExecutionProof::None => "none",
        ExecutionProof::HostModel => "host-model",
        ExecutionProof::Qemu => "qemu",
        ExecutionProof::FreshPi => "fresh-pi",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_contract_exposes_exact_executable_matrix_and_bounds() {
        let contract = runtime_contract();
        assert_eq!(contract.task_abi_schema, "worker-task-abi/v2");
        assert_eq!(contract.task_abi_version, 2);
        assert_eq!(contract.observation_schema, "cohesix-worker-observation/v1");
        assert_eq!(
            contract.integration_evidence_schema,
            "cohesix-worker-integration-evidence/v1"
        );
        assert_eq!(contract.maximum_live_tasks, 256);
        assert_eq!(contract.shard_bits, 6);
        assert!(contract.legacy_worker_alias);
        assert_eq!(
            contract.canonical_telemetry_template,
            "/shard/<label>/worker/<id>/telemetry"
        );
        assert_eq!(
            contract
                .roles
                .iter()
                .map(|role| (role.role, role.declaration, role.executable_slots))
                .collect::<Vec<_>>(),
            vec![
                ("worker-heartbeat", "executable", 1),
                ("worker-gpu", "executable", 127),
                ("worker-bus", "model-only", 0),
                ("worker-lora", "executable", 128),
            ]
        );
    }

    #[test]
    fn renderer_keeps_all_worker_state_axes_distinct() {
        let projection = WorkerProjection {
            role: "worker-lora",
            state: WorkerState {
                declaration: DeclarationState::Executable,
                lifecycle: LifecycleState::Queued,
                artifact: ArtifactState::Verified,
                receipt: ReceiptState::Pending,
                execution_proof: ExecutionProof::None,
            },
        };
        assert_eq!(
            projection.render(),
            "worker role=worker-lora declaration=executable lifecycle=queued artifact=verified receipt=pending proof=none"
        );
        assert!(!projection.render().contains("lifecycle=ready"));
    }
}
