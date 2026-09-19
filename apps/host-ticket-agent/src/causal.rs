// Author: Lukas Bower
// Purpose: Extend verified gateway admission chains with native provider observations and exact Root terminal readback under agent custody.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]

use crate::{executors::ExecutorConfig, unix_time_ms_now, HostTicketResult, HostTicketSpec};
use anyhow::{anyhow, ensure, Result};
use cohesix_evidence::producer::{Custody, Operation};
use cohesix_evidence::ticket::{caller_request, require_binding};
use cohesix_evidence::{Kind, Outcome};
use cohsh::{Session, Transport};

pub(crate) fn open(config: &ExecutorConfig, spec: &HostTicketSpec) -> Result<Option<Operation>> {
    let Some(directory) = &config.evidence_enrollment_dir else {
        return Ok(None);
    };
    ensure!(
        (spec.schema == "host-ticket/v1" || spec.schema == "host-ticket/v2")
            && (spec.action == "peft.release"
                || [
                    "systemd.",
                    "launchd.",
                    "mac_release.",
                    "endpoint_compliance.",
                    "docker.",
                    "k8s.",
                    "modbus.",
                    "dnp3.",
                    "gpu.workload."
                ]
                .iter()
                .any(|prefix| spec.action.starts_with(prefix))),
        "not_supported native signed provider action"
    );
    let operation = Operation::open(
        directory,
        &spec.id,
        &spec.idempotency_key,
        Custody::NativeOperation,
        unix_time_ms_now(),
    )?;
    operation.bind(
        &spec.action,
        spec.writer_epoch
            .ok_or_else(|| anyhow!("EPERM evidence writer epoch"))?,
        cohsh::CohshPolicy::manifest_hash(),
        "host-ticket-agent",
        config
            .evidence_executable_sha256
            .as_deref()
            .ok_or_else(|| anyhow!("EPERM evidence component measurement"))?,
    )?;
    require_binding(
        &operation.trust.expected,
        &serde_json::to_value(spec)?,
        spec.schema == "host-ticket/v2",
    )?;
    Ok(Some(operation))
}

/// Fail before native I/O if the admitted request lacks an independently verified grant.
pub fn preflight(config: &ExecutorConfig, spec: &HostTicketSpec) -> Result<()> {
    let Some(operation) = open(config, spec)? else {
        return Ok(());
    };
    ensure!(
        operation.record(Kind::Grant).is_some() && operation.record(Kind::Execution).is_none(),
        "EPERM evidence grant missing or native execution requires reconciliation"
    );
    let timeout = cohesix_authority::provider::action(&spec.action)?["timeout_ms"]
        .as_u64()
        .ok_or_else(|| anyhow!("EPERM evidence provider deadline"))?;
    ensure!(
        operation
            .expires_unix_ms()
            .saturating_sub(unix_time_ms_now())
            >= timeout.saturating_add(1000)
            && spec
                .expires_unix_ms
                .is_none_or(|expiry| operation.expires_unix_ms() <= expiry),
        "EPERM evidence deadline cannot cover native observation"
    );
    // The request fields must be bound by the gateway's signed intent artifact.
    let admitted = serde_json::to_value(spec)?;
    let v2 = spec.schema == "host-ticket/v2";
    operation.require_json(Kind::Intent, &caller_request(&admitted, v2)?, true)?;
    let grant = operation.read_json(Kind::Grant)?;
    let grant_admitted = &grant["admitted"];
    ensure!(
        if v2 {
            *grant_admitted == admitted
        } else {
            caller_request(grant_admitted, false)? == admitted
        },
        "EPERM signed grant differs from Root admission"
    );
    let gateway_key = operation.phase_public_key(Kind::Grant)?.to_owned();
    ensure!(
        operation.public_key() != gateway_key,
        "EPERM native and gateway keys must differ"
    );
    if v2 {
        let expected = operation.trust.expected.clone();
        let native_key = operation.public_key().to_owned();
        drop(operation);
        let witness = open_worker(config, spec)?;
        ensure!(
            witness.public_key() != native_key && witness.public_key() != gateway_key,
            "EPERM Worker witness requires separate key custody"
        );
        ensure!(
            witness.trust.expected == expected,
            "EPERM witness enrollment mismatch"
        );
        let grant = witness.read_json(Kind::Grant)?;
        let before = grant["worker_current"]
            .as_array()
            .filter(|rows| rows.len() == 1)
            .and_then(|rows| rows[0].as_str())
            .ok_or_else(|| anyhow!("EPERM Worker admission sequence"))?;
        let before = crate::parse_host_ticket_current(before)?;
        require_worker_identity(&before, spec)?;
        ensure!(
            before.state == "pending" && before.sequence[1] == 0,
            "EPERM Worker admission control already in flight"
        );
    }
    Ok(())
}

