// Author: Lukas Bower
// Purpose: Preserve independent signature, causal ordering, terminal uniqueness and CAS integrity contracts.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

use cohesix_evidence::*;
use ed25519_dalek::{Signer, SigningKey};

mod support;
use support::fixture;

fn resign(record: &mut SignedRecord) {
    let bytes = serde_json::to_vec(&record.record).unwrap();
    record.sha256 = digest(&bytes);
    record.signature = hex::encode(SigningKey::from_bytes(&[71; 32]).sign(&bytes).to_bytes());
}

fn check(graph: &Graph, trust: &Trust) -> Result<VerifiedGraph, Error> {
    verify(&serde_json::to_vec(graph).unwrap(), trust, |artifact| {
        if artifact.sha256 != digest(b"observed immutable bytes") || artifact.bytes != 24 {
            return Err(Error::Digest);
        }
        Ok(())
    })
}

#[test]
fn exact_signatures_causality_identity_and_expiry_are_all_required() {
    let (graph, trust) = fixture();
    assert_eq!(check(&graph, &trust).unwrap().outcome(), Outcome::Succeeded);
    let mut forged = graph.clone();
    forged.records[7].signature = "00".repeat(64);
    assert_eq!(check(&forged, &trust).unwrap_err(), Error::Signature);
    let mut duplicate = graph.clone();
    let mut terminal = duplicate.records[7].clone();
    terminal.record.sequence = 9;
    resign(&mut terminal);
    duplicate.records.push(terminal);
    assert_eq!(check(&duplicate, &trust).unwrap_err(), Error::Causality);
    let mut wrong = trust.clone();
    wrong.expected.writer_epoch = 4;
    assert_eq!(check(&graph, &wrong).unwrap_err(), Error::Identity);
    wrong = trust.clone();
    wrong.keys[0].kinds.remove(&Kind::Grant);
    assert_eq!(check(&graph, &wrong).unwrap_err(), Error::Signature);
    wrong = trust.clone();
    wrong.verification_unix_ms = 5000;
    assert_eq!(check(&graph, &wrong).unwrap_err(), Error::Chronology);
    let mut disconnected = graph.clone();
    disconnected.records[7].record.parents.clear();
    resign(&mut disconnected.records[7]);
    assert_eq!(check(&disconnected, &trust).unwrap_err(), Error::Causality);
    let mut native = graph.clone();
    native.records[7].record.native_identity = Some("different-invocation".into());
    resign(&mut native.records[7]);
    assert_eq!(check(&native, &trust).unwrap_err(), Error::Identity);
    let pretty = serde_json::to_vec_pretty(&graph).unwrap();
    assert_eq!(
        verify(&pretty, &trust, |_| Ok(())).unwrap_err(),
        Error::Schema
    );
    assert_eq!(
        verify(
            br#"{"schema":"cohesix-operation-report/v1","authoritative":true}"#,
            &trust,
            |_| Ok(())
        )
        .unwrap_err(),
        Error::Schema
    );
}

#[test]
fn artifact_file_presence_never_substitutes_for_length_and_hash() {
    let directory = tempfile::tempdir().unwrap();
    let artifact = Artifact {
        sha256: digest(b"abc"),
        bytes: 3,
        media_type: "text/plain".into(),
    };
    assert_eq!(verify_cas(directory.path(), &artifact), Err(Error::Missing));
    let file = directory.path().join(&artifact.sha256);
    std::fs::write(&file, b"abc").unwrap();
    assert_eq!(verify_cas(directory.path(), &artifact), Ok(()));
    std::fs::write(&file, b"abd").unwrap();
    assert_eq!(verify_cas(directory.path(), &artifact), Err(Error::Digest));
}

#[test]
fn recovery_requires_a_new_admitted_chain_and_exact_original_terminal() {
    let (original_graph, original_trust) = fixture();
    let original = check(&original_graph, &original_trust).unwrap();
    let (mut graph, mut trust) = fixture();
    graph.binding.ticket_id = "recovery-1".into();
    graph.binding.idempotency_key = "recovery-once".into();
    graph.binding.recovery_of = Some(RecoveryLink {
        ticket_id: "request-1".into(),
        graph_sha256: original.digest().into(),
        terminal_sha256: original_graph.records[7].sha256.clone(),
    });
    trust.expected = graph.binding.clone();
    let mut parents = Vec::new();
    for record in &mut graph.records {
        record.record.binding = graph.binding.clone();
        record.record.observed_unix_ms += 100;
        record.record.parents = parents.clone();
        resign(record);
        parents.push(record.sha256.clone());
    }
    let bytes = serde_json::to_vec(&graph).unwrap();
    assert!(verify_recovery(&original, &bytes, &trust, |_| Ok(())).is_ok());
    trust.keys[0].kinds.remove(&Kind::Grant);
    assert!(verify_recovery(&original, &bytes, &trust, |_| Ok(())).is_err());
    trust.keys[0].kinds.insert(Kind::Grant);
    let mut wrong_original = original_graph;
    wrong_original.records[7].record.observed_unix_ms += 1;
    resign(&mut wrong_original.records[7]);
    let wrong = check(&wrong_original, &original_trust).unwrap();
    assert!(verify_recovery(&wrong, &bytes, &trust, |_| Ok(())).is_err());
}
