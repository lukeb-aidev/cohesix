// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Define capability ticket claims and validation for Cohesix roles.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![no_std]

//! Capability ticket primitives shared across Cohesix crates, reflecting
//! `docs/ARCHITECTURE.md` §1-§3.

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use blake3::Hash;
use thiserror::Error;

#[cfg(test)]
extern crate std;

const CLAIMS_VERSION: u8 = 1;
const COMPACT_CLAIMS_VERSION: u8 = 2;
const TICKET_PREFIX: &str = "cohesix-ticket-";
const MAX_MOUNT_FIELD_LEN: usize = 255;
const MAX_SCOPE_COUNT: usize = 16;
const FLAG_TICKS: u8 = 0b0000_0001;
const FLAG_OPS: u8 = 0b0000_0010;
const FLAG_TTL: u8 = 0b0000_0100;
const FLAG_SUBJECT: u8 = 0b0000_1000;
const FLAG_SCOPES: u8 = 0b0001_0000;
const FLAG_QUOTAS: u8 = 0b0010_0000;

/// Roles recognised by the Cohesix capability system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// Queen orchestration role controlling worker lifecycles.
    Queen,
    /// Worker responsible for emitting heartbeat telemetry.
    WorkerHeartbeat,
    /// Future GPU worker role.
    WorkerGpu,
    /// Field bus worker role.
    WorkerBus,
    /// AI LoRA model-adapter control worker role.
    WorkerLora,
}

impl Role {
    fn as_u8(self) -> u8 {
        match self {
            Role::Queen => 0,
            Role::WorkerHeartbeat => 1,
            Role::WorkerGpu => 2,
            Role::WorkerBus => 3,
            Role::WorkerLora => 4,
        }
    }

    fn from_u8(value: u8) -> Result<Self, TicketError> {
        match value {
            0 => Ok(Role::Queen),
            1 => Ok(Role::WorkerHeartbeat),
            2 => Ok(Role::WorkerGpu),
            3 => Ok(Role::WorkerBus),
            4 => Ok(Role::WorkerLora),
            other => Err(TicketError::UnsupportedRole(other)),
        }
    }
}

/// Budget specification describing limits applied to a ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetSpec {
    ticks: Option<u64>,
    ops: Option<u64>,
    ttl_s: Option<u64>,
}

impl BudgetSpec {
    /// Budget without restrictions, used during bootstrap flows.
    #[must_use]
    pub fn unbounded() -> Self {
        Self {
            ticks: None,
            ops: None,
            ttl_s: None,
        }
    }

    /// Default limits for heartbeat workers; tuned as real scheduling logic arrives.
    #[must_use]
    pub fn default_heartbeat() -> Self {
        Self {
            ticks: Some(1_000),
            ops: Some(10_000),
            ttl_s: Some(300),
        }
    }

    /// Default limits for GPU workers mirroring lease guardrails.
    #[must_use]
    pub fn default_gpu() -> Self {
        Self {
            ticks: None,
            ops: Some(64),
            ttl_s: Some(120),
        }
    }

    /// Override the tick budget.
    #[must_use]
    pub fn with_ticks(mut self, ticks: Option<u64>) -> Self {
        self.ticks = ticks;
        self
    }

    /// Override the operation budget.
    #[must_use]
    pub fn with_ops(mut self, ops: Option<u64>) -> Self {
        self.ops = ops;
        self
    }

    /// Override the time-to-live budget in seconds.
    #[must_use]
    pub fn with_ttl(mut self, ttl_s: Option<u64>) -> Self {
        self.ttl_s = ttl_s;
        self
    }

    /// Retrieve the configured tick budget.
    #[must_use]
    pub fn ticks(&self) -> Option<u64> {
        self.ticks
    }

    /// Retrieve the configured operation budget.
    #[must_use]
    pub fn ops(&self) -> Option<u64> {
        self.ops
    }

    /// Retrieve the configured time-to-live budget in seconds.
    #[must_use]
    pub fn ttl_s(&self) -> Option<u64> {
        self.ttl_s
    }
}

