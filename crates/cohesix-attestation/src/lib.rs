// Author: Lukas Bower
// Purpose: Define bounded, nonce-bound TPM2 evidence and fail-closed trust verification.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::{string::String, vec::Vec};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[cfg(feature = "verify")]
mod tpm;
#[cfg(feature = "verify")]
mod verify;
#[cfg(feature = "verify")]
pub use verify::verify;

/// Maximum encoded evidence size; one bounded Secure9P file.
pub const MAX_EVIDENCE_BYTES: usize = 8192;
/// Maximum verifier-owned trust policy, including enrolled certificates.
pub const MAX_POLICY_BYTES: usize = 32768;
/// Maximum encoded ephemeral challenge size.
pub const MAX_CHALLENGE_BYTES: usize = 256;
/// Protocol version for a target capability advertisement.
pub const CAPABILITIES_SCHEMA: &str = "cohesix-attestation-capabilities/v1";

/// A proof class is selected by enrollment, never by the transport or evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceClass {
    /// A separately enrolled physical device.
    Pi4Device,
    /// A virtual TPM, never physical-device acceptance.
    QemuVirtual,
    /// A cryptographic parser fixture, never fresh target acceptance.
    OfflineFixture,
}

impl EvidenceClass {
    /// Stable result token.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pi4Device => "pi4-device",
            Self::QemuVirtual => "qemu-virtual",
            Self::OfflineFixture => "offline-fixture",
        }
    }
}

/// Every artifact identity is required; no absent/zero digest is a wildcard.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifacts {
    pub firmware: String,
    pub uboot: String,
    pub kernel: String,
    pub root_task: String,
    pub runtimes: String,
    pub resolved_manifest: String,
    pub image: String,
    pub dtb: String,
}

impl Artifacts {
    fn values(&self) -> [&str; 8] {
        [
            &self.firmware,
            &self.uboot,
            &self.kernel,
            &self.root_task,
            &self.runtimes,
            &self.resolved_manifest,
            &self.image,
            &self.dtb,
        ]
    }
}

/// Expected boot context, enrolled independently of the returned evidence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    pub class: EvidenceClass,
    /// SHA-256 of the enrolled device identifier, not a secret.
    pub device_id: String,
    pub boot_id: String,
    pub artifacts: Artifacts,
}

impl Context {
    /// Hash fixed-width fields in the documented order, with domain separation.
    pub fn digest(&self) -> Result<[u8; 32], Error> {
        let mut hash = Sha256::new();
        hash.update(b"cohesix-attestation-context/v1\0");
        hash.update([match self.class {
            EvidenceClass::Pi4Device => 1,
            EvidenceClass::QemuVirtual => 2,
            EvidenceClass::OfflineFixture => 3,
        }]);
        hash.update(digest_bytes(&self.device_id)?);
        hash.update(digest_bytes(&self.boot_id)?);
        for value in self.artifacts.values() {
            hash.update(digest_bytes(value)?);
        }
        Ok(hash.finalize().into())
    }
}

/// The only payload accepted by the ephemeral challenge file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Challenge {
    pub schema: String,
    /// 256 bits from the verifier's operating-system random source.
    pub nonce: String,
    pub context_sha256: String,
}

impl Challenge {
    /// Construct a request from caller-provided cryptographic randomness.
    pub fn new(nonce: [u8; 32], context: &Context) -> Result<Self, Error> {
        if nonce == [0; 32] {
            return Err(Error::Nonce);
        }
        Ok(Self {
            schema: "cohesix-attestation-challenge/v1".into(),
            nonce: hex::encode(nonce),
            context_sha256: hex::encode(context.digest()?),
        })
    }

    /// Validate a fixed-schema request before it can reach a device owner.
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        let request: Self = parse(bytes, MAX_CHALLENGE_BYTES)?;
        request.qualifying_data()?;
        Ok(request)
    }

    /// TPM2 Quote qualifyingData, binding the entire request without JSON ambiguity.
    pub fn qualifying_data(&self) -> Result<[u8; 32], Error> {
        if self.schema != "cohesix-attestation-challenge/v1" {
            return Err(Error::Schema);
        }
        let mut hash = Sha256::new();
        hash.update(b"cohesix-tpm2-quote/v1\0");
        hash.update(digest_bytes(&self.nonce).map_err(|_| Error::Nonce)?);
        hash.update(digest_bytes(&self.context_sha256)?);
        Ok(hash.finalize().into())
    }
}

