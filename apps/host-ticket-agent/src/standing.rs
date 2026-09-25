// Author: Lukas Bower
// Purpose: Bind selected ticket bytes to one durable standing reservation before native dispatch.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

use anyhow::{anyhow, ensure, Result};
use cohesix_authority::standing::{AdmissionFacts, StandingRefusal};
use cohesix_authority::standing_ledger::{JobExecution, JobRecord};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::executors::ExecutorConfig;
use crate::{HostTicketSpec, HOST_TICKET_V1_SCHEMA, HOST_TICKET_V2_SCHEMA};

/// A selected job must be present in the private durable ledger. Legacy
/// correlation without an explicit standing scope remains non-granting.
pub fn selected_record(
    spec: &HostTicketSpec,
    config: &ExecutorConfig,
) -> Result<Option<JobRecord>> {
    let Some(admission) = spec.admission.as_ref() else {
        return Ok(None);
    };
    if admission.standing_scope_id.is_none() {
        return Ok(None);
    }
    let ledger = config
        .standing_ledger
        .as_ref()
        .ok_or_else(|| anyhow!("EPERM standing-ledger-required"))?;
    let record = ledger
        .status(&admission.admission_id)?
        .ok_or_else(|| anyhow!("EPERM standing-admission-unknown"))?;
    verify_ticket(spec, &record)?;
    Ok(Some(record))
}

/// Commit a fresh, current-state dispatch barrier immediately before the
/// provider call. Reservation alone never authorizes a native effect.
pub fn begin_dispatch(
    spec: &HostTicketSpec,
    config: &ExecutorConfig,
    facts: &AdmissionFacts,
    now_unix_ms: u64,
) -> Result<()> {
    let Some(record) = selected_record(spec, config)? else {
        return Ok(());
    };
    let ledger = config
        .standing_ledger
        .as_ref()
        .ok_or_else(|| anyhow!("EPERM standing-ledger-required"))?;
    if record.binding.policy_sha256 != facts.policy_sha256 {
        return Err(anyhow!("{}", StandingRefusal::Stale));
    }
    ledger.begin_dispatch(&record.binding.admission_id, facts, now_unix_ms)?;
    Ok(())
}

/// Derive two nonzero generations from a direct typed native observation.
/// The full observation remains in the signed native evidence object; these
/// bounded fields only fence change between admission and dispatch.
pub fn service_generations(observation: &serde_json::Value) -> Result<(u64, u64)> {
    let encoded = serde_json::to_vec(observation)?;
    let state = Sha256::digest(&encoded);
    let invocation = observation
        .get("invocation_id")
        .and_then(Value::as_str)
        .unwrap_or("none");
    let resource = Sha256::digest(invocation.as_bytes());
    let mut state_bytes = [0u8; 8];
    state_bytes.copy_from_slice(&state[..8]);
    let mut resource_bytes = [0u8; 8];
    resource_bytes.copy_from_slice(&resource[..8]);
    Ok((
        u64::from_be_bytes(state_bytes).max(1),
        u64::from_be_bytes(resource_bytes).max(1),
    ))
}

/// Fence one selected release against a changed accepted generation or
/// serving artifact. The complete accepted state is retained in the private
/// release registry; the compact fields only guard admission to dispatch.
pub fn release_generations(accepted: &serde_json::Value) -> Result<(u64, u64)> {
    let state: coh::peft::release::DeploymentState = serde_json::from_value(accepted.clone())?;
    ensure!(
        state.healthy && state.rollback_verified,
        "EPERM release-incumbent-unverified"
    );
    let state_digest = Sha256::digest(serde_json::to_vec(&state)?);
    let resource_digest = Sha256::digest(serde_json::to_vec(&(
        &state.served_artifact_sha256,
        &state.runtime_sha256,
    ))?);
    let mut state_bytes = [0u8; 8];
    state_bytes.copy_from_slice(&state_digest[..8]);
    let mut resource_bytes = [0u8; 8];
    resource_bytes.copy_from_slice(&resource_digest[..8]);
    Ok((
        u64::from_be_bytes(state_bytes).max(1),
        u64::from_be_bytes(resource_bytes).max(1),
    ))
}

