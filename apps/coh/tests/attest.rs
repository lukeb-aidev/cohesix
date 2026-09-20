// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Check signed offline verification and reject non-attested or misclassified evidence.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use coh::operator::{attest, Availability, Observation, Snapshot, SNAPSHOT_SCHEMA};

#[test]
fn matching_public_measurement_never_attests_a_device() {
    for source in ["offline-pack", "mock", "live-console", "host-projection"] {
        let snapshot = Snapshot { schema: SNAPSHOT_SCHEMA.to_owned(), source_class: source.to_owned(), observations: vec![Observation { path: "/proc/boot".to_owned(), status: Availability::Observed, content: Some("manifest_sha256=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa evidence_sha256=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned()), reason: None }], violations: vec![] };
        let result = attest(&snapshot);
        assert_eq!(result.verdict, "FAIL");
        assert_eq!(result.reason, "unsigned-measurement");
        assert_eq!(result.evidence_class, "measurement-only");
        assert_eq!(result.source_class, source);
    }
}

#[test]
fn unavailable_and_unsupported_signed_contracts_fail_closed() {
    let mut snapshot = Snapshot {
        schema: SNAPSHOT_SCHEMA.to_owned(),
        source_class: "offline-pack".to_owned(),
        observations: vec![],
        violations: vec![],
    };
    assert_eq!(attest(&snapshot).verdict, "UNAVAILABLE");
    snapshot.observations.push(Observation {
        path: "/proc/attest/evidence".to_owned(),
        status: Availability::Observed,
        content: Some(r#"{"verdict":"PASS","signature":"untrusted","device":"pi4"}"#.to_owned()),
        reason: None,
    });
    assert_eq!(attest(&snapshot).verdict, "UNAVAILABLE");
    snapshot.violations.push("invalid-inventory".to_owned());
    assert_eq!(attest(&snapshot).reason, "inconsistent-evidence");
}

#[test]
fn canonical_pack_verifies_a_signed_record_without_claiming_live_hardware() {
    use std::{path::Path, process::Command};
    let temp = tempfile::tempdir().unwrap();
    let fixtures =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/operator/attestation");
    let pack = temp.path().join("pack");
    let output = Command::new(env!("CARGO_BIN_EXE_coh"))
        .args(["evidence", "pack", "--mock", "--out"])
        .arg(&pack)
        .arg("--attestation-record")
        .arg(fixtures.join("record.json"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_coh"))
            .args(["attest", "--input"])
            .arg(&pack)
            .arg("--trust-policy")
            .arg(fixtures.join("policy.json"))
            .output()
            .unwrap()
    };
    let output = run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["verdict"], "PASS");
    assert_eq!(result["proof_scope"], "offline-signature");
    assert_eq!(result["evidence_class"], "offline-fixture");
    assert_eq!(result["source_class"], "offline-pack");
    assert_eq!(run().stdout, output.stdout);
    std::fs::write(pack.join("attachments/attestation-record.json"), b"{}").unwrap();
    let corrupted = run();
    assert!(!corrupted.status.success());
    // Pack digest validation rejects changed bytes before attestation parsing.
    assert!(corrupted.stdout.is_empty());
    assert_eq!(
        String::from_utf8_lossy(&corrupted.stderr),
        "Error: pack-file-digest-or-inventory\n"
    );
}

#[test]
fn unavailable_target_does_not_receive_a_challenge_or_mutation() {
    struct Unavailable;
    impl coh::CohAccess for Unavailable {
        fn list_dir(&mut self, _: &str, _: usize) -> anyhow::Result<Vec<String>> {
            panic!("unexpected list")
        }
        fn read_file(&mut self, path: &str, _: usize) -> anyhow::Result<Vec<u8>> {
            assert_eq!(path, "/proc/attest/capabilities");
            Ok(br#"{"schema":"cohesix-attestation-capabilities/v1","signed_evidence":false,"challenge":false}"#.to_vec())
        }
        fn write_append(&mut self, _: &str, _: &[u8]) -> anyhow::Result<usize> {
            panic!("unavailable device must not receive writes")
        }
    }
    let mut policy: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/operator/attestation/policy.json"
    ))
    .unwrap();
    policy["context"]["class"] = "qemu-virtual".into();
    let (result, record) = coh::attestation::live(
        &mut Unavailable,
        &serde_json::to_vec(&policy).unwrap(),
        "live-console",
    )
    .unwrap();
    assert_eq!(result.verdict, "UNAVAILABLE");
    assert_eq!(result.reason, "device-provider-unavailable");
    assert!(record.is_none());
}
