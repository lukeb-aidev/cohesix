// Author: Lukas Bower
// Purpose: Preserve bounded intent identity and replay results without issuing admission decisions.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

/// Bounded authenticated host observation and freshness contracts.
pub mod snapshot;
#[cfg(any(feature = "std", test))]
extern crate std;

use alloc::{string::String, vec::Vec};
use core::fmt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Compiler-owned host field-bus endpoints and exact operation maps.
#[cfg(feature = "std")]
pub mod bus;
/// Immutable native release inputs and scoped macOS observations.
#[cfg(feature = "std")]
pub mod mac_release;
/// Compiler-selected native macOS control targets.
#[cfg(feature = "std")]
pub mod macos;

/// Shared bounded GPU workload ticket arguments.
pub mod gpu;
/// Host-only package requirements; never a VM installer or namespace.
#[cfg(feature = "std")]
pub mod package;
/// Immutable host-side private adapter release requests.
pub mod peft;
pub mod policy;
#[cfg(feature = "std")]
pub mod secret;

/// Versioned namespace for strict intents; legacy `/queen/ctl` stays separate.
pub const QUEEN_INTENT_PATH: &str = "/queen/intents/ctl";
/// Bounded introspection for strict intent decisions.
pub const QUEEN_DEDUPE_PATH: &str = "/proc/queen/dedupe";
/// Explicit envelope version, independent from console transport framing.
pub const QUEEN_INTENT_SCHEMA: &str = "queen-intent/v1";
/// Absolute wire bound, further constrained by the selected manifest.
pub const MAX_INTENT_BYTES: usize = 2048;
/// Upper bound on a single identifier.
pub const MAX_ID_BYTES: usize = 128;

/// Known development credentials are never valid live deployment secrets.
pub const PLACEHOLDER_CREDENTIALS: &[&str] = &[
    "changeme",
    "bootstrap",
    "worker",
    "worker-gpu",
    "worker-bus",
    "worker-lora",
    "password",
    "secret",
    "default",
    "placeholder",
    "replace-me",
    "your-token",
    "your-token-here",
];

pub fn is_placeholder(value: &str) -> bool {
    PLACEHOLDER_CREDENTIALS
        .iter()
        .any(|candidate| value.trim().eq_ignore_ascii_case(candidate))
}

/// Correlation with a future authoritative admission decision. Presence does
/// not constitute validation, freshness, or a locally issued grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmissionCorrelation {
    pub admission_id: String,
    pub intent_hash: String,
    pub policy_hash: String,
    pub state_epoch: u64,
    pub resource_generation: u64,
    pub decision_expiry: u64,
}

impl AdmissionCorrelation {
    /// Validate structure only. Writer ownership is a separate field.
    pub fn validate(&self) -> Result<(), AuthorityError> {
        validate_id(&self.admission_id)?;
        for hash in [&self.intent_hash, &self.policy_hash] {
            if hash.len() != 64
                || !hash
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(AuthorityError::Invalid);
            }
        }
        if self.decision_expiry == 0 {
            return Err(AuthorityError::Invalid);
        }
        Ok(())
    }
}

/// Caller-authored strict Queen control envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueenIntent {
    pub schema: String,
    pub id: String,
    pub idempotency_key: String,
    pub issued_unix_ms: u64,
    pub cmd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub writer_epoch: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admission: Option<AdmissionCorrelation>,
}

impl QueenIntent {
    /// Parse a bounded envelope without accepting unknown or duplicate fields.
    pub fn parse(bytes: &[u8], max_bytes: usize) -> Result<Self, AuthorityError> {
        if bytes.is_empty() || bytes.len() > max_bytes.min(MAX_INTENT_BYTES) {
            return Err(AuthorityError::Limit);
        }
        let intent: Self = serde_json::from_slice(bytes).map_err(|_| AuthorityError::Invalid)?;
        if intent.schema != QUEEN_INTENT_SCHEMA || intent.issued_unix_ms == 0 {
            return Err(AuthorityError::Invalid);
        }
        validate_id(&intent.id)?;
        validate_id(&intent.idempotency_key)?;
        if intent.cmd.is_empty() || intent.cmd.bytes().any(|b| b.is_ascii_control()) {
            return Err(AuthorityError::Invalid);
        }
        if let Some(admission) = &intent.admission {
            admission.validate()?;
        }
        Ok(intent)
    }

    fn digest(&self) -> Result<[u8; 32], AuthorityError> {
        let canonical = serde_json::to_vec(self).map_err(|_| AuthorityError::Invalid)?;
        Ok(Sha256::digest(canonical).into())
    }
}