impl Default for BudgetSpec {
    fn default() -> Self {
        Self::unbounded()
    }
}

/// Mount specification attached to a ticket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountSpec {
    /// Service identifier to mount.
    pub service: String,
    /// Session-scoped mount point.
    pub at: String,
}

impl MountSpec {
    /// Construct an empty mount specification.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            service: String::new(),
            at: String::new(),
        }
    }

    /// Return true when no mount data is present.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.service.is_empty() && self.at.is_empty()
    }
}

/// Verbs allowed by a ticket scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketVerb {
    /// Read-only access.
    Read,
    /// Write-only access.
    Write,
    /// Read and write access.
    ReadWrite,
}

impl TicketVerb {
    fn as_u8(self) -> u8 {
        match self {
            TicketVerb::Read => 0,
            TicketVerb::Write => 1,
            TicketVerb::ReadWrite => 2,
        }
    }

    fn from_u8(value: u8) -> Result<Self, TicketError> {
        match value {
            0 => Ok(TicketVerb::Read),
            1 => Ok(TicketVerb::Write),
            2 => Ok(TicketVerb::ReadWrite),
            other => Err(TicketError::InvalidScopeVerb(other)),
        }
    }
}

/// Scope entry for ticket-bound UI access.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketScope {
    /// Canonical path prefix (absolute).
    pub path: String,
    /// Allowed verbs for the scope.
    pub verb: TicketVerb,
    /// Rate limit per second (0 = unlimited).
    pub rate_per_s: u32,
}

impl TicketScope {
    /// Construct a new scope entry.
    #[must_use]
    pub fn new(path: impl Into<String>, verb: TicketVerb, rate_per_s: u32) -> Self {
        Self {
            path: path.into(),
            verb,
            rate_per_s,
        }
    }
}

/// Optional per-ticket quota limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TicketQuotas {
    /// Maximum bandwidth in bytes (None = unlimited).
    pub bandwidth_bytes: Option<u64>,
    /// Maximum number of cursor resume reads (None = unlimited).
    pub cursor_resumes: Option<u32>,
    /// Maximum number of cursor advances (None = unlimited).
    pub cursor_advances: Option<u32>,
}

impl TicketQuotas {
    /// Quotas without restrictions.
    #[must_use]
    pub fn unbounded() -> Self {
        Self::default()
    }

    fn has_any(self) -> bool {
        self.bandwidth_bytes.is_some()
            || self.cursor_resumes.is_some()
            || self.cursor_advances.is_some()
    }

    fn encode(self, payload: &mut Vec<u8>) {
        payload.extend_from_slice(&self.bandwidth_bytes.unwrap_or(0).to_le_bytes());
        payload.extend_from_slice(&self.cursor_resumes.unwrap_or(0).to_le_bytes());
        payload.extend_from_slice(&self.cursor_advances.unwrap_or(0).to_le_bytes());
    }

    fn decode(cursor: &mut PayloadCursor<'_>) -> Result<Self, TicketError> {
        let bandwidth_bytes = cursor.read_u64()?;
        let cursor_resumes = cursor.read_u32()?;
        let cursor_advances = cursor.read_u32()?;
        Ok(Self {
            bandwidth_bytes: (bandwidth_bytes > 0).then_some(bandwidth_bytes),
            cursor_resumes: (cursor_resumes > 0).then_some(cursor_resumes),
            cursor_advances: (cursor_advances > 0).then_some(cursor_advances),
        })
    }
}

/// Claims embedded in capability tickets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketClaims {
    /// Role granted by the ticket.
    pub role: Role,
    /// Budget limits included with the ticket.
    pub budget: BudgetSpec,
    /// Optional subject identifier bound to the ticket.
    pub subject: Option<String>,
    /// Optional mount specification.
    pub mounts: MountSpec,
    /// Millisecond timestamp when the ticket was issued.
    pub issued_at_ms: u64,
    /// Optional UI scopes granted to the ticket.
    pub scopes: Vec<TicketScope>,
    /// Optional per-ticket quota limits.
    pub quotas: TicketQuotas,
}

