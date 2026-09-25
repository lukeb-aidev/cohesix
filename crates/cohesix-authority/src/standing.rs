// Author: Lukas Bower
// Purpose: Evaluate bounded standing scopes against exact job intent and fresh authoritative facts.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::validate_id;

/// The compiler-owned wire contract for a standing scope.
pub const STANDING_SCOPE_SCHEMA: &str = "cohesix-standing-scope/v1";
/// The exact selected job identity contract shared by all client surfaces.
pub const JOB_BINDING_SCHEMA: &str = "cohesix-job-binding/v1";
/// Compiler-selected ceiling for M28 effect actions, including local PEFT release.
pub const STANDING_CONTROLS_SCHEMA: &str = "cohesix-standing-controls/v1";
/// Private deployment file whose entries must attenuate the selected profile.
pub const STANDING_SCOPE_FILE_SCHEMA: &str = "cohesix-standing-scope-file/v1";

/// The gateway and native agent load the same immutable scope selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StandingScopeFile {
    pub schema: String,
    pub scopes: Vec<StandingScope>,
}

impl StandingScopeFile {
    pub fn validate(&self, controls: &StandingControls) -> Result<(), StandingRefusal> {
        if self.schema != STANDING_SCOPE_FILE_SCHEMA
            || self.scopes.is_empty()
            || self.scopes.len() > controls.max_scopes as usize
        {
            return Err(StandingRefusal::Invalid);
        }
        for (index, scope) in self.scopes.iter().enumerate() {
            controls.permits_scope(scope)?;
            if self.scopes[..index]
                .iter()
                .any(|earlier| earlier.id == scope.id)
            {
                return Err(StandingRefusal::Invalid);
            }
        }
        Ok(())
    }
}

/// Upper bounds and action set selected by the source manifest. Runtime
/// scopes may attenuate these limits but cannot enable another native action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct StandingControls {
    pub schema: String,
    pub enabled: bool,
    pub actions: Vec<String>,
    pub max_scopes: u8,
    pub max_jobs: u16,
    pub max_job_units: u64,
    pub max_total_units: u64,
    pub max_concurrent: u16,
    pub max_retries: u16,
    pub max_cooldown_ms: u64,
    pub max_fact_age_ms: u64,
    pub max_decision_ttl_ms: u64,
}

impl Default for StandingControls {
    fn default() -> Self {
        Self {
            schema: STANDING_CONTROLS_SCHEMA.into(),
            enabled: false,
            actions: Vec::new(),
            max_scopes: 0,
            max_jobs: 0,
            max_job_units: 0,
            max_total_units: 0,
            max_concurrent: 0,
            max_retries: 0,
            max_cooldown_ms: 0,
            max_fact_age_ms: 0,
            max_decision_ttl_ms: 0,
        }
    }
}

impl StandingControls {
    pub fn validate(&self) -> Result<(), StandingRefusal> {
        if self.schema != STANDING_CONTROLS_SCHEMA {
            return Err(StandingRefusal::Invalid);
        }
        if !self.enabled {
            return if self.actions.is_empty()
                && self.max_scopes == 0
                && self.max_jobs == 0
                && self.max_job_units == 0
                && self.max_total_units == 0
                && self.max_concurrent == 0
                && self.max_retries == 0
                && self.max_cooldown_ms == 0
                && self.max_fact_age_ms == 0
                && self.max_decision_ttl_ms == 0
            {
                Ok(())
            } else {
                Err(StandingRefusal::Invalid)
            };
        }
        if self.actions.is_empty()
            || self.actions.len() > 3
            || self.actions.iter().any(|action| {
                !matches!(
                    action.as_str(),
                    "gpu.workload.submit" | "systemd.restart" | "peft.release"
                )
            })
            || self.actions.iter().enumerate().any(|(index, action)| {
                self.actions[..index]
                    .iter()
                    .any(|earlier| earlier == action)
            })
            || self.max_scopes == 0
            || self.max_scopes > 16
            || self.max_jobs == 0
            || self.max_jobs > 256
            || self.max_job_units == 0
            || self.max_total_units < self.max_job_units
            || self.max_concurrent == 0
            || self.max_concurrent > self.max_jobs
            || self.max_retries > 8
            || self.max_cooldown_ms > 86_400_000
            || self.max_fact_age_ms == 0
            || self.max_fact_age_ms > 5_000
            || self.max_decision_ttl_ms == 0
            || self.max_decision_ttl_ms > 5_000
        {
            return Err(StandingRefusal::Invalid);
        }
        Ok(())
    }

