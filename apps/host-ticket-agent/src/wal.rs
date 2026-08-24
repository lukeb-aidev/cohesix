// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Persist relay and receipt execution state with crash-safe local fencing.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{anyhow, Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};

use crate::claim::{SpecSource, TicketKey};
use crate::HostTicketSpec;

/// Maximum durable bytes retained by the 26e receipt execution journal.
pub const EXECUTION_JOURNAL_MAX_BYTES: usize = 1024 * 1024;
/// Maximum non-compacted version-2 operations retained in one journal.
pub const EXECUTION_JOURNAL_MAX_ENTRIES: usize = 256;
/// Maximum supported durable execution-lane count.
pub const EXECUTION_LANE_MAX_COUNT: u8 = 64;

const EXECUTION_LANE_TOPOLOGY_SCHEMA: &str = "host-ticket-execution-lanes/v1";
const EXECUTION_JOURNAL_SCHEMA_V2: &str = "host-ticket-execution-journal/v2";
const EXECUTION_JOURNAL_SCHEMA_V3: &str = "host-ticket-execution-journal/v3";

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ExecutionLaneTopology {
    schema: String,
    lanes: u8,
}

/// Bind one journal family to an immutable execution-lane count.
///
/// Changing the modulo lane assignment while an operation is only provider-
/// committed would orphan its journal and risk replay. The topology sidecar
/// therefore fails closed until the operator selects a fresh state directory.
pub fn bind_execution_lane_topology(journal_path: &Path, lanes: u8) -> Result<PathBuf> {
    if lanes == 0 || lanes > EXECUTION_LANE_MAX_COUNT {
        return Err(anyhow!(
            "execution lane count must be within 1..={EXECUTION_LANE_MAX_COUNT}"
        ));
    }
    let topology_path = journal_path.with_extension("topology.json");
    match fs::read(&topology_path) {
        Ok(payload) => {
            let topology: ExecutionLaneTopology = serde_json::from_slice(&payload)
                .with_context(|| format!("parse lane topology {}", topology_path.display()))?;
            if topology.schema != EXECUTION_LANE_TOPOLOGY_SCHEMA || topology.lanes != lanes {
                return Err(anyhow!(
                    "execution lane topology mismatch: state has {} lanes, requested {lanes}",
                    topology.lanes
                ));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let topology = ExecutionLaneTopology {
                schema: EXECUTION_LANE_TOPOLOGY_SCHEMA.to_owned(),
                lanes,
            };
            let payload = serde_json::to_vec_pretty(&topology)
                .context("serialize execution lane topology")?;
            durable_atomic_write(&topology_path, &payload, 256, "execution lane topology")?;
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("read lane topology {}", topology_path.display()))
        }
    }
    Ok(topology_path)
}

/// Relay WAL entry lifecycle state.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RelayWalState {
    /// Entry has not yet been forwarded to the target hive.
    Pending,
    /// Entry was forwarded and acknowledged by the target hive.
    Delivered,
}

/// One federated relay WAL record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelayWalEntry {
    /// Monotonic insertion sequence.
    pub seq: u64,
    /// Stable correlation key (`id:idempotency_key:source:target`).
    pub key: String,
    /// Target hive identifier.
    pub target_hive: String,
    /// JSON payload to forward to `/host/tickets/spec` on the target hive.
    pub payload: String,
    /// Delivery state.
    pub state: RelayWalState,
    /// Delivery attempt counter.
    pub attempts: u32,
    /// Last delivery error summary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// In-memory relay WAL with atomic file persistence.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RelayWal {
    #[serde(default)]
    next_seq: u64,
    #[serde(default)]
    entries: Vec<RelayWalEntry>,
}

impl RelayWal {
    /// Load relay WAL from disk; missing files resolve to an empty WAL.
    pub fn load(path: &Path) -> Result<Self> {
        let payload = match fs::read(path) {
            Ok(payload) => payload,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => {
                return Err(err).with_context(|| format!("read relay WAL {}", path.display()))
            }
        };
        let wal: Self = serde_json::from_slice(&payload)
            .with_context(|| format!("parse relay WAL {}", path.display()))?;
        Ok(wal)
    }