/// Strict single-component identifiers cannot become paths or argv options.
pub fn validate_id(value: &str) -> Result<(), AuthorityError> {
    if value.is_empty()
        || value.len() > MAX_ID_BYTES
        || value.starts_with('-')
        || value.contains("..")
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err(AuthorityError::Invalid);
    }
    Ok(())
}

/// Deterministic authority refusal classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorityError {
    Invalid,
    Limit,
    Conflict,
    InFlight,
    StaleWriter,
}

impl fmt::Display for AuthorityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Invalid => "EPERM invalid-authority-envelope",
            Self::Limit => "ELIMIT authority-capacity",
            Self::Conflict => "EPERM idempotency-conflict",
            Self::InFlight => "ELIMIT intent-in-flight",
            Self::StaleWriter => "EPERM stale-writer",
        })
    }
}

/// Result of reserving an intent before any side effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reservation<T> {
    Fresh,
    Duplicate(T),
}

#[derive(Debug, Clone)]
struct Entry<T> {
    id: String,
    key: String,
    digest: [u8; 32],
    issued: u64,
    accepted: Option<bool>,
    result: Option<T>,
}

/// Bounded replay table for one authority lifetime. Capacity exhaustion refuses
/// new identities; acknowledged identities are never evicted or made fresh.
/// Reboot persistence is owned by the selected persistent authority profile.
#[derive(Debug, Clone)]
pub struct IntentDedupe<T> {
    entries: Vec<Entry<T>>,
    capacity: usize,
    duplicates: u64,
}

impl<T: Clone> IntentDedupe<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: Vec::new(),
            capacity,
            duplicates: 0,
        }
    }

    pub fn reserve(&mut self, intent: &QueenIntent) -> Result<Reservation<T>, AuthorityError> {
        let digest = intent.digest()?;
        if let Some(entry) = self
            .entries
            .iter()
            .find(|e| e.id == intent.id && e.key == intent.idempotency_key)
        {
            if entry.digest != digest {
                return Err(AuthorityError::Conflict);
            }
            let result = entry.result.clone().ok_or(AuthorityError::InFlight)?;
            self.duplicates = self.duplicates.saturating_add(1);
            return Ok(Reservation::Duplicate(result));
        }
        if self.capacity == 0 {
            return Err(AuthorityError::Limit);
        }
        if self.entries.len() == self.capacity {
            return Err(AuthorityError::Limit);
        }
        self.entries.push(Entry {
            id: intent.id.clone(),
            key: intent.idempotency_key.clone(),
            digest,
            issued: intent.issued_unix_ms,
            accepted: None,
            result: None,
        });
        Ok(Reservation::Fresh)
    }

    pub fn finish(
        &mut self,
        intent: &QueenIntent,
        result: T,
        accepted: bool,
    ) -> Result<(), AuthorityError> {
        let digest = intent.digest()?;
        let entry = self
            .entries
            .iter_mut()
            .find(|e| e.id == intent.id && e.key == intent.idempotency_key)
            .ok_or(AuthorityError::Invalid)?;
        if entry.digest != digest || entry.result.is_some() {
            return Err(AuthorityError::Conflict);
        }
        entry.accepted = Some(accepted);
        entry.result = Some(result);
        Ok(())
    }

    /// NDJSON summary plus at most sixteen recent outcomes, each below the
    /// 256-byte console line bound. Identity hashes bind id and key together.
    pub fn snapshot_lines(&self) -> Result<Vec<u8>, AuthorityError> {
        #[derive(Serialize)]
        struct Recent {
            identity_sha256: String,
            issued_unix_ms: u64,
            outcome: &'static str,
        }
        let mut output =
            serde_json::to_vec(&self.snapshot()).map_err(|_| AuthorityError::Invalid)?;
        output.push(b'\n');
        for entry in self.entries.iter().rev().take(16) {
            let mut hasher = Sha256::new();
            hasher.update(entry.id.as_bytes());
            hasher.update([0]);
            hasher.update(entry.key.as_bytes());
            let mut identity_sha256 = String::with_capacity(64);
            use core::fmt::Write;
            for byte in hasher.finalize() {
                write!(identity_sha256, "{byte:02x}").map_err(|_| AuthorityError::Invalid)?;
            }
            let recent = Recent {
                identity_sha256,
                issued_unix_ms: entry.issued,
                outcome: match entry.accepted {
                    Some(true) => "ACK",
                    Some(false) => "ERR",
                    None => "in_flight",
                },
            };
            output.extend(serde_json::to_vec(&recent).map_err(|_| AuthorityError::Invalid)?);
            output.push(b'\n');
        }
        Ok(output)
    }

    pub fn snapshot(&self) -> DedupeSnapshot {
        DedupeSnapshot {
            schema: "queen-dedupe/v1",
            entries: self.entries.len(),
            capacity: self.capacity,
            duplicates: self.duplicates,
        }
    }
}

