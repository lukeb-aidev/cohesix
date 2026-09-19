// Author: Lukas Bower
// Purpose: Refuse field-bus controls with missing signed grants, changed operands, wrong epochs or insufficient remaining authority.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

use super::*;
use cohesix_evidence::producer::{operation_key, Custody, Enrollment, Operation};
use cohesix_evidence::{Binding, Kind, Outcome, Trust, TrustedKey};
use ed25519_dalek::SigningKey;
use std::{
    collections::{BTreeMap, BTreeSet},
    os::unix::fs::PermissionsExt,
};

#[test]
fn controls_require_exact_independently_signed_grant_and_unexpired_map() {
    let root = tempfile::tempdir().unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let gateway = root.path().join("gateway");
    let native = root.path().join("native");
    let store = root.path().join("store");
    for path in [&gateway, &native, &store] {
        fs::create_dir(path).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let expected = Binding {
        ticket_id: "one".into(),
        idempotency_key: "once".into(),
        subject: "operator".into(),
        action: "modbus.control".into(),
        writer_epoch: 7,
        target_manifest_sha256: "11".repeat(32),
        provider_graph_sha256: cohesix_authority::provider::registry().unwrap()["graph_sha256"]
            .as_str()
            .unwrap()
            .into(),
        implementation_graph_sha256: "22".repeat(32),
        use_case_graph_sha256: "33".repeat(32),
        component_sha256: BTreeMap::from([("sidecar-bus".into(), "44".repeat(32))]),
        worker: None,
        recovery_of: None,
    };
    let mut keys = Vec::new();
    for (index, phases) in [
        vec![Kind::Intent, Kind::Facts, Kind::Approval, Kind::Grant],
        vec![
            Kind::Execution,
            Kind::Observation,
            Kind::Verification,
            Kind::Terminal,
        ],
    ]
    .into_iter()
    .enumerate()
    {
        let key = SigningKey::from_bytes(&[81 + index as u8; 32]);
        let path = root.path().join(format!("test-key-{index}"));
        fs::write(&path, hex::encode(key.to_bytes())).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        keys.push(TrustedKey {
            id: format!("key-{index}"),
            source: format!("custodian-{index}"),
            public_key: hex::encode(key.verifying_key().to_bytes()),
            kinds: BTreeSet::from_iter(phases),
            not_before_unix_ms: 900,
            not_after_unix_ms: 6000,
        });
    }
    let trust = Trust {
        schema: "cohesix-evidence-trust/v1".into(),
        expected,
        keys,
        verification_unix_ms: 1000,
        maximum_record_ttl_ms: 5000,
    };
    for (index, directory) in [&gateway, &native].into_iter().enumerate() {
        let enrollment = Enrollment {
            schema: "cohesix-producer-enrollment/v1".into(),
            store: store.clone(),
            key_id: format!("key-{index}"),
            signing_key_ref: format!(
                "file:{}",
                root.path().join(format!("test-key-{index}")).display()
            ),
            trust: trust.clone(),
            expires_unix_ms: 5000,
        };
        let path = directory.join(format!("{}.json", operation_key("one", "once").unwrap()));
        fs::write(&path, serde_json::to_vec(&enrollment).unwrap()).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    let (endpoint, request) = super::tests::selected("127.0.0.1:502".into(), true);
    let point = &endpoint.points[0];
    let admitted = serde_json::json!({"schema":"host-ticket/v1","id":"one","idempotency_key":"once",
        "action":"modbus.control","writer_epoch":7,"target":"plc","args":{"endpoint":"plc","point":"register"}});
    let mut operation =
        Operation::open(&gateway, "one", "once", Custody::GatewayAdmission, 1000).unwrap();
    operation
        .emit(
            Kind::Intent,
            &serde_json::to_vec(&admitted).unwrap(),
            None,
            Outcome::Observed,
            1000,
        )
        .unwrap();
    operation
        .emit(Kind::Facts, b"{}", None, Outcome::Observed, 1000)
        .unwrap();
    drop(operation);
    let operation =
        Operation::open(&native, "one", "once", Custody::NativeOperation, 1000).unwrap();
    assert!(require_control(&request, &admitted, &operation, &endpoint, point, 1000).is_err());
    drop(operation);
    let mut operation =
        Operation::open(&gateway, "one", "once", Custody::GatewayAdmission, 1000).unwrap();
    operation
        .emit(Kind::Approval, b"{}", None, Outcome::Admitted, 1000)
        .unwrap();
    operation
        .emit(Kind::Grant, b"{}", None, Outcome::Admitted, 1000)
        .unwrap();
    drop(operation);
    let operation =
        Operation::open(&native, "one", "once", Custody::NativeOperation, 1000).unwrap();
    assert!(require_control(&request, &admitted, &operation, &endpoint, point, 1000).is_ok());
    for (field, value) in [
        ("writer_epoch", serde_json::json!(8)),
        ("target", serde_json::json!("other")),
        (
            "args",
            serde_json::json!({"endpoint":"plc","point":"different"}),
        ),
        ("action", serde_json::json!("modbus.read")),
    ] {
        let mut changed = admitted.clone();
        changed[field] = value;
        assert!(require_control(&request, &changed, &operation, &endpoint, point, 1000).is_err());
    }
    let mut changed = admitted.clone();
    changed["extra"] = serde_json::json!(true);
    assert!(require_control(&request, &changed, &operation, &endpoint, point, 1000).is_err());
    assert!(require_control(&request, &admitted, &operation, &endpoint, point, 4000).is_err());
    let mut unapproved = point.clone();
    unapproved.approval_required = false;
    assert!(require_control(
        &request,
        &admitted,
        &operation,
        &endpoint,
        &unapproved,
        1000
    )
    .is_err());
}
