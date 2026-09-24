// Author: Lukas Bower
// Purpose: Extend the recipe journal with a bounded native adapter release and verified compensation without replaying uncertain phases.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]

use super::release::{self, Comparison, DeploymentState, Evaluation, EvaluationPolicy};
use crate::{operator::read_bounded, recipe};
use anyhow::{anyhow, ensure, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

/// Entry identity never invents an export/training job for an imported adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Entry {
    /// Import a genuine externally produced adapter without inventing a training job.
    Import,
    /// Train under the pinned HF stack; checkpoints do not confer authority.
    Train,
}

/// An immutable input belongs to a separately admitted `peft.release` ticket.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Exact versioned release request grammar.
    pub schema: String,
    /// Logical operation identity retained across interruption.
    pub operation_id: String,
    /// Native subject named by the admitted ticket.
    pub model_id: String,
    /// Native import and training converge on the same release phases.
    pub entry: Entry,
    /// Pinned configured native stack and dataset contract.
    pub profile_sha256: String,
    /// CAS reference to complete input provenance and artifact bindings.
    pub input_sha256: String,
    /// Predeclared comparison bounds bound by the ticket.
    pub evaluation_policy: EvaluationPolicy,
    /// Exact accepted generation and authorized rollback target.
    pub baseline: DeploymentState,
}

impl Request {
    /// Validate the admitted immutable request before a journal or native phase is created.
    pub fn validate(&self) -> Result<()> {
        let digest = |value: &str| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        };
        ensure!(
            self.schema == "cohesix-peft-release/v1",
            "invalid_request release-schema"
        );
        ensure!(
            digest(&self.profile_sha256)
                && digest(&self.input_sha256)
                && digest(&self.baseline.served_artifact_sha256)
                && digest(&self.baseline.runtime_sha256),
            "invalid_request release-artifact-identity"
        );
        if let Some(adapter) = &self.baseline.adapter_sha256 {
            ensure!(
                digest(adapter) && adapter == &self.baseline.served_artifact_sha256,
                "invalid_request incumbent-adapter"
            );
        } else {
            ensure!(
                self.baseline.generation == 0,
                "invalid_request base-generation"
            );
        }
        self.evaluation_policy.validate()?;
        Ok(())
    }
}

/// Workflow position is separate from native checkpoints and deployable artifacts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Validate exact artifact and provenance bindings.
    Validate,
    /// Train under the pinned HF stack; checkpoints do not confer authority.
    Train,
    /// Obtain native candidate and baseline evaluator reports.
    Evaluate,
    /// Reject unsafe formats, corrupt tensors and incompatible metadata.
    Scan,
    /// Materialize the verified adapter in the confined registry.
    Stage,
    /// Reload the exact candidate in the real native serving runtime.
    Load,
    /// Observe bounded serving behavior and health.
    Canary,
    /// Commit only the compared current deployment generation.
    Promote,
    /// Restore and verify the previously accepted runtime state.
    Rollback,
}

impl Phase {
    /// Stable phase key in the existing host journal.
    pub fn name(self) -> &'static str {
        match self {
            Self::Validate => "validate",
            Self::Train => "train",
            Self::Evaluate => "evaluate",
            Self::Scan => "scan",
            Self::Stage => "stage",
            Self::Load => "load",
            Self::Canary => "canary",
            Self::Promote => "promote",
            Self::Rollback => "rollback",
        }
    }
}

/// Native results are observations; the existing evidence custodian signs them.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    /// Logical operation identity retained across interruption.
    pub operation_id: String,
    /// Digest of the original complete release request.
    pub request_sha256: String,
    /// Native phase that produced this observation.
    pub phase: Phase,
    /// Observed native manager invocation, never a client receipt.
    pub native_identity: String,
    /// Original native completion time.
    pub completed_unix_ms: u64,
    /// Native phase outcome; successful rollback does not release a failed candidate.
    pub succeeded: bool,
    /// Bounded native report retained for independent verification.
    pub detail: Value,
}

/// A phase adapter owns native calls, durable completion records and actual state reads.
/// It must not infer non-execution merely from a lost process or missing response.
pub trait Native {
    /// Read the native wall-clock used for finite evidence and grant lifetimes.
    fn now_ms(&self) -> Result<u64>;
    /// Observe the current accepted deployment generation.
    fn accepted(&mut self) -> Result<DeploymentState>;
    /// Recheck the current exact ticket before a phase or compensation.
    fn authorize(&mut self, phase: Phase) -> Result<()>;
    /// Execute a never-dispatched phase and durably retain its native outcome.
    fn execute(&mut self, phase: Phase) -> Result<Observation>;
    /// Read an original durable outcome without replaying native work.
    fn reconcile(&mut self, phase: Phase) -> Result<Option<Observation>>;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    phase: Phase,
    result: Option<Observation>,
}