/// Fixed-size introspection; does not expose caller secrets or command bodies.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct DedupeSnapshot {
    pub schema: &'static str,
    pub entries: usize,
    pub capacity: usize,
    pub duplicates: u64,
}

// Compatibility vocabulary is now emitted from the same provider IR as host clients.
mod provider_generated;
pub use provider_generated::PROVIDER_V1_FIELDS;

/// Host-only provider registry; no native provider implementation enters the VM.
#[cfg(feature = "std")]
pub mod provider;

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{format, string::ToString};

    fn intent(id: &str, issued: u64) -> QueenIntent {
        QueenIntent::parse(format!(r#"{{"schema":"queen-intent/v1","id":"{id}","idempotency_key":"retry","issued_unix_ms":{issued},"cmd":"spawn heartbeat"}}"#).as_bytes(), 2048).expect("valid fixture")
    }

    #[test]
    fn retry_preserves_result_and_all_correlation() {
        let mut table = IntentDedupe::new(2);
        let mut first = intent("first", 1);
        assert_eq!(table.reserve(&first), Ok(Reservation::Fresh));
        assert_eq!(table.reserve(&first), Err(AuthorityError::InFlight));
        table.finish(&first, "ACK", true).expect("record");
        assert_eq!(table.reserve(&first), Ok(Reservation::Duplicate("ACK")));
        first.writer_epoch = Some(2);
        assert_eq!(table.reserve(&first), Err(AuthorityError::Conflict));
        first.writer_epoch = None;
        first.cmd = "kill worker-1".to_string();
        assert_eq!(table.reserve(&first), Err(AuthorityError::Conflict));
    }

    #[test]
    fn full_table_retains_replay_identity_and_refuses_new_work() {
        let mut table = IntentDedupe::new(1);
        let first = intent("first", 10);
        table.reserve(&first).expect("reserve");
        table.finish(&first, false, false).expect("record refusal");
        assert_eq!(table.reserve(&first), Ok(Reservation::Duplicate(false)));
        assert_eq!(
            table.reserve(&intent("second", 11)),
            Err(AuthorityError::Limit)
        );
        assert_eq!(table.reserve(&first), Ok(Reservation::Duplicate(false)));
        assert_eq!(
            table.reserve(&intent("third", 11)),
            Err(AuthorityError::Limit)
        );
        assert_eq!(table.snapshot().entries, 1);
        let mut changed = first.clone();
        changed.issued_unix_ms = 11;
        assert_eq!(table.reserve(&changed), Err(AuthorityError::Conflict));
    }

    #[test]
    fn release_a_capacity_retains_all_512_outcomes_at_exhaustion() {
        let mut table = IntentDedupe::new(512);
        for index in 0..512 {
            let request = intent(&format!("entry-{index}"), 1000 + index);
            assert_eq!(table.reserve(&request), Ok(Reservation::Fresh));
            table
                .finish(&request, index, index % 2 == 0)
                .expect("terminal outcome");
        }
        assert_eq!(table.snapshot().entries, 512);
        assert_eq!(
            table.reserve(&intent("overflow", 2000)),
            Err(AuthorityError::Limit)
        );
        for index in 0..512 {
            let mut request = intent(&format!("entry-{index}"), 1000 + index);
            assert_eq!(table.reserve(&request), Ok(Reservation::Duplicate(index)));
            request.writer_epoch = Some(2);
            assert_eq!(table.reserve(&request), Err(AuthorityError::Conflict));
        }
        assert_eq!(table.snapshot().entries, 512);
        assert_eq!(table.snapshot().duplicates, 512);
    }

    #[test]
    fn strict_fields_and_identifier_bounds_are_independent_of_transport() {
        for id in ["", "../x", "-option", "a/b", "x\ny", "a..b"] {
            assert_eq!(validate_id(id), Err(AuthorityError::Invalid));
        }
        let good = serde_json::to_vec(&intent("ok", 1)).expect("encode fixture");
        assert_eq!(
            QueenIntent::parse(&good, good.len() - 1),
            Err(AuthorityError::Limit)
        );
        let bad = String::from_utf8(good)
            .expect("utf8")
            .replace("\"id\":\"ok\"", "\"id\":\"ok\",\"id\":\"other\"");
        assert_eq!(
            QueenIntent::parse(bad.as_bytes(), 2048),
            Err(AuthorityError::Invalid)
        );
    }
}