/// Sign only a provider-owned observed identity and verified postcondition payload.
/// The caller captures pre/post native objects; no ticket field chooses native identity.
pub fn observed(
    config: &ExecutorConfig,
    spec: &HostTicketSpec,
    payload: &[u8],
    native_identity: &str,
) -> Result<()> {
    observed_outcome(config, spec, payload, native_identity, Outcome::Verified)
}

/// Failure outcomes require an actual retained native terminal observation;
/// validation failures and dispatch errors cannot manufacture these phases.
pub fn observed_outcome(
    config: &ExecutorConfig,
    spec: &HostTicketSpec,
    payload: &[u8],
    native_identity: &str,
    outcome: Outcome,
) -> Result<()> {
    ensure!(
        matches!(
            outcome,
            Outcome::Verified | Outcome::Failed | Outcome::Cancelled | Outcome::Expired
        ),
        "invalid native verification outcome"
    );
    let Some(mut operation) = open(config, spec)? else {
        return Ok(());
    };
    ensure!(
        operation.record(Kind::Grant).is_some(),
        "EPERM evidence admission missing"
    );
    // Native generation is the enrolled agent writer epoch; native object incarnation
    // identifiers remain explicit in the identity and hashed operation payload.
    let generation = operation.trust.expected.writer_epoch;
    for (kind, outcome) in [
        (Kind::Execution, Outcome::Observed),
        (Kind::Observation, Outcome::Observed),
        (Kind::Verification, outcome),
    ] {
        operation.emit(
            kind,
            payload,
            Some((native_identity, generation)),
            outcome,
            unix_time_ms_now(),
        )?;
    }
    Ok(())
}

/// Reconciliation may reuse a previously signed native observation, never a
/// new dispatch. The phase artifact and request are reverified by `open`.
pub fn retained_success(config: &ExecutorConfig, spec: &HostTicketSpec) -> Result<Option<String>> {
    let Some(operation) = open(config, spec)? else {
        return Ok(None);
    };
    let Some(verification) = operation.record(Kind::Verification) else {
        return Ok(None);
    };
    ensure!(
        verification.outcome == Outcome::Verified && verification.artifacts.len() == 1,
        "EPERM retained observation is not successful"
    );
    let admitted = serde_json::to_value(spec)?;
    operation.require_json(
        Kind::Intent,
        &caller_request(&admitted, spec.schema == "host-ticket/v2")?,
        true,
    )?;
    ensure!(
        operation.read_json(Kind::Grant)?["admitted"] == admitted,
        "EPERM retained native grant mismatch"
    );
    Ok(Some(format!(
        "native_observation=sha256:{}",
        verification.artifacts[0].sha256
    )))
}

