// Author: Lukas Bower
// Purpose: Preserve independent comparison, crash, authority and recovered-failure contracts for native adapter release.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
use anyhow::{ensure, Result};
use coh::peft::{
    release::{self, *},
    transaction::{self, Entry, Native, Observation, Phase, Request, State},
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    panic::{catch_unwind, AssertUnwindSafe},
};

#[path = "../../../crates/cohesix-evidence/tests/support/mod.rs"]
mod support;

fn reports() -> (Evaluation, Evaluation, EvaluationPolicy, DeploymentState) {
    let context = EvaluationContext {
        dataset_sha256: "11".repeat(32),
        split: "heldout".into(),
        preprocessing_sha256: "22".repeat(32),
        base_sha256: "33".repeat(32),
        tokenizer_sha256: "44".repeat(32),
        evaluator: "transformers.Trainer.evaluate".into(),
        evaluator_version: "4.56.2".into(),
        parameters_sha256: "55".repeat(32),
        seed_policy: "fixed:41".into(),
        runtime_sha256: "66".repeat(32),
        resource_sha256: "77".repeat(32),
    };
    let baseline = Evaluation {
        schema: "cohesix-native-evaluation/v1".into(),
        artifact_sha256: "33".repeat(32),
        context: context.clone(),
        samples: 16,
        completed_unix_ms: 1000,
        metrics: BTreeMap::from([("eval_loss".into(), 3.0)]),
        native_report_sha256: "88".repeat(32),
    };
    let candidate = Evaluation {
        artifact_sha256: "99".repeat(32),
        metrics: BTreeMap::from([("eval_loss".into(), 2.5)]),
        ..baseline.clone()
    };
    let policy = EvaluationPolicy {
        minimum_samples: 16,
        maximum_age_ms: 1000,
        metrics: BTreeMap::from([(
            "eval_loss".into(),
            MetricBound {
                direction: Direction::Lower,
                absolute_bound: 4.0,
                maximum_regression: 0.0,
            },
        )]),
    };
    let current = DeploymentState {
        generation: 0,
        adapter_sha256: None,
        served_artifact_sha256: "33".repeat(32),
        runtime_sha256: context.runtime_sha256,
        healthy: true,
        rollback_verified: true,
    };
    (candidate, baseline, policy, current)
}

#[test]
fn native_comparison_requires_exact_context_samples_current_generation_and_quality() {
    let (candidate, baseline, policy, current) = reports();
    let comparison = compare(&candidate, &baseline, &policy, &current, 1100).unwrap();
    recheck(&comparison, &candidate, &baseline, &policy, &current, 1101).unwrap();
    for field in [
        "dataset_sha256",
        "split",
        "preprocessing_sha256",
        "base_sha256",
        "tokenizer_sha256",
        "evaluator",
        "evaluator_version",
        "parameters_sha256",
        "seed_policy",
        "runtime_sha256",
        "resource_sha256",
    ] {
        let mut value = serde_json::to_value(&candidate).unwrap();
        value["context"][field] = json!("aa".repeat(32));
        let changed = serde_json::from_value(value).unwrap();
        assert!(
            compare(&changed, &baseline, &policy, &current, 1100).is_err(),
            "{field}"
        );
    }
    for (samples, time, loss) in [
        (15, 1000, 2.5),
        (16, 0, 2.5),
        (16, 1200, 2.5),
        (16, 1000, 3.01),
        (16, 1000, f64::NAN),
        (16, 1000, f64::INFINITY),
    ] {
        let mut changed = candidate.clone();
        changed.samples = samples;
        changed.completed_unix_ms = time;
        changed.metrics.insert("eval_loss".into(), loss);
        assert!(compare(&changed, &baseline, &policy, &current, 1100).is_err());
    }
    assert!(compare(&candidate, &baseline, &policy, &current, 2000).is_err());
    let mut changed = current.clone();
    changed.generation = 1;
    assert!(recheck(&comparison, &candidate, &baseline, &policy, &changed, 1100).is_err());
    let mut changed = candidate.clone();
    changed.metrics.clear();
    assert!(compare(&changed, &baseline, &policy, &current, 1100).is_err());
    let mut changed = current.clone();
    changed.rollback_verified = false;
    assert!(compare(&candidate, &baseline, &policy, &changed, 1100).is_err());
}