/// Recovered candidate failure remains failed even when rollback succeeds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Native release has not reached a verified terminal outcome.
    Running,
    /// The exact candidate was served, verified and promoted.
    Succeeded,
    /// The candidate was refused or failed before serving changed.
    Failed,
    /// The candidate failed and the previous runtime was verified restored.
    RecoveredFailure,
    /// Execution is uncertain; an original native phase cannot be replayed.
    Ambiguous,
    /// Recovery is unverified and further promotion is blocked.
    RollbackFailed,
}

/// Bounded release extension of the existing recipe journal.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Journal {
    schema: String,
    /// Logical operation identity retained across interruption.
    pub operation_id: String,
    /// Digest of the original complete release request.
    pub request_sha256: String,
    /// Original admitted ticket identity.
    pub ticket_id: String,
    /// Original idempotency identity, retained across recovery.
    pub idempotency_key: String,
    /// Writer fence fixed by the original admission.
    pub writer_epoch: u64,
    /// Candidate outcome kept distinct from recovery outcome.
    pub state: State,
    phases: Vec<Record>,
    comparison: Option<Comparison>,
    /// Bounded refusal or uncertainty retained for operator recovery.
    pub blocker: Option<String>,
    /// A separate, immutable compensation attempt retains the original release identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_authority: Option<RecoveryAuthority>,
}

/// Fresh admission can restore only the original frozen rollback target.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryAuthority {
    /// Separately approved compensation ticket; the original ticket is never rewritten.
    pub ticket_id: String,
    /// Original identity of this compensation attempt.
    pub idempotency_key: String,
    /// Current admission fence for the new compensation.
    pub writer_epoch: u64,
}

impl Journal {
    /// Check the frozen request and possible runtime mutation before compensation I/O.
    pub fn require_recovery_scope(&self, request: &Request) -> Result<()> {
        ensure!(
            self.request_sha256 == release::identity(request)?
                && self.operation_id == request.operation_id
                && matches!(
                    self.state,
                    State::Running | State::Ambiguous | State::RollbackFailed
                ),
            "EPERM recovery-scope"
        );
        Ok(())
    }
    /// Original native completion identity and time keep signed retries byte-stable.
    pub fn terminal_observation(&self) -> Result<&Observation> {
        self.phases
            .iter()
            .rev()
            .find_map(|r| r.result.as_ref())
            .ok_or_else(|| anyhow!("invalid_evidence no-native-observation"))
    }
}

fn phases(entry: Entry) -> Vec<Phase> {
    let mut phases = vec![Phase::Validate];
    if entry == Entry::Train {
        phases.push(Phase::Train);
    }
    phases.extend([
        Phase::Evaluate,
        Phase::Scan,
        Phase::Stage,
        Phase::Load,
        Phase::Canary,
        Phase::Promote,
    ]);
    phases
}

fn save(directory: &Path, journal: &Journal) -> Result<()> {
    recipe::save_record(directory, "recipe.json", journal)
}

fn validate_observation(j: &Journal, phase: Phase, observation: &Observation) -> Result<()> {
    ensure!(
        observation.operation_id == j.operation_id
            && observation.request_sha256 == j.request_sha256
            && observation.phase == phase
            && observation.completed_unix_ms > 0
            && !observation.native_identity.is_empty()
            && observation.native_identity.len() <= 256
            && serde_json::to_vec(observation)?.len() <= 16384,
        "invalid_evidence native-release-binding"
    );
    Ok(())
}

fn invoke(
    native: &mut dyn Native,
    directory: &Path,
    journal: &mut Journal,
    phase: Phase,
    _recovery_only: bool,
) -> Result<Observation> {
    let old = journal
        .phases
        .iter()
        .position(|record| record.phase == phase);
    let observation = if let Some(index) = old {
        if let Some(result) = &journal.phases[index].result {
            return Ok(result.clone());
        }
        native
            .reconcile(phase)?
            .ok_or_else(|| anyhow!("ambiguous native-phase {}", phase.name()))?
    } else {
        // No native call can precede its durable intent. An absent phase is
        // therefore known unexecuted, even when continuing after recovery.
        native.authorize(phase)?;
        journal.phases.push(Record {
            phase,
            result: None,
        });
        save(directory, journal)?;
        native.execute(phase)?
    };
    validate_observation(journal, phase, &observation)?;
    if let Some(record) = journal
        .phases
        .iter_mut()
        .find(|record| record.phase == phase)
    {
        record.result = Some(observation.clone());
    }
    save(directory, journal)?;
    Ok(observation)
}

