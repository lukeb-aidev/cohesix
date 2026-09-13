// Author: Lukas Bower
// Purpose: Check TPM/X.509 conformance against independently signed offline fixtures.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![cfg(feature = "verify")]

use cohesix_attestation::{verify, Challenge, Error, EvidenceClass, PendingChallenge, TrustPolicy};
use serde_json::Value;

const POLICY: &[u8] = include_bytes!("../../../tests/fixtures/operator/attestation/policy.json");
const EVIDENCE: &[u8] =
    include_bytes!("../../../tests/fixtures/operator/attestation/evidence.json");
const CHALLENGE: &[u8] =
    include_bytes!("../../../tests/fixtures/operator/attestation/challenge.json");
const ISSUED: u64 = 1_789_344_000_000;

fn pending() -> PendingChallenge {
    PendingChallenge::new(Challenge::parse(CHALLENGE).unwrap(), ISSUED).unwrap()
}

fn changed(bytes: &[u8], mutate: impl FnOnce(&mut Value)) -> Vec<u8> {
    let mut value: Value = serde_json::from_slice(bytes).unwrap();
    mutate(&mut value);
    serde_json::to_vec(&value).unwrap()
}

#[test]
fn independent_signature_passes_once_and_preserves_fixture_class() {
    let mut request = pending();
    let verified = verify(POLICY, EVIDENCE, &mut request, ISSUED + 100).unwrap();
    assert_eq!(verified.evidence_class, EvidenceClass::OfflineFixture);
    assert_eq!(
        (verified.clock, verified.reset_count, verified.restart_count),
        (1000, 1, 2)
    );
    assert_eq!(
        verify(POLICY, EVIDENCE, &mut request, ISSUED + 101),
        Err(Error::Replay)
    );
}

#[test]
fn nonce_and_freshness_are_owned_by_the_verifier() {
    let mut challenge = Challenge::parse(CHALLENGE).unwrap();
    challenge.nonce = "12".repeat(32);
    assert_eq!(
        verify(
            POLICY,
            EVIDENCE,
            &mut PendingChallenge::new(challenge, ISSUED).unwrap(),
            ISSUED + 1
        ),
        Err(Error::Nonce)
    );
    for time in [ISSUED - 1, ISSUED + 30_001] {
        assert_eq!(
            verify(POLICY, EVIDENCE, &mut pending(), time),
            Err(Error::Stale)
        );
    }
    assert!(verify(POLICY, EVIDENCE, &mut pending(), ISSUED + 30_000).is_ok());
}

#[test]
fn every_artifact_device_boot_and_proof_class_is_bound() {
    for field in [
        "firmware",
        "uboot",
        "kernel",
        "root_task",
        "runtimes",
        "resolved_manifest",
        "image",
        "dtb",
    ] {
        let evidence = changed(EVIDENCE, |v| {
            v["context"]["artifacts"][field] = "11".repeat(32).into()
        });
        assert_eq!(
            verify(POLICY, &evidence, &mut pending(), ISSUED + 1),
            Err(Error::Context),
            "{field}"
        );
    }
    for field in ["device_id", "boot_id"] {
        let evidence = changed(EVIDENCE, |v| v["context"][field] = "11".repeat(32).into());
        assert_eq!(
            verify(POLICY, &evidence, &mut pending(), ISSUED + 1),
            Err(Error::Context)
        );
    }
    let evidence = changed(EVIDENCE, |v| v["context"]["class"] = "pi4-device".into());
    assert_eq!(
        verify(POLICY, &evidence, &mut pending(), ISSUED + 1),
        Err(Error::Context)
    );
}

#[test]
fn wrong_measurements_pcr_selection_reset_and_clock_fail() {
    let cases: &[(&str, Value, Error)] = &[
        ("reset_count", 0.into(), Error::TpmReset),
        ("restart_count", 0.into(), Error::TpmReset),
        ("minimum_clock", 1001.into(), Error::Clock),
    ];
    for (field, value, expected) in cases {
        let policy = changed(POLICY, |v| v[*field] = value.clone());
        assert_eq!(
            verify(&policy, EVIDENCE, &mut pending(), ISSUED + 1),
            Err(*expected)
        );
    }
    let policy = changed(POLICY, |v| v["pcrs"][0]["sha256"] = "22".repeat(32).into());
    assert_eq!(
        verify(&policy, EVIDENCE, &mut pending(), ISSUED + 1),
        Err(Error::Measurement)
    );
    let policy = changed(POLICY, |v| {
        v["pcrs"].as_array_mut().unwrap().remove(0);
    });
    assert_eq!(
        verify(&policy, EVIDENCE, &mut pending(), ISSUED + 1),
        Err(Error::PcrSelection)
    );
}

