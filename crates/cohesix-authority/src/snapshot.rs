// Author: Lukas Bower
// Purpose: Validate bounded host observations and monotonic snapshot replacement without treating them as execution proof.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]

use alloc::{string::String, vec::Vec};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{validate_id, AuthorityError};

/// Host observation schema; authentication belongs to the selected publication channel.
pub const SCHEMA: &str = "cohesix-host-snapshot/v1";
/// Absolute parser bound even when a deployment policy is malformed.
pub const MAX_BYTES: usize = 8192;
/// Absolute number of native observation records per selected provider.
pub const MAX_ENTRIES: usize = 64;

/// Generated receiver constraints; a publication cannot choose these values.
#[derive(Debug, Clone, Copy)]
pub struct Limits<'a> {
    /// Exact enrolled host publisher identity.
    pub source_id: &'a str,
    /// Current generated writer epoch.
    pub epoch: u64,
    /// Complete set of compiler-selected provider ids.
    pub providers: &'a [&'a str],
    /// Serialized snapshot maximum, at most MAX_BYTES.
    pub max_bytes: usize,
    /// Native record maximum, at most MAX_ENTRIES.
    pub max_entries: usize,
    /// UTF-8 value maximum per record, at most 4096 bytes.
    pub max_value_bytes: usize,
    /// Receiver-relative freshness bound, at most 30000 milliseconds.
    pub max_ttl_ms: u64,
}

impl Limits<'_> {
    /// Fail closed on invalid compiler-owned bounds before parsing caller data.
    pub fn validate(&self) -> Result<(), AuthorityError> {
        validate_id(self.source_id)?;
        if self.epoch == 0
            || self.providers.is_empty()
            || self.providers.len() > 32
            || !(1..=MAX_BYTES).contains(&self.max_bytes)
            || !(1..=MAX_ENTRIES).contains(&self.max_entries)
            || !(1..=4096).contains(&self.max_value_bytes)
            || !(1..=30000).contains(&self.max_ttl_ms)
        {
            return Err(AuthorityError::Limit);
        }
        for (index, provider) in self.providers.iter().enumerate() {
            validate_id(provider)?;
            if self.providers[..index].contains(provider) {
                return Err(AuthorityError::Invalid);
            }
        }
        Ok(())
    }
}

/// Fixed source-failure classes contain no raw native command output or credentials.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnavailableReason {
    /// Host or native feature cannot implement this provider.
    NotSupported,
    /// An optional provider has no enrolled runtime or credential.
    NotEnabled,
    /// Native observation failed or returned invalid state.
    SourceFailed,
    /// A bounded native call exceeded its observation deadline.
    TimedOut,
}

/// A canonical provider-relative native record. Values are observations only;
/// they cannot be imported as an admission or execution receipt.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// Relative path components, e.g. units/ssh.service/status.
    pub path: String,
    /// Bounded UTF-8 native observation, produced by an allowlisted adapter.
    pub value: String,
}

/// Versioned host observation, bound to one enrolled source and provider.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    /// Exact SCHEMA tag.
    pub schema: String,
    /// Exact compiler-selected provider id.
    pub provider: String,
    /// Exact enrolled publisher id.
    pub source_id: String,
    /// Generated writer fence, not a caller-selected new authority generation.
    pub epoch: u64,
    /// Strictly increasing durable publisher sequence.
    pub sequence: u64,
    /// Source observation Unix timestamp, retained for provenance.
    pub observed_unix_ms: u64,
    /// Receiver freshness interval measured with its own monotonic timebase.
    pub ttl_ms: u64,
    /// True only when the native adapter obtained its current selected state.
    pub available: bool,
    /// A source failure immediately withdraws data; no healthy entries remain.
    pub reason: Option<UnavailableReason>,
    /// Native records in stable lexicographic path order.
    pub entries: Vec<Entry>,
}

impl Snapshot {
    /// Validate before serializing or admitting any observation.
    pub fn validate(&self, limits: Limits<'_>) -> Result<(), AuthorityError> {
        limits.validate()?;
        if self.schema != SCHEMA
            || self.source_id != limits.source_id
            || self.epoch != limits.epoch
            || self.sequence == 0
            || self.observed_unix_ms == 0
            || !limits.providers.contains(&self.provider.as_str())
            || !(1..=limits.max_ttl_ms).contains(&self.ttl_ms)
            || self.available == self.reason.is_some()
            || (!self.available && !self.entries.is_empty())
        {
            return Err(AuthorityError::Invalid);
        }
        if self.entries.len() > limits.max_entries {
            return Err(AuthorityError::Limit);
        }
        let mut previous: Option<&str> = None;
        for entry in &self.entries {
            if entry.path.len() > 192
                || entry.path.starts_with('/')
                || entry.path.split('/').count() > 6
                || entry
                    .path
                    .split('/')
                    .any(|part| part == "." || validate_id(part).is_err())
                || previous.is_some_and(|previous| previous >= entry.path.as_str())
                || entry.value.len() > limits.max_value_bytes
                || entry.value.contains('\0')
            {
                return Err(AuthorityError::Invalid);
            }
            previous = Some(&entry.path);
        }
        Ok(())
    }