    /// Persist relay WAL to disk atomically.
    pub fn save(&self, path: &Path) -> Result<()> {
        let payload = serde_json::to_vec_pretty(self).context("serialize relay WAL")?;
        durable_atomic_write(path, &payload, usize::MAX, "relay WAL")
    }

    /// Return true if the key already exists in delivered state.
    #[must_use]
    pub fn contains_delivered(&self, key: &str) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.key == key && entry.state == RelayWalState::Delivered)
    }

    /// Return true if the key exists in any state.
    #[must_use]
    pub fn contains_key(&self, key: &str) -> bool {
        self.entries.iter().any(|entry| entry.key == key)
    }

    /// Insert a pending record if it does not yet exist.
    pub fn upsert_pending(&mut self, key: &str, target_hive: &str, payload: &str) {
        if self.entries.iter().any(|entry| entry.key == key) {
            return;
        }
        self.next_seq = self.next_seq.saturating_add(1);
        self.entries.push(RelayWalEntry {
            seq: self.next_seq,
            key: key.to_owned(),
            target_hive: target_hive.to_owned(),
            payload: payload.to_owned(),
            state: RelayWalState::Pending,
            attempts: 0,
            last_error: None,
        });
    }

    /// Mark a key as delivered.
    pub fn mark_delivered(&mut self, key: &str) {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.key == key) {
            entry.state = RelayWalState::Delivered;
            entry.last_error = None;
            entry.attempts = entry.attempts.saturating_add(1);
        }
    }

    /// Mark a delivery attempt as failed with an error message.
    pub fn mark_failed(&mut self, key: &str, detail: &str) {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.key == key) {
            entry.attempts = entry.attempts.saturating_add(1);
            entry.last_error = Some(detail.to_owned());
        }
    }

    /// Borrow pending entries in deterministic insertion order.
    #[must_use]
    pub fn pending_entries(&self) -> Vec<RelayWalEntry> {
        let mut entries = self
            .entries
            .iter()
            .filter(|entry| entry.state == RelayWalState::Pending)
            .cloned()
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.seq);
        entries
    }

    /// Number of pending entries.
    #[must_use]
    pub fn pending_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.state == RelayWalState::Pending)
            .count()
    }

    /// Approximate serialized byte size of the WAL payload.
    #[must_use]
    pub fn serialized_len(&self) -> usize {
        serde_json::to_vec(self)
            .map(|payload| payload.len())
            .unwrap_or(usize::MAX)
    }

    /// Enforce deterministic WAL retention limits.
    pub fn enforce_limits(&mut self, max_entries: usize, max_bytes: usize) {
        if max_entries == 0 || max_bytes == 0 {
            self.entries.clear();
            return;
        }
        while self.entries.len() > max_entries || self.serialized_len() > max_bytes {
            if !self.drop_oldest_delivered() {
                if self.entries.is_empty() {
                    break;
                }
                self.entries.sort_by_key(|entry| entry.seq);
                self.entries.remove(0);
            }
        }
    }

    fn drop_oldest_delivered(&mut self) -> bool {
        self.entries.sort_by_key(|entry| entry.seq);
        if let Some((idx, _entry)) = self
            .entries
            .iter()
            .enumerate()
            .find(|(_idx, entry)| entry.state == RelayWalState::Delivered)
        {
            self.entries.remove(idx);
            true
        } else {
            false
        }
    }
}

/// Crash-safe version-2 execution state.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum ExecutionJournalState {
    /// Root-admitted request and immutable identity have been persisted.
    Prepared,
    /// Provider execution may have started and must never be blindly replayed.
    Executing,
    /// Provider outcome has been durably persisted locally.
    ProviderResultPersisted,
    /// The exact result was published, or an identical terminal result was observed.
    ResultPublished,
    /// Publication and cursor advancement have both completed.
    Terminal,
}

