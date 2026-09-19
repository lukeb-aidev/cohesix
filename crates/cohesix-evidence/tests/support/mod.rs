// Author: Lukas Bower
// Purpose: Supply one deterministic signed causal fixture to Rust host conformance consumers.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

use cohesix_evidence::*;
use ed25519_dalek::{Signer, SigningKey};
use std::collections::{BTreeMap, BTreeSet};

pub fn fixture() -> (Graph, Trust) {
    // This deterministic test key is confined to tests and cannot enter a live bundle.
    let key = SigningKey::from_bytes(&[71; 32]);
    let binding = Binding {
        ticket_id: "request-1".into(),
        subject: "operator-1".into(),
        action: "systemd.restart".into(),
        idempotency_key: "once-1".into(),
        writer_epoch: 3,
        target_manifest_sha256: "11".repeat(32),
        provider_graph_sha256: "22".repeat(32),
        implementation_graph_sha256: "33".repeat(32),
        use_case_graph_sha256: "44".repeat(32),
        component_sha256: BTreeMap::from([("provider".into(), "55".repeat(32))]),
        worker: None,
        recovery_of: None,
    };
    let kinds = [
        Kind::Intent,
        Kind::Facts,
        Kind::Approval,
        Kind::Grant,
        Kind::Execution,
        Kind::Observation,
        Kind::Verification,
        Kind::Terminal,
    ];
    let mut records: Vec<SignedRecord> = Vec::new();
    for (index, kind) in kinds.iter().enumerate() {
        let native = index >= 4;
        let record = Record {
            schema: "cohesix-causal-record/v1".into(),
            binding: binding.clone(),
            kind: *kind,
            source: "test-custodian".into(),
            sequence: index as u64 + 1,
            observed_unix_ms: 1000 + index as u64,
            expires_unix_ms: 5000,
            parents: records.iter().map(|record| record.sha256.clone()).collect(),
            artifacts: vec![Artifact {
                sha256: digest(b"observed immutable bytes"),
                bytes: 24,
                media_type: "text/plain".into(),
            }],
            native_identity: native.then(|| "unit-invocation-unique".into()),
            resource_generation: if native { 4 } else { 0 },
            event_cursor: None,
            outcome: match kind {
                Kind::Approval | Kind::Grant => Outcome::Admitted,
                Kind::Verification => Outcome::Verified,
                Kind::Terminal => Outcome::Succeeded,
                _ => Outcome::Observed,
            },
        };
        let bytes = serde_json::to_vec(&record).unwrap();
        records.push(SignedRecord {
            record,
            sha256: digest(&bytes),
            key_id: "fixture-key".into(),
            signature: hex::encode(key.sign(&bytes).to_bytes()),
        });
    }
    let trust = Trust {
        schema: "cohesix-evidence-trust/v1".into(),
        expected: binding.clone(),
        keys: vec![TrustedKey {
            id: "fixture-key".into(),
            source: "test-custodian".into(),
            public_key: hex::encode(key.verifying_key().to_bytes()),
            kinds: BTreeSet::from_iter(kinds),
            not_before_unix_ms: 900,
            not_after_unix_ms: 10_000,
        }],
        verification_unix_ms: 2000,
        maximum_record_ttl_ms: 5000,
    };
    (
        Graph {
            schema: SCHEMA.into(),
            binding,
            records,
        },
        trust,
    )
}