impl TicketClaims {
    /// Create a new claims bundle.
    #[must_use]
    pub fn new(
        role: Role,
        budget: BudgetSpec,
        subject: Option<String>,
        mounts: MountSpec,
        issued_at_ms: u64,
    ) -> Self {
        Self {
            role,
            budget,
            subject,
            mounts,
            issued_at_ms,
            scopes: Vec::new(),
            quotas: TicketQuotas::default(),
        }
    }

    /// Override the scopes attached to the ticket.
    #[must_use]
    pub fn with_scopes(mut self, scopes: Vec<TicketScope>) -> Self {
        self.scopes = scopes;
        self
    }

    /// Override the quota limits attached to the ticket.
    #[must_use]
    pub fn with_quotas(mut self, quotas: TicketQuotas) -> Self {
        self.quotas = quotas;
        self
    }

    fn encode_payload(&self) -> Result<Vec<u8>, TicketError> {
        self.encode_payload_version(CLAIMS_VERSION)
    }

    fn encode_payload_version(&self, version: u8) -> Result<Vec<u8>, TicketError> {
        let mut payload = Vec::new();
        payload.push(version);
        payload.push(self.role.as_u8());
        let mut flags = 0u8;
        if self.budget.ticks.is_some() {
            flags |= FLAG_TICKS;
        }
        if self.budget.ops.is_some() {
            flags |= FLAG_OPS;
        }
        if self.budget.ttl_s.is_some() {
            flags |= FLAG_TTL;
        }
        if self.subject.is_some() {
            flags |= FLAG_SUBJECT;
        }
        if !self.scopes.is_empty() {
            flags |= FLAG_SCOPES;
        }
        if self.quotas.has_any() {
            flags |= FLAG_QUOTAS;
        }
        payload.push(flags);
        if let Some(ticks) = self.budget.ticks {
            encode_integer(ticks, version, &mut payload);
        }
        if let Some(ops) = self.budget.ops {
            encode_integer(ops, version, &mut payload);
        }
        if let Some(ttl_s) = self.budget.ttl_s {
            encode_integer(ttl_s, version, &mut payload);
        }
        if let Some(subject) = &self.subject {
            encode_string(subject, &mut payload)?;
        }
        encode_integer(self.issued_at_ms, version, &mut payload);
        encode_string(&self.mounts.service, &mut payload)?;
        encode_string(&self.mounts.at, &mut payload)?;
        if flags & FLAG_SCOPES != 0 {
            encode_scopes(&self.scopes, &mut payload)?;
        }
        if flags & FLAG_QUOTAS != 0 {
            self.quotas.encode(&mut payload);
        }
        Ok(payload)
    }

    fn decode_payload(bytes: &[u8]) -> Result<Self, TicketError> {
        let mut cursor = PayloadCursor::new(bytes);
        let version = cursor.read_u8()?;
        if !matches!(version, CLAIMS_VERSION | COMPACT_CLAIMS_VERSION) {
            return Err(TicketError::UnsupportedVersion(version));
        }
        let role = Role::from_u8(cursor.read_u8()?)?;
        let flags = cursor.read_u8()?;
        if version == COMPACT_CLAIMS_VERSION && flags & 0xc0 != 0 {
            return Err(TicketError::MalformedToken);
        }
        let ticks = if flags & FLAG_TICKS != 0 {
            Some(cursor.read_integer(version)?)
        } else {
            None
        };
        let ops = if flags & FLAG_OPS != 0 {
            Some(cursor.read_integer(version)?)
        } else {
            None
        };
        let ttl_s = if flags & FLAG_TTL != 0 {
            Some(cursor.read_integer(version)?)
        } else {
            None
        };
        let subject = if flags & FLAG_SUBJECT != 0 {
            Some(cursor.read_string()?)
        } else {
            None
        };
        let issued_at_ms = cursor.read_integer(version)?;
        let service = cursor.read_string()?;
        let at = cursor.read_string()?;
        let scopes = if flags & FLAG_SCOPES != 0 {
            decode_scopes(&mut cursor)?
        } else {
            Vec::new()
        };
        let quotas = if flags & FLAG_QUOTAS != 0 {
            TicketQuotas::decode(&mut cursor)?
        } else {
            TicketQuotas::default()
        };
        cursor.ensure_empty()?;
        let claims = Self {
            role,
            budget: BudgetSpec { ticks, ops, ttl_s },
            subject,
            mounts: MountSpec { service, at },
            issued_at_ms,
            scopes,
            quotas,
        };
        if version == COMPACT_CLAIMS_VERSION && claims.encode_payload_version(version)? != bytes {
            return Err(TicketError::MalformedToken);
        }
        Ok(claims)
    }
}