/// Durable terminal provider outcome retained before VM result publication.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct JournalProviderResult {
    /// Receipt projection selected from the durable provider outcome.
    pub outcome: JournalProviderOutcome,
    /// UTF-8-safe bounded provider detail.
    pub message: String,
    /// Whether this outcome was reconstructed by action-specific recovery.
    pub reconciled: bool,
}

/// Durable receipt projection derived from provider execution or reconciliation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JournalProviderOutcome {
    /// Exact provider operation committed and maps to a confirmed Worker receipt.
    Confirmed,
    /// Provider operation was deterministically rejected.
    Rejected,
    /// The binding or observation window became stale without replaying the provider.
    Stale,
}

/// One immutable receipt-bearing operation and its durable state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionJournalEntry {
    /// Stable length-prefixed ticket/idempotency key.
    pub key: String,
    /// Root-normalized admitted version-2 request.
    pub spec: HostTicketSpec,
    /// Current crash-safe execution phase.
    pub state: ExecutionJournalState,
    /// Provider result persisted before any VM result write.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_result: Option<JournalProviderResult>,
    /// Exact canonical result destination.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_path: Option<String>,
    /// Exact canonical result JSON line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_line: Option<String>,
}

/// Bounded durable journal for the seven version-2 receipt actions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionJournal {
    schema: String,
    /// Highest global admission sequence durably completed by this lane.
    ///
    /// Every lane observes the same monotonically increasing root sequence.
    /// Once its cursor has advanced through this value, all earlier work
    /// assigned to the lane is terminal and its full recovery payload can be
    /// removed without permitting provider replay.
    #[serde(default)]
    completed_through_admission_sequence: u64,
    entries: BTreeMap<String, ExecutionJournalEntry>,
}

impl Default for ExecutionJournal {
    fn default() -> Self {
        Self {
            schema: EXECUTION_JOURNAL_SCHEMA_V3.to_owned(),
            completed_through_admission_sequence: 0,
            entries: BTreeMap::new(),
        }
    }
}