struct Provider {
    request: Request,
    current: DeploymentState,
    results: Vec<Observation>,
    calls: Vec<Phase>,
    crash: Option<Phase>,
    fail: Option<Phase>,
    denied: Option<Phase>,
    race: bool,
}
impl Provider {
    fn new(entry: Entry) -> Self {
        let (_, _, policy, current) = reports();
        Self {
            request: Request {
                schema: "cohesix-peft-release/v1".into(),
                operation_id: "release-one".into(),
                model_id: "adapter-one".into(),
                entry,
                profile_sha256: "aa".repeat(32),
                input_sha256: "bb".repeat(32),
                evaluation_policy: policy,
                baseline: current.clone(),
            },
            current,
            results: vec![],
            calls: vec![],
            crash: None,
            fail: None,
            denied: None,
            race: false,
        }
    }
}
impl Native for Provider {
    fn now_ms(&self) -> Result<u64> {
        Ok(1100)
    }
    fn accepted(&mut self) -> Result<DeploymentState> {
        Ok(self.current.clone())
    }
    fn authorize(&mut self, phase: Phase) -> Result<()> {
        ensure!(self.denied != Some(phase), "expired_or_revoked");
        Ok(())
    }
    fn reconcile(&mut self, phase: Phase) -> Result<Option<Observation>> {
        Ok(self.results.iter().find(|r| r.phase == phase).cloned())
    }
    fn execute(&mut self, phase: Phase) -> Result<Observation> {
        assert!(
            !self.calls.contains(&phase),
            "native phase dispatched twice"
        );
        self.calls.push(phase);
        let (candidate, baseline, _, _) = reports();
        let detail = match phase {
            Phase::Evaluate => json!({"candidate":candidate,"baseline":baseline}),
            Phase::Scan | Phase::Stage => json!({"adapter_sha256":candidate.artifact_sha256}),
            _ => json!({}),
        };
        if phase == Phase::Promote && self.fail != Some(phase) {
            self.current.generation += 1;
            self.current.adapter_sha256 = Some(candidate.artifact_sha256.clone());
            self.current.served_artifact_sha256 = candidate.artifact_sha256;
        }
        if phase == Phase::Stage && self.race {
            self.current.generation += 1;
        }
        if phase == Phase::Rollback && self.fail != Some(phase) {
            self.current = self.request.baseline.clone();
        }
        let result = Observation {
            operation_id: self.request.operation_id.clone(),
            request_sha256: release::identity(&self.request)?,
            phase,
            native_identity: "systemd:observed-invocation".into(),
            completed_unix_ms: 1100,
            succeeded: self.fail != Some(phase),
            detail,
        };
        self.results.push(result.clone());
        if self.crash == Some(phase) {
            self.crash = None;
            panic!("controller lost phase ACK");
        }
        Ok(result)
    }
}

#[test]
fn every_native_phase_survives_crash_and_duplicate_without_second_dispatch() {
    for entry in [Entry::Import, Entry::Train] {
        let phases = [
            Phase::Validate,
            Phase::Train,
            Phase::Evaluate,
            Phase::Scan,
            Phase::Stage,
            Phase::Load,
            Phase::Canary,
            Phase::Promote,
        ];
        for crash in phases {
            if entry == Entry::Import && crash == Phase::Train {
                continue;
            }
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("journal");
            let mut native = Provider::new(entry);
            native.crash = Some(crash);
            let request = native.request.clone();
            assert!(catch_unwind(AssertUnwindSafe(|| transaction::run(
                &path,
                &request,
                "ticket",
                "once",
                1,
                &mut native,
                false
            )))
            .is_err());
            let result =
                transaction::run(&path, &request, "ticket", "once", 1, &mut native, true).unwrap();
            assert_eq!(
                result.state,
                State::Succeeded,
                "{entry:?} {crash:?}: {:?}",
                result.blocker
            );
            let count = native.calls.len();
            assert_eq!(
                transaction::run(&path, &request, "ticket", "once", 1, &mut native, true)
                    .unwrap()
                    .state,
                State::Succeeded
            );
            assert_eq!(native.calls.len(), count);
            assert_eq!(count, if entry == Entry::Train { 8 } else { 7 });
        }
    }
}