/// Ticket signing key derived from a shared secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TicketKey([u8; 32]);

impl TicketKey {
    /// Derive a signing key from the provided shared secret string.
    #[must_use]
    pub fn from_secret(secret: &str) -> Self {
        let hash = blake3::hash(secret.as_bytes());
        Self(*hash.as_bytes())
    }

    /// Rehydrate a signing key from manifest-generated bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Return the raw signing key bytes for manifest code generation.
    #[must_use]
    pub const fn to_bytes(self) -> [u8; 32] {
        self.0
    }

    fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Minted ticket token containing claims and a MAC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketToken {
    claims: TicketClaims,
    mac: [u8; 32],
    version: u8,
}

impl TicketToken {
    /// Return the claims embedded in the ticket.
    #[must_use]
    pub fn claims(&self) -> &TicketClaims {
        &self.claims
    }

    /// Encode the ticket into its text representation.
    pub fn encode(&self) -> Result<String, TicketError> {
        let payload = self.claims.encode_payload_version(self.version)?;
        let payload_hex = hex::encode(payload);
        let mac_hex = hex::encode(self.mac);
        Ok(format!("{TICKET_PREFIX}{payload_hex}.{mac_hex}"))
    }

    /// Decode a ticket using the supplied key.
    pub fn decode(token: &str, key: &TicketKey) -> Result<Self, TicketError> {
        let (payload_bytes, mac) = parse_token(token)?;
        let expected = keyed_mac(key, &payload_bytes);
        // BLAKE3 Hash equality compares MAC bytes in constant time.
        if blake3::Hash::from(expected) != blake3::Hash::from(mac) {
            return Err(TicketError::MacMismatch);
        }
        let claims = TicketClaims::decode_payload(&payload_bytes)?;
        let version = *payload_bytes.first().ok_or(TicketError::Truncated)?;
        Ok(Self {
            claims,
            mac,
            version,
        })
    }

    /// Decode a ticket without validating the MAC.
    pub fn decode_unverified(token: &str) -> Result<TicketClaims, TicketError> {
        let (payload_bytes, _mac) = parse_token(token)?;
        TicketClaims::decode_payload(&payload_bytes)
    }
}

/// Issuer for minted tickets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TicketIssuer {
    key: TicketKey,
}

impl TicketIssuer {
    /// Create a new issuer from the shared secret.
    #[must_use]
    pub fn new(secret: &str) -> Self {
        Self {
            key: TicketKey::from_secret(secret),
        }
    }

    /// Issue a signed ticket for the supplied claims.
    pub fn issue(&self, claims: TicketClaims) -> Result<TicketToken, TicketError> {
        let payload = claims.encode_payload()?;
        let mac = keyed_mac(&self.key, &payload);
        Ok(TicketToken {
            claims,
            mac,
            version: CLAIMS_VERSION,
        })
    }

    /// Issue version 2 with canonical unsigned LEB128 budget/time fields.
    /// Scope, role, string and MAC semantics remain identical to version 1;
    /// callers retain their existing absolute encoded-token byte bound.
    pub fn issue_compact(&self, claims: TicketClaims) -> Result<TicketToken, TicketError> {
        let payload = claims.encode_payload_version(COMPACT_CLAIMS_VERSION)?;
        // Optional zero-valued quota fields are not canonical on this wire.
        // Never issue bytes that the strict version-2 decoder would refuse.
        TicketClaims::decode_payload(&payload)?;
        let mac = keyed_mac(&self.key, &payload);
        Ok(TicketToken {
            claims,
            mac,
            version: COMPACT_CLAIMS_VERSION,
        })
    }