    /// Serialize canonical field/order bytes used by the publication digest.
    pub fn encode(&self, limits: Limits<'_>) -> Result<Vec<u8>, AuthorityError> {
        self.validate(limits)?;
        let bytes = serde_json::to_vec(self).map_err(|_| AuthorityError::Invalid)?;
        if bytes.len() > limits.max_bytes {
            return Err(AuthorityError::Limit);
        }
        Ok(bytes)
    }

    /// Bound input before allocating nested fields; reject noncanonical encoding
    /// so an identical digest is an unambiguous retry identity.
    pub fn decode(bytes: &[u8], limits: Limits<'_>) -> Result<Self, AuthorityError> {
        limits.validate()?;
        if bytes.is_empty() || bytes.len() > limits.max_bytes {
            return Err(AuthorityError::Limit);
        }
        let snapshot: Self = serde_json::from_slice(bytes).map_err(|_| AuthorityError::Invalid)?;
        if snapshot.encode(limits)? != bytes {
            return Err(AuthorityError::Invalid);
        }
        Ok(snapshot)
    }
}

/// Result of one validated replacement; a retry never renews freshness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Replacement {
    /// New native sequence replaces the prior snapshot atomically.
    Applied,
    /// The exact already accepted bytes were retried after an ambiguous ACK.
    Duplicate,
}

/// One provider's receiver state. Expiry clears data but retains replay fences.
#[derive(Debug, Default)]
pub struct Slot {
    domain: Option<(String, String, u64)>,
    snapshot: Option<Snapshot>,
    digest: Option<[u8; 32]>,
    sequence: u64,
    observed_unix_ms: u64,
    expires_at_ms: Option<u64>,
    last_now_ms: u64,
}

impl Slot {
    /// Apply only a bounded snapshot in the selected provider/source domain.
    /// Freshness begins at the first received upload byte, so transfer latency
    /// consumes the TTL instead of extending it.
    /// Retain the last sequence after failure/expiry; only a new receiver epoch
    /// (new generated policy and new slot) can reset it.
    pub fn apply(
        &mut self,
        bytes: &[u8],
        limits: Limits<'_>,
        provider: &str,
        received_at_ms: u64,
        now_ms: u64,
    ) -> Result<Replacement, AuthorityError> {
        if now_ms < self.last_now_ms || received_at_ms > now_ms {
            return Err(AuthorityError::Invalid);
        }
        let snapshot = Snapshot::decode(bytes, limits)?;
        if snapshot.provider != provider || snapshot.observed_unix_ms < self.observed_unix_ms {
            return Err(AuthorityError::Invalid);
        }
        if self
            .domain
            .as_ref()
            .is_some_and(|(source, selected, epoch)| {
                source != &snapshot.source_id
                    || selected != &snapshot.provider
                    || *epoch != snapshot.epoch
            })
        {
            return Err(AuthorityError::Invalid);
        }
        let digest: [u8; 32] = Sha256::digest(bytes).into();
        if snapshot.sequence == self.sequence && self.digest == Some(digest) {
            self.withdraw_expired(now_ms)?;
            return Ok(Replacement::Duplicate);
        }
        if snapshot.sequence <= self.sequence {
            return Err(AuthorityError::Invalid);
        }
        let expires_at_ms = received_at_ms
            .checked_add(snapshot.ttl_ms)
            .ok_or(AuthorityError::Limit)?;
        if now_ms >= expires_at_ms {
            return Err(AuthorityError::Invalid);
        }
        self.sequence = snapshot.sequence;
        self.observed_unix_ms = snapshot.observed_unix_ms;
        self.digest = Some(digest);
        self.last_now_ms = now_ms;
        self.expires_at_ms = snapshot.available.then_some(expires_at_ms);
        self.domain = Some((
            snapshot.source_id.clone(),
            snapshot.provider.clone(),
            snapshot.epoch,
        ));
        self.snapshot = Some(snapshot);
        Ok(Replacement::Applied)
    }

    /// Withdraw bytes using receiver time, without trusting a caller clock.
    pub fn withdraw_expired(&mut self, now_ms: u64) -> Result<bool, AuthorityError> {
        if now_ms < self.last_now_ms {
            self.snapshot = None;
            self.expires_at_ms = None;
            return Err(AuthorityError::Invalid);
        }
        self.last_now_ms = now_ms;
        if self
            .expires_at_ms
            .is_some_and(|deadline| now_ms >= deadline)
        {
            self.expires_at_ms = None;
            self.snapshot = None;
            return Ok(true);
        }
        Ok(false)
    }

    /// Current accepted source record, absent after expiry. A source-failure
    /// record is retained with no entries and available=false.
    pub fn snapshot(&self, now_ms: u64) -> Option<&Snapshot> {
        if now_ms < self.last_now_ms
            || self
                .expires_at_ms
                .is_some_and(|deadline| now_ms >= deadline)
        {
            return None;
        }
        self.snapshot.as_ref()
    }

    /// Last accepted sequence survives withdrawal and ambiguous retries.
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{string::ToString, vec};

