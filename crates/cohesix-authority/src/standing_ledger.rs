// Author: Lukas Bower
// Purpose: Retain shared standing reservations, revocation and delivery obligations across restarts.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

//! One local durable custody point for a gateway and native ticket executor.
//! Both processes must use the same private filesystem and policy profile.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::{borrow::ToOwned, format, string::String, vec::Vec};

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::standing::{
    evaluate, hex_lower, AdmissionDecision, AdmissionFacts, BudgetView, JobBinding,
    StandingControls, StandingRefusal, StandingScope,
};

/// Hard byte ceiling; the compiler-selected profile may lower it.
pub const LEDGER_MAX_BYTES: usize = 1024 * 1024;
/// Hard retained-identity ceiling. No unresolved or acknowledged identity is
/// evicted to create new capacity.
pub const LEDGER_MAX_JOBS: usize = 256;
/// Hard number of scope identities in one custody point.
pub const LEDGER_MAX_SCOPES: usize = 16;
const LEDGER_SCHEMA: &str = "cohesix-standing-ledger/v1";
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Native state and result delivery are independent durable obligations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobExecution {
    Reserved,
    Dispatching,
    Uncertain,
    Confirmed,
    RefusedNoEffect,
}

/// Delivery acknowledgement never changes whether the native effect happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobDelivery {
    Pending,
    Acknowledged,
}

/// One retained admission identity, including the original decision ceiling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobRecord {
    pub binding: JobBinding,
    pub intent_sha256: String,
    pub scope_generation: u64,
    pub decision_expires_unix_ms: u64,
    pub reserved_unix_ms: u64,
    pub dispatched_unix_ms: Option<u64>,
    pub execution: JobExecution,
    pub delivery: JobDelivery,
    pub result_sha256: Option<String>,
    /// A request to stop before dispatch, or to arrange a separately admitted
    /// native cancellation after dispatch. It does not prove termination.
    #[serde(default)]
    pub cancel_requested: bool,
}

/// Administrative observation of a selected scope and its retained spending.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeStatus {
    pub scope: StandingScope,
    pub budget: BudgetView,
}

/// A repeated exact admission returns the original state without a second
/// reservation or provider invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReserveOutcome {
    Reserved(AdmissionDecision),
    Existing(JobRecord),
}

/// Errors distinguish policy refusals from corrupt or full persistent state.
#[derive(Debug)]
pub enum LedgerError {
    Refused(StandingRefusal),
    Capacity,
    Uninitialized,
    Invalid,
    Conflict,
    Phase,
    Io(std::io::Error),
    Codec(serde_json::Error),
}

impl std::fmt::Display for LedgerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(reason) => reason.fmt(f),
            Self::Capacity => f.write_str("ELIMIT standing-ledger-capacity"),
            Self::Uninitialized => f.write_str("EPERM standing-ledger-uninitialized"),
            Self::Invalid => f.write_str("EPERM standing-ledger-invalid"),
            Self::Conflict => f.write_str("EPERM standing-admission-conflict"),
            Self::Phase => f.write_str("EPERM standing-admission-phase"),
            Self::Io(error) => write!(f, "standing-ledger-io: {error}"),
            Self::Codec(error) => write!(f, "standing-ledger-codec: {error}"),
        }
    }
}