    /// Return the key used by the issuer.
    #[must_use]
    pub fn key(&self) -> TicketKey {
        self.key
    }
}

/// Errors raised while parsing or validating tickets.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TicketError {
    /// The supplied token is missing the expected prefix.
    #[error("ticket missing prefix")]
    MissingPrefix,
    /// The token does not match the expected encoding.
    #[error("ticket malformed")]
    MalformedToken,
    /// The embedded MAC had an unexpected length.
    #[error("ticket MAC length {0} is invalid")]
    InvalidMacLength(usize),
    /// The MAC did not validate against the payload.
    #[error("ticket MAC validation failed")]
    MacMismatch,
    /// The claims payload uses an unsupported version.
    #[error("claims version {0} is unsupported")]
    UnsupportedVersion(u8),
    /// The claims payload references an unknown role.
    #[error("claims role {0} is unsupported")]
    UnsupportedRole(u8),
    /// The mount data exceeds expected limits.
    #[error("mount data exceeds allowed length")]
    MountTooLarge,
    /// The ticket scope verb was invalid.
    #[error("scope verb {0} is invalid")]
    InvalidScopeVerb(u8),
    /// Too many scopes were included in the ticket.
    #[error("scope count exceeds allowed maximum")]
    ScopeTooMany,
    /// The claims payload is incomplete.
    #[error("claims payload truncated")]
    Truncated,
    /// Extra bytes remain in the claims payload.
    #[error("claims payload contains trailing data")]
    TrailingData,
}