/// Settle a durable provider result once, independently of pending result
/// publication. A compatibility failure after dispatch remains uncertain.
pub fn settle_result(
    spec: &HostTicketSpec,
    config: &ExecutorConfig,
    result_line: &str,
    native_outcome_known: bool,
) -> Result<()> {
    let Some(record) = selected_record(spec, config)? else {
        return Ok(());
    };
    let ledger = config
        .standing_ledger
        .as_ref()
        .ok_or_else(|| anyhow!("EPERM standing-ledger-required"))?;
    let result: crate::HostTicketResult = serde_json::from_str(result_line)?;
    match record.execution {
        JobExecution::Reserved if result.state == "succeeded" => {
            return Err(anyhow!("EPERM standing-effect-without-dispatch"));
        }
        JobExecution::Reserved => {
            ledger.refuse_before_dispatch(&record.binding.admission_id)?;
        }
        JobExecution::Dispatching | JobExecution::Uncertain if native_outcome_known => {
            let digest = hex::encode(Sha256::digest(result_line.as_bytes()));
            ledger.confirm(&record.binding.admission_id, &digest)?;
        }
        JobExecution::Dispatching => {
            ledger.mark_uncertain(&record.binding.admission_id)?;
        }
        JobExecution::Uncertain | JobExecution::RefusedNoEffect => {}
        JobExecution::Confirmed => {
            let digest = hex::encode(Sha256::digest(result_line.as_bytes()));
            ensure!(
                record.result_sha256.as_deref() == Some(digest.as_str()),
                "EPERM standing-result-conflict"
            );
        }
    }
    Ok(())
}

/// Publishing or observing the exact terminal result satisfies delivery only
/// after the execution lane has a confirmed native outcome or a no-effect refusal.
pub fn acknowledge_delivery(spec: &HostTicketSpec, config: &ExecutorConfig) -> Result<()> {
    let Some(record) = selected_record(spec, config)? else {
        return Ok(());
    };
    if matches!(
        record.execution,
        JobExecution::Confirmed | JobExecution::RefusedNoEffect
    ) {
        config
            .standing_ledger
            .as_ref()
            .ok_or_else(|| anyhow!("EPERM standing-ledger-required"))?
            .acknowledge_delivery(&record.binding.admission_id)?;
    }
    Ok(())
}

/// Preserve a possible native effect when transport or reaping is ambiguous.
pub fn mark_uncertain_if_dispatched(spec: &HostTicketSpec, config: &ExecutorConfig) -> Result<()> {
    let Some(record) = selected_record(spec, config)? else {
        return Ok(());
    };
    if record.execution == JobExecution::Dispatching {
        config
            .standing_ledger
            .as_ref()
            .ok_or_else(|| anyhow!("EPERM standing-ledger-required"))?
            .mark_uncertain(&record.binding.admission_id)?;
    }
    Ok(())
}