/// One explicitly selected SHA-256 PCR; order must be strictly increasing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pcr {
    pub index: u8,
    pub sha256: String,
}

/// Verifier-owned enrollment and measurement policy. Never load this from a target.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustPolicy {
    pub schema: String,
    pub context: Context,
    /// DER X.509 CA certificates as lowercase hex, from trusted enrollment.
    pub roots: Vec<String>,
    /// Exact accepted AK certificate identity, pinned in addition to its CA chain.
    pub ak_certificate_sha256: String,
    /// TPM qualified Name obtained during AK enrollment (including nameAlg).
    pub qualified_signer: String,
    pub pcrs: Vec<Pcr>,
    /// Inclusive validity window for this enrollment policy in Unix seconds.
    pub not_before: u64,
    pub not_after: u64,
    pub max_age_ms: u32,
    /// Expected reset epoch. A TPM reset requires explicit re-enrollment.
    pub reset_count: u32,
    pub restart_count: u32,
    pub minimum_clock: u64,
    /// SHA-256 certificate denylist; the policy itself has a bounded validity.
    pub revoked_certificates: Vec<String>,
}

impl TrustPolicy {
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        let policy: Self = parse(bytes, MAX_POLICY_BYTES)?;
        policy.validate()?;
        Ok(policy)
    }

    pub fn validate(&self) -> Result<(), Error> {
        if self.schema != "cohesix-attestation-trust/v1" {
            return Err(Error::Schema);
        }
        self.context.digest()?;
        digest_bytes(&self.ak_certificate_sha256)?;
        let signer = decode(&self.qualified_signer, 34)?;
        if signer.len() != 34 || signer[..2] != [0, 11] {
            return Err(Error::Signer);
        }
        if self.roots.is_empty()
            || self.roots.len() > 4
            || self.revoked_certificates.len() > 64
            || self.not_after <= self.not_before
            || self.max_age_ms == 0
            || self.max_age_ms > 60_000
        {
            return Err(Error::Policy);
        }
        for root in &self.roots {
            decode(root, 2048)?;
        }
        for revoked in &self.revoked_certificates {
            digest_bytes(revoked)?;
        }
        self.pcr_digest()?;
        Ok(())
    }

    /// TPM's quoted digest is SHA-256 of the selected PCR values in index order.
    pub fn pcr_digest(&self) -> Result<[u8; 32], Error> {
        if self.pcrs.is_empty() || self.pcrs.len() > 24 {
            return Err(Error::PcrSelection);
        }
        let mut previous = None;
        let mut digest = Sha256::new();
        for pcr in &self.pcrs {
            if pcr.index > 23 || previous.is_some_and(|index| pcr.index <= index) {
                return Err(Error::PcrSelection);
            }
            previous = Some(pcr.index);
            // A reset PCR is valid; artifact identity digests cannot be zero.
            let value = decode(&pcr.sha256, 32)?;
            if value.len() != 32 {
                return Err(Error::Measurement);
            }
            digest.update(value);
        }
        Ok(digest.finalize().into())
    }
}

/// Raw TPM structures are retained exactly, never redacted/re-encoded before verification.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub schema: String,
    pub kind: String,
    pub context: Context,
    /// TPMS_ATTEST bytes, excluding TPM2B size prefix.
    pub quote: String,
    /// TPMT_SIGNATURE bytes (ECDSA P-256, SHA-256 only in v1).
    pub signature: String,
    /// Leaf AK certificate followed by intermediates; roots come from policy only.
    pub certificates: Vec<String>,
}

