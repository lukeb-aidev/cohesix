// Author: Lukas Bower
// Purpose: Define capability ticket claims and validation for Cohesix roles.
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
const TICKET_PREFIX: &str = "cohesix-ticket-";
const MAX_MOUNT_FIELD_LEN: usize = 255;

/// Roles recognised by the Cohesix capability system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// Queen orchestration role controlling worker lifecycles.
    Queen,
    /// Worker responsible for emitting heartbeat telemetry.
    WorkerHeartbeat,
    /// Future GPU worker role.
    WorkerGpu,
}

impl Role {
    fn as_u8(self) -> u8 {
        match self {
            Role::Queen => 0,
            Role::WorkerHeartbeat => 1,
            Role::WorkerGpu => 2,
        }
    }

    fn from_u8(value: u8) -> Result<Self, TicketError> {
        match value {
            0 => Ok(Role::Queen),
            1 => Ok(Role::WorkerHeartbeat),
            2 => Ok(Role::WorkerGpu),
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
        }
    }

    fn encode_payload(&self) -> Result<Vec<u8>, TicketError> {
        let mut payload = Vec::new();
        payload.push(CLAIMS_VERSION);
        payload.push(self.role.as_u8());
        let mut flags = 0u8;
        if self.budget.ticks.is_some() {
            flags |= 0b0000_0001;
        }
        if self.budget.ops.is_some() {
            flags |= 0b0000_0010;
        }
        if self.budget.ttl_s.is_some() {
            flags |= 0b0000_0100;
        }
        if self.subject.is_some() {
            flags |= 0b0000_1000;
        }
        payload.push(flags);
        if let Some(ticks) = self.budget.ticks {
            payload.extend_from_slice(&ticks.to_le_bytes());
        }
        if let Some(ops) = self.budget.ops {
            payload.extend_from_slice(&ops.to_le_bytes());
        }
        if let Some(ttl_s) = self.budget.ttl_s {
            payload.extend_from_slice(&ttl_s.to_le_bytes());
        }
        if let Some(subject) = &self.subject {
            encode_string(subject, &mut payload)?;
        }
        payload.extend_from_slice(&self.issued_at_ms.to_le_bytes());
        encode_string(&self.mounts.service, &mut payload)?;
        encode_string(&self.mounts.at, &mut payload)?;
        Ok(payload)
    }

    fn decode_payload(bytes: &[u8]) -> Result<Self, TicketError> {
        let mut cursor = PayloadCursor::new(bytes);
        let version = cursor.read_u8()?;
        if version != CLAIMS_VERSION {
            return Err(TicketError::UnsupportedVersion(version));
        }
        let role = Role::from_u8(cursor.read_u8()?)?;
        let flags = cursor.read_u8()?;
        let ticks = if flags & 0b0000_0001 != 0 {
            Some(cursor.read_u64()?)
        } else {
            None
        };
        let ops = if flags & 0b0000_0010 != 0 {
            Some(cursor.read_u64()?)
        } else {
            None
        };
        let ttl_s = if flags & 0b0000_0100 != 0 {
            Some(cursor.read_u64()?)
        } else {
            None
        };
        let subject = if flags & 0b0000_1000 != 0 {
            Some(cursor.read_string()?)
        } else {
            None
        };
        let issued_at_ms = cursor.read_u64()?;
        let service = cursor.read_string()?;
        let at = cursor.read_string()?;
        cursor.ensure_empty()?;
        Ok(Self {
            role,
            budget: BudgetSpec { ticks, ops, ttl_s },
            subject,
            mounts: MountSpec { service, at },
            issued_at_ms,
        })
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

    fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Minted ticket token containing claims and a MAC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketToken {
    claims: TicketClaims,
    mac: [u8; 32],
}

impl TicketToken {
    /// Return the claims embedded in the ticket.
    #[must_use]
    pub fn claims(&self) -> &TicketClaims {
        &self.claims
    }

    /// Encode the ticket into its text representation.
    #[must_use]
    pub fn encode(&self) -> Result<String, TicketError> {
        let payload = self.claims.encode_payload()?;
        let payload_hex = hex::encode(payload);
        let mac_hex = hex::encode(self.mac);
        Ok(format!("{TICKET_PREFIX}{payload_hex}.{mac_hex}"))
    }

    /// Decode a ticket using the supplied key.
    pub fn decode(token: &str, key: &TicketKey) -> Result<Self, TicketError> {
        let (payload_bytes, mac) = parse_token(token)?;
        let expected = keyed_mac(key, &payload_bytes);
        if expected != mac {
            return Err(TicketError::MacMismatch);
        }
        let claims = TicketClaims::decode_payload(&payload_bytes)?;
        Ok(Self { claims, mac })
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
        Ok(TicketToken { claims, mac })
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
    /// The claims payload is incomplete.
    #[error("claims payload truncated")]
    Truncated,
    /// Extra bytes remain in the claims payload.
    #[error("claims payload contains trailing data")]
    TrailingData,
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
    let (payload_hex, mac_hex) = payload
        .split_once('.')
        .ok_or(TicketError::MalformedToken)?;
    let payload_bytes = hex::decode(payload_hex).map_err(|_| TicketError::MalformedToken)?;
    let mac_bytes = hex::decode(mac_hex).map_err(|_| TicketError::MalformedToken)?;
    if mac_bytes.len() != 32 {
        return Err(TicketError::InvalidMacLength(mac_bytes.len()));
    }
    let mut mac = [0u8; 32];
    mac.copy_from_slice(&mac_bytes);
    Ok((payload_bytes, mac))
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