impl ExecutionJournal {
    /// Load and validate a bounded journal; a missing file is empty state.
    pub fn load(path: &Path) -> Result<Self> {
        let payload = match fs::read(path) {
            Ok(payload) => payload,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("read execution journal {}", path.display()))
            }
        };
        if payload.len() > EXECUTION_JOURNAL_MAX_BYTES {
            return Err(anyhow!(
                "execution journal {} exceeds {} bytes",
                path.display(),
                EXECUTION_JOURNAL_MAX_BYTES
            ));
        }
        let mut journal: Self = serde_json::from_slice(&payload)
            .with_context(|| format!("parse execution journal {}", path.display()))?;
        match journal.schema.as_str() {
            EXECUTION_JOURNAL_SCHEMA_V2 => {
                // A v2 terminal entry proves that result publication and
                // durable cursor advancement both completed. Promote that
                // proof into the compact v3 fence during the rolling upgrade.
                journal.completed_through_admission_sequence = journal
                    .entries
                    .values()
                    .filter(|entry| entry.state == ExecutionJournalState::Terminal)
                    .filter_map(|entry| entry.spec.admission_sequence)
                    .max()
                    .unwrap_or(0);
                journal.schema = EXECUTION_JOURNAL_SCHEMA_V3.to_owned();
            }
            EXECUTION_JOURNAL_SCHEMA_V3 => {}
            _ => return Err(anyhow!("unsupported execution journal schema")),
        }
        journal.validate()?;
        Ok(journal)
    }

    /// Durably persist the complete bounded journal.
    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        let payload = serde_json::to_vec_pretty(self).context("serialize execution journal")?;
        durable_atomic_write(
            path,
            &payload,
            EXECUTION_JOURNAL_MAX_BYTES,
            "execution journal",
        )
    }

    /// Persist a root-admitted request, rejecting key reuse with different bytes.
    pub fn prepare(&mut self, spec: &HostTicketSpec) -> Result<()> {
        crate::claim::validate_spec(spec, SpecSource::AdmittedSnapshot)?;
        let key = TicketKey::new(&spec.id, &spec.idempotency_key).journal_key();
        if self.entries.contains_key(&key) {
            let existing = self
                .entries
                .get(&key)
                .ok_or_else(|| anyhow!("execution journal lookup failed"))?;
            if existing.spec != *spec {
                return Err(anyhow!(
                    "execution journal key {key} was reused with a different admitted request"
                ));
            }
            return Ok(());
        }
        if self.entries.len() >= EXECUTION_JOURNAL_MAX_ENTRIES {
            return Err(anyhow!(
                "execution journal reached {} entries",
                EXECUTION_JOURNAL_MAX_ENTRIES
            ));
        }
        self.entries.insert(
            key.clone(),
            ExecutionJournalEntry {
                key: key.clone(),
                spec: spec.clone(),
                state: ExecutionJournalState::Prepared,
                provider_result: None,
                result_path: None,
                result_line: None,
            },
        );
        Ok(())
    }

    /// Borrow one entry by ticket key.
    #[must_use]
    pub fn get(&self, key: &TicketKey) -> Option<&ExecutionJournalEntry> {
        self.entries.get(&key.journal_key())
    }

    /// Highest root admission sequence covered by the durable completion fence.
    #[must_use]
    pub fn completed_through_admission_sequence(&self) -> u64 {
        self.completed_through_admission_sequence
    }

    /// Whether a root admission is already covered by the durable completion fence.
    pub fn is_compacted_terminal(&self, spec: &HostTicketSpec) -> Result<bool> {
        crate::claim::validate_spec(spec, SpecSource::AdmittedSnapshot)?;
        let admission_sequence = spec
            .admission_sequence
            .ok_or_else(|| anyhow!("version-2 journal entry lacks admission_sequence"))?;
        Ok(admission_sequence <= self.completed_through_admission_sequence)
    }

    /// Compact completed recovery payloads after durable cursor advancement.
    ///
    /// Prepared, executing, and provider-result-persisted entries are never
    /// removable. `ResultPublished` is removable only when the caller is
    /// atomically advancing this journal's completion fence after the guest
    /// publication; `Terminal` carries equivalent ordering proof from an
    /// earlier journal lifecycle.
    pub fn compact_completed_through(&mut self, admission_sequence: u64) -> Result<bool> {
        if admission_sequence < self.completed_through_admission_sequence {
            return Err(anyhow!(
                "execution journal completion fence cannot move backward"
            ));
        }
        let mut completed_keys = Vec::new();
        for (key, entry) in &self.entries {
            let entry_sequence = entry
                .spec
                .admission_sequence
                .ok_or_else(|| anyhow!("version-2 journal entry lacks admission_sequence"))?;
            if entry_sequence > admission_sequence {
                continue;
            }
            if !matches!(
                entry.state,
                ExecutionJournalState::ResultPublished | ExecutionJournalState::Terminal
            ) {
                return Err(anyhow!(
                    "execution journal cursor passed nonterminal admission {entry_sequence}"
                ));
            }
            completed_keys.push(key.clone());
        }
        let changed = admission_sequence != self.completed_through_admission_sequence
            || !completed_keys.is_empty();
        for key in completed_keys {
            self.entries.remove(&key);
        }
        self.completed_through_admission_sequence = admission_sequence;
        Ok(changed)
    }

    /// Advance one entry to `executing` before provider dispatch.
    pub fn mark_executing(&mut self, key: &TicketKey) -> Result<()> {
        let entry = self.entry_mut(key)?;
        match entry.state {
            ExecutionJournalState::Prepared => {
                entry.state = ExecutionJournalState::Executing;
                Ok(())
            }
            ExecutionJournalState::Executing => Ok(()),
            other => Err(anyhow!("cannot mark journal {other:?} as executing")),
        }
    }

    /// Persist the provider result before attempting VM publication.
    pub fn persist_provider_result(
        &mut self,
        key: &TicketKey,
        result: JournalProviderResult,
    ) -> Result<()> {
        let entry = self.entry_mut(key)?;
        if entry.state != ExecutionJournalState::Executing {
            return Err(anyhow!(
                "provider result requires executing state, got {:?}",
                entry.state
            ));
        }
        entry.provider_result = Some(result);
        entry.state = ExecutionJournalState::ProviderResultPersisted;
        Ok(())
    }

    /// Persist exact result bytes before attempting VM publication.
    pub fn stage_result(&mut self, key: &TicketKey, path: &str, line: &str) -> Result<()> {
        let entry = self.entry_mut(key)?;
        if entry.state != ExecutionJournalState::ProviderResultPersisted {
            return Err(anyhow!(
                "result publication requires provider-result-persisted state, got {:?}",
                entry.state
            ));
        }
        entry.result_path = Some(path.to_owned());
        entry.result_line = Some(line.to_owned());
        Ok(())
    }

    /// Advance to `result-published` only after write success or exact observation.
    pub fn mark_result_published(&mut self, key: &TicketKey) -> Result<()> {
        let entry = self.entry_mut(key)?;
        if entry.state != ExecutionJournalState::ProviderResultPersisted
            || entry.result_path.is_none()
            || entry.result_line.is_none()
        {
            return Err(anyhow!(
                "result-published transition requires staged provider result"
            ));
        }
        entry.state = ExecutionJournalState::ResultPublished;
        Ok(())
    }

    /// Mark a published operation terminal after durable cursor advancement.
    pub fn mark_terminal(&mut self, key: &TicketKey) -> Result<()> {
        let entry = self.entry_mut(key)?;
        if entry.state != ExecutionJournalState::ResultPublished {
            return Err(anyhow!(
                "terminal transition requires result-published state, got {:?}",
                entry.state
            ));
        }
        entry.state = ExecutionJournalState::Terminal;
        Ok(())
    }

    fn entry_mut(&mut self, key: &TicketKey) -> Result<&mut ExecutionJournalEntry> {
        self.entries
            .get_mut(&key.journal_key())
            .ok_or_else(|| anyhow!("execution journal entry not found"))
    }

    fn validate(&self) -> Result<()> {
        if self.schema != EXECUTION_JOURNAL_SCHEMA_V3 {
            return Err(anyhow!("unsupported execution journal schema"));
        }
        if self.entries.len() > EXECUTION_JOURNAL_MAX_ENTRIES {
            return Err(anyhow!(
                "execution journal exceeds {} entries",
                EXECUTION_JOURNAL_MAX_ENTRIES
            ));
        }
        for (key, entry) in &self.entries {
            if key != &entry.key {
                return Err(anyhow!("execution journal map/entry key mismatch"));
            }
            crate::claim::validate_spec(&entry.spec, SpecSource::AdmittedSnapshot)?;
            let expected =
                TicketKey::new(&entry.spec.id, &entry.spec.idempotency_key).journal_key();
            if expected != *key {
                return Err(anyhow!(
                    "execution journal key does not match ticket identity"
                ));
            }
            let admission_sequence = entry
                .spec
                .admission_sequence
                .ok_or_else(|| anyhow!("version-2 journal entry lacks admission_sequence"))?;
            if admission_sequence <= self.completed_through_admission_sequence
                && entry.state != ExecutionJournalState::Terminal
            {
                return Err(anyhow!(
                    "execution journal completion fence covers nonterminal entry"
                ));
            }
            if entry.state >= ExecutionJournalState::ProviderResultPersisted
                && entry.provider_result.is_none()
            {
                return Err(anyhow!("persisted journal state lacks provider result"));
            }
            if entry.state >= ExecutionJournalState::ResultPublished
                && (entry.result_path.is_none() || entry.result_line.is_none())
            {
                return Err(anyhow!("published journal state lacks exact result bytes"));
            }
            if let Some(result) = &entry.provider_result {
                if result.message.is_empty()
                    || result.message.len() > 192
                    || result.message.chars().any(char::is_control)
                {
                    return Err(anyhow!(
                        "execution journal provider message must be control-free and 1..=192 bytes"
                    ));
                }
            }
            if let Some(path) = entry.result_path.as_deref() {
                if !path.starts_with('/')
                    || !(path.ends_with("/tickets/status") || path.ends_with("/tickets/deadletter"))
                {
                    return Err(anyhow!("execution journal result path is not canonical"));
                }
            }
            if let Some(line) = entry.result_line.as_ref() {
                let parsed = crate::claim::parse_result_lines_from(
                    std::slice::from_ref(line),
                    &[crate::HOST_TICKET_RESULT_V2_SCHEMA.to_owned()],
                    8192,
                )?;
                let result = parsed
                    .first()
                    .ok_or_else(|| anyhow!("execution journal result line is empty"))?;
                if result.id != entry.spec.id
                    || result.idempotency_key != entry.spec.idempotency_key
                    || result.action != entry.spec.action
                    || result.operation_id != entry.spec.operation_id
                    || result.subject_ref != entry.spec.subject_ref
                    || result.receipt_worker_role != entry.spec.receipt_worker_role
                    || result.receipt_worker_id != entry.spec.receipt_worker_id
                    || result.receipt_supervisor_generation
                        != entry.spec.receipt_supervisor_generation
                    || result.receipt_cap_generation != entry.spec.receipt_cap_generation
                    || result.resolved_worker_slot != entry.spec.resolved_worker_slot
                    || result.resolved_lease_epoch != entry.spec.resolved_lease_epoch
                    || result.admission_sequence != entry.spec.admission_sequence
                {
                    return Err(anyhow!(
                        "execution journal result does not echo admitted Worker binding"
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Process-lifetime exclusive fence for one local host-ticket agent state root.
#[derive(Debug)]
pub struct AgentFence {
    file: File,
}

impl AgentFence {
    /// Acquire a nonblocking exclusive fence and record the current process id.
    pub fn acquire(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create agent lock dir {}", parent.display()))?;
        }
        if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return Err(anyhow!(
                "agent lock {} must not be a symlink",
                path.display()
            ));
        }
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .with_context(|| format!("open agent lock {}", path.display()))?;
        if let Err(err) = file.try_lock_exclusive() {
            if err.kind() == std::io::ErrorKind::WouldBlock {
                return Err(anyhow!(
                    "host-ticket-agent already owns execution fence {}",
                    path.display()
                ));
            }
            return Err(err).with_context(|| format!("lock agent fence {}", path.display()));
        }
        file.set_len(0)
            .with_context(|| format!("truncate agent lock {}", path.display()))?;
        writeln!(file, "pid={}", std::process::id())
            .with_context(|| format!("write agent lock {}", path.display()))?;
        file.sync_all()
            .with_context(|| format!("sync agent lock {}", path.display()))?;
        Ok(Self { file })
    }
}

impl Drop for AgentFence {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.file);
    }
}

fn durable_atomic_write(path: &Path, payload: &[u8], max_bytes: usize, label: &str) -> Result<()> {
    if payload.len() > max_bytes {
        return Err(anyhow!(
            "{label} payload {} exceeds bound {max_bytes}",
            payload.len()
        ));
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("create {label} dir {}", parent.display()))?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("{label} path has no UTF-8 file name"))?;
    let (temp, mut file) = create_unique_temp(parent, name, label)?;
    let write_result = (|| -> Result<()> {
        file.write_all(payload)
            .with_context(|| format!("write {label} temp {}", temp.display()))?;
        file.sync_all()
            .with_context(|| format!("sync {label} temp {}", temp.display()))?;
        fs::rename(&temp, path).with_context(|| format!("commit {label} {}", path.display()))?;
        File::open(parent)
            .with_context(|| format!("open {label} parent {}", parent.display()))?
            .sync_all()
            .with_context(|| format!("sync {label} parent {}", parent.display()))?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    write_result
}

fn create_unique_temp(parent: &Path, name: &str, label: &str) -> Result<(PathBuf, File)> {
    for _ in 0..32 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temp = parent.join(format!(
            ".{name}.{}.{}.partial",
            std::process::id(),
            sequence
        ));
        match OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(file) => return Ok((temp, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("create {label} temp {}", temp.display()));
            }
        }
    }
    Err(anyhow!(
        "could not allocate a unique {label} temp file under {}",
        parent.display()
    ))
}

/// Persist the bounded cursor using the same file+directory durability protocol.
pub(crate) fn save_cursor_durable(path: &Path, payload: &[u8]) -> Result<()> {
    durable_atomic_write(path, payload, 4096, "ticket cursor")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ReceiptMode, HOST_TICKET_RESULT_V2_SCHEMA, HOST_TICKET_V2_SCHEMA};

    fn admitted_v2_spec() -> HostTicketSpec {
        HostTicketSpec {
            schema: HOST_TICKET_V2_SCHEMA.to_owned(),
            id: "ticket-v2".to_owned(),
            idempotency_key: "idem-v2".to_owned(),
            action: "gpu.lease.grant".to_owned(),
            args: serde_json::json!({"ttl_s": 30}),
            receipt_mode: Some(ReceiptMode::Worker),
            operation_id: Some("lease-1".to_owned()),
            subject_ref: Some("GPU-0".to_owned()),
            receipt_worker_role: Some("worker-gpu".to_owned()),
            receipt_worker_id: Some("worker-gpu-1".to_owned()),
            receipt_supervisor_generation: Some(2),
            receipt_cap_generation: Some(3),
            resolved_worker_slot: Some(0),
            resolved_lease_epoch: Some(4),
            admission_sequence: Some(5),
            ..HostTicketSpec::default()
        }
    }

    #[test]
    fn wal_roundtrip_and_limits() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let path = temp.path().join("relay-wal.json");

        let mut wal = RelayWal::default();
        wal.upsert_pending("a", "hive-b", "{\"line\":1}");
        wal.mark_failed("a", "timeout");
        wal.save(&path).expect("save wal");

        let loaded = RelayWal::load(&path).expect("load wal");
        assert_eq!(loaded.pending_count(), 1);
        assert!(!loaded.contains_delivered("a"));

        let mut mutable = loaded;
        mutable.mark_delivered("a");
        assert!(mutable.contains_delivered("a"));
        mutable.upsert_pending("b", "hive-c", "{\"line\":2}");
        mutable.enforce_limits(1, 1024);
        assert_eq!(mutable.entries.len(), 1);
        assert_eq!(mutable.pending_count(), 1);
    }

    #[test]
    fn execution_lane_topology_is_durable_and_immutable() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let journal = temp.path().join("execution-journal.json");
        let topology = bind_execution_lane_topology(&journal, 8).expect("bind topology");
        assert_eq!(
            topology.file_name().and_then(|name| name.to_str()),
            Some("execution-journal.topology.json")
        );
        assert_eq!(
            bind_execution_lane_topology(&journal, 8).expect("reuse topology"),
            topology
        );
        let error = bind_execution_lane_topology(&journal, 4)
            .expect_err("lane-count change must fail closed");
        assert!(error.to_string().contains("topology mismatch"));
    }

    #[test]
    fn execution_journal_persists_every_v2_phase() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let path = temp.path().join("execution.json");
        let spec = admitted_v2_spec();
        let key = TicketKey::new(&spec.id, &spec.idempotency_key);
        let mut journal = ExecutionJournal::default();

        journal.prepare(&spec).expect("prepare");
        journal.save(&path).expect("save prepared");
        journal = ExecutionJournal::load(&path).expect("load prepared");
        assert_eq!(
            journal.get(&key).map(|entry| entry.state),
            Some(ExecutionJournalState::Prepared)
        );

        journal.mark_executing(&key).expect("executing");
        journal.save(&path).expect("save executing");
        journal
            .persist_provider_result(
                &key,
                JournalProviderResult {
                    outcome: JournalProviderOutcome::Confirmed,
                    message: "provider committed".to_owned(),
                    reconciled: false,
                },
            )
            .expect("provider result");
        let line = crate::status::build_result_line(
            &spec,
            HOST_TICKET_RESULT_V2_SCHEMA,
            "succeeded",
            Some("provider committed"),
            2048,
        )
        .expect("result line");
        journal
            .stage_result(&key, "/host/tickets/status", &line)
            .expect("stage result");
        journal.save(&path).expect("save provider result");
        journal.mark_result_published(&key).expect("published");
        journal.save(&path).expect("save published");
        journal.mark_terminal(&key).expect("terminal");
        journal.save(&path).expect("save terminal");

        let loaded = ExecutionJournal::load(&path).expect("load terminal");
        assert_eq!(
            loaded.get(&key).map(|entry| entry.state),
            Some(ExecutionJournalState::Terminal)
        );
        assert!(temp.path().read_dir().expect("read dir").all(|entry| !entry
            .expect("entry")
            .file_name()
            .to_string_lossy()
            .contains("partial")));
    }

    #[test]
    fn execution_journal_compacts_terminal_work_without_a_lifetime_ceiling() {
        let mut journal = ExecutionJournal::default();
        for admission_sequence in 1..=1_024_u64 {
            let mut spec = admitted_v2_spec();
            spec.id = format!("ticket-{admission_sequence}");
            spec.idempotency_key = format!("idem-{admission_sequence}");
            spec.operation_id = Some(format!("lease-{admission_sequence}"));
            spec.admission_sequence = Some(admission_sequence);
            let key = TicketKey::new(&spec.id, &spec.idempotency_key);
            journal.prepare(&spec).expect("prepare bounded operation");
            journal.mark_executing(&key).expect("mark executing");
            journal
                .persist_provider_result(
                    &key,
                    JournalProviderResult {
                        outcome: JournalProviderOutcome::Confirmed,
                        message: "provider committed".to_owned(),
                        reconciled: false,
                    },
                )
                .expect("persist provider result");
            let line = crate::status::build_result_line(
                &spec,
                HOST_TICKET_RESULT_V2_SCHEMA,
                "succeeded",
                Some("provider committed"),
                2048,
            )
            .expect("result line");
            journal
                .stage_result(&key, "/host/tickets/status", &line)
                .expect("stage result");
            journal.mark_result_published(&key).expect("published");
            assert!(journal
                .compact_completed_through(admission_sequence)
                .expect("compact after durable cursor"));
            assert!(journal.get(&key).is_none());
            assert_eq!(journal.entries.len(), 0);
        }
        assert_eq!(journal.completed_through_admission_sequence(), 1_024);
        journal.validate().expect("compacted journal remains valid");
    }

    #[test]
    fn execution_journal_never_compacts_nonterminal_recovery_state() {
        let spec = admitted_v2_spec();
        let mut journal = ExecutionJournal::default();
        journal.prepare(&spec).expect("prepare");
        let error = journal
            .compact_completed_through(spec.admission_sequence.expect("admission sequence"))
            .expect_err("prepared operation must remain recoverable");
        assert!(error.to_string().contains("passed nonterminal admission"));
        assert!(journal
            .get(&TicketKey::new(&spec.id, &spec.idempotency_key))
            .is_some());
    }

    #[test]
    fn agent_fence_refuses_a_second_owner() {
        let temp = tempfile::TempDir::new().expect("temp dir");
        let path = temp.path().join("agent.lock");
        let first = AgentFence::acquire(&path).expect("first fence");
        let second = AgentFence::acquire(&path).expect_err("second owner must fail");
        assert!(second.to_string().contains("already owns"));
        drop(first);
        AgentFence::acquire(&path).expect("lock released");
    }

    #[test]
    fn execution_journal_rejects_reused_key_with_changed_binding() {
        let mut journal = ExecutionJournal::default();
        let spec = admitted_v2_spec();
        journal.prepare(&spec).expect("prepare");
        let mut changed = spec;
        changed.resolved_lease_epoch = Some(99);
        let err = journal
            .prepare(&changed)
            .expect_err("binding reuse must fail");
        assert!(err.to_string().contains("different admitted request"));
    }
}