impl Evidence {
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        let evidence: Self = parse(bytes, MAX_EVIDENCE_BYTES)?;
        if evidence.schema != "cohesix-attestation-evidence/v1" {
            return Err(Error::Schema);
        }
        if evidence.kind != "tpm2_quote" {
            return Err(Error::UnsupportedAlgorithm);
        }
        evidence.context.digest()?;
        if evidence.certificates.is_empty() || evidence.certificates.len() > 4 {
            return Err(Error::Chain);
        }
        decode(&evidence.quote, 512)?;
        decode(&evidence.signature, 72)?;
        for certificate in &evidence.certificates {
            decode(certificate, 2048)?;
        }
        Ok(evidence)
    }
}

/// Local request lifetime; timestamps never come from evidence or the target clock.
#[derive(Debug)]
pub struct PendingChallenge {
    challenge: Challenge,
    issued_at_ms: u64,
    consumed: bool,
}

impl PendingChallenge {
    pub fn new(challenge: Challenge, issued_at_ms: u64) -> Result<Self, Error> {
        challenge.qualifying_data()?;
        Ok(Self {
            challenge,
            issued_at_ms,
            consumed: false,
        })
    }

    pub fn challenge(&self) -> &Challenge {
        &self.challenge
    }

    /// Consume even failed responses: one request has exactly one verification attempt.
    pub fn consume(&mut self, now_ms: u64, max_age_ms: u32) -> Result<(), Error> {
        if core::mem::replace(&mut self.consumed, true) {
            return Err(Error::Replay);
        }
        let age = now_ms.checked_sub(self.issued_at_ms).ok_or(Error::Stale)?;
        if age > u64::from(max_age_ms) {
            return Err(Error::Stale);
        }
        Ok(())
    }
}

/// Verified TPM clock data to persist with the verifier's next enrollment state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verified {
    pub evidence_class: EvidenceClass,
    pub evidence_sha256: String,
    pub trust_policy_sha256: String,
    pub nonce_sha256: String,
    pub clock: u64,
    pub reset_count: u32,
    pub restart_count: u32,
}

/// Stable failures shared by CLI, packs and projections.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Bounds,
    Schema,
    Encoding,
    Policy,
    Nonce,
    Context,
    Stale,
    Replay,
    UnsupportedAlgorithm,
    Chain,
    Revoked,
    Signer,
    Signature,
    Measurement,
    PcrSelection,
    TpmReset,
    Clock,
    Truncated,
    TrailingBytes,
}

impl Error {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bounds => "attestation-bound",
            Self::Schema => "attestation-schema",
            Self::Encoding => "attestation-encoding",
            Self::Policy => "trust-policy-invalid",
            Self::Nonce => "nonce-mismatch",
            Self::Context => "context-mismatch",
            Self::Stale => "stale-challenge",
            Self::Replay => "replayed-challenge",
            Self::UnsupportedAlgorithm => "unsupported-algorithm",
            Self::Chain => "certificate-chain",
            Self::Revoked => "revoked-certificate",
            Self::Signer => "wrong-attestation-key",
            Self::Signature => "invalid-signature",
            Self::Measurement => "measurement-mismatch",
            Self::PcrSelection => "pcr-selection",
            Self::TpmReset => "tpm-reset",
            Self::Clock => "unsafe-or-rolled-back-clock",
            Self::Truncated => "truncated-evidence",
            Self::TrailingBytes => "trailing-evidence",
        }
    }
}

impl core::fmt::Display for Error {
    fn fmt(&self, out: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        out.write_str(self.as_str())
    }
}
impl core::error::Error for Error {}

fn parse<T: serde::de::DeserializeOwned>(bytes: &[u8], max: usize) -> Result<T, Error> {
    if bytes.is_empty() || bytes.len() > max {
        return Err(Error::Bounds);
    }
    serde_json::from_slice(bytes).map_err(|_| Error::Schema)
}

fn decode(value: &str, max: usize) -> Result<Vec<u8>, Error> {
    if value.is_empty()
        || value.len() > max * 2
        || !value.len().is_multiple_of(2)
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::Encoding);
    }
    hex::decode(value).map_err(|_| Error::Encoding)
}

fn digest_bytes(value: &str) -> Result<[u8; 32], Error> {
    let bytes: [u8; 32] = decode(value, 32)?.try_into().map_err(|_| Error::Encoding)?;
    if bytes == [0; 32] {
        return Err(Error::Encoding);
    }
    Ok(bytes)
}
