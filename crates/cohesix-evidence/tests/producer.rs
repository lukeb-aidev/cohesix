// Author: Lukas Bower
// Purpose: Prove phase custody, durable prefix continuity and final verification with independent producer keys.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

use cohesix_evidence::producer::{
    operation_key, Custody, Enrollment, Event, Journal, Operation, Producer,
};
use cohesix_evidence::*;
use ed25519_dalek::SigningKey;
use std::collections::BTreeSet;

mod support;

#[test]
fn enrolled_operation_binds_profile_request_and_retry_without_renewing_expiry() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let (_, mut trust) = support::fixture();
    trust.keys[0].kinds = BTreeSet::from([Kind::Intent, Kind::Facts, Kind::Approval, Kind::Grant]);
    trust.expected.provider_graph_sha256 = cohesix_authority::provider::registry().unwrap()
        ["graph_sha256"]
        .as_str()
        .unwrap()
        .into();
    trust
        .expected
        .component_sha256
        .insert("hive-gateway".into(), "66".repeat(32));
    let key = root.path().join("secret");
    std::fs::write(&key, hex::encode([71; 32])).unwrap();
    let store = root.path().join("store");
    std::fs::create_dir(&store).unwrap();
    std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o700)).unwrap();
    let config = Enrollment {
        schema: "cohesix-producer-enrollment/v1".into(),
        store,
        key_id: trust.keys[0].id.clone(),
        signing_key_ref: format!("file:{}", key.display()),
        trust: trust.clone(),
        expires_unix_ms: 5000,
    };
    let path = root.path().join(format!(
        "{}.json",
        operation_key("request-1", "once-1").unwrap()
    ));
    std::fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let mut operation = Operation::open(
        root.path(),
        "request-1",
        "once-1",
        Custody::GatewayAdmission,
        1000,
    )
    .unwrap();
    operation
        .bind(
            "systemd.restart",
            3,
            &"11".repeat(32),
            "hive-gateway",
            &"66".repeat(32),
        )
        .unwrap();
    assert!(operation
        .bind(
            "systemd.restart",
            4,
            &"11".repeat(32),
            "hive-gateway",
            &"66".repeat(32)
        )
        .is_err());
    assert!(operation
        .bind(
            "systemd.restart",
            3,
            &"12".repeat(32),
            "hive-gateway",
            &"66".repeat(32)
        )
        .is_err());
    assert!(operation
        .bind(
            "systemd.restart",
            3,
            &"11".repeat(32),
            "hive-gateway",
            &"67".repeat(32)
        )
        .is_err());
    let payload = br#"{"args":null,"id":"request-1","writer_epoch":3}"#;
    operation
        .emit(Kind::Intent, payload, None, Outcome::Observed, 1000)
        .unwrap();
    operation
        .emit(Kind::Intent, payload, None, Outcome::Observed, 1100)
        .unwrap();
    assert_eq!(
        operation.record(Kind::Intent).unwrap().observed_unix_ms,
        1000
    );
    operation
        .require_json(
            Kind::Intent,
            &serde_json::json!({"id":"request-1", "writer_epoch":3}),
            true,
        )
        .unwrap();
    assert_eq!(
        operation.read_json(Kind::Intent).unwrap(),
        serde_json::from_slice::<serde_json::Value>(payload).unwrap()
    );
    assert_eq!(
        operation.phase_public_key(Kind::Intent).unwrap(),
        operation.public_key()
    );
    assert!(operation.read_json(Kind::Grant).is_err());
    assert!(operation
        .require_json(
            Kind::Intent,
            &serde_json::json!({"id":"request-1", "writer_epoch":4}),
            true
        )
        .is_err());
    assert!(operation
        .emit(Kind::Intent, b"{}", None, Outcome::Observed, 1100)
        .is_err());
    drop(operation);
    assert!(Operation::open(
        root.path(),
        "request-1",
        "once-1",
        Custody::NativeOperation,
        1200
    )
    .is_err());
    assert!(Operation::open(
        root.path(),
        "request-1",
        "once-1",
        Custody::GatewayAdmission,
        5000
    )
    .is_err());
    let operation = Operation::open(
        root.path(),
        "request-1",
        "once-1",
        Custody::GatewayAdmission,
        1200,
    )
    .unwrap();
    assert_eq!(
        operation.record(Kind::Intent).unwrap().expires_unix_ms,
        5000
    );
}