    /// Decode the selected resolved manifest; older manifests default closed.
    pub fn from_resolved_manifest(bytes: &[u8]) -> Result<Self, StandingRefusal> {
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|_| StandingRefusal::Invalid)?;
        let Some(fragment) = value.get("standing_authority") else {
            return Ok(Self::default());
        };
        let controls: Self =
            serde_json::from_value(fragment.clone()).map_err(|_| StandingRefusal::Invalid)?;
        controls.validate()?;
        Ok(controls)
    }

    pub fn permits_scope(&self, scope: &StandingScope) -> Result<(), StandingRefusal> {
        self.validate()?;
        scope.validate()?;
        if !self.enabled
            || !self.actions.iter().any(|action| action == &scope.action)
            || scope.max_job_units > self.max_job_units
            || scope.max_total_units > self.max_total_units
            || scope.max_concurrent > self.max_concurrent
            || scope.max_retries > self.max_retries
            || scope.cooldown_ms > self.max_cooldown_ms
            || scope.fact_max_age_ms > self.max_fact_age_ms
            || scope.decision_ttl_ms > self.max_decision_ttl_ms
        {
            return Err(StandingRefusal::Scope);
        }
        Ok(())
    }
}

/// One narrow scope. Revocation is durable ledger state, never a field supplied
/// by a job caller. A scope selects one action and exact target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StandingScope {
    pub schema: String,
    pub id: String,
    pub subject: String,
    pub action: String,
    pub target: String,
    pub policy_sha256: String,
    pub generation: u64,
    pub expires_unix_ms: u64,
    pub max_job_units: u64,
    pub max_total_units: u64,
    pub max_concurrent: u16,
    pub max_retries: u16,
    pub cooldown_ms: u64,
    pub fact_max_age_ms: u64,
    pub decision_ttl_ms: u64,
}

impl StandingScope {
    /// Reject malformed configured scope before creating persistent state.
    pub fn validate(&self) -> Result<(), StandingRefusal> {
        if self.schema != STANDING_SCOPE_SCHEMA
            || self.generation == 0
            || self.expires_unix_ms == 0
            || self.max_job_units == 0
            || self.max_total_units == 0
            || self.max_concurrent == 0
            || self.fact_max_age_ms == 0
            || self.decision_ttl_ms == 0
        {
            return Err(StandingRefusal::Invalid);
        }
        validate_id(&self.id).map_err(|_| StandingRefusal::Invalid)?;
        validate_id(&self.subject).map_err(|_| StandingRefusal::Invalid)?;
        validate_id(&self.action).map_err(|_| StandingRefusal::Invalid)?;
        validate_target(&self.target)?;
        validate_hash(&self.policy_sha256)
    }
}

/// Immutable identity of one selected effect, including the exact input and
/// resource generations. The stable ticket/idempotency pair is never remapped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobBinding {
    pub schema: String,
    pub scope_id: String,
    pub admission_id: String,
    pub ticket_id: String,
    pub idempotency_key: String,
    pub subject: String,
    pub action: String,
    pub target: String,
    pub input_sha256: String,
    pub policy_sha256: String,
    pub state_epoch: u64,
    pub resource_generation: u64,
    pub deadline_unix_ms: u64,
    pub units: u64,
    pub attempt: u16,
}

