// Author: Lukas Bower
// Purpose: Submit ephemeral signed-evidence challenges and preserve offline verification records.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

use crate::{operator::AttestationResult, CohAccess};
use anyhow::{ensure, Context as _, Result};
use cohesix_attestation::{Challenge, Evidence, PendingChallenge, TrustPolicy};
use serde::{Deserialize, Serialize};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Bound for a retained request/evidence record, including JSON envelope overhead.
pub const MAX_RECORD_BYTES: usize = 12_288;

/// Retained host observation; its timestamps do not establish fresh offline proof.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    /// Canonical record version.
    pub schema: String,
    /// Original request, including its public nonce.
    pub challenge: Challenge,
    /// Host issue time.
    pub issued_at_ms: u64,
    /// Host response time.
    pub received_at_ms: u64,
    /// Exact validated fields of the device envelope.
    pub evidence: Evidence,
}

impl Record {
    /// Validate every retained field before attachment; arbitrary payloads are refused.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        ensure!(bytes.len() <= MAX_RECORD_BYTES, "attestation-record-bound");
        let record: Self = serde_json::from_slice(bytes).context("attestation-record-schema")?;
        ensure!(
            record.schema == "cohesix-attestation-record/v1",
            "attestation-record-schema"
        );
        record.challenge.qualifying_data()?;
        Evidence::parse(&serde_json::to_vec(&record.evidence)?)?;
        ensure!(
            record
                .received_at_ms
                .checked_sub(record.issued_at_ms)
                .is_some_and(|age| age <= 60_000),
            "attestation-record-duration"
        );
        Ok(record)
    }
}

#[derive(Deserialize)]
struct Capabilities {
    schema: String,
    signed_evidence: bool,
    challenge: bool,
}

/// Submit exactly one challenge only when the target advertises an issuer.
pub fn live(
    access: &mut dyn CohAccess,
    policy: &[u8],
    source_class: &str,
) -> Result<(AttestationResult, Option<Record>)> {
    let policy_contract = TrustPolicy::parse(policy)?;
    ensure!(
        policy_contract.context.class != cohesix_attestation::EvidenceClass::OfflineFixture,
        "offline-fixture-live-forbidden"
    );
    let capabilities = match access.read_file("/proc/attest/capabilities", 512) {
        Ok(bytes) => bytes,
        Err(_) => return Ok((unavailable(source_class), None)),
    };
    ensure!(capabilities.len() <= 512, "attestation-capabilities-bound");
    let capabilities: Capabilities =
        serde_json::from_slice(&capabilities).context("attestation-capabilities-schema")?;
    ensure!(
        capabilities.schema == cohesix_attestation::CAPABILITIES_SCHEMA,
        "attestation-capabilities-schema"
    );
    if !capabilities.signed_evidence || !capabilities.challenge {
        return Ok((unavailable(source_class), None));
    }
    // OS CSPRNG failure is fatal. Never substitute time, counters or manifest hashes.
    let mut nonce = [0u8; 32];
    getrandom::fill(&mut nonce).map_err(|_| anyhow::anyhow!("attestation-random-source"))?;
    let challenge = Challenge::new(nonce, &policy_contract.context)?;
    let payload = serde_json::to_vec(&challenge)?;
    ensure!(
        payload.len() <= cohesix_attestation::MAX_CHALLENGE_BYTES,
        "attestation-challenge-bound"
    );
    let issued_at_ms = now_ms()?;
    let monotonic = Instant::now();
    let mut pending = PendingChallenge::new(challenge.clone(), issued_at_ms)?;
    // No retry: a short write or error consumes this nonce and cannot be reissued.
    let written = access.write_append("/proc/attest/challenge", &payload)?;
    ensure!(written == payload.len(), "attestation-short-challenge");
    let evidence = access.read_file(
        "/proc/attest/evidence",
        cohesix_attestation::MAX_EVIDENCE_BYTES,
    )?;
    let elapsed_ms =
        u64::try_from(monotonic.elapsed().as_millis()).context("attestation-duration-bound")?;
    let monotonic_received_at_ms = issued_at_ms
        .checked_add(elapsed_ms)
        .context("attestation-time-overflow")?;
    // Use monotonic request duration and reject host wall-clock rollback as well.
    let wall_received_at_ms = now_ms()?;
    ensure!(
        wall_received_at_ms >= issued_at_ms,
        "attestation-clock-rollback"
    );
    let received_at_ms = wall_received_at_ms.max(monotonic_received_at_ms);
    let parsed = Evidence::parse(&evidence);
    let canonical = parsed.as_ref().ok().map(serde_json::to_vec).transpose()?;
    let verified = cohesix_attestation::verify(
        policy,
        canonical.as_deref().unwrap_or(&evidence),
        &mut pending,
        received_at_ms,
    );
    let result = result(verified, source_class, false);
    // Invalid envelope data is not retained: signatures/certificates/context fields
    // must be canonical bounded hex, preventing secret-bearing diagnostic blobs.
    let record = match parsed {
        Ok(evidence) => Some(Record {
            schema: "cohesix-attestation-record/v1".into(),
            challenge,
            issued_at_ms,
            received_at_ms,
            evidence,
        }),
        Err(_) => None,
    };
    Ok((result, record))
}

/// Reverify a retained signature at its recorded time, without a transport.
/// The resulting scope is always `offline-signature`, even for a physical AK.
pub fn offline(policy: &[u8], bytes: &[u8]) -> Result<AttestationResult> {
    let record = Record::parse(bytes)?;
    let mut pending = PendingChallenge::new(record.challenge, record.issued_at_ms)?;
    let evidence = serde_json::to_vec(&record.evidence)?;
    Ok(result(
        cohesix_attestation::verify(policy, &evidence, &mut pending, record.received_at_ms),
        "offline-pack",
        true,
    ))
}

fn result(
    verification: Result<cohesix_attestation::Verified, cohesix_attestation::Error>,
    source_class: &str,
    offline: bool,
) -> AttestationResult {
    match verification {
        Ok(verified) => AttestationResult {
            schema: "cohesix-attestation-result/v1".into(),
            verdict: "PASS".into(),
            reason: if offline {
                "valid-recorded-signature"
            } else {
                "valid-fresh-signature"
            }
            .into(),
            evidence_class: verified.evidence_class.as_str().into(),
            source_class: source_class.into(),
            proof_scope: if offline {
                "offline-signature"
            } else {
                "live-signature"
            }
            .into(),
            verified: Some(verified),
        },
        Err(error) => AttestationResult {
            schema: "cohesix-attestation-result/v1".into(),
            verdict: "FAIL".into(),
            reason: error.as_str().into(),
            evidence_class: "unavailable".into(),
            source_class: source_class.into(),
            proof_scope: "none".into(),
            verified: None,
        },
    }
}

fn unavailable(source_class: &str) -> AttestationResult {
    AttestationResult {
        schema: "cohesix-attestation-result/v1".into(),
        verdict: "UNAVAILABLE".into(),
        reason: "device-provider-unavailable".into(),
        evidence_class: "unavailable".into(),
        source_class: source_class.into(),
        proof_scope: "none".into(),
        verified: None,
    }
}

fn now_ms() -> Result<u64> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("attestation-host-clock")?;
    u64::try_from(elapsed.as_millis()).context("attestation-host-clock-bound")
}