    fn limits() -> Limits<'static> {
        Limits {
            source_id: "native-host",
            epoch: 7,
            providers: &["systemd", "network"],
            max_bytes: 4096,
            max_entries: 8,
            max_value_bytes: 512,
            max_ttl_ms: 5000,
        }
    }
    fn observation() -> Snapshot {
        Snapshot {
            schema: SCHEMA.to_string(),
            provider: "systemd".into(),
            source_id: "native-host".into(),
            epoch: 7,
            sequence: 1,
            observed_unix_ms: 100_000,
            ttl_ms: 100,
            available: true,
            reason: None,
            entries: vec![Entry {
                path: "units/ssh.service/status".into(),
                value: "{\"active_state\":\"active\"}".into(),
            }],
        }
    }

    #[test]
    fn snapshot_contract_refuses_unselected_domains_noncanonical_data_and_bounds() {
        let valid = observation();
        let bytes = valid.encode(limits()).unwrap();
        assert_eq!(Snapshot::decode(&bytes, limits()).unwrap(), valid);
        for defect in [
            "source",
            "epoch",
            "provider",
            "zero_sequence",
            "ttl",
            "failure",
            "path",
            "duplicate",
            "unsorted",
            "value",
        ] {
            let mut bad = valid.clone();
            match defect {
                "source" => bad.source_id = "another-host".into(),
                "epoch" => bad.epoch = 8,
                "provider" => bad.provider = "unselected".into(),
                "zero_sequence" => bad.sequence = 0,
                "ttl" => bad.ttl_ms = 5001,
                "failure" => {
                    bad.available = false;
                    bad.reason = Some(UnavailableReason::SourceFailed);
                }
                "path" => bad.entries[0].path = "../escape".into(),
                "duplicate" => bad.entries.push(bad.entries[0].clone()),
                "unsorted" => bad.entries.push(Entry {
                    path: "a".into(),
                    value: "x".into(),
                }),
                _ => bad.entries[0].value = "x".repeat(513),
            }
            assert!(bad.encode(limits()).is_err(), "{defect}");
        }
        let mut trailing = bytes.clone();
        trailing.push(b' ');
        assert!(Snapshot::decode(&trailing, limits()).is_err());
        let duplicate = core::str::from_utf8(&bytes).unwrap().replacen(
            "\"epoch\":7",
            "\"epoch\":7,\"epoch\":7",
            1,
        );
        assert!(Snapshot::decode(duplicate.as_bytes(), limits()).is_err());
        assert_eq!(
            Snapshot::decode(&vec![0; 4097], limits()).unwrap_err(),
            AuthorityError::Limit
        );
    }

    #[test]
    fn snapshot_expiry_failed_sources_and_ambiguous_retries_never_restore_stale_data() {
        let mut slot = Slot::default();
        let mut snapshot = observation();
        let first = snapshot.encode(limits()).unwrap();
        let mut delayed = Slot::default();
        assert!(delayed.apply(&first, limits(), "systemd", 10, 110).is_err());
        assert_eq!(delayed.sequence(), 0);
        delayed.apply(&first, limits(), "systemd", 10, 50).unwrap();
        assert!(delayed.snapshot(109).is_some());
        assert!(delayed.snapshot(110).is_none());
        assert_eq!(
            slot.apply(&first, limits(), "systemd", 10, 10).unwrap(),
            Replacement::Applied
        );
        assert_eq!(
            slot.apply(&first, limits(), "systemd", 50, 50).unwrap(),
            Replacement::Duplicate
        );
        assert!(slot.snapshot(109).is_some());
        assert!(slot.snapshot(110).is_none()); // Getter alone enforces expiry.
        assert_eq!(
            slot.apply(&first, limits(), "systemd", 110, 110).unwrap(),
            Replacement::Duplicate
        );
        assert!(slot.snapshot(110).is_none());
        assert_eq!(slot.sequence(), 1);
        snapshot.sequence = 2;
        snapshot.observed_unix_ms += 1;
        let second = snapshot.encode(limits()).unwrap();
        assert_eq!(
            slot.apply(&second, limits(), "systemd", 111, 111).unwrap(),
            Replacement::Applied
        );
        assert!(slot.apply(&first, limits(), "systemd", 112, 112).is_err());
        assert!(slot.apply(&second, limits(), "network", 112, 112).is_err());
        snapshot.sequence = 3;
        snapshot.available = false;
        snapshot.reason = Some(UnavailableReason::SourceFailed);
        snapshot.entries.clear();
        let failed = snapshot.encode(limits()).unwrap();
        assert_eq!(
            slot.apply(&failed, limits(), "systemd", 112, 112).unwrap(),
            Replacement::Applied
        );
        assert!(!slot.snapshot(112).unwrap().available);
        assert!(slot.snapshot(112).unwrap().entries.is_empty());
        assert!(slot.snapshot(111).is_none()); // Regressed receiver clock is not fresh.
        assert!(slot.withdraw_expired(111).is_err());
        assert!(slot.snapshot(112).is_none());
        assert_eq!(slot.sequence(), 3);
    }
}
