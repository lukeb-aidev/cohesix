// Author: Lukas Bower
// Purpose: Verify enrolled AK certificate chains, quote signatures and request freshness.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

use crate::{decode, tpm, Error, Evidence, PendingChallenge, TrustPolicy, Verified};
use alloc::vec::Vec;
use core::time::Duration;
use pki_types::{CertificateDer, UnixTime};
use sha2::{Digest, Sha256};

/// Validate against caller-owned policy and time, consuming the challenge once.
///
/// An offline caller must retain the original request/time and classify the
/// result as offline verification; this function does not create target proof.
pub fn verify(
    policy_bytes: &[u8],
    evidence_bytes: &[u8],
    pending: &mut PendingChallenge,
    now_ms: u64,
) -> Result<Verified, Error> {
    // Consume before parsing: even malformed input cannot reuse an issued nonce.
    pending.consume(now_ms, 60_000)?;
    let policy = TrustPolicy::parse(policy_bytes)?;
    if now_ms
        .checked_sub(pending.issued_at_ms)
        .ok_or(Error::Stale)?
        > u64::from(policy.max_age_ms)
    {
        return Err(Error::Stale);
    }
    if now_ms / 1000 < policy.not_before || now_ms / 1000 > policy.not_after {
        return Err(Error::Policy);
    }
    let evidence = Evidence::parse(evidence_bytes)?;
    if evidence.context != policy.context
        || hex::encode(policy.context.digest()?) != pending.challenge.context_sha256
    {
        return Err(Error::Context);
    }
    let quote_bytes = decode(&evidence.quote, 512)?;
    let quote = tpm::Quote::parse(&quote_bytes, &policy)?;
    if quote.extra_data != pending.challenge.qualifying_data()? {
        return Err(Error::Nonce);
    }
    if quote.signer != decode(&policy.qualified_signer, 34)? {
        return Err(Error::Signer);
    }
    if quote.pcr_digest != policy.pcr_digest()? {
        return Err(Error::Measurement);
    }

    let roots = policy
        .roots
        .iter()
        .map(|value| certificate(value, &policy))
        .collect::<Result<Vec<_>, _>>()?;
    let anchors = roots
        .iter()
        .map(|cert| webpki::anchor_from_trusted_cert(cert).map_err(|_| Error::Chain))
        .collect::<Result<Vec<_>, _>>()?;
    let chain = evidence
        .certificates
        .iter()
        .map(|value| certificate(value, &policy))
        .collect::<Result<Vec<_>, _>>()?;
    let leaf = chain.first().ok_or(Error::Chain)?;
    if hex::encode(Sha256::digest(leaf.as_ref())) != policy.ak_certificate_sha256 {
        return Err(Error::Signer);
    }
    let cert = webpki::EndEntityCert::try_from(leaf).map_err(|_| Error::Chain)?;
    let algorithm = webpki::ring::ECDSA_P256_SHA256;
    // TCG id-kp-AIKCertificate (2.23.133.8.3). No TLS/server EKU fallback.
    cert.verify_for_usage(
        &[algorithm],
        &anchors,
        &chain[1..],
        UnixTime::since_unix_epoch(Duration::from_millis(now_ms)),
        webpki::KeyUsage::required(&[0x67, 0x81, 0x05, 0x08, 0x03]),
        None,
        None,
    )
    .map_err(|_| Error::Chain)?;
    let signature = tpm::signature_der(&decode(&evidence.signature, 72)?)?;
    cert.verify_signature(algorithm, &quote_bytes, &signature)
        .map_err(|_| Error::Signature)?;
    Ok(Verified {
        evidence_class: policy.context.class,
        evidence_sha256: hex::encode(Sha256::digest(evidence_bytes)),
        trust_policy_sha256: hex::encode(Sha256::digest(policy_bytes)),
        nonce_sha256: hex::encode(Sha256::digest(decode(&pending.challenge.nonce, 32)?)),
        clock: quote.clock,
        reset_count: quote.reset_count,
        restart_count: quote.restart_count,
    })
}

fn certificate(value: &str, policy: &TrustPolicy) -> Result<CertificateDer<'static>, Error> {
    let bytes = decode(value, 2048)?;
    if policy
        .revoked_certificates
        .contains(&hex::encode(Sha256::digest(&bytes)))
    {
        return Err(Error::Revoked);
    }
    Ok(CertificateDer::from(bytes))
}