fn encode_integer(mut value: u64, version: u8, payload: &mut Vec<u8>) {
    if version == CLAIMS_VERSION {
        payload.extend_from_slice(&value.to_le_bytes());
        return;
    }
    while value >= 128 {
        payload.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
    payload.push(value as u8);
}

fn encode_string(value: &str, payload: &mut Vec<u8>) -> Result<(), TicketError> {
    if value.len() > MAX_MOUNT_FIELD_LEN {
        return Err(TicketError::MountTooLarge);
    }
    let len: u16 = value
        .len()
        .try_into()
        .map_err(|_| TicketError::MountTooLarge)?;
    payload.extend_from_slice(&len.to_le_bytes());
    payload.extend_from_slice(value.as_bytes());
    Ok(())
}

fn keyed_mac(key: &TicketKey, payload: &[u8]) -> [u8; 32] {
    let hash: Hash = blake3::keyed_hash(key.as_bytes(), payload);
    *hash.as_bytes()
}

fn parse_token(token: &str) -> Result<(Vec<u8>, [u8; 32]), TicketError> {
    let payload = token
        .strip_prefix(TICKET_PREFIX)
        .ok_or(TicketError::MissingPrefix)?;
    let (payload_hex, mac_hex) = payload.split_once('.').ok_or(TicketError::MalformedToken)?;
    let payload_bytes = hex::decode(payload_hex).map_err(|_| TicketError::MalformedToken)?;
    let mac_bytes = hex::decode(mac_hex).map_err(|_| TicketError::MalformedToken)?;
    if mac_bytes.len() != 32 {
        return Err(TicketError::InvalidMacLength(mac_bytes.len()));
    }
    let mut mac = [0u8; 32];
    mac.copy_from_slice(&mac_bytes);
    Ok((payload_bytes, mac))
}

fn encode_scopes(scopes: &[TicketScope], payload: &mut Vec<u8>) -> Result<(), TicketError> {
    if scopes.len() > MAX_SCOPE_COUNT {
        return Err(TicketError::ScopeTooMany);
    }
    payload.push(scopes.len() as u8);
    for scope in scopes {
        encode_string(&scope.path, payload)?;
        payload.push(scope.verb.as_u8());
        payload.extend_from_slice(&scope.rate_per_s.to_le_bytes());
    }
    Ok(())
}

fn decode_scopes(cursor: &mut PayloadCursor<'_>) -> Result<Vec<TicketScope>, TicketError> {
    let count = cursor.read_u8()? as usize;
    if count > MAX_SCOPE_COUNT {
        return Err(TicketError::ScopeTooMany);
    }
    let mut scopes = Vec::with_capacity(count);
    for _ in 0..count {
        let path = cursor.read_string()?;
        let verb = TicketVerb::from_u8(cursor.read_u8()?)?;
        let rate_per_s = cursor.read_u32()?;
        scopes.push(TicketScope {
            path,
            verb,
            rate_per_s,
        });
    }
    Ok(scopes)
}

struct PayloadCursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> PayloadCursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn read_exact(&mut self, len: usize) -> Result<&'a [u8], TicketError> {
        let end = self.pos.saturating_add(len);
        if end > self.bytes.len() {
            return Err(TicketError::Truncated);
        }
        let slice = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn read_u8(&mut self) -> Result<u8, TicketError> {
        Ok(self.read_exact(1)?[0])
    }

    fn read_u64(&mut self) -> Result<u64, TicketError> {
        let mut buf = [0u8; 8];
        buf.copy_from_slice(self.read_exact(8)?);
        Ok(u64::from_le_bytes(buf))
    }

    fn read_integer(&mut self, version: u8) -> Result<u64, TicketError> {
        if version == CLAIMS_VERSION {
            return self.read_u64();
        }
        let mut value = 0_u64;
        for index in 0..10 {
            let byte = self.read_u8()?;
            if index == 9 && byte > 1 {
                return Err(TicketError::MalformedToken);
            }
            value |= u64::from(byte & 0x7f) << (index * 7);
            if byte & 0x80 == 0 {
                if index > 0 && byte == 0 {
                    return Err(TicketError::MalformedToken);
                }
                return Ok(value);
            }
        }
        Err(TicketError::MalformedToken)
    }

    fn read_u32(&mut self) -> Result<u32, TicketError> {
        let mut buf = [0u8; 4];
        buf.copy_from_slice(self.read_exact(4)?);
        Ok(u32::from_le_bytes(buf))
    }

    fn read_string(&mut self) -> Result<String, TicketError> {
        let mut len_buf = [0u8; 2];
        len_buf.copy_from_slice(self.read_exact(2)?);
        let len = u16::from_le_bytes(len_buf) as usize;
        if len > MAX_MOUNT_FIELD_LEN {
            return Err(TicketError::MountTooLarge);
        }
        let bytes = self.read_exact(len)?;
        let value = core::str::from_utf8(bytes).map_err(|_| TicketError::MalformedToken)?;
        Ok(value.to_string())
    }

    fn ensure_empty(&self) -> Result<(), TicketError> {
        if self.pos == self.bytes.len() {
            Ok(())
        } else {
            Err(TicketError::TrailingData)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_ticket_matches_the_versioned_wire_vector_and_preserves_legacy() {
        let claims = TicketClaims::new(
            Role::Queen,
            BudgetSpec::unbounded()
                .with_ops(Some(1))
                .with_ttl(Some(200)),
            Some("mi-test".into()),
            MountSpec::empty(),
            1_000_000,
        )
        .with_scopes(alloc::vec![TicketScope::new(
            "/host/tickets/spec",
            TicketVerb::Write,
            0
        )]);
        // v2: header; unsigned LEB128 ops/TTL; u16-length subject; unsigned
        // LEB128 issuer milliseconds; empty mounts; one ordinary v1 scope.
        let expected = "02001e01c80107006d692d74657374c0843d000000000112002f686f73742f7469636b6574732f737065630100000000";
        assert_eq!(
            hex::encode(claims.encode_payload_version(2).unwrap()),
            expected
        );
        let issuer = TicketIssuer::new("compact-ticket-fixture");
        let compact = issuer
            .issue_compact(claims.clone())
            .unwrap()
            .encode()
            .unwrap();
        assert!(compact.starts_with(&format!("cohesix-ticket-{expected}.")));
        let parsed = TicketToken::decode(&compact, &issuer.key()).unwrap();
        assert_eq!(parsed.claims(), &claims);
        assert_eq!(parsed.encode().unwrap(), compact);
        let legacy = issuer.issue(claims.clone()).unwrap().encode().unwrap();
        assert!(legacy.starts_with("cohesix-ticket-01001e0100000000000000c800000000000000"));
        assert_eq!(
            TicketToken::decode(&legacy, &issuer.key())
                .unwrap()
                .claims(),
            &claims
        );
        assert_eq!(
            TicketToken::decode(&compact.replacen("-02", "-01", 1), &issuer.key()).unwrap_err(),
            TicketError::MacMismatch
        );
    }

    #[test]
    fn compact_integers_refuse_overflow_nonminimal_truncation_and_ambiguous_flags() {
        for (value, encoded) in [
            (0_u64, alloc::vec![0]),
            (127, alloc::vec![0x7f]),
            (128, alloc::vec![0x80, 0x01]),
            (16_384, alloc::vec![0x80, 0x80, 0x01]),
            (
                u64::MAX,
                alloc::vec![0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 1],
            ),
        ] {
            let mut actual = Vec::new();
            encode_integer(value, 2, &mut actual);
            assert_eq!(actual, encoded);
            let mut cursor = PayloadCursor::new(&encoded);
            assert_eq!(cursor.read_integer(2).unwrap(), value);
            cursor.ensure_empty().unwrap();
        }
        for bad in [
            alloc::vec![0x80, 0],
            alloc::vec![0x81, 0],
            alloc::vec![0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 2],
            alloc::vec![0x80; 10],
        ] {
            assert_eq!(
                PayloadCursor::new(&bad).read_integer(2).unwrap_err(),
                TicketError::MalformedToken
            );
        }
        assert_eq!(
            PayloadCursor::new(&[0x80]).read_integer(2).unwrap_err(),
            TicketError::Truncated
        );
        assert_eq!(
            TicketClaims::decode_payload(&[2, 0, 0x80]).unwrap_err(),
            TicketError::MalformedToken
        );
        let claims = TicketClaims::new(
            Role::Queen,
            BudgetSpec::unbounded(),
            None,
            MountSpec::empty(),
            1,
        )
        .with_quotas(TicketQuotas {
            bandwidth_bytes: Some(0),
            ..TicketQuotas::default()
        });
        assert_eq!(
            TicketIssuer::new("compact-ticket-fixture")
                .issue_compact(claims)
                .unwrap_err(),
            TicketError::MalformedToken
        );
    }

    #[test]
    fn default_heartbeat_limits_are_finite() {
        let budget = BudgetSpec::default_heartbeat();
        assert!(budget.ticks.is_some());
        assert!(budget.ops.is_some());
        assert!(budget.ttl_s.is_some());
    }

    #[test]
    fn default_gpu_limits_enforce_ttl_and_ops() {
        let budget = BudgetSpec::default_gpu();
        assert!(budget.ticks().is_none());
        assert_eq!(budget.ops(), Some(64));
        assert_eq!(budget.ttl_s(), Some(120));
    }

    #[test]
    fn ticket_key_rehydrates_from_generated_bytes() {
        let key = TicketKey::from_secret("secret");

        assert_eq!(TicketKey::from_bytes(key.to_bytes()), key);
    }

    #[test]
    fn ticket_round_trip_preserves_claims() {
        let issuer = TicketIssuer::new("secret");
        let claims = TicketClaims::new(
            Role::WorkerHeartbeat,
            BudgetSpec::default_heartbeat(),
            None,
            MountSpec::empty(),
            42,
        );
        let token = issuer.issue(claims.clone()).unwrap();
        let encoded = token.encode().unwrap();
        let decoded = TicketToken::decode(&encoded, &issuer.key()).unwrap();
        assert_eq!(decoded.claims(), &claims);
    }
}