/// Verify that a root-admitted ticket is the exact selected job reserved under
/// the authenticated caller. Correlation fields never grant authority by
/// themselves; the executor must separately recheck current facts and commit
/// the durable dispatch barrier.
pub fn verify_ticket(spec: &HostTicketSpec, record: &JobRecord) -> Result<()> {
    let admission = spec
        .admission
        .as_ref()
        .ok_or_else(|| anyhow!("EPERM standing-admission-required"))?;
    ensure!(
        admission.standing_scope_id.as_deref() == Some(record.binding.scope_id.as_str())
            && admission.admission_id == record.binding.admission_id
            && admission.intent_hash == record.intent_sha256
            && admission.policy_hash == record.binding.policy_sha256
            && admission.state_epoch == record.binding.state_epoch
            && admission.resource_generation == record.binding.resource_generation
            && admission.decision_expiry == record.decision_expires_unix_ms,
        "EPERM standing-correlation-mismatch"
    );
    ensure!(
        record.intent_sha256
            == record
                .binding
                .intent_sha256()
                .map_err(|error| anyhow!("{error}"))?
            && record.binding.units == 1
            && spec.id == record.binding.ticket_id
            && spec.idempotency_key == record.binding.idempotency_key
            && spec.action == record.binding.action
            && spec.expires_unix_ms == Some(record.binding.deadline_unix_ms),
        "EPERM standing-ticket-identity"
    );
    let input_sha256 = match (spec.schema.as_str(), spec.action.as_str()) {
        (HOST_TICKET_V1_SCHEMA, "systemd.restart") => {
            ensure!(
                spec.target.as_deref() == Some(record.binding.target.as_str()),
                "EPERM standing-service-target"
            );
            hex::encode(Sha256::digest(serde_json::to_vec(&spec.args)?))
        }
        (HOST_TICKET_V2_SCHEMA, "gpu.workload.submit") => {
            let gpu_id = spec
                .subject_ref
                .as_deref()
                .ok_or_else(|| anyhow!("EPERM standing-gpu-subject"))?;
            ensure!(
                record.binding.target == format!("/gpu/{gpu_id}/workload") && spec.target.is_none(),
                "EPERM standing-gpu-target"
            );
            request_hash(&spec.args)?
        }
        (HOST_TICKET_V2_SCHEMA, "peft.release") => {
            let model_id = spec
                .subject_ref
                .as_deref()
                .ok_or_else(|| anyhow!("EPERM standing-release-subject"))?;
            cohesix_authority::validate_id(model_id)
                .map_err(|_| anyhow!("EPERM standing-release-subject"))?;
            ensure!(
                record.binding.target == format!("/models/{model_id}/release")
                    && spec.target.is_none(),
                "EPERM standing-release-target"
            );
            request_hash(&spec.args)?
        }
        _ => return Err(anyhow!("EPERM unsupported-standing-action")),
    };
    ensure!(
        record.binding.input_sha256 == input_sha256,
        "EPERM standing-input-mismatch"
    );
    Ok(())
}

