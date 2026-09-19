// Author: Lukas Bower
// Purpose: Preserve signed intent and terminal authority across workflow projection without accepting client status as completion.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#[path = "../../../crates/cohesix-evidence/tests/support/mod.rs"]
mod support;
use coh::workflow::{Deployment, Package, Step};
use cohesix_evidence::{Artifact, Kind};
use ed25519_dalek::{Signer, SigningKey};
use serde_json::json;
use std::{collections::BTreeMap, fs};

#[test]
fn workflow_requires_exact_signed_intent_and_terminal_before_advancement() {
    let root = tempfile::tempdir().unwrap();
    let cas = root.path().join("cas");
    fs::create_dir(&cas).unwrap();
    let (mut graph, mut trust) = support::fixture();
    let registry = cohesix_authority::provider::registry().unwrap();
    let hash = registry["graph_sha256"].as_str().unwrap();
    graph.binding.action = "endpoint_compliance.observe".into();
    graph.binding.provider_graph_sha256 = hash.into();
    trust.expected = graph.binding.clone();
    let request = json!({"schema":"host-ticket/v1","id":"request-1","idempotency_key":"once-1",
        "action":"endpoint_compliance.observe","writer_epoch":3,"target":"scan","args":{"target_id":"scan"}});
    let bytes = serde_json::to_vec(&request).unwrap();
    fs::write(cas.join(cohesix_evidence::digest(&bytes)), &bytes).unwrap();
    fs::write(
        cas.join(cohesix_evidence::digest(b"observed immutable bytes")),
        b"observed immutable bytes",
    )
    .unwrap();
    let key = SigningKey::from_bytes(&[71; 32]);
    let mut parents = Vec::new();
    for record in &mut graph.records {
        record.record.binding = graph.binding.clone();
        record.record.parents = parents.clone();
        if record.record.kind == Kind::Intent {
            record.record.artifacts = vec![Artifact {
                sha256: cohesix_evidence::digest(&bytes),
                bytes: bytes.len() as u64,
                media_type: "application/json".into(),
            }];
        }
        let canonical = serde_json::to_vec(&record.record).unwrap();
        record.sha256 = cohesix_evidence::digest(&canonical);
        record.signature = hex::encode(key.sign(&canonical).to_bytes());
        parents.push(record.sha256.clone());
    }
    let graph_path = root.path().join("graph.json");
    let trust_path = root.path().join("trust.json");
    fs::write(&graph_path, serde_json::to_vec(&graph).unwrap()).unwrap();
    fs::write(&trust_path, serde_json::to_vec(&trust).unwrap()).unwrap();
    let mut recovery_request = request.clone();
    recovery_request["id"] = "recovery-1".into();
    let mut recovery_trust = trust.clone();
    recovery_trust.expected.ticket_id = "recovery-1".into();
    let recovery_trust_path = root.path().join("recovery-trust.json");
    fs::write(
        &recovery_trust_path,
        serde_json::to_vec(&recovery_trust).unwrap(),
    )
    .unwrap();
    let mut deployment = Deployment {
        schema: "cohesix-workflow-deployment/v1".into(),
        workflow_id: "mac-endpoint-compliance".into(),
        provider_graph_sha256: hash.into(),
        topology: BTreeMap::from([
            ("controller".into(), "mac".into()),
            ("target-hive".into(), "hive".into()),
            ("provider-host".into(), "mac".into()),
        ]),
        package: Package {
            input: root.path().join("absent-package"),
            trust: root.path().join("absent-package-trust"),
        },
        steps: vec![Step {
            request,
            graph: graph_path.clone(),
            trust: trust_path,
            cas: cas.clone(),
        }],
        recovery: vec![Step {
            request: recovery_request,
            graph: root.path().join("recovery.json"),
            trust: recovery_trust_path,
            cas,
        }],
    };
    let report = coh::workflow::inspect(&deployment).unwrap();
    assert_eq!(report["all_steps_verified"], true);
    assert_eq!(report["authoritative"], false);
    assert_eq!(report["production_use_case_accepted"], false);
    deployment.steps[0].request["args"]["target_id"] = "other".into();
    assert!(coh::workflow::inspect(&deployment)
        .unwrap_err()
        .to_string()
        .contains("intent-substitution"));
    deployment.steps[0].request["args"]["target_id"] = "scan".into();
    graph.records.pop();
    fs::write(&graph_path, serde_json::to_vec(&graph).unwrap()).unwrap();
    assert_eq!(
        coh::workflow::inspect(&deployment).unwrap()["all_steps_verified"],
        false
    );
    graph.records[0].signature = "00".repeat(64);
    // An incomplete record remains pending; it never becomes accepted authority.
    fs::write(&graph_path, serde_json::to_vec(&graph).unwrap()).unwrap();
    assert_eq!(
        coh::workflow::inspect(&deployment).unwrap()["all_steps_verified"],
        false
    );
}
