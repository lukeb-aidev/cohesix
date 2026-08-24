// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Project generated Worker declarations, canonical paths, and independent state axes in cohsh.
// Author: Lukas Bower

//! Read-only Worker contract helpers for `cohsh`.
//!
//! These helpers deliberately keep a successful control write separate from
//! runtime READY. `OK SPAWN` and `OK KILL` report bounded request admission;
//! only a later structured target observation can supply lifecycle state.

use anyhow::{anyhow, Result};
use cohesix_worker_evidence::{
    ArtifactState, DeclarationState, ExecutionProof, LifecycleState, ReceiptState, WorkerState,
};
use cohsh_core::{parse_ack, AckStatus};
use sha2::{Digest, Sha256};

/// One compiler-generated Worker role declaration exposed by `cohsh`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkerRoleContract {
    /// Canonical role label.
    pub role: &'static str,
    /// `executable` or `model-only`.
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

/// Meaning of one control acknowledgement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerControlAdmission {
    /// The control write was admitted; this is not Worker READY.
    Admitted,
    /// The control write was rejected before admission.
    Rejected,
}

/// Public axes rendered for one Worker observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkerProjection<'a> {
    /// Canonical Worker role label.
    pub role: &'a str,
    /// Optional control-write outcome, independent of lifecycle.
    pub control: Option<WorkerControlAdmission>,
    /// Independent declaration, lifecycle, artifact, receipt, and proof axes.
    pub state: WorkerState,
}

impl WorkerProjection<'_> {
    /// Render every state axis explicitly without collapsing it into health.
    #[must_use]
    pub fn render(&self) -> String {
        format!(
            "worker role={} control={} declaration={} lifecycle={} artifact={} receipt={} proof={}",
            self.role,
            control_label(self.control),
            declaration_label(self.state.declaration),
            lifecycle_label(self.state.lifecycle),
            artifact_label(self.state.artifact),
            receipt_label(self.state.receipt),
            proof_label(self.state.execution_proof),
        )
    }
}

/// Return the selected profile's generated Worker role matrix.
#[must_use]
pub fn role_contracts() -> Vec<WorkerRoleContract> {
    crate::generated_client::WORKER_ROLE_CONTRACTS
        .iter()
        .map(|contract| WorkerRoleContract {
            role: contract.role,
            declaration: contract.declaration,
            executable_slots: contract.executable_slots,
            ticket_scope: contract.ticket_scope,
            telemetry_path_template: contract.telemetry_path_template,
            lease_path_template: contract.lease_path_template,
        })
        .collect()
}

/// Return the compiler-admitted maximum live executable task count.
#[must_use]
pub const fn maximum_live_tasks() -> u16 {
    crate::generated_client::WORKER_MAXIMUM_LIVE_TASKS
}

/// Return the exact canonical telemetry path for a public Worker id.
pub fn canonical_telemetry_path(worker_id: &str) -> Result<String> {
    validate_component(worker_id, "worker id")?;
    let shard = shard_label(worker_id);
    Ok(crate::generated_client::WORKER_CANONICAL_TELEMETRY_TEMPLATE
        .replace("<label>", shard.as_str())
        .replace("<id>", worker_id))
}

/// Return the compatibility `/worker` path only when the profile enables it.
pub fn legacy_telemetry_path(worker_id: &str) -> Result<String> {
    if !crate::generated_client::WORKER_LEGACY_ALIAS {
        return Err(anyhow!(
            "legacy /worker alias is disabled by the generated profile"
        ));
    }
    validate_component(worker_id, "worker id")?;
    Ok(format!("/worker/{worker_id}/telemetry"))
}

/// Classify only Worker-control ACKs, never a lifecycle or proof state.
#[must_use]
pub fn classify_control_ack(line: &str) -> Option<WorkerControlAdmission> {
    let ack = parse_ack(line)?;
    if !matches!(ack.verb, "SPAWN" | "KILL") {
        return None;
    }
    Some(match ack.status {
        AckStatus::Ok => WorkerControlAdmission::Admitted,
        AckStatus::Err => WorkerControlAdmission::Rejected,
    })
}

fn shard_label(worker_id: &str) -> String {
    if crate::generated_client::WORKER_SHARD_BITS == 0 {
        return "00".to_owned();
    }
    let digest = Sha256::digest(worker_id.as_bytes());
    let mut shard = digest[0];
    if crate::generated_client::WORKER_SHARD_BITS < 8 {
        shard >>= 8 - crate::generated_client::WORKER_SHARD_BITS;
    }
    format!("{shard:02x}")
}

fn validate_component(value: &str, name: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || value == "."
        || value == ".."
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(anyhow!("{name} is not a bounded path component"));
    }
    Ok(())
}

const fn control_label(control: Option<WorkerControlAdmission>) -> &'static str {
    match control {
        Some(WorkerControlAdmission::Admitted) => "admitted",
        Some(WorkerControlAdmission::Rejected) => "rejected",
        None => "none",
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
    fn generated_role_matrix_preserves_multiworker_topology() {
        let roles = role_contracts();
        assert_eq!(roles.len(), 4);
        assert_eq!(maximum_live_tasks(), 37);
        for (role, executable_slots) in [
            ("worker-heartbeat", 1),
            ("worker-gpu", 15),
            ("worker-lora", 21),
        ] {
            assert_eq!(
                roles
                    .iter()
                    .find(|contract| contract.role == role)
                    .map(|contract| (contract.declaration, contract.executable_slots)),
                Some(("executable", executable_slots))
            );
        }
        assert_eq!(
            roles
                .iter()
                .find(|role| role.role == "worker-bus")
                .map(|role| (role.declaration, role.executable_slots)),
            Some(("model-only", 0))
        );
    }

    #[test]
    fn canonical_path_uses_generated_shard_and_gates_alias() {
        assert_eq!(
            canonical_telemetry_path("worker-1").expect("canonical path"),
            "/shard/13/worker/worker-1/telemetry"
        );
        assert_eq!(
            legacy_telemetry_path("worker-1").expect("enabled compatibility alias"),
            "/worker/worker-1/telemetry"
        );
        assert!(canonical_telemetry_path("../worker-1").is_err());
    }

    #[test]
    fn control_ack_is_admission_only_and_axes_remain_distinct() {
        assert_eq!(
            classify_control_ack("OK SPAWN path=/queen/ctl bytes=16"),
            Some(WorkerControlAdmission::Admitted)
        );
        assert_eq!(
            classify_control_ack("ERR KILL reason=not-found"),
            Some(WorkerControlAdmission::Rejected)
        );
        assert_eq!(
            classify_control_ack("OK CAT path=/proc/lifecycle/state"),
            None
        );

        let projection = WorkerProjection {
            role: "worker-lora",
            control: Some(WorkerControlAdmission::Admitted),
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
            "worker role=worker-lora control=admitted declaration=executable lifecycle=queued artifact=verified receipt=pending proof=none"
        );
        assert!(!projection.render().contains("lifecycle=ready"));
    }
}
