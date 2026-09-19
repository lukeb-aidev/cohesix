// Author: Lukas Bower
// Purpose: Require canonical signed causal records, immutable artifacts and exact authority bindings before deriving a terminal evidence projection.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]

//! Host-only evidence validation. Trust comes from locally configured keys and
//! phase ownership, never from a graph's labels, signatures alone, or file presence.

use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::Path;

pub mod producer;
pub mod ticket;

pub const SCHEMA: &str = "cohesix-causal-evidence/v1";
pub const MAX_GRAPH_BYTES: usize = 262_144;
pub const MAX_NODES: usize = 64;
const MAX_ARTIFACT_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("ELIMIT evidence-bound")]
    Limit,
    #[error("EPERM evidence-schema-or-canonical-bytes")]
    Schema,
    #[error("EPERM evidence-identity-or-manifest")]
    Identity,
    #[error("EPERM evidence-signature-or-custodian")]
    Signature,
    #[error("EPERM evidence-stale-or-chronology")]
    Chronology,
    #[error("EPERM evidence-causal-chain-or-terminal")]
    Causality,
    #[error("EUNAVAILABLE evidence-artifact")]
    Missing,
    #[error("EPERM evidence-artifact-digest")]
    Digest,
}

/// Phase owners are configured out of band. A provider signing key cannot sign grants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Intent,
    Facts,
    Approval,
    Grant,
    Execution,
    Observation,
    Verification,
    Worker,
    Terminal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub ticket_id: String,
    pub subject: String,
    pub action: String,
    pub idempotency_key: String,
    pub writer_epoch: u64,
    pub target_manifest_sha256: String,
    pub provider_graph_sha256: String,
    pub implementation_graph_sha256: String,
    pub use_case_graph_sha256: String,
    pub component_sha256: BTreeMap<String, String>,
    pub worker: Option<WorkerIdentity>,
    /// Recovery is a separately admitted ticket; this link alone never proves reversal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_of: Option<RecoveryLink>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryLink {
    pub ticket_id: String,
    pub graph_sha256: String,
    pub terminal_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerIdentity {
    pub id: String,
    pub role: String,
    pub generation: u64,
    pub image_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub sha256: String,
    pub bytes: u64,
    /// Immutable CAS key only; callers never fetch arbitrary URLs from a graph.
    pub media_type: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Admitted,
    Observed,
    Verified,
    Succeeded,
    Failed,
    Cancelled,
    Expired,
}

