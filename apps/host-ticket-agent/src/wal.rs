// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Persist relay write-ahead state for deterministic cross-hive ticket forwarding.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

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
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create relay WAL dir {}", parent.display()))?;
        }
        let payload = serde_json::to_vec_pretty(self).context("serialize relay WAL")?;
        let tmp = path.with_extension("partial");
        fs::write(&tmp, &payload).with_context(|| format!("write relay WAL {}", tmp.display()))?;
        fs::rename(&tmp, path).with_context(|| format!("commit relay WAL {}", path.display()))?;
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