fn request_hash(args: &Value) -> Result<String> {
    args.get("request_sha256")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("EPERM standing-gpu-input"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cohesix_authority::standing::{
        JobBinding, StandingControls, StandingScope, JOB_BINDING_SCHEMA, STANDING_CONTROLS_SCHEMA,
        STANDING_SCOPE_SCHEMA,
    };
    use cohesix_authority::standing_ledger::{
        JobDelivery, JobExecution, ReserveOutcome, StandingLedger,
    };
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Arc;

    fn service() -> (HostTicketSpec, JobRecord) {
        let args = serde_json::json!({"unit":"cohesix-agent.service"});
        let binding = JobBinding {
            schema: JOB_BINDING_SCHEMA.into(),
            scope_id: "scope-1".into(),
            admission_id: "admit-1".into(),
            ticket_id: "ticket-1".into(),
            idempotency_key: "once-1".into(),
            subject: "operator".into(),
            action: "systemd.restart".into(),
            target: "/host/systemd/cohesix-agent.service/restart".into(),
            input_sha256: hex::encode(Sha256::digest(serde_json::to_vec(&args).unwrap())),
            policy_sha256: "a".repeat(64),
            state_epoch: 1,
            resource_generation: 2,
            deadline_unix_ms: 2000,
            units: 1,
            attempt: 1,
        };
        let digest = binding.intent_sha256().unwrap();
        let record = JobRecord {
            binding,
            intent_sha256: digest.clone(),
            scope_generation: 1,
            decision_expires_unix_ms: 1900,
            reserved_unix_ms: 1800,
            dispatched_unix_ms: None,
            execution: JobExecution::Reserved,
            delivery: JobDelivery::Pending,
            result_sha256: None,
            cancel_requested: false,
        };
        let spec = HostTicketSpec {
            schema: HOST_TICKET_V1_SCHEMA.into(),
            id: "ticket-1".into(),
            idempotency_key: "once-1".into(),
            action: "systemd.restart".into(),
            target: Some("/host/systemd/cohesix-agent.service/restart".into()),
            args,
            expires_unix_ms: Some(2000),
            admission: Some(cohesix_authority::AdmissionCorrelation {
                admission_id: "admit-1".into(),
                intent_hash: digest,
                policy_hash: "a".repeat(64),
                state_epoch: 1,
                resource_generation: 2,
                decision_expiry: 1900,
                standing_scope_id: Some("scope-1".into()),
            }),
            ..HostTicketSpec::default()
        };
        (spec, record)
    }

    #[test]
    fn service_identity_refuses_changed_target_input_and_subject_correlation() {
        let (mut spec, record) = service();
        verify_ticket(&spec, &record).expect("selected job");
        spec.target = Some("/host/systemd/ssh.service/restart".into());
        assert!(verify_ticket(&spec, &record).is_err());
        spec.target = Some(record.binding.target.clone());
        spec.args = serde_json::json!({"unit":"ssh.service"});
        assert!(verify_ticket(&spec, &record).is_err());
        spec.args = serde_json::json!({"unit":"cohesix-agent.service"});
        spec.admission.as_mut().unwrap().standing_scope_id = None;
        assert!(verify_ticket(&spec, &record).is_err());
    }

    #[test]
    fn caller_cannot_choose_a_different_budget_cost() {
        let (mut spec, mut record) = service();
        record.binding.units = 2;
        record.intent_sha256 = record.binding.intent_sha256().unwrap();
        spec.admission.as_mut().unwrap().intent_hash = record.intent_sha256.clone();
        let error = verify_ticket(&spec, &record).expect_err("selected action has fixed cost");
        assert!(error.to_string().contains("standing-ticket-identity"));
    }

    #[test]
    fn gpu_identity_uses_cas_digest_and_exact_published_subject() {
        let (mut spec, mut record) = service();
        spec.schema = HOST_TICKET_V2_SCHEMA.into();
        spec.action = "gpu.workload.submit".into();
        spec.target = None;
        spec.subject_ref = Some("GPU-0".into());
        spec.args = serde_json::json!({"request_sha256":"b".repeat(64)});
        record.binding.action = spec.action.clone();
        record.binding.target = "/gpu/GPU-0/workload".into();
        record.binding.input_sha256 = "b".repeat(64);
        record.intent_sha256 = record.binding.intent_sha256().unwrap();
        let admission = spec.admission.as_mut().unwrap();
        admission.intent_hash = record.intent_sha256.clone();
        verify_ticket(&spec, &record).expect("GPU selected job");
        spec.subject_ref = Some("GPU-1".into());
        assert!(verify_ticket(&spec, &record).is_err());
    }

    #[test]
    fn peft_release_identity_requires_exact_model_and_request() {
        let (mut spec, mut record) = service();
        spec.schema = HOST_TICKET_V2_SCHEMA.into();
        spec.action = "peft.release".into();
        spec.target = None;
        spec.subject_ref = Some("local-model".into());
        spec.args = serde_json::json!({"request_sha256":"b".repeat(64)});
        record.binding.action = spec.action.clone();
        record.binding.target = "/models/local-model/release".into();
        record.binding.input_sha256 = "b".repeat(64);
        record.intent_sha256 = record.binding.intent_sha256().unwrap();
        spec.admission.as_mut().unwrap().intent_hash = record.intent_sha256.clone();
        verify_ticket(&spec, &record).expect("selected release identity");
        spec.subject_ref = Some("other-model".into());
        assert!(verify_ticket(&spec, &record).is_err());
        spec.subject_ref = Some("local-model".into());
        spec.args = serde_json::json!({"request_sha256":"c".repeat(64)});
        assert!(verify_ticket(&spec, &record).is_err());
    }

    #[test]
    fn release_fence_tracks_generation_and_served_bytes() {
        let mut incumbent = serde_json::json!({
            "generation": 0,
            "adapter_sha256": null,
            "served_artifact_sha256": "a".repeat(64),
            "runtime_sha256": "b".repeat(64),
            "healthy": true,
            "rollback_verified": true,
        });
        let original = release_generations(&incumbent).expect("verified incumbent");
        incumbent["generation"] = serde_json::json!(1);
        let next = release_generations(&incumbent).expect("new generation");
        assert_ne!(next.0, original.0);
        assert_eq!(next.1, original.1);
        incumbent["served_artifact_sha256"] = serde_json::json!("c".repeat(64));
        assert_ne!(release_generations(&incumbent).unwrap().1, original.1);
        incumbent["healthy"] = serde_json::json!(false);
        assert!(release_generations(&incumbent).is_err());
        incumbent["healthy"] = serde_json::json!(true);
        incumbent["unrecognized"] = serde_json::json!(true);
        assert!(release_generations(&incumbent).is_err());
    }

    #[test]
    fn interrupted_delivery_retains_one_native_result_and_refuses_redispatch() {
        let dir = tempfile::tempdir().expect("state directory");
        #[cfg(unix)]
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))
            .expect("private state");
        let (spec, record) = service();
        let controls = StandingControls {
            schema: STANDING_CONTROLS_SCHEMA.into(),
            enabled: true,
            actions: vec!["systemd.restart".into()],
            max_scopes: 1,
            max_jobs: 2,
            max_job_units: 1,
            max_total_units: 2,
            max_concurrent: 1,
            max_retries: 1,
            max_cooldown_ms: 0,
            max_fact_age_ms: 100,
            max_decision_ttl_ms: 100,
        };
        let scope = StandingScope {
            schema: STANDING_SCOPE_SCHEMA.into(),
            id: record.binding.scope_id.clone(),
            subject: record.binding.subject.clone(),
            action: record.binding.action.clone(),
            target: record.binding.target.clone(),
            policy_sha256: record.binding.policy_sha256.clone(),
            generation: 1,
            expires_unix_ms: 2500,
            max_job_units: 1,
            max_total_units: 2,
            max_concurrent: 1,
            max_retries: 1,
            cooldown_ms: 0,
            fact_max_age_ms: 100,
            decision_ttl_ms: 100,
        };
        let path = dir.path().join("standing.json");
        let ledger = StandingLedger::new(
            path.clone(),
            record.binding.policy_sha256.clone(),
            controls.clone(),
            vec![scope.clone()],
        )
        .expect("selected policy");
        ledger.initialize().expect("provision");
        let facts = AdmissionFacts {
            observed_unix_ms: 1800,
            state_epoch: 1,
            resource_generation: 2,
            policy_sha256: record.binding.policy_sha256.clone(),
        };
        assert!(matches!(
            ledger.reserve(record.binding.clone(), &facts, 1800),
            Ok(ReserveOutcome::Reserved(_))
        ));
        let config = ExecutorConfig {
            standing_ledger: Some(Arc::new(ledger)),
            ..ExecutorConfig::default()
        };
        begin_dispatch(&spec, &config, &facts, 1801).expect("fresh dispatch");
        let result = serde_json::json!({
            "schema":"host-ticket-result/v1", "id":spec.id,
            "idempotency_key":spec.idempotency_key, "action":spec.action,
            "state":"succeeded", "message":"signed native result retained"
        })
        .to_string();
        settle_result(&spec, &config, &result, true).expect("settle once");
        let recovered =
            StandingLedger::new(path, record.binding.policy_sha256, controls, vec![scope])
                .expect("same policy");
        let recovered_config = ExecutorConfig {
            standing_ledger: Some(Arc::new(recovered)),
            ..ExecutorConfig::default()
        };
        let pending = selected_record(&spec, &recovered_config)
            .expect("status")
            .expect("retained job");
        assert_eq!(pending.execution, JobExecution::Confirmed);
        assert_eq!(pending.delivery, JobDelivery::Pending);
        assert!(begin_dispatch(&spec, &recovered_config, &facts, 1802).is_err());
        acknowledge_delivery(&spec, &recovered_config).expect("delivery retry");
        let delivered = selected_record(&spec, &recovered_config).unwrap().unwrap();
        assert_eq!(delivered.delivery, JobDelivery::Acknowledged);
    }
}