/// Every signed node repeats the entire binding to prevent cross-ticket splicing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub schema: String,
    pub binding: Binding,
    pub kind: Kind,
    pub source: String,
    pub sequence: u64,
    pub observed_unix_ms: u64,
    pub expires_unix_ms: u64,
    pub parents: Vec<String>,
    pub artifacts: Vec<Artifact>,
    pub native_identity: Option<String>,
    pub resource_generation: u64,
    pub event_cursor: Option<String>,
    pub outcome: Outcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedRecord {
    pub record: Record,
    pub sha256: String,
    pub key_id: String,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Graph {
    pub schema: String,
    pub binding: Binding,
    pub records: Vec<SignedRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustedKey {
    pub id: String,
    pub source: String,
    pub public_key: String,
    pub kinds: BTreeSet<Kind>,
    pub not_before_unix_ms: u64,
    pub not_after_unix_ms: u64,
}

/// Supplied by the operator's verifier configuration, never loaded from the pack.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trust {
    pub schema: String,
    pub expected: Binding,
    pub keys: Vec<TrustedKey>,
    /// Verify at an explicit recorded time for offline reconstruction, or at the
    /// current time for admission to a live projection. These claims stay distinct.
    pub verification_unix_ms: u64,
    pub maximum_record_ttl_ms: u64,
}

/// Only the verifier can construct this type. All projections remain derived.
#[derive(Debug, Clone, Serialize)]
pub struct VerifiedGraph {
    schema: &'static str,
    authoritative: bool,
    graph_sha256: String,
    binding: Binding,
    outcome: Outcome,
    terminal_sha256: String,
    verified_at_unix_ms: u64,
    records: Vec<ProjectionNode>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectionNode {
    pub sha256: String,
    pub kind: Kind,
    pub source: String,
    pub observed_unix_ms: u64,
    pub outcome: Outcome,
    pub native_identity: Option<String>,
    pub artifacts: Vec<Artifact>,
    pub parents: Vec<String>,
}

impl VerifiedGraph {
    pub fn binding(&self) -> &Binding {
        &self.binding
    }
    pub fn digest(&self) -> &str {
        &self.graph_sha256
    }
    pub fn outcome(&self) -> Outcome {
        self.outcome
    }
    pub fn nodes(&self) -> &[ProjectionNode] {
        &self.records
    }
}

/// Derived correlation of two independently admitted and verified operations.
#[derive(Debug, Serialize)]
pub struct VerifiedRecovery {
    schema: &'static str,
    authoritative: bool,
    original_graph_sha256: String,
    recovery: VerifiedGraph,
}

/// Validate recovery as its own intent/facts/approval/grant/execution/terminal chain.
/// A provider cannot append a self-authorized compensation node to an old ticket.
pub fn verify_recovery(
    original: &VerifiedGraph,
    bytes: &[u8],
    trust: &Trust,
    resolve: impl FnMut(&Artifact) -> Result<(), Error>,
) -> Result<VerifiedRecovery, Error> {
    let recovery = verify(bytes, trust, resolve)?;
    let expected = RecoveryLink {
        ticket_id: original.binding.ticket_id.clone(),
        graph_sha256: original.graph_sha256.clone(),
        terminal_sha256: original.terminal_sha256.clone(),
    };
    let original_terminal = original
        .records
        .iter()
        .find(|node| node.kind == Kind::Terminal)
        .ok_or(Error::Causality)?;
    let recovery_intent = recovery
        .records
        .iter()
        .find(|node| node.kind == Kind::Intent)
        .ok_or(Error::Causality)?;
    if recovery.binding.recovery_of.as_ref() != Some(&expected)
        || recovery.binding.ticket_id == original.binding.ticket_id
        || recovery.binding.idempotency_key == original.binding.idempotency_key
        || recovery.binding.writer_epoch < original.binding.writer_epoch
        || recovery.binding.target_manifest_sha256 != original.binding.target_manifest_sha256
        || recovery_intent.observed_unix_ms < original_terminal.observed_unix_ms
    {
        return Err(Error::Causality);
    }
    Ok(VerifiedRecovery {
        schema: "cohesix-verified-recovery-projection/v1",
        authoritative: false,
        original_graph_sha256: original.graph_sha256.clone(),
        recovery,
    })
}

pub fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn is_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn bounded_id(value: &str) -> Result<(), Error> {
    cohesix_authority::validate_id(value).map_err(|_| Error::Identity)
}

fn binding_valid(binding: &Binding) -> Result<(), Error> {
    if let Some(link) = &binding.recovery_of {
        bounded_id(&link.ticket_id)?;
        if !is_hash(&link.graph_sha256)
            || !is_hash(&link.terminal_sha256)
            || link.ticket_id == binding.ticket_id
        {
            return Err(Error::Identity);
        }
    }
    for id in [
        &binding.ticket_id,
        &binding.subject,
        &binding.action,
        &binding.idempotency_key,
    ] {
        bounded_id(id)?;
    }
    if binding.writer_epoch == 0
        || binding.component_sha256.is_empty()
        || binding.component_sha256.len() > 32
    {
        return Err(Error::Identity);
    }
    for hash in [
        &binding.target_manifest_sha256,
        &binding.provider_graph_sha256,
        &binding.implementation_graph_sha256,
        &binding.use_case_graph_sha256,
    ] {
        if !is_hash(hash) {
            return Err(Error::Identity);
        }
    }
    for (id, hash) in &binding.component_sha256 {
        bounded_id(id)?;
        if !is_hash(hash) {
            return Err(Error::Identity);
        }
    }
    if let Some(worker) = &binding.worker {
        bounded_id(&worker.id)?;
        if !matches!(
            worker.role.as_str(),
            "worker-gpu" | "worker-lora" | "worker-heartbeat"
        ) || worker.generation == 0
            || !is_hash(&worker.image_sha256)
        {
            return Err(Error::Identity);
        }
    }
    cohesix_authority::provider::action(&binding.action).map_err(|_| Error::Identity)?;
    Ok(())
}

/// Verify canonical graph bytes, every signature, phase ownership and referenced
/// object. The resolver must check content bytes against both digest and length.
pub fn verify(
    bytes: &[u8],
    trust: &Trust,
    resolve: impl FnMut(&Artifact) -> Result<(), Error>,
) -> Result<VerifiedGraph, Error> {
    let (graph, projections) = validate_prefix(bytes, trust, resolve)?;
    let phases: BTreeMap<_, _> = graph
        .records
        .iter()
        .map(|signed| (signed.record.kind, signed))
        .collect();
    let terminal = phases.get(&Kind::Terminal).ok_or(Error::Causality)?;
    let verification = phases.get(&Kind::Verification).ok_or(Error::Causality)?;
    let expected = match verification.record.outcome {
        Outcome::Verified => Outcome::Succeeded,
        other => other,
    };
    if terminal.record.outcome != expected
        || phases
            .get(&Kind::Worker)
            .is_some_and(|worker| worker.record.outcome != expected)
    {
        return Err(Error::Causality);
    }
    if phases.contains_key(&Kind::Worker) != graph.binding.worker.is_some() {
        return Err(Error::Identity);
    }
    let outcome = terminal.record.outcome;
    let terminal_sha256 = terminal.sha256.clone();
    Ok(VerifiedGraph {
        schema: "cohesix-verified-graph-projection/v1",
        authoritative: false,
        graph_sha256: digest(bytes),
        binding: graph.binding,
        outcome,
        terminal_sha256,
        verified_at_unix_ms: trust.verification_unix_ms,
        records: projections,
    })
}

/// Verify signatures and causal predecessors while an operation is still running.
/// This internal result cannot be exported as a terminal verified graph.
fn validate_prefix(
    bytes: &[u8],
    trust: &Trust,
    mut resolve: impl FnMut(&Artifact) -> Result<(), Error>,
) -> Result<(Graph, Vec<ProjectionNode>), Error> {
    if bytes.is_empty() || bytes.len() > MAX_GRAPH_BYTES {
        return Err(Error::Limit);
    }
    let graph: Graph = serde_json::from_slice(bytes).map_err(|_| Error::Schema)?;
    if graph.schema != SCHEMA || serde_json::to_vec(&graph).map_err(|_| Error::Schema)? != bytes {
        return Err(Error::Schema);
    }
    if trust.schema != "cohesix-evidence-trust/v1" || graph.binding != trust.expected {
        return Err(Error::Identity);
    }
    binding_valid(&graph.binding)?;
    if graph.records.is_empty()
        || graph.records.len() > MAX_NODES
        || trust.keys.is_empty()
        || trust.keys.len() > 32
        || trust.maximum_record_ttl_ms == 0
        || trust.maximum_record_ttl_ms > 86_400_000
    {
        return Err(Error::Limit);
    }
    let mut keys = BTreeMap::new();
    for key in &trust.keys {
        bounded_id(&key.id)?;
        bounded_id(&key.source)?;
        if key.kinds.is_empty()
            || key.not_before_unix_ms >= key.not_after_unix_ms
            || keys.insert(&key.id, key).is_some()
        {
            return Err(Error::Identity);
        }
    }
    let mut seen: BTreeMap<&str, &Record> = BTreeMap::new();
    let mut phases: BTreeMap<Kind, &SignedRecord> = BTreeMap::new();
    let mut artifact_count = 0usize;
    let mut projections = Vec::new();
    for (index, signed) in graph.records.iter().enumerate() {
        let record = &signed.record;
        let canonical = serde_json::to_vec(record).map_err(|_| Error::Schema)?;
        if record.schema != "cohesix-causal-record/v1"
            || record.binding != graph.binding
            || digest(&canonical) != signed.sha256
        {
            return Err(Error::Identity);
        }
        if record.sequence != index as u64 + 1
            || record.observed_unix_ms == 0
            || record.observed_unix_ms > u64::MAX / 1_000_000
            || record.expires_unix_ms <= record.observed_unix_ms
            || record.expires_unix_ms - record.observed_unix_ms > trust.maximum_record_ttl_ms
            || trust.verification_unix_ms < record.observed_unix_ms
            || trust.verification_unix_ms >= record.expires_unix_ms
        {
            return Err(Error::Chronology);
        }
        if record.parents.len() > 8 || record.artifacts.is_empty() || record.artifacts.len() > 8 {
            return Err(Error::Limit);
        }
        let key = keys.get(&signed.key_id).ok_or(Error::Signature)?;
        if key.source != record.source
            || !key.kinds.contains(&record.kind)
            || record.observed_unix_ms < key.not_before_unix_ms
            || record.expires_unix_ms > key.not_after_unix_ms
        {
            return Err(Error::Signature);
        }
        let key_bytes: [u8; 32] = hex::decode(&key.public_key)
            .map_err(|_| Error::Signature)?
            .try_into()
            .map_err(|_| Error::Signature)?;
        let signature_bytes: [u8; 64] = hex::decode(&signed.signature)
            .map_err(|_| Error::Signature)?
            .try_into()
            .map_err(|_| Error::Signature)?;
        VerifyingKey::from_bytes(&key_bytes)
            .map_err(|_| Error::Signature)?
            .verify_strict(&canonical, &Signature::from_bytes(&signature_bytes))
            .map_err(|_| Error::Signature)?;
        if record.parents.iter().collect::<BTreeSet<_>>().len() != record.parents.len() {
            return Err(Error::Causality);
        }
        for parent in &record.parents {
            let previous = seen.get(parent.as_str()).ok_or(Error::Causality)?;
            if previous.observed_unix_ms > record.observed_unix_ms {
                return Err(Error::Chronology);
            }
        }
        // Exactly one phase record; bounded native subevents live in hashed artifacts.
        if phases.contains_key(&record.kind) || seen.contains_key(signed.sha256.as_str()) {
            return Err(Error::Causality);
        }
        let required: &[Kind] = match record.kind {
            Kind::Intent => &[],
            Kind::Facts => &[Kind::Intent],
            Kind::Approval => &[Kind::Intent, Kind::Facts],
            Kind::Grant => &[Kind::Intent, Kind::Facts, Kind::Approval],
            Kind::Execution => &[Kind::Grant],
            Kind::Observation => &[Kind::Execution],
            Kind::Verification => &[Kind::Observation, Kind::Grant],
            Kind::Worker => &[Kind::Verification],
            Kind::Terminal => {
                if graph.binding.worker.is_some() {
                    &[Kind::Verification, Kind::Worker]
                } else {
                    &[Kind::Verification]
                }
            }
        };
        if record.kind == Kind::Intent && !record.parents.is_empty() {
            return Err(Error::Causality);
        }
        for kind in required {
            let required = phases.get(kind).ok_or(Error::Causality)?;
            if !record.parents.contains(&required.sha256) {
                return Err(Error::Causality);
            }
        }
        match record.kind {
            Kind::Grant | Kind::Approval if record.outcome != Outcome::Admitted => {
                return Err(Error::Causality)
            }
            Kind::Verification
                if !matches!(
                    record.outcome,
                    Outcome::Verified | Outcome::Failed | Outcome::Cancelled | Outcome::Expired
                ) =>
            {
                return Err(Error::Causality)
            }
            Kind::Terminal | Kind::Worker
                if !matches!(
                    record.outcome,
                    Outcome::Succeeded | Outcome::Failed | Outcome::Cancelled | Outcome::Expired
                ) =>
            {
                return Err(Error::Causality)
            }
            _ => {}
        }
        if matches!(
            record.kind,
            Kind::Execution
                | Kind::Observation
                | Kind::Verification
                | Kind::Worker
                | Kind::Terminal
        ) {
            let identity = record.native_identity.as_deref().ok_or(Error::Identity)?;
            if identity.is_empty()
                || identity.len() > 256
                || identity.bytes().any(|b| b.is_ascii_control())
                || record.resource_generation == 0
            {
                return Err(Error::Identity);
            }
            if let Some(execution) = phases.get(&Kind::Execution) {
                if execution.record.native_identity != record.native_identity
                    || execution.record.resource_generation != record.resource_generation
                {
                    return Err(Error::Identity);
                }
            }
        }
        if record.event_cursor.as_ref().is_some_and(|v| {
            v.is_empty() || v.len() > 256 || v.bytes().any(|b| b.is_ascii_control())
        }) {
            return Err(Error::Identity);
        }
        artifact_count += record.artifacts.len();
        if artifact_count > 128 {
            return Err(Error::Limit);
        }
        for artifact in &record.artifacts {
            if !is_hash(&artifact.sha256)
                || artifact.bytes == 0
                || artifact.bytes > MAX_ARTIFACT_BYTES
                || artifact.media_type.len() > 64
                || !artifact
                    .media_type
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"/-+.".contains(&b))
            {
                return Err(Error::Identity);
            }
            resolve(artifact)?;
        }
        seen.insert(&signed.sha256, record);
        phases.insert(record.kind, signed);
        projections.push(ProjectionNode {
            sha256: signed.sha256.clone(),
            kind: record.kind,
            source: record.source.clone(),
            observed_unix_ms: record.observed_unix_ms,
            outcome: record.outcome,
            native_identity: record.native_identity.clone(),
            artifacts: record.artifacts.clone(),
            parents: record.parents.clone(),
        });
    }
    Ok((graph, projections))
}

/// Read only immutable hash filenames under an operator-selected CAS directory.
pub fn verify_cas(root: &Path, artifact: &Artifact) -> Result<(), Error> {
    if !is_hash(&artifact.sha256) || artifact.bytes > MAX_ARTIFACT_BYTES {
        return Err(Error::Limit);
    }
    let path = root.join(&artifact.sha256);
    let metadata = std::fs::symlink_metadata(&path).map_err(|_| Error::Missing)?;
    if !metadata.is_file() || metadata.len() != artifact.bytes {
        return Err(Error::Digest);
    }
    let mut file = std::fs::File::open(&path)
        .map_err(|_| Error::Missing)?
        .take(artifact.bytes + 1);
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    let mut total = 0u64;
    loop {
        let count = file.read(&mut buffer).map_err(|_| Error::Missing)?;
        if count == 0 {
            break;
        }
        total += count as u64;
        hasher.update(&buffer[..count]);
    }
    if total != artifact.bytes || hex::encode(hasher.finalize()) != artifact.sha256 {
        return Err(Error::Digest);
    }
    Ok(())
}