#[test]
fn worker_custody_closes_success_failure_cancel_and_expiry_without_promoting_missing_phases() {
    use std::os::unix::fs::PermissionsExt;
    for outcome in [
        Outcome::Succeeded,
        Outcome::Failed,
        Outcome::Cancelled,
        Outcome::Expired,
    ] {
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let (_, mut trust) = support::fixture();
        trust.expected.action = "gpu.workload.submit".into();
        trust.expected.worker = Some(WorkerIdentity {
            id: "worker-1".into(),
            role: "worker-gpu".into(),
            generation: 2,
            image_sha256: "99".repeat(32),
        });
        let request = serde_json::json!({"schema":"host-ticket/v2", "id":"request-1", "idempotency_key":"once-1",
            "writer_epoch":3, "action":"gpu.workload.submit", "receipt_mode":"worker",
            "receipt_worker_id":"worker-1", "receipt_worker_role":"worker-gpu", "receipt_supervisor_generation":2,
            "receipt_cap_generation":3});
        ticket::require_binding(&trust.expected, &request, false).unwrap();
        let mut changed = request.clone();
        changed["receipt_supervisor_generation"] = 3.into();
        assert!(ticket::require_binding(&trust.expected, &changed, false).is_err());
        changed = request.clone();
        changed["receipt_worker_id"] = "other".into();
        assert!(ticket::require_binding(&trust.expected, &changed, false).is_err());
        trust.keys.clear();
        let groups = [
            (
                Custody::GatewayAdmission,
                vec![Kind::Intent, Kind::Facts, Kind::Approval, Kind::Grant],
            ),
            (
                Custody::NativeOperation,
                vec![Kind::Execution, Kind::Observation, Kind::Verification],
            ),
            (Custody::WorkerWitness, vec![Kind::Worker, Kind::Terminal]),
        ];
        let mut producers = Vec::new();
        for (index, (custody, kinds)) in groups.iter().enumerate() {
            let key = SigningKey::from_bytes(&[80 + index as u8; 32]);
            let path = root.path().join(format!("fixture-key-{index}"));
            std::fs::write(&path, hex::encode(key.to_bytes())).unwrap();
            let enrolled = TrustedKey {
                id: format!("key-{index}"),
                source: format!("custodian-{index}"),
                public_key: hex::encode(key.verifying_key().to_bytes()),
                kinds: kinds.iter().copied().collect(),
                not_before_unix_ms: 900,
                not_after_unix_ms: 10000,
            };
            producers.push(
                Producer::open(
                    &format!("file:{}", path.display()),
                    enrolled.clone(),
                    *custody,
                )
                .unwrap(),
            );
            trust.keys.push(enrolled);
        }
        let mut journal = Journal::open(root.path(), trust.expected.clone()).unwrap();
        let artifact = journal
            .retain(br#"{"fixture":"native-terminal"}"#, "application/json")
            .unwrap();
        for (index, (_, kinds)) in groups.iter().enumerate() {
            for kind in kinds {
                trust.verification_unix_ms += 1;
                let event = || Event {
                    kind: *kind,
                    expires_unix_ms: 5000,
                    artifacts: vec![artifact.clone()],
                    native_identity: (index > 0).then(|| "exact-native-job".into()),
                    resource_generation: if index > 0 { 4 } else { 0 },
                    event_cursor: None,
                    outcome: match kind {
                        Kind::Approval | Kind::Grant => Outcome::Admitted,
                        Kind::Verification if outcome == Outcome::Succeeded => Outcome::Verified,
                        Kind::Verification | Kind::Worker | Kind::Terminal => outcome,
                        _ => Outcome::Observed,
                    },
                };
                if index == 2 {
                    assert!(journal.append(&producers[1], &trust, event()).is_err());
                }
                journal.append(&producers[index], &trust, event()).unwrap();
                if *kind != Kind::Terminal {
                    assert!(verify(&journal.bytes().unwrap(), &trust, |a| verify_cas(
                        &root.path().join("cas"),
                        a
                    ))
                    .is_err());
                }
            }
        }
        let bytes = journal.bytes().unwrap();
        assert_eq!(
            verify(&bytes, &trust, |a| verify_cas(&root.path().join("cas"), a))
                .unwrap()
                .outcome(),
            outcome
        );
        let mut changed: Graph = serde_json::from_slice(&bytes).unwrap();
        changed.records[7]
            .record
            .binding
            .worker
            .as_mut()
            .unwrap()
            .generation = 3;
        assert!(verify(&serde_json::to_vec(&changed).unwrap(), &trust, |_| Ok(())).is_err());
    }
}

#[test]
fn independent_custodians_preserve_a_durable_chain_and_refuse_forged_or_replayed_phases() {
    let root = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let (fixture, mut trust) = support::fixture();
    trust.keys.clear();
    let phases = [
        (
            Custody::GatewayAdmission,
            vec![Kind::Intent, Kind::Facts, Kind::Approval, Kind::Grant],
        ),
        (
            Custody::NativeOperation,
            vec![
                Kind::Execution,
                Kind::Observation,
                Kind::Verification,
                Kind::Terminal,
            ],
        ),
    ];
    let mut producers = Vec::new();
    for (index, (custody, kinds)) in phases.iter().enumerate() {
        let key = SigningKey::from_bytes(&[70 + index as u8; 32]);
        let path = root.path().join(format!("test-key-{index}"));
        std::fs::write(&path, hex::encode(key.to_bytes())).unwrap();
        let enrollment = TrustedKey {
            id: format!("key-{index}"),
            source: format!("custodian-{index}"),
            public_key: hex::encode(key.verifying_key().to_bytes()),
            kinds: BTreeSet::from_iter(kinds.iter().copied()),
            not_before_unix_ms: 900,
            not_after_unix_ms: 10_000,
        };
        let reference = format!("file:{}", path.display());
        producers.push(Producer::open(&reference, enrollment.clone(), *custody).unwrap());
        if index == 0 {
            assert!(
                Producer::open(&reference, enrollment.clone(), Custody::NativeOperation).is_err()
            );
        }
        trust.keys.push(enrollment);
    }
    let mut journal = Journal::open(root.path(), fixture.binding.clone()).unwrap();
    assert!(Journal::open(root.path(), fixture.binding.clone()).is_err());
    let artifact = journal
        .retain(b"observed immutable bytes", "text/plain")
        .unwrap();
    for (index, signed) in fixture.records.iter().enumerate() {
        let native = index >= 4;
        let event = || Event {
            kind: signed.record.kind,
            expires_unix_ms: 5000,
            artifacts: vec![artifact.clone()],
            native_identity: signed.record.native_identity.clone(),
            resource_generation: signed.record.resource_generation,
            event_cursor: None,
            outcome: signed.record.outcome,
        };
        trust.verification_unix_ms = 1000 + index as u64;
        if !native {
            assert!(journal.append(&producers[1], &trust, event()).is_err());
        }
        journal
            .append(&producers[usize::from(native)], &trust, event())
            .unwrap();
        assert!(journal
            .append(&producers[usize::from(native)], &trust, event())
            .is_err());
        let bytes = journal.bytes().unwrap();
        if index < 7 {
            assert!(verify(&bytes, &trust, |object| verify_cas(
                &root.path().join("cas"),
                object
            ))
            .is_err());
        }
        if index == 3 {
            drop(journal);
            journal = Journal::open(root.path(), fixture.binding.clone()).unwrap();
            assert_eq!(journal.bytes().unwrap(), bytes);
        }
    }
    let bytes = journal.bytes().unwrap();
    let verified = verify(&bytes, &trust, |object| {
        verify_cas(&root.path().join("cas"), object)
    })
    .unwrap();
    assert_eq!(verified.outcome(), Outcome::Succeeded);
    let mut bad: Graph = serde_json::from_slice(&bytes).unwrap();
    bad.records[3].record.outcome = Outcome::Failed;
    assert!(journal
        .import(&serde_json::to_vec(&bad).unwrap(), &trust)
        .is_err());
    trust.verification_unix_ms = 5000;
    assert!(journal.import(&bytes, &trust).is_err());
    let directory = digest(&serde_json::to_vec(&fixture.binding).unwrap());
    drop(journal);
    std::fs::remove_file(root.path().join(directory).join("graph.json")).unwrap();
    assert!(Journal::open(root.path(), fixture.binding).is_err());
}