impl JobBinding {
    /// Hash the canonical typed identity. Validation precedes hashing so the
    /// digest cannot normalize malformed or ambiguous fields into a grant.
    pub fn intent_sha256(&self) -> Result<String, StandingRefusal> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|_| StandingRefusal::Invalid)?;
        Ok(hex_lower(&Sha256::digest(bytes)))
    }

    fn validate(&self) -> Result<(), StandingRefusal> {
        if self.schema != JOB_BINDING_SCHEMA
            || self.scope_id.is_empty()
            || self.deadline_unix_ms == 0
            || self.units == 0
            || self.attempt == 0
            || self.state_epoch == 0
            || self.resource_generation == 0
        {
            return Err(StandingRefusal::Invalid);
        }
        for value in [
            &self.scope_id,
            &self.admission_id,
            &self.ticket_id,
            &self.idempotency_key,
            &self.subject,
            &self.action,
        ] {
            validate_id(value).map_err(|_| StandingRefusal::Invalid)?;
        }
        validate_target(&self.target)?;
        validate_hash(&self.input_sha256)?;
        validate_hash(&self.policy_sha256)
    }
}

/// Current facts from a separately enrolled observer; callers cannot supply
/// these through a job request or reuse an earlier observation after mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmissionFacts {
    pub observed_unix_ms: u64,
    pub state_epoch: u64,
    pub resource_generation: u64,
    pub policy_sha256: String,
}

/// Durable cumulative accounting read under the same lock as reservation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetView {
    pub settled_units: u64,
    pub reserved_units: u64,
    pub active: u16,
    pub attempts: u16,
    pub last_dispatch_unix_ms: Option<u64>,
    pub last_clock_unix_ms: u64,
    pub revoked: bool,
}

/// A short decision to be committed to the ledger before dispatch. This
/// value alone is never a provider grant or proof of a native effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdmissionDecision {
    pub expires_unix_ms: u64,
    pub reserved_units: u64,
    pub scope_generation: u64,
}

/// Deterministic refusal class; the caller records it without changing the
/// native execution or delivery state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandingRefusal {
    Invalid,
    Scope,
    Revoked,
    Expired,
    Stale,
    Budget,
    Concurrency,
    Retry,
    Cooldown,
    Clock,
}

impl fmt::Display for StandingRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Invalid => "EPERM invalid-standing-input",
            Self::Scope => "EPERM standing-scope-mismatch",
            Self::Revoked => "EPERM standing-scope-revoked",
            Self::Expired => "EPERM standing-scope-expired",
            Self::Stale => "EPERM standing-facts-stale",
            Self::Budget => "ELIMIT standing-budget",
            Self::Concurrency => "ELIMIT standing-concurrency",
            Self::Retry => "ELIMIT standing-retries",
            Self::Cooldown => "EPERM standing-cooldown",
            Self::Clock => "EPERM standing-clock",
        })
    }
}

/// Check the complete immutable intent, scope, fresh facts and durable budget.
/// The caller must atomically reserve this decision before any effect and must
/// re-evaluate against current facts immediately before dispatch.
pub fn evaluate(
    scope: &StandingScope,
    job: &JobBinding,
    facts: &AdmissionFacts,
    budget: BudgetView,
    now_unix_ms: u64,
) -> Result<AdmissionDecision, StandingRefusal> {
    job.validate()?;
    scope.validate()?;
    if scope.id != job.scope_id
        || scope.subject != job.subject
        || scope.action != job.action
        || scope.target != job.target
        || scope.policy_sha256 != job.policy_sha256
    {
        return Err(StandingRefusal::Scope);
    }
    if budget.revoked {
        return Err(StandingRefusal::Revoked);
    }
    if now_unix_ms == 0 || now_unix_ms < budget.last_clock_unix_ms {
        return Err(StandingRefusal::Clock);
    }
    if now_unix_ms >= scope.expires_unix_ms || now_unix_ms >= job.deadline_unix_ms {
        return Err(StandingRefusal::Expired);
    }
    if facts.observed_unix_ms == 0
        || facts.observed_unix_ms > now_unix_ms
        || now_unix_ms - facts.observed_unix_ms > scope.fact_max_age_ms
        || facts.state_epoch != job.state_epoch
        || facts.resource_generation != job.resource_generation
        || facts.policy_sha256 != scope.policy_sha256
    {
        return Err(StandingRefusal::Stale);
    }
    if job.units > scope.max_job_units
        || budget
            .settled_units
            .checked_add(budget.reserved_units)
            .and_then(|used| used.checked_add(job.units))
            .is_none_or(|used| used > scope.max_total_units)
    {
        return Err(StandingRefusal::Budget);
    }
    if budget.active >= scope.max_concurrent {
        return Err(StandingRefusal::Concurrency);
    }
    if job.attempt > scope.max_retries.saturating_add(1)
        || budget.attempts >= scope.max_retries.saturating_add(1)
    {
        return Err(StandingRefusal::Retry);
    }
    if budget.last_dispatch_unix_ms.is_some_and(|last| {
        last.checked_add(scope.cooldown_ms)
            .is_none_or(|available| available > now_unix_ms)
    }) {
        return Err(StandingRefusal::Cooldown);
    }
    let expires_unix_ms = now_unix_ms
        .checked_add(scope.decision_ttl_ms)
        .ok_or(StandingRefusal::Clock)?
        .min(scope.expires_unix_ms)
        .min(job.deadline_unix_ms);
    if expires_unix_ms <= now_unix_ms {
        return Err(StandingRefusal::Expired);
    }
    Ok(AdmissionDecision {
        expires_unix_ms,
        reserved_units: job.units,
        scope_generation: scope.generation,
    })
}