#[test]
fn signature_key_chain_revocation_and_expiry_fail_closed() {
    let evidence = changed(EVIDENCE, |v| {
        let mut bytes = hex::decode(v["signature"].as_str().unwrap()).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        v["signature"] = hex::encode(bytes).into();
    });
    assert_eq!(
        verify(POLICY, &evidence, &mut pending(), ISSUED + 1),
        Err(Error::Signature)
    );
    let policy = changed(POLICY, |v| {
        v["ak_certificate_sha256"] = "11".repeat(32).into()
    });
    assert_eq!(
        verify(&policy, EVIDENCE, &mut pending(), ISSUED + 1),
        Err(Error::Signer)
    );
    let policy = changed(POLICY, |v| {
        v["revoked_certificates"] = serde_json::json!([v["ak_certificate_sha256"].clone()])
    });
    assert_eq!(
        verify(&policy, EVIDENCE, &mut pending(), ISSUED + 1),
        Err(Error::Revoked)
    );
    let policy = changed(POLICY, |v| v["roots"] = serde_json::json!(["0001"]));
    assert_eq!(
        verify(&policy, EVIDENCE, &mut pending(), ISSUED + 1),
        Err(Error::Chain)
    );
    let policy = changed(POLICY, |v| v["not_after"] = (ISSUED / 1000 - 1).into());
    assert_eq!(
        verify(&policy, EVIDENCE, &mut pending(), ISSUED + 1),
        Err(Error::Policy)
    );
    let policy = changed(POLICY, |v| v["not_after"] = 2_200_000_000u64.into());
    let future = 2_100_000_000_000;
    let mut request = PendingChallenge::new(Challenge::parse(CHALLENGE).unwrap(), future).unwrap();
    assert_eq!(
        verify(&policy, EVIDENCE, &mut request, future),
        Err(Error::Chain)
    );
}

#[test]
fn malformed_and_public_hash_substitutions_cannot_verify() {
    for bytes in [
        b"{}".as_slice(),
        b"{\"manifest_sha256\":\"public\"}",
        &EVIDENCE[..EVIDENCE.len() - 2],
    ] {
        assert!(verify(POLICY, bytes, &mut pending(), ISSUED + 1).is_err());
    }
    for kind in ["dice_evidence", "measurement_only", "tpm-or-dice"] {
        let evidence = changed(EVIDENCE, |v| v["kind"] = kind.into());
        assert_eq!(
            verify(POLICY, &evidence, &mut pending(), ISSUED + 1),
            Err(Error::UnsupportedAlgorithm)
        );
    }
    let evidence = changed(EVIDENCE, |v| v["signature"] = "secret-canary".into());
    assert_eq!(
        verify(POLICY, &evidence, &mut pending(), ISSUED + 1),
        Err(Error::Encoding)
    );
    assert!(TrustPolicy::parse(&vec![b' '; 32769]).is_err());
    assert!(Challenge::parse(&vec![b' '; 257]).is_err());
    let mut request = pending();
    assert!(verify(POLICY, b"{}", &mut request, ISSUED + 1).is_err());
    assert_eq!(
        verify(POLICY, EVIDENCE, &mut request, ISSUED + 2),
        Err(Error::Replay)
    );
}

#[test]
fn every_quote_truncation_and_trailing_bytes_are_rejected() {
    let value: Value = serde_json::from_slice(EVIDENCE).unwrap();
    let quote = value["quote"].as_str().unwrap();
    for length in (0..quote.len()).step_by(2) {
        let evidence = changed(EVIDENCE, |v| v["quote"] = quote[..length].into());
        assert!(
            verify(POLICY, &evidence, &mut pending(), ISSUED + 1).is_err(),
            "length={length}"
        );
    }
    let evidence = changed(EVIDENCE, |v| v["quote"] = format!("{quote}00").into());
    assert_eq!(
        verify(POLICY, &evidence, &mut pending(), ISSUED + 1),
        Err(Error::TrailingBytes)
    );
}