fn evaluations(journal: &Journal) -> Result<(Evaluation, Evaluation)> {
    let evaluation = journal
        .phases
        .iter()
        .find(|r| r.phase == Phase::Evaluate)
        .and_then(|r| r.result.as_ref())
        .ok_or_else(|| anyhow!("invalid_evidence evaluations-missing"))?;
    Ok((
        serde_json::from_value(evaluation.detail["candidate"].clone())?,
        serde_json::from_value(evaluation.detail["baseline"].clone())?,
    ))
}

fn promoted(request: &Request, journal: &Journal, state: &DeploymentState) -> Result<()> {
    let comparison = journal
        .comparison
        .as_ref()
        .ok_or_else(|| anyhow!("invalid_evidence comparison"))?;
    ensure!(
        state.generation
            == request
                .baseline
                .generation
                .checked_add(1)
                .ok_or_else(|| anyhow!("ELIMIT deployment-generation"))?
            && state.adapter_sha256.as_ref() == Some(&comparison.candidate_sha256)
            && state.served_artifact_sha256 == comparison.candidate_sha256
            && state.runtime_sha256 == request.baseline.runtime_sha256
            && state.healthy
            && state.rollback_verified,
        "invalid_evidence promotion-postcondition"
    );
    Ok(())
}

/// Continue the existing journal's release extension with original intent and bounds.
/// Caller owns the enclosing registry lock, so generation CAS spans all native phases.
pub fn run(
    directory: &Path,
    request: &Request,
    ticket_id: &str,
    idempotency_key: &str,
    writer_epoch: u64,
    native: &mut dyn Native,
    recovery_only: bool,
) -> Result<Journal> {
    request.validate()?;
    for id in [
        &request.operation_id,
        &request.model_id,
        ticket_id,
        idempotency_key,
    ] {
        crate::validate_component(id)?;
    }
    ensure!(writer_epoch > 0, "EPERM release-writer-epoch");
    let _lock = recipe::lock_directory(directory, !recovery_only)?;
    let request_sha256 = release::identity(request)?;
    let path = directory.join("recipe.json");
    let mut j = if path.try_exists()? {
        let j: Journal = serde_json::from_slice(&read_bounded(&path, 262144)?)?;
        ensure!(
            j.schema == "cohesix-peft-recipe-journal/v1"
                && j.operation_id == request.operation_id
                && j.request_sha256 == request_sha256
                && j.ticket_id == ticket_id
                && j.idempotency_key == idempotency_key
                && j.writer_epoch == writer_epoch
                && j.phases.len() <= 10,
            "conflict release-journal-identity"
        );
        j
    } else {
        ensure!(!recovery_only, "ambiguous release-journal-missing");
        let j = Journal {
            schema: "cohesix-peft-recipe-journal/v1".into(),
            operation_id: request.operation_id.clone(),
            request_sha256,
            ticket_id: ticket_id.into(),
            idempotency_key: idempotency_key.into(),
            writer_epoch,
            state: State::Running,
            phases: vec![],
            comparison: None,
            blocker: None,
            recovery_authority: None,
        };
        save(directory, &j)?;
        j
    };
    if matches!(
        j.state,
        State::Succeeded | State::Failed | State::RecoveredFailure | State::RollbackFailed
    ) {
        return Ok(j);
    }
    let result = (|| -> Result<()> {
        // A lost promotion ACK must first reconcile the native generation; it
        // cannot reinterpret the committed successor as a stale baseline race.
        if j.phases.iter().any(|record| record.phase == Phase::Promote) {
            let observation = invoke(native, directory, &mut j, Phase::Promote, true)?;
            ensure!(observation.succeeded, "failed native-phase promote");
            promoted(request, &j, &native.accepted()?)?;
            return Ok(());
        }
        for phase in phases(request.entry) {
            if phase == Phase::Evaluate || phase == Phase::Load || phase == Phase::Promote {
                ensure!(
                    native.accepted()? == request.baseline,
                    "conflict baseline-generation"
                );
            }
            if phase == Phase::Promote {
                native.authorize(phase)?;
                let (candidate, baseline) = evaluations(&j)?;
                release::recheck(
                    j.comparison
                        .as_ref()
                        .ok_or_else(|| anyhow!("invalid_evidence comparison"))?,
                    &candidate,
                    &baseline,
                    &request.evaluation_policy,
                    &native.accepted()?,
                    native.now_ms()?,
                )?;
            }
            let observation = invoke(native, directory, &mut j, phase, recovery_only)?;
            ensure!(
                observation.succeeded,
                "failed native-phase {}",
                phase.name()
            );
            if phase == Phase::Evaluate {
                let (candidate, baseline) = evaluations(&j)?;
                j.comparison = Some(release::compare(
                    &candidate,
                    &baseline,
                    &request.evaluation_policy,
                    &native.accepted()?,
                    native.now_ms()?,
                )?);
                save(directory, &j)?;
            }
            if phase == Phase::Scan || phase == Phase::Stage {
                let comparison = j
                    .comparison
                    .as_ref()
                    .ok_or_else(|| anyhow!("invalid_evidence comparison"))?;
                ensure!(
                    observation.detail["adapter_sha256"] == comparison.candidate_sha256,
                    "invalid_evidence candidate-changed-after-evaluation"
                );
            }
            if phase == Phase::Promote {
                promoted(request, &j, &native.accepted()?)?;
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        j.blocker = Some(error.to_string().chars().take(256).collect());
        let runtime_touched = j.phases.iter().any(|r| r.phase == Phase::Load);
        if runtime_touched {
            // Even recovery requires current authority for this exact frozen baseline.
            let rollback = (|| -> Result<()> {
                native.authorize(Phase::Rollback)?;
                let current = native.accepted()?;
                if current != request.baseline {
                    ensure!(
                        j.phases.iter().any(|r| r.phase == Phase::Promote),
                        "conflict rollback-baseline-generation"
                    );
                    promoted(request, &j, &current)?;
                }
                let observation = invoke(native, directory, &mut j, Phase::Rollback, false)?;
                ensure!(
                    observation.succeeded && native.accepted()? == request.baseline,
                    "failed rollback-postcondition"
                );
                Ok(())
            })();
            j.state = if rollback.is_ok() {
                State::RecoveredFailure
            } else {
                State::RollbackFailed
            };
        } else if j.phases.iter().any(|r| r.result.is_none()) {
            j.state = State::Ambiguous;
        } else {
            j.state = State::Failed;
        }
    } else {
        j.state = State::Succeeded;
    }
    save(directory, &j)?;
    Ok(j)
}

/// Read-only reporting cannot advance or dispatch a phase.
pub fn inspect(directory: &Path) -> Result<Journal> {
    let _lock = recipe::lock_directory(directory, false)?;
    let j: Journal =
        serde_json::from_slice(&read_bounded(&directory.join("recipe.json"), 262144)?)?;
    ensure!(j.phases.len() <= 10, "ELIMIT release-phases");
    Ok(j)
}

/// Recover under a new grant without modifying the failed release or replaying work.
/// The caller holds the registry lock and quiesces original native invocations first.
pub fn compensate(
    directory: &Path,
    request: &Request,
    authority: RecoveryAuthority,
    native: &mut dyn Native,
) -> Result<Journal> {
    crate::validate_component(&authority.ticket_id)?;
    crate::validate_component(&authority.idempotency_key)?;
    ensure!(authority.writer_epoch > 0, "EPERM recovery-writer-epoch");
    let original = inspect(directory)?;
    original.require_recovery_scope(request)?;
    ensure!(
        original.ticket_id != authority.ticket_id,
        "EPERM fresh-recovery-authority-required"
    );
    let attempt = directory.join(format!("recovery-{}", authority.ticket_id));
    let entries = std::fs::read_dir(directory)?.collect::<std::io::Result<Vec<_>>>()?;
    ensure!(
        attempt.try_exists()?
            || entries
                .iter()
                .filter(|entry| entry.file_name().to_string_lossy().starts_with("recovery-"))
                .count()
                < 4,
        "ELIMIT recovery-attempts"
    );
    let _lock = recipe::lock_directory(&attempt, true)?;
    let path = attempt.join("recipe.json");
    let mut journal = if path.try_exists()? {
        let journal: Journal = serde_json::from_slice(&read_bounded(&path, 65536)?)?;
        ensure!(
            journal.recovery_authority.as_ref() == Some(&authority)
                && journal.request_sha256 == original.request_sha256,
            "conflict recovery-identity"
        );
        journal
    } else {
        let mut journal = original.clone();
        journal.recovery_authority = Some(authority);
        journal.phases.retain(|r| r.phase != Phase::Rollback);
        journal.state = State::Running;
        save(&attempt, &journal)?;
        journal
    };
    if journal.state != State::Running {
        return Ok(journal);
    }
    let result = (|| -> Result<()> {
        let current = native.accepted()?;
        if current != request.baseline {
            ensure!(
                original.phases.iter().any(|r| r.phase == Phase::Promote),
                "conflict recovery-baseline-generation"
            );
            promoted(request, &original, &current)?;
        }
        native.authorize(Phase::Rollback)?;
        let observation = invoke(native, &attempt, &mut journal, Phase::Rollback, true)?;
        ensure!(
            observation.succeeded && native.accepted()? == request.baseline,
            "failed recovery-postcondition"
        );
        Ok(())
    })();
    journal.state = if result.is_ok() {
        State::RecoveredFailure
    } else {
        State::RollbackFailed
    };
    if let Err(error) = result {
        journal.blocker = Some(error.to_string().chars().take(256).collect());
    }
    save(&attempt, &journal)?;
    Ok(journal)
}