/// Sign terminal only after the Root projection echoes the exact durably retained result.
/// Missing native verification leaves an incomplete chain, which exporters must refuse.
pub fn terminal(
    transport: &mut dyn Transport,
    session: &Session,
    config: &ExecutorConfig,
    spec: &HostTicketSpec,
    path: &str,
    result: &HostTicketResult,
) -> Result<()> {
    if config.evidence_enrollment_dir.is_none() || result.state != "succeeded" {
        return Ok(());
    }
    let Some(mut operation) = open(config, spec)? else {
        return Ok(());
    };
    let verification = operation
        .record(Kind::Verification)
        .ok_or_else(|| anyhow!("unconfirmed native evidence verification absent"))?;
    let identity = verification
        .native_identity
        .clone()
        .ok_or_else(|| anyhow!("EPERM native evidence identity"))?;
    let generation = verification.resource_generation;
    ensure!(
        verification.artifacts.len() == 1
            && result.message.as_deref()
                == Some(
                    format!(
                        "native_observation=sha256:{}",
                        verification.artifacts[0].sha256
                    )
                    .as_str()
                ),
        "EPERM native terminal observation reference"
    );
    let lines = transport.read(session, path)?;
    ensure!(
        exact_result(&lines, result)?,
        "unconfirmed native result readback absent"
    );
    operation.emit(
        Kind::Terminal,
        &serde_json::to_vec(&serde_json::json!({
            "schema":"cohesix-native-terminal-witness/v1", "source":"authenticated-root-console",
            "worker_proof":false, "device_attested":false, "result":result,
        }))?,
        Some((&identity, generation)),
        Outcome::Succeeded,
        unix_time_ms_now(),
    )?;
    Ok(())
}

/// Reconcile a possibly lost result ACK without appending a second terminal row.
pub fn already_published(
    transport: &mut dyn Transport,
    session: &Session,
    config: &ExecutorConfig,
    path: &str,
    result: &HostTicketResult,
) -> Result<bool> {
    if config.evidence_enrollment_dir.is_none() || result.state != "succeeded" {
        return Ok(false);
    }
    exact_result(&transport.read(session, path)?, result)
}

fn exact_result(lines: &[String], result: &HostTicketResult) -> Result<bool> {
    let expected = serde_json::to_value(result)?;
    ensure!(
        lines.len() <= 1024 && lines.iter().map(String::len).sum::<usize>() <= 2 * 1024 * 1024,
        "ELIMIT native result readback"
    );
    let mut matches = 0;
    for line in lines {
        let row: HostTicketResult = serde_json::from_str(line)?;
        if row.id == result.id
            && row.idempotency_key == result.idempotency_key
            && matches!(row.state.as_str(), "succeeded" | "failed" | "expired")
        {
            ensure!(
                serde_json::to_value(row)? == expected,
                "EPERM native result readback changed"
            );
            matches += 1;
        }
    }
    ensure!(
        matches <= 1,
        "unconfirmed native result readback not unique"
    );
    Ok(matches == 1)
}

fn open_worker(config: &ExecutorConfig, spec: &HostTicketSpec) -> Result<Operation> {
    let directory = config
        .worker_evidence_enrollment_dir
        .as_deref()
        .ok_or_else(|| anyhow!("not_enabled separately enrolled Worker witness"))?;
    let operation = Operation::open(
        directory,
        &spec.id,
        &spec.idempotency_key,
        Custody::WorkerWitness,
        unix_time_ms_now(),
    )?;
    operation.bind(
        &spec.action,
        spec.writer_epoch
            .ok_or_else(|| anyhow!("EPERM writer epoch"))?,
        cohsh::CohshPolicy::manifest_hash(),
        "host-ticket-agent",
        config
            .evidence_executable_sha256
            .as_deref()
            .ok_or_else(|| anyhow!("EPERM witness executable"))?,
    )?;
    require_binding(
        &operation.trust.expected,
        &serde_json::to_value(spec)?,
        true,
    )?;
    Ok(operation)
}

fn require_worker_identity(
    current: &crate::HostTicketCurrent,
    spec: &HostTicketSpec,
) -> Result<()> {
    ensure!(
        Some(current.worker_id.as_str()) == spec.receipt_worker_id.as_deref()
            && Some(current.role.as_str()) == spec.receipt_worker_role.as_deref()
            && current.lifecycle == "ready"
            && Some(current.identity[0]) == spec.resolved_worker_slot.map(u64::from)
            && Some(current.identity[1]) == spec.resolved_lease_epoch
            && Some(current.identity[2]) == spec.receipt_supervisor_generation
            && Some(current.identity[3]) == spec.receipt_cap_generation
            && Some(current.admission_sequence) == spec.admission_sequence
            && current.sequence[0] > 0,
        "EPERM witness Root-pinned Worker identity"
    );
    Ok(())
}