#[test]
fn failed_canary_recovers_honestly_and_races_or_expired_phases_never_promote() {
    for failed in [Phase::Load, Phase::Canary] {
        let directory = tempfile::tempdir().unwrap();
        let mut native = Provider::new(Entry::Import);
        native.fail = Some(failed);
        let request = native.request.clone();
        let result = transaction::run(
            &directory.path().join("journal"),
            &request,
            "ticket",
            "once",
            1,
            &mut native,
            false,
        )
        .unwrap();
        assert_eq!(result.state, State::RecoveredFailure);
        assert_eq!(native.current, request.baseline);
        assert!(!native.calls.contains(&Phase::Promote));
    }
    for denied in [
        Phase::Validate,
        Phase::Train,
        Phase::Evaluate,
        Phase::Scan,
        Phase::Stage,
        Phase::Load,
        Phase::Canary,
        Phase::Promote,
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut native = Provider::new(Entry::Train);
        native.denied = Some(denied);
        let request = native.request.clone();
        let result = transaction::run(
            &directory.path().join("journal"),
            &request,
            "ticket",
            "once",
            1,
            &mut native,
            false,
        )
        .unwrap();
        assert_ne!(result.state, State::Succeeded);
        assert!(!native.calls.contains(&denied));
        assert!(!native.calls.contains(&Phase::Promote));
    }
    let directory = tempfile::tempdir().unwrap();
    let mut native = Provider::new(Entry::Import);
    native.race = true;
    let request = native.request.clone();
    let result = transaction::run(
        &directory.path().join("journal"),
        &request,
        "ticket",
        "once",
        1,
        &mut native,
        false,
    )
    .unwrap();
    assert_eq!(result.state, State::Failed);
    assert!(!native.calls.contains(&Phase::Load));
    assert_eq!(native.current.generation, 1);
}

#[test]
fn fresh_compensation_preserves_failed_release_and_never_replays_candidate_work() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("journal");
    let mut native = Provider::new(Entry::Import);
    native.fail = Some(Phase::Canary);
    native.denied = Some(Phase::Rollback);
    let request = native.request.clone();
    let original =
        transaction::run(&path, &request, "ticket", "once", 1, &mut native, false).unwrap();
    assert_eq!(original.state, State::RollbackFailed);
    let frozen = std::fs::read(path.join("recipe.json")).unwrap();
    let authority = transaction::RecoveryAuthority {
        ticket_id: "fresh-recovery".into(),
        idempotency_key: "recover-once".into(),
        writer_epoch: 2,
    };
    native.denied = None;
    native.crash = Some(Phase::Rollback);
    assert!(catch_unwind(AssertUnwindSafe(|| transaction::compensate(
        &path,
        &request,
        authority.clone(),
        &mut native
    )))
    .is_err());
    let recovered =
        transaction::compensate(&path, &request, authority.clone(), &mut native).unwrap();
    assert_eq!(recovered.state, State::RecoveredFailure);
    assert_eq!(native.current, request.baseline);
    assert_eq!(recovered.ticket_id, "ticket");
    assert_eq!(recovered.recovery_authority, Some(authority.clone()));
    assert_eq!(std::fs::read(path.join("recipe.json")).unwrap(), frozen);
    let calls = native.calls.len();
    transaction::compensate(&path, &request, authority, &mut native).unwrap();
    assert_eq!(native.calls.len(), calls);
    assert!(!native.calls.contains(&Phase::Promote));
    assert_eq!(
        native
            .calls
            .iter()
            .filter(|p| **p == Phase::Rollback)
            .count(),
        1
    );
}

#[test]
fn fresh_compensation_can_cancel_interrupted_training_before_any_evaluation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("journal");
    let mut native = Provider::new(Entry::Train);
    native.crash = Some(Phase::Train);
    let request = native.request.clone();
    assert!(catch_unwind(AssertUnwindSafe(|| transaction::run(
        &path,
        &request,
        "ticket",
        "once",
        1,
        &mut native,
        false
    )))
    .is_err());
    let frozen = std::fs::read(path.join("recipe.json")).unwrap();
    let recovered = transaction::compensate(
        &path,
        &request,
        transaction::RecoveryAuthority {
            ticket_id: "fresh-cancel".into(),
            idempotency_key: "cancel-once".into(),
            writer_epoch: 2,
        },
        &mut native,
    )
    .unwrap();
    assert_eq!(recovered.state, State::RecoveredFailure);
    assert_eq!(native.current, request.baseline);
    assert_eq!(
        native.calls,
        vec![Phase::Validate, Phase::Train, Phase::Rollback]
    );
    assert_eq!(std::fs::read(path.join("recipe.json")).unwrap(), frozen);
}