impl std::error::Error for LedgerError {}
impl From<std::io::Error> for LedgerError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}
impl From<serde_json::Error> for LedgerError {
    fn from(value: serde_json::Error) -> Self {
        Self::Codec(value)
    }
}
impl From<StandingRefusal> for LedgerError {
    fn from(value: StandingRefusal) -> Self {
        Self::Refused(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScopeRecord {
    scope: StandingScope,
    revoked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LedgerFile {
    schema: String,
    policy_sha256: String,
    controls_sha256: String,
    last_clock_unix_ms: u64,
    scopes: BTreeMap<String, ScopeRecord>,
    jobs: BTreeMap<String, JobRecord>,
}

impl LedgerFile {
    fn validate(
        &self,
        selected: &BTreeMap<String, StandingScope>,
        policy: &str,
        controls_sha256: &str,
    ) -> Result<(), LedgerError> {
        if self.schema != LEDGER_SCHEMA
            || self.policy_sha256 != policy
            || self.controls_sha256 != controls_sha256
            || self.scopes.len() != selected.len()
            || self.scopes.len() > LEDGER_MAX_SCOPES
            || self.jobs.len() > LEDGER_MAX_JOBS
        {
            return Err(LedgerError::Invalid);
        }
        for (id, expected) in selected {
            let actual = self.scopes.get(id).ok_or(LedgerError::Invalid)?;
            if actual.scope != *expected || actual.scope.id != *id {
                return Err(LedgerError::Invalid);
            }
        }
        for (id, job) in &self.jobs {
            if id != &job.binding.admission_id
                || job.intent_sha256 != job.binding.intent_sha256()?
                || job.scope_generation
                    != self
                        .scopes
                        .get(&job.binding.scope_id)
                        .ok_or(LedgerError::Invalid)?
                        .scope
                        .generation
                || job.decision_expires_unix_ms <= job.reserved_unix_ms
                || job.reserved_unix_ms > self.last_clock_unix_ms
                || job.delivery == JobDelivery::Acknowledged
                    && !matches!(
                        job.execution,
                        JobExecution::Confirmed | JobExecution::RefusedNoEffect
                    )
                || matches!(job.execution, JobExecution::Confirmed) != job.result_sha256.is_some()
                || matches!(
                    job.execution,
                    JobExecution::Reserved | JobExecution::RefusedNoEffect
                ) && job.dispatched_unix_ms.is_some()
                || matches!(
                    job.execution,
                    JobExecution::Dispatching | JobExecution::Uncertain | JobExecution::Confirmed
                ) && job.dispatched_unix_ms.is_none()
            {
                return Err(LedgerError::Invalid);
            }
            if let Some(hash) = &job.result_sha256 {
                if !is_sha256(hash) {
                    return Err(LedgerError::Invalid);
                }
            }
        }
        Ok(())
    }

    fn budget(&self, scope_id: &str, exclude: Option<&str>) -> Result<BudgetView, LedgerError> {
        let mut view = BudgetView {
            settled_units: 0,
            reserved_units: 0,
            active: 0,
            attempts: 0,
            last_dispatch_unix_ms: None,
            last_clock_unix_ms: self.last_clock_unix_ms,
            revoked: self
                .scopes
                .get(scope_id)
                .ok_or(LedgerError::Invalid)?
                .revoked,
        };
        for (id, job) in &self.jobs {
            if job.binding.scope_id != scope_id || exclude == Some(id.as_str()) {
                continue;
            }
            view.attempts = view.attempts.checked_add(1).ok_or(LedgerError::Invalid)?;
            match job.execution {
                JobExecution::Reserved | JobExecution::Dispatching | JobExecution::Uncertain => {
                    view.reserved_units = view
                        .reserved_units
                        .checked_add(job.binding.units)
                        .ok_or(LedgerError::Invalid)?;
                    view.active = view.active.checked_add(1).ok_or(LedgerError::Invalid)?;
                }
                JobExecution::Confirmed => {
                    view.settled_units = view
                        .settled_units
                        .checked_add(job.binding.units)
                        .ok_or(LedgerError::Invalid)?;
                }
                JobExecution::RefusedNoEffect => {}
            }
            if let Some(dispatched) = job.dispatched_unix_ms {
                view.last_dispatch_unix_ms =
                    Some(view.last_dispatch_unix_ms.unwrap_or(0).max(dispatched));
            }
        }
        Ok(view)
    }
}

/// Private persistent custody shared by the M28 gateway and executor on one
/// host. The policy and scopes must come from the selected generated profile.
#[derive(Debug)]
pub struct StandingLedger {
    path: PathBuf,
    lock_path: PathBuf,
    policy_sha256: String,
    controls: StandingControls,
    controls_sha256: String,
    scopes: BTreeMap<String, StandingScope>,
}

impl StandingLedger {
    /// Read the selected immutable scope when constructing a complete
    /// correlation before charging the caller's final exact ticket bytes.
    pub fn scope(&self, scope_id: &str) -> Option<&StandingScope> {
        self.scopes.get(scope_id)
    }

    pub fn new(
        path: PathBuf,
        policy_sha256: String,
        controls: StandingControls,
        scopes: Vec<StandingScope>,
    ) -> Result<Self, LedgerError> {
        controls.validate()?;
        if !path.is_absolute()
            || !is_sha256(&policy_sha256)
            || scopes.is_empty()
            || scopes.len() > LEDGER_MAX_SCOPES
            || scopes.len() > controls.max_scopes as usize
        {
            return Err(LedgerError::Invalid);
        }
        let mut selected = BTreeMap::new();
        for scope in scopes {
            controls.permits_scope(&scope)?;
            if scope.policy_sha256 != policy_sha256
                || selected.insert(scope.id.clone(), scope).is_some()
            {
                return Err(LedgerError::Invalid);
            }
        }
        let lock_path = path.with_extension("standing.lock");
        let controls_sha256 = hex_lower(&Sha256::digest(serde_json::to_vec(&controls)?));
        Ok(Self {
            path,
            lock_path,
            policy_sha256,
            controls,
            controls_sha256,
            scopes: selected,
        })
    }

    /// Provision a fresh private file exactly once under an independent
    /// administrative step. Runtime admission never recreates lost state.
    pub fn initialize(&self) -> Result<(), LedgerError> {
        let _lock = self.acquire_lock()?;
        if self.path.exists() || self.path.is_symlink() {
            return Err(LedgerError::Conflict);
        }
        let ledger = LedgerFile {
            schema: LEDGER_SCHEMA.into(),
            policy_sha256: self.policy_sha256.clone(),
            controls_sha256: self.controls_sha256.clone(),
            last_clock_unix_ms: 0,
            scopes: self
                .scopes
                .iter()
                .map(|(id, scope)| {
                    (
                        id.clone(),
                        ScopeRecord {
                            scope: scope.clone(),
                            revoked: false,
                        },
                    )
                })
                .collect(),
            jobs: BTreeMap::new(),
        };
        ledger.validate(&self.scopes, &self.policy_sha256, &self.controls_sha256)?;
        self.store(&ledger)
    }

    /// Reserve a fresh identity atomically. Exact duplicates only return their
    /// retained record; a changed intent using the same id is a conflict.
    pub fn reserve(
        &self,
        binding: JobBinding,
        facts: &AdmissionFacts,
        now: u64,
    ) -> Result<ReserveOutcome, LedgerError> {
        self.reserve_with_expiry(binding, facts, now, None)
    }

    /// The exact request may choose an earlier expiry so its authenticated
    /// serialized ticket can be fixed before the atomic reservation.
    pub fn reserve_with_expiry(
        &self,
        binding: JobBinding,
        facts: &AdmissionFacts,
        now: u64,
        requested_expiry: Option<u64>,
    ) -> Result<ReserveOutcome, LedgerError> {
        self.transact(|ledger| {
            let digest = binding.intent_sha256()?;
            if let Some(existing) = ledger.jobs.get(&binding.admission_id) {
                return if existing.intent_sha256 == digest && existing.binding == binding {
                    Ok((ReserveOutcome::Existing(existing.clone()), false))
                } else {
                    Err(LedgerError::Conflict)
                };
            }
            if ledger.jobs.len() >= self.controls.max_jobs as usize {
                return Err(LedgerError::Capacity);
            }
            let scope = &ledger
                .scopes
                .get(&binding.scope_id)
                .ok_or(LedgerError::Refused(StandingRefusal::Scope))?
                .scope;
            let decision = evaluate(
                scope,
                &binding,
                facts,
                ledger.budget(&binding.scope_id, None)?,
                now,
            )?;
            let expiry = requested_expiry
                .map(|requested| requested.min(decision.expires_unix_ms))
                .unwrap_or(decision.expires_unix_ms);
            if requested_expiry.is_some_and(|requested| requested != expiry) || expiry <= now {
                return Err(LedgerError::Refused(StandingRefusal::Expired));
            }
            ledger.last_clock_unix_ms = now;
            ledger.jobs.insert(
                binding.admission_id.clone(),
                JobRecord {
                    binding,
                    intent_sha256: digest,
                    scope_generation: decision.scope_generation,
                    decision_expires_unix_ms: expiry,
                    reserved_unix_ms: now,
                    dispatched_unix_ms: None,
                    execution: JobExecution::Reserved,
                    delivery: JobDelivery::Pending,
                    result_sha256: None,
                    cancel_requested: false,
                },
            );
            Ok((
                ReserveOutcome::Reserved(AdmissionDecision {
                    expires_unix_ms: expiry,
                    ..decision
                }),
                true,
            ))
        })
    }

    /// Recheck current scope and facts at the provider boundary, then commit
    /// the dispatch barrier before invoking the native executor.
    pub fn begin_dispatch(
        &self,
        admission_id: &str,
        facts: &AdmissionFacts,
        now: u64,
    ) -> Result<JobRecord, LedgerError> {
        self.transact(|ledger| {
            let old = ledger
                .jobs
                .get(admission_id)
                .ok_or(LedgerError::Invalid)?
                .clone();
            if old.execution != JobExecution::Reserved {
                return Err(LedgerError::Phase);
            }
            if old.cancel_requested {
                return Err(LedgerError::Phase);
            }
            if now >= old.decision_expires_unix_ms {
                return Err(LedgerError::Refused(StandingRefusal::Expired));
            }
            let scope = &ledger
                .scopes
                .get(&old.binding.scope_id)
                .ok_or(LedgerError::Invalid)?
                .scope;
            if old.scope_generation != scope.generation {
                return Err(LedgerError::Refused(StandingRefusal::Stale));
            }
            let budget = ledger.budget(&old.binding.scope_id, Some(admission_id))?;
            evaluate(scope, &old.binding, facts, budget, now)?;
            ledger.last_clock_unix_ms = now;
            let entry = ledger
                .jobs
                .get_mut(admission_id)
                .ok_or(LedgerError::Invalid)?;
            entry.execution = JobExecution::Dispatching;
            entry.dispatched_unix_ms = Some(now);
            Ok((entry.clone(), true))
        })
    }

    /// Retain the allocation after an uncertain native outcome. Reconciliation
    /// needs the original provider identity and verified native observation.
    pub fn mark_uncertain(&self, admission_id: &str) -> Result<JobRecord, LedgerError> {
        self.transition(
            admission_id,
            JobExecution::Dispatching,
            JobExecution::Uncertain,
            None,
        )
    }

    /// A verified native outcome charges the reserved units exactly once.
    /// Repeated delivery of the same proof is idempotent.
    pub fn confirm(
        &self,
        admission_id: &str,
        result_sha256: &str,
    ) -> Result<JobRecord, LedgerError> {
        if !is_sha256(result_sha256) {
            return Err(LedgerError::Invalid);
        }
        self.transact(|ledger| {
            let entry = ledger
                .jobs
                .get_mut(admission_id)
                .ok_or(LedgerError::Invalid)?;
            if entry.execution == JobExecution::Confirmed
                && entry.result_sha256.as_deref() == Some(result_sha256)
            {
                return Ok((entry.clone(), false));
            }
            if !matches!(
                entry.execution,
                JobExecution::Dispatching | JobExecution::Uncertain
            ) {
                return Err(LedgerError::Phase);
            }
            entry.execution = JobExecution::Confirmed;
            entry.result_sha256 = Some(result_sha256.to_owned());
            Ok((entry.clone(), true))
        })
    }

    /// Only a pre-dispatch refusal proves that no native allocation occurred.
    pub fn refuse_before_dispatch(&self, admission_id: &str) -> Result<JobRecord, LedgerError> {
        self.transition(
            admission_id,
            JobExecution::Reserved,
            JobExecution::RefusedNoEffect,
            None,
        )
    }

    /// Delivery acknowledgement leaves the original effect and accounting
    /// record intact until an explicit retention migration is defined.
    pub fn acknowledge_delivery(&self, admission_id: &str) -> Result<JobRecord, LedgerError> {
        self.transact(|ledger| {
            let entry = ledger
                .jobs
                .get_mut(admission_id)
                .ok_or(LedgerError::Invalid)?;
            if !matches!(
                entry.execution,
                JobExecution::Confirmed | JobExecution::RefusedNoEffect
            ) {
                return Err(LedgerError::Phase);
            }
            let changed = entry.delivery != JobDelivery::Acknowledged;
            entry.delivery = JobDelivery::Acknowledged;
            Ok((entry.clone(), changed))
        })
    }

    /// Revocation prevents a new reservation or dispatch. Existing native work
    /// and pending delivery retain their original state.
    pub fn revoke(&self, scope_id: &str) -> Result<(), LedgerError> {
        self.transact(|ledger| {
            let scope = ledger
                .scopes
                .get_mut(scope_id)
                .ok_or(LedgerError::Invalid)?;
            let changed = !scope.revoked;
            scope.revoked = true;
            Ok(((), changed))
        })
    }

    pub fn status(&self, admission_id: &str) -> Result<Option<JobRecord>, LedgerError> {
        self.transact(|ledger| Ok((ledger.jobs.get(admission_id).cloned(), false)))
    }

    /// Inspect one scope under the same durable lock used for admission.
    pub fn scope_status(&self, scope_id: &str) -> Result<ScopeStatus, LedgerError> {
        self.transact(|ledger| {
            let scope = ledger.scopes.get(scope_id).ok_or(LedgerError::Invalid)?;
            Ok((
                ScopeStatus {
                    scope: scope.scope.clone(),
                    budget: ledger.budget(scope_id, None)?,
                },
                false,
            ))
        })
    }

    /// Request cancellation without claiming that a dispatched effect stopped.
    /// The native executor will refuse a reservation before its dispatch barrier.
    pub fn request_cancel(&self, admission_id: &str) -> Result<JobRecord, LedgerError> {
        self.transact(|ledger| {
            let entry = ledger
                .jobs
                .get_mut(admission_id)
                .ok_or(LedgerError::Invalid)?;
            let changed = !entry.cancel_requested;
            entry.cancel_requested = true;
            Ok((entry.clone(), changed))
        })
    }

    fn transition(
        &self,
        admission_id: &str,
        from: JobExecution,
        to: JobExecution,
        result: Option<String>,
    ) -> Result<JobRecord, LedgerError> {
        self.transact(|ledger| {
            let entry = ledger
                .jobs
                .get_mut(admission_id)
                .ok_or(LedgerError::Invalid)?;
            if entry.execution != from {
                return Err(LedgerError::Phase);
            }
            entry.execution = to;
            entry.result_sha256 = result;
            Ok((entry.clone(), true))
        })
    }

    fn transact<T>(
        &self,
        operation: impl FnOnce(&mut LedgerFile) -> Result<(T, bool), LedgerError>,
    ) -> Result<T, LedgerError> {
        let _lock = self.acquire_lock()?;
        let mut ledger = self.load()?;
        ledger.validate(&self.scopes, &self.policy_sha256, &self.controls_sha256)?;
        let (value, dirty) = operation(&mut ledger)?;
        if dirty {
            ledger.validate(&self.scopes, &self.policy_sha256, &self.controls_sha256)?;
            self.store(&ledger)?;
        }
        Ok(value)
    }

    fn acquire_lock(&self) -> Result<File, LedgerError> {
        let parent = self.path.parent().ok_or(LedgerError::Invalid)?;
        if !parent.is_dir()
            || parent.is_symlink()
            || self.path.is_symlink()
            || self.lock_path.is_symlink()
        {
            return Err(LedgerError::Invalid);
        }
        #[cfg(unix)]
        if parent.metadata()?.permissions().mode() & 0o077 != 0 {
            return Err(LedgerError::Invalid);
        }
        let mut lock_options = OpenOptions::new();
        lock_options.read(true).write(true).create(true);
        #[cfg(unix)]
        lock_options.mode(0o600);
        let lock = lock_options.open(&self.lock_path)?;
        #[cfg(unix)]
        if lock.metadata()?.permissions().mode() & 0o077 != 0 {
            return Err(LedgerError::Invalid);
        }
        lock.lock_exclusive()?;
        Ok(lock)
    }

    fn load(&self) -> Result<LedgerFile, LedgerError> {
        let file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(LedgerError::Uninitialized);
            }
            Err(error) => return Err(error.into()),
        };
        #[cfg(unix)]
        if file.metadata()?.permissions().mode() & 0o077 != 0 {
            return Err(LedgerError::Invalid);
        }
        let mut bytes = Vec::new();
        file.take((LEDGER_MAX_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > LEDGER_MAX_BYTES {
            return Err(LedgerError::Capacity);
        }
        Ok(serde_json::from_slice(&bytes)?)
    }

    fn store(&self, ledger: &LedgerFile) -> Result<(), LedgerError> {
        let bytes = serde_json::to_vec(ledger)?;
        if bytes.len() > LEDGER_MAX_BYTES {
            return Err(LedgerError::Capacity);
        }
        let temp_path = self.path.with_extension(format!(
            "standing-tmp-{}-{}",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| -> Result<(), LedgerError> {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            options.mode(0o600);
            let mut file = options.open(&temp_path)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::rename(&temp_path, &self.path)?;
            File::open(self.path.parent().ok_or(LedgerError::Invalid)?)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp_path);
        }
        result
    }
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::standing::{JOB_BINDING_SCHEMA, STANDING_SCOPE_SCHEMA};
    use std::sync::{Arc, Barrier};
    use std::{path::Path, vec};

    fn setup(path: &Path) -> (StandingLedger, JobBinding, AdmissionFacts) {
        #[cfg(unix)]
        fs::set_permissions(path.parent().unwrap(), fs::Permissions::from_mode(0o700))
            .expect("private state directory");
        let policy = "a".repeat(64);
        let controls = StandingControls {
            schema: crate::standing::STANDING_CONTROLS_SCHEMA.into(),
            enabled: true,
            actions: vec!["gpu.workload.submit".into()],
            max_scopes: 1,
            max_jobs: 256,
            max_job_units: 5,
            max_total_units: 10,
            max_concurrent: 1,
            max_retries: 2,
            max_cooldown_ms: 0,
            max_fact_age_ms: 100,
            max_decision_ttl_ms: 30,
        };
        let ledger = StandingLedger::new(
            path.to_owned(),
            policy.clone(),
            controls,
            vec![StandingScope {
                schema: STANDING_SCOPE_SCHEMA.into(),
                id: "scope-1".into(),
                subject: "operator".into(),
                action: "gpu.workload.submit".into(),
                target: "/gpu/GPU-0/workload".into(),
                policy_sha256: policy.clone(),
                generation: 1,
                expires_unix_ms: 2000,
                max_job_units: 5,
                max_total_units: 10,
                max_concurrent: 1,
                max_retries: 2,
                cooldown_ms: 0,
                fact_max_age_ms: 100,
                decision_ttl_ms: 30,
            }],
        )
        .expect("policy");
        let binding = JobBinding {
            schema: JOB_BINDING_SCHEMA.into(),
            scope_id: "scope-1".into(),
            admission_id: "admit-1".into(),
            ticket_id: "ticket-1".into(),
            idempotency_key: "once-1".into(),
            subject: "operator".into(),
            action: "gpu.workload.submit".into(),
            target: "/gpu/GPU-0/workload".into(),
            input_sha256: "b".repeat(64),
            policy_sha256: policy.clone(),
            state_epoch: 1,
            resource_generation: 1,
            deadline_unix_ms: 1500,
            units: 4,
            attempt: 1,
        };
        let facts = AdmissionFacts {
            observed_unix_ms: 1000,
            state_epoch: 1,
            resource_generation: 1,
            policy_sha256: policy,
        };
        (ledger, binding, facts)
    }

    #[test]
    fn restart_and_duplicate_preserve_one_allocation_and_pending_delivery() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("ledger.json");
        let (ledger, binding, facts) = setup(&path);
        ledger.initialize().expect("initialize private ledger");
        assert!(matches!(
            ledger.reserve(binding.clone(), &facts, 1001),
            Ok(ReserveOutcome::Reserved(_))
        ));
        assert_eq!(
            ledger
                .begin_dispatch("admit-1", &facts, 1002)
                .expect("dispatch")
                .execution,
            JobExecution::Dispatching
        );
        let (restarted, _, _) = setup(&path);
        assert!(matches!(
            restarted.reserve(binding, &facts, 1003),
            Ok(ReserveOutcome::Existing(_))
        ));
        assert_eq!(
            restarted
                .mark_uncertain("admit-1")
                .expect("uncertain")
                .execution,
            JobExecution::Uncertain
        );
        let mut second = restarted
            .status("admit-1")
            .expect("status")
            .expect("entry")
            .binding;
        second.admission_id = "admit-2".into();
        second.ticket_id = "ticket-2".into();
        second.idempotency_key = "once-2".into();
        second.attempt = 2;
        assert!(matches!(
            restarted.reserve(second, &facts, 1003),
            Err(LedgerError::Refused(StandingRefusal::Concurrency))
        ));
        assert_eq!(
            restarted
                .confirm("admit-1", &"c".repeat(64))
                .expect("confirm")
                .delivery,
            JobDelivery::Pending
        );
        assert_eq!(
            restarted
                .acknowledge_delivery("admit-1")
                .expect("delivery")
                .delivery,
            JobDelivery::Acknowledged
        );
        assert_eq!(
            restarted
                .confirm("admit-1", &"c".repeat(64))
                .expect("idempotent")
                .execution,
            JobExecution::Confirmed
        );
    }

    #[test]
    fn revocation_and_corruption_refuse_without_losing_existing_work() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("ledger.json");
        let (ledger, binding, facts) = setup(&path);
        ledger.initialize().expect("initialize private ledger");
        ledger
            .reserve(binding.clone(), &facts, 1001)
            .expect("reserve");
        ledger.revoke("scope-1").expect("revoke");
        assert!(matches!(
            ledger.begin_dispatch("admit-1", &facts, 1002),
            Err(LedgerError::Refused(StandingRefusal::Revoked))
        ));
        assert_eq!(
            ledger
                .status("admit-1")
                .expect("status")
                .expect("entry")
                .execution,
            JobExecution::Reserved
        );
        ledger.refuse_before_dispatch("admit-1").expect("no effect");
        fs::write(&path, b"{broken").expect("corrupt");
        assert!(matches!(
            ledger.status("admit-1"),
            Err(LedgerError::Codec(_))
        ));
    }

    #[test]
    fn simultaneous_clients_share_one_concurrency_and_budget_reservation() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("ledger.json");
        let (ledger, binding, facts) = setup(&path);
        ledger.initialize().expect("initialize private ledger");
        let ledger = Arc::new(ledger);
        let start = Arc::new(Barrier::new(3));
        let mut workers = Vec::new();
        for number in 0..2 {
            let ledger = Arc::clone(&ledger);
            let start = Arc::clone(&start);
            let facts = facts.clone();
            let mut request = binding.clone();
            request.admission_id = format!("admit-{number}");
            request.ticket_id = format!("ticket-{number}");
            request.idempotency_key = format!("once-{number}");
            workers.push(std::thread::spawn(move || {
                start.wait();
                ledger.reserve(request, &facts, 1001)
            }));
        }
        start.wait();
        let results = workers
            .into_iter()
            .map(|worker| worker.join().expect("thread"))
            .collect::<Vec<_>>();
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Ok(ReserveOutcome::Reserved(_))))
                .count(),
            1
        );
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(
                    result,
                    Err(LedgerError::Refused(StandingRefusal::Concurrency))
                ))
                .count(),
            1
        );
    }

    #[test]
    fn missing_ledger_after_native_dispatch_never_resets_accounting() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("ledger.json");
        let (ledger, binding, facts) = setup(&path);
        assert!(matches!(
            ledger.reserve(binding.clone(), &facts, 1001),
            Err(LedgerError::Uninitialized)
        ));
        ledger.initialize().expect("provision");
        ledger.reserve(binding, &facts, 1001).expect("reserve");
        ledger
            .begin_dispatch("admit-1", &facts, 1002)
            .expect("dispatch");
        fs::remove_file(&path).expect("simulate lost state");
        assert!(matches!(
            ledger.status("admit-1"),
            Err(LedgerError::Uninitialized)
        ));
    }

    #[test]
    fn authenticated_short_expiry_and_cancel_block_dispatch_without_spending() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("ledger.json");
        let (ledger, binding, facts) = setup(&path);
        ledger.initialize().expect("initialize");
        assert!(matches!(
            ledger.reserve_with_expiry(binding.clone(), &facts, 1001, Some(1032)),
            Err(LedgerError::Refused(StandingRefusal::Expired))
        ));
        let decision = ledger
            .reserve_with_expiry(binding, &facts, 1001, Some(1020))
            .expect("short decision");
        assert!(
            matches!(decision, ReserveOutcome::Reserved(value) if value.expires_unix_ms == 1020)
        );
        assert!(
            ledger
                .request_cancel("admit-1")
                .expect("cancel")
                .cancel_requested
        );
        assert!(matches!(
            ledger.begin_dispatch("admit-1", &facts, 1002),
            Err(LedgerError::Phase)
        ));
        assert_eq!(
            ledger
                .refuse_before_dispatch("admit-1")
                .expect("no effect")
                .execution,
            JobExecution::RefusedNoEffect
        );
    }
}