fn require_worker_terminal(
    current: &crate::HostTicketCurrent,
    before: &crate::HostTicketCurrent,
    spec: &HostTicketSpec,
    result: &HostTicketResult,
) -> Result<()> {
    require_worker_identity(before, spec)?;
    require_worker_identity(current, spec)?;
    let expected = match result.state.as_str() {
        "succeeded" => "confirmed",
        "failed" => "rejected",
        "expired" => "stale",
        _ => return Err(anyhow!("EPERM nonterminal Worker result")),
    };
    ensure!(
        before.state == "pending" && before.sequence[1] == 0,
        "EPERM witness admission did not precede Worker control"
    );
    ensure!(
        current.state == expected
            && current.sequence[1] == 0
            && current.sequence[2] > before.sequence[2]
            && current.sequence[2] == current.sequence[3],
        "unconfirmed exact Worker terminal receipt"
    );
    Ok(())
}

/// A separately enrolled custodian witnesses Root's exact Worker result. This
/// proves the authenticated Root projection, not an independently attested device.
/// Pending/missing evidence prevents cursor advancement and never replays native work.
pub fn terminal_worker(
    transport: &mut dyn Transport,
    session: &Session,
    config: &ExecutorConfig,
    spec: &HostTicketSpec,
    path: &str,
    result: &HostTicketResult,
) -> Result<()> {
    if config.evidence_enrollment_dir.is_none() {
        return Ok(());
    }
    crate::validate_result_binding(spec, result)?;
    let mut operation = open_worker(config, spec)?;
    // A verified retained terminal survives later Worker lifecycle changes.
    if operation.record(Kind::Terminal).is_some() {
        ensure!(
            operation.read_json(Kind::Terminal)?["result"] == serde_json::to_value(result)?,
            "EPERM retained Worker terminal changed"
        );
        return Ok(());
    }
    let verification = operation
        .record(Kind::Verification)
        .ok_or_else(|| anyhow!("unconfirmed native verification absent"))?
        .clone();
    let outcome = match verification.outcome {
        Outcome::Verified if result.state == "succeeded" => Outcome::Succeeded,
        Outcome::Failed | Outcome::Cancelled if result.state == "failed" => verification.outcome,
        Outcome::Expired if result.state == "expired" || result.state == "failed" => {
            Outcome::Expired
        }
        _ => return Err(anyhow!("EPERM native/Worker outcome mismatch")),
    };
    let identity = verification
        .native_identity
        .as_deref()
        .ok_or_else(|| anyhow!("EPERM native identity"))?;
    ensure!(
        verification.artifacts.len() == 1,
        "EPERM native artifact count"
    );
    let reference = format!(
        "native_observation=sha256:{}",
        verification.artifacts[0].sha256
    );
    ensure!(
        result
            .message
            .as_deref()
            .and_then(|message| message.split_whitespace().last())
            == Some(reference.as_str()),
        "EPERM native terminal reference"
    );
    ensure!(
        exact_result(&transport.read(session, path)?, result)?,
        "unconfirmed Root result readback"
    );
    if operation.record(Kind::Worker).is_some() {
        let retained = operation.read_json(Kind::Worker)?;
        ensure!(
            retained["result"] == serde_json::to_value(result)?
                && retained["admitted"] == serde_json::to_value(spec)?,
            "EPERM retained Worker witness changed"
        );
        return Ok(operation.emit(
            Kind::Terminal,
            &serde_json::to_vec(&retained)?,
            Some((identity, verification.resource_generation)),
            outcome,
            unix_time_ms_now(),
        )?);
    }
    let admitted = operation.read_json(Kind::Grant)?;
    ensure!(
        admitted["admitted"] == serde_json::to_value(spec)?,
        "EPERM Worker grant mismatch"
    );
    let before = admitted["worker_current"]
        .as_array()
        .filter(|rows| rows.len() == 1)
        .and_then(|rows| rows[0].as_str())
        .ok_or_else(|| anyhow!("EPERM Worker pre-execution sequence absent"))?;
    let before = crate::parse_host_ticket_current(before)?;
    let current = crate::read_host_ticket_current(transport, session, spec)?;
    require_worker_terminal(&current, &before, spec, result)?;
    let payload = serde_json::to_vec(&serde_json::json!({
        "schema":"cohesix-worker-terminal-witness/v1", "source":"authenticated-root-console",
        "device_attested":false, "image_binding":"independently-enrolled-selected-root-image",
        "worker":operation.trust.expected.worker, "admitted":admitted["admitted"],
        "admission_current":before, "terminal_current":current, "result":result,
    }))?;
    for kind in [Kind::Worker, Kind::Terminal] {
        operation.emit(
            kind,
            &payload,
            Some((identity, verification.resource_generation)),
            outcome,
            unix_time_ms_now(),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn worker_witness_requires_a_new_completed_receipt_at_every_pinned_generation() {
        let spec = HostTicketSpec {
            schema: "host-ticket/v2".into(),
            id: "one".into(),
            idempotency_key: "once".into(),
            action: "gpu.workload.submit".into(),
            receipt_worker_id: Some("worker-1".into()),
            receipt_worker_role: Some("worker-gpu".into()),
            resolved_worker_slot: Some(0),
            resolved_lease_epoch: Some(4),
            receipt_supervisor_generation: Some(2),
            receipt_cap_generation: Some(3),
            admission_sequence: Some(5),
            ..Default::default()
        };
        let before = crate::parse_host_ticket_current("HOST_TICKET_CURRENT schema=host-ticket-current/v1 state=pending role=worker-gpu worker=worker-1 lifecycle=ready identity=0,4,2,3 sequence=1,0,6,6 admission=5").unwrap();
        let line = "HOST_TICKET_CURRENT schema=host-ticket-current/v1 state=confirmed role=worker-gpu worker=worker-1 lifecycle=ready identity=0,4,2,3 sequence=1,0,7,7 admission=5";
        let mut current = crate::parse_host_ticket_current(line).unwrap();
        let mut result: HostTicketResult = serde_json::from_value(serde_json::json!({
            "schema":"host-ticket-result/v2", "id":"one", "idempotency_key":"once",
            "action":"gpu.workload.submit", "state":"succeeded"}))
        .unwrap();
        require_worker_terminal(&current, &before, &spec, &result).unwrap();
        for changed in [
            line.replace("0,4,2,3", "0,4,2,4"),
            line.replace("1,0,7,7", "1,0,6,6"),
            line.replace("1,0,7,7", "1,7,7,6"),
            line.replace("admission=5", "admission=6"),
            line.replace("state=confirmed", "state=pending"),
            line.replace("worker=worker-1", "worker=worker-2"),
        ] {
            assert!(require_worker_terminal(
                &crate::parse_host_ticket_current(&changed).unwrap(),
                &before,
                &spec,
                &result
            )
            .is_err());
        }
        result.state = "failed".into();
        assert!(require_worker_terminal(&current, &before, &spec, &result).is_err());
        current.state = "rejected".into();
        require_worker_terminal(&current, &before, &spec, &result).unwrap();
        result.state = "expired".into();
        current.state = "stale".into();
        require_worker_terminal(&current, &before, &spec, &result).unwrap();
    }
    #[test]
    fn lost_terminal_ack_requires_exact_single_root_record_before_retry_is_suppressed() {
        let value = serde_json::json!({"schema":"host-ticket-result/v1", "id":"one", "idempotency_key":"once", "action":"systemd.status-check", "state":"succeeded", "writer_epoch":7, "message":"native_observation=sha256:abc"});
        let expected: HostTicketResult = serde_json::from_value(value.clone()).unwrap();
        assert!(!exact_result(&[], &expected).unwrap());
        let line = value.to_string();
        assert!(exact_result(std::slice::from_ref(&line), &expected).unwrap());
        assert!(exact_result(&[line.clone(), line], &expected).is_err());
        let mut changed = value.clone();
        changed["message"] = serde_json::json!("different observation");
        assert!(exact_result(&[changed.to_string()], &expected).is_err());
        changed = value;
        changed["writer_epoch"] = serde_json::json!(6);
        assert!(exact_result(&[changed.to_string()], &expected).is_err());
        changed["writer_epoch"] = serde_json::json!(7);
        changed["state"] = serde_json::json!("failed");
        assert!(exact_result(&[changed.to_string()], &expected).is_err());
    }
}