#[test]
fn uncertain_native_intent_is_never_reissued_and_rollback_failure_blocks() {
    for crash in [
        Phase::Validate,
        Phase::Train,
        Phase::Evaluate,
        Phase::Scan,
        Phase::Stage,
        Phase::Load,
        Phase::Canary,
        Phase::Promote,
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("journal");
        let mut native = Provider::new(Entry::Train);
        native.crash = Some(crash);
        let request = native.request.clone();
        assert!(catch_unwind(AssertUnwindSafe(|| transaction::run(
            &path,
            &request,
            "ticket",
            "once",
            1,
            &mut native,
            false
        )))
        .is_err());
        native.results.retain(|r| r.phase != crash);
        let count = native.calls.len();
        let uncertain =
            transaction::run(&path, &request, "ticket", "once", 1, &mut native, true).unwrap();
        let runtime = matches!(crash, Phase::Load | Phase::Canary | Phase::Promote);
        assert_eq!(
            uncertain.state,
            if runtime {
                State::RecoveredFailure
            } else {
                State::Ambiguous
            }
        );
        assert_eq!(native.calls.len(), count + usize::from(runtime));
        assert_eq!(
            native.calls.iter().filter(|phase| **phase == crash).count(),
            1
        );
    }
    let directory = tempfile::tempdir().unwrap();
    let mut native = Provider::new(Entry::Import);
    native.denied = Some(Phase::Canary);
    native.fail = Some(Phase::Rollback);
    let request = native.request.clone();
    let failed = transaction::run(
        &directory.path().join("journal"),
        &request,
        "ticket",
        "once",
        1,
        &mut native,
        false,
    )
    .unwrap();
    assert_eq!(failed.state, State::RollbackFailed);
    assert!(!native.calls.contains(&Phase::Promote));
}

#[test]
fn controller_lost_ack_preserves_single_submission_and_rejects_request_substitution() {
    use coh::{
        peft::controller::{self, Deployment},
        workflow::Step,
        CohAccess,
    };
    struct LostAck(usize);
    impl CohAccess for LostAck {
        fn list_dir(&mut self, _: &str, _: usize) -> Result<Vec<String>> {
            unreachable!()
        }
        fn read_file(&mut self, _: &str, _: usize) -> Result<Vec<u8>> {
            unreachable!()
        }
        fn write_append(&mut self, path: &str, _: &[u8]) -> Result<usize> {
            assert_eq!(path, "/host/tickets/spec");
            self.0 += 1;
            anyhow::bail!("lost acknowledgement after committed write")
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let native = Provider::new(Entry::Import);
    let request = json!({"schema":"host-ticket/v2", "id":"release-one", "idempotency_key":"once", "action":"peft.release",
        "writer_epoch":3, "expires_unix_ms":5000, "receipt_mode":"worker", "operation_id":"release-one",
        "subject_ref":"adapter-one", "receipt_worker_role":"worker-lora", "receipt_worker_id":"worker-1",
        "receipt_supervisor_generation":1, "receipt_cap_generation":1,
        "args":{"request_sha256":release::identity(&native.request).unwrap()}});
    let (_, mut trust) = support::fixture();
    trust.expected.ticket_id = "release-one".into();
    trust.expected.idempotency_key = "once".into();
    trust.expected.action = "peft.release".into();
    trust.expected.worker = Some(cohesix_evidence::WorkerIdentity {
        id: "worker-1".into(),
        role: "worker-lora".into(),
        generation: 1,
        image_sha256: "55".repeat(32),
    });
    std::fs::write(root.join("trust.json"), serde_json::to_vec(&trust).unwrap()).unwrap();
    let deployment = Deployment {
        schema: "cohesix-peft-deployment/v1".into(),
        journal: root.join("journal"),
        request: native.request,
        execution: Step {
            request,
            trust: root.join("trust.json"),
            graph: root.join("missing-graph.json"),
            cas: root.join("cas"),
        },
    };
    let path = root.join("deployment.json");
    std::fs::write(&path, serde_json::to_vec(&deployment).unwrap()).unwrap();
    let deployment = controller::load(&path).unwrap();
    controller::advance(&deployment, "plan", None, 1000).unwrap();
    let mut access = LostAck(0);
    assert!(controller::advance(&deployment, "apply", Some(&mut access), 1000).is_err());
    let recovered = controller::advance(&deployment, "recover", None, 1100).unwrap();
    assert_eq!(recovered["ambiguous"], true);
    assert_eq!(recovered["acknowledged"], false);
    controller::advance(&deployment, "apply", Some(&mut access), 1100).unwrap();
    assert_eq!(access.0, 1);
    assert!(controller::advance(&deployment, "verify", None, 1100).is_err());
    let mut substituted = serde_json::to_value(&deployment).unwrap();
    substituted["request"]["input_sha256"] = json!("ff".repeat(32));
    std::fs::write(&path, serde_json::to_vec(&substituted).unwrap()).unwrap();
    assert!(controller::load(&path).is_err());
}