fn validate_target(value: &str) -> Result<(), StandingRefusal> {
    if value.is_empty()
        || value.len() > 255
        || !value.starts_with('/')
        || value
            .split('/')
            .skip(1)
            .any(|part| validate_id(part).is_err())
    {
        return Err(StandingRefusal::Invalid);
    }
    Ok(())
}

fn validate_hash(value: &str) -> Result<(), StandingRefusal> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(StandingRefusal::Invalid);
    }
    Ok(())
}

pub(crate) fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 15) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (StandingScope, JobBinding, AdmissionFacts, BudgetView) {
        let policy = "a".repeat(64);
        (
            StandingScope {
                schema: STANDING_SCOPE_SCHEMA.into(),
                id: "scope-1".into(),
                subject: "operator-1".into(),
                action: "gpu.workload.submit".into(),
                target: "/gpu/GPU-0/workload".into(),
                policy_sha256: policy.clone(),
                generation: 1,
                expires_unix_ms: 2000,
                max_job_units: 5,
                max_total_units: 10,
                max_concurrent: 1,
                max_retries: 1,
                cooldown_ms: 10,
                fact_max_age_ms: 100,
                decision_ttl_ms: 20,
            },
            JobBinding {
                schema: JOB_BINDING_SCHEMA.into(),
                scope_id: "scope-1".into(),
                admission_id: "admit-1".into(),
                ticket_id: "ticket-1".into(),
                idempotency_key: "once-1".into(),
                subject: "operator-1".into(),
                action: "gpu.workload.submit".into(),
                target: "/gpu/GPU-0/workload".into(),
                input_sha256: "b".repeat(64),
                policy_sha256: policy.clone(),
                state_epoch: 2,
                resource_generation: 3,
                deadline_unix_ms: 1500,
                units: 4,
                attempt: 1,
            },
            AdmissionFacts {
                observed_unix_ms: 1000,
                state_epoch: 2,
                resource_generation: 3,
                policy_sha256: policy,
            },
            BudgetView {
                settled_units: 3,
                reserved_units: 2,
                active: 0,
                attempts: 0,
                last_dispatch_unix_ms: None,
                last_clock_unix_ms: 1000,
                revoked: false,
            },
        )
    }

    #[test]
    fn fresh_bound_intent_admits_with_short_expiry() {
        let (scope, job, facts, budget) = fixture();
        assert_eq!(job.intent_sha256().expect("digest").len(), 64);
        assert_eq!(
            evaluate(&scope, &job, &facts, budget, 1005),
            Ok(AdmissionDecision {
                expires_unix_ms: 1025,
                reserved_units: 4,
                scope_generation: 1,
            })
        );
    }

    #[test]
    fn stale_changed_revoked_and_over_budget_inputs_refuse() {
        let (scope, job, facts, budget) = fixture();
        let mut bad_facts = facts.clone();
        bad_facts.resource_generation = 4;
        assert_eq!(
            evaluate(&scope, &job, &bad_facts, budget, 1005),
            Err(StandingRefusal::Stale)
        );
        let mut bad_job = job.clone();
        bad_job.subject = "other".into();
        assert_eq!(
            evaluate(&scope, &bad_job, &facts, budget, 1005),
            Err(StandingRefusal::Scope)
        );
        let mut revoked = budget;
        revoked.revoked = true;
        assert_eq!(
            evaluate(&scope, &job, &facts, revoked, 1005),
            Err(StandingRefusal::Revoked)
        );
        let mut spent = budget;
        spent.reserved_units = 4;
        assert_eq!(
            evaluate(&scope, &job, &facts, spent, 1005),
            Err(StandingRefusal::Budget)
        );
    }

    #[test]
    fn concurrency_retry_cooldown_and_clock_refuse() {
        let (scope, job, facts, budget) = fixture();
        let mut busy = budget;
        busy.active = 1;
        assert_eq!(
            evaluate(&scope, &job, &facts, busy, 1005),
            Err(StandingRefusal::Concurrency)
        );
        let mut retried = budget;
        retried.attempts = 2;
        assert_eq!(
            evaluate(&scope, &job, &facts, retried, 1005),
            Err(StandingRefusal::Retry)
        );
        let mut cooling = budget;
        cooling.last_dispatch_unix_ms = Some(1000);
        assert_eq!(
            evaluate(&scope, &job, &facts, cooling, 1005),
            Err(StandingRefusal::Cooldown)
        );
        assert_eq!(
            evaluate(&scope, &job, &facts, budget, 999),
            Err(StandingRefusal::Clock)
        );
    }

    #[test]
    fn controls_default_closed_and_runtime_scope_cannot_widen_ceiling() {
        let absent = StandingControls::from_resolved_manifest(br#"{}"#).expect("old profile");
        assert!(!absent.enabled);
        let (scope, _, _, _) = fixture();
        assert_eq!(absent.permits_scope(&scope), Err(StandingRefusal::Scope));
        let controls = StandingControls {
            schema: STANDING_CONTROLS_SCHEMA.into(),
            enabled: true,
            actions: alloc::vec!["gpu.workload.submit".into()],
            max_scopes: 1,
            max_jobs: 2,
            max_job_units: 5,
            max_total_units: 10,
            max_concurrent: 1,
            max_retries: 1,
            max_cooldown_ms: 10,
            max_fact_age_ms: 100,
            max_decision_ttl_ms: 20,
        };
        assert_eq!(controls.permits_scope(&scope), Ok(()));
        let mut widened = scope.clone();
        widened.max_total_units = 11;
        assert_eq!(
            controls.permits_scope(&widened),
            Err(StandingRefusal::Scope)
        );
        let mut invalid = controls.clone();
        invalid.actions.push("docker.run".into());
        assert_eq!(invalid.validate(), Err(StandingRefusal::Invalid));
        let profile = serde_json::json!({"standing_authority":controls});
        let decoded =
            StandingControls::from_resolved_manifest(&serde_json::to_vec(&profile).unwrap())
                .unwrap();
        assert!(decoded.enabled);
    }

    #[test]
    fn optional_peft_release_requires_selected_ceiling_and_exact_scope() {
        let (mut scope, mut job, facts, budget) = fixture();
        let mut controls = StandingControls {
            schema: STANDING_CONTROLS_SCHEMA.into(),
            enabled: true,
            actions: alloc::vec![
                "gpu.workload.submit".into(),
                "systemd.restart".into(),
                "peft.release".into()
            ],
            max_scopes: 3,
            max_jobs: 3,
            max_job_units: 5,
            max_total_units: 10,
            max_concurrent: 1,
            max_retries: 1,
            max_cooldown_ms: 10,
            max_fact_age_ms: 100,
            max_decision_ttl_ms: 20,
        };
        scope.action = "peft.release".into();
        scope.target = "/models/local-model/release".into();
        job.action = scope.action.clone();
        job.target = scope.target.clone();
        assert_eq!(controls.permits_scope(&scope), Ok(()));
        assert!(evaluate(&scope, &job, &facts, budget, 1100).is_ok());
        controls.actions.pop();
        assert_eq!(controls.permits_scope(&scope), Err(StandingRefusal::Scope));
        controls.actions.push("peft.release".into());
        controls.actions.push("peft.release".into());
        assert_eq!(controls.validate(), Err(StandingRefusal::Invalid));
    }
}
