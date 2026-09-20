// Author: Lukas Bower
// Purpose: Forward only root-admitted GPU workloads to the authenticated bridge while rechecking exact Worker and lease state until terminal reaping.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]

use super::{provider_pending, ExecutorConfig, ReconcileOutcome};
use crate::{HostTicketSpec, HOST_TICKET_V2_SCHEMA};
use anyhow::{anyhow, bail, ensure, Result};
use cohesix_authority::gpu::{WorkloadControl, WorkloadSubmit};
use cohsh::{Session, Transport};
use gpu_bridge_host::workload::{self, Binding, Command, Input, Job};
use gpu_bridge_host::PublishedDevice;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

fn binding(spec: &HostTicketSpec) -> Result<Binding> {
    ensure!(
        spec.schema == HOST_TICKET_V2_SCHEMA,
        "GPU workload requires root-admitted host-ticket/v2"
    );
    crate::claim::validate_spec(spec, crate::claim::SpecSource::AdmittedSnapshot)?;
    Ok(Binding {
        ticket_id: spec.id.clone(),
        idempotency_key: spec.idempotency_key.clone(),
        action: spec.action.clone(),
        operation_id: spec
            .operation_id
            .clone()
            .ok_or_else(|| anyhow!("operation_id required"))?,
        gpu_id: spec
            .subject_ref
            .clone()
            .ok_or_else(|| anyhow!("subject_ref required"))?,
        worker_id: spec
            .receipt_worker_id
            .clone()
            .ok_or_else(|| anyhow!("receipt Worker required"))?,
        worker_slot: spec
            .resolved_worker_slot
            .ok_or_else(|| anyhow!("root slot required"))?,
        lease_epoch: spec
            .resolved_lease_epoch
            .ok_or_else(|| anyhow!("root lease epoch required"))?,
        supervisor_generation: spec
            .receipt_supervisor_generation
            .ok_or_else(|| anyhow!("Worker generation required"))?,
        cap_generation: spec
            .receipt_cap_generation
            .ok_or_else(|| anyhow!("Worker capability generation required"))?,
        admission_sequence: spec
            .admission_sequence
            .ok_or_else(|| anyhow!("root admission required"))?,
        writer_epoch: spec
            .writer_epoch
            .ok_or_else(|| anyhow!("writer epoch required"))?,
        expires_unix_ms: spec
            .expires_unix_ms
            .ok_or_else(|| anyhow!("finite workload ticket expiry required"))?,
        provider_graph_sha256: cohesix_authority::provider::registry()?["graph_sha256"]
            .as_str()
            .ok_or_else(|| anyhow!("registry graph required"))?
            .into(),
    })
}
fn endpoint(config: &ExecutorConfig) -> Result<(&Path, &str)> {
    let socket = config
        .gpu_executor_socket
        .as_deref()
        .ok_or_else(|| anyhow!("not_enabled gpu.workload executor"))?;
    let key = config
        .gpu_executor_credential_ref
        .as_deref()
        .ok_or_else(|| anyhow!("not_enabled gpu.workload credential"))?;
    ensure!(socket.is_absolute(), "absolute GPU socket required");
    Ok((socket, key))
}
fn call(config: &ExecutorConfig, command: Command) -> Result<Job> {
    let (socket, key) = endpoint(config)?;
    workload::call(socket, key, command)
}
fn parse_lease(lines: &[String], lease: &str, binding: &Binding) -> Result<u64> {
    ensure!(
        lines.len() == 1 && lines[0].len() <= 1024,
        "lease unavailable or ambiguous"
    );
    let mut fields = BTreeMap::new();
    for token in lines[0].split_whitespace() {
        let (key, value) = token
            .split_once('=')
            .ok_or_else(|| anyhow!("invalid lease record"))?;
        ensure!(fields.insert(key, value).is_none(), "duplicate lease field");
    }
    ensure!(
        fields.get("id") == Some(&lease)
            && fields.get("resource") == Some(&binding.gpu_id.as_str())
            && fields.get("subject") == Some(&binding.worker_id.as_str())
            && fields.get("state") == Some(&"ACTIVE"),
        "lease binding revoked or changed"
    );
    let sequence = fields
        .get("seq")
        .ok_or_else(|| anyhow!("lease sequence missing"))?
        .parse::<u64>()?;
    ensure!(sequence > 0, "lease sequence invalid");
    Ok(sequence)
}
fn check_lease(
    transport: &mut dyn Transport,
    session: &Session,
    spec: &HostTicketSpec,
    lease: &str,
    binding: &Binding,
) -> Result<u64> {
    crate::validate_ready_worker_binding(transport, session, spec)?;
    ensure!(
        workload::now_ms()? < binding.expires_unix_ms,
        "workload ticket expired"
    );
    parse_lease(
        &transport.read(session, &format!("/proc/lease/by-id/{lease}"))?,
        lease,
        binding,
    )
}

fn parse_inventory(lines: &[String], gpu_id: &str) -> Result<PublishedDevice> {
    #[derive(serde::Deserialize)]
    struct Projection {
        id: String,
        execution_identity: PublishedDevice,
    }
    let bytes = lines.join("\n");
    ensure!(bytes.len() <= 8192, "GPU inventory projection bound");
    let projection: Projection = serde_json::from_str(&bytes)
        .map_err(|_| anyhow!("not_enabled exact native GPU publication required"))?;
    let identity = projection.execution_identity;
    ensure!(
        projection.id == gpu_id
            && identity.schema == "cohesix-gpu-device/v1"
            && identity.source_id == "gpu-bridge-host/cuda-reference"
            && identity.source_mode == "production"
            && identity.source_epoch > 0,
        "GPU publication identity or mode mismatch"
    );
    Ok(identity)
}

fn inventory(
    transport: &mut dyn Transport,
    session: &Session,
    binding: &Binding,
) -> Result<PublishedDevice> {
    // Root withdraws this entry on snapshot expiry. The embedded publisher epoch
    // avoids racing multipart refresh status while still fencing replacement.
    parse_inventory(
        &transport.read(session, &format!("/gpu/{}/info", binding.gpu_id))?,
        &binding.gpu_id,
    )
}

fn bind_inventory(identity: &PublishedDevice, input: &Input) -> Result<()> {
    ensure!(
        identity.device_uuid == input.request.device_uuid
            && identity.device_ordinal == input.request.device_ordinal
            && identity.topology_sha256 == input.topology_sha256
            && identity.helper_sha256 == input.artifact_sha256
            && identity.provider_graph_sha256 == input.request.provider_graph_sha256,
        "GPU request differs from root-published device topology"
    );
    Ok(())
}
#[derive(Debug, PartialEq, Eq)]
struct ReservedResources {
    memory_bytes: u64,
    streams: u64,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ReservationRecord {
    schema: String,
    state: String,
    gpu_id: String,
    worker_id: String,
    mem_mb: u64,
    streams: u64,
    ttl_s: u32,
    #[serde(rename = "priority")]
    _priority: u8,
}

fn parse_resources(lines: &[String], gpu_id: &str, worker_id: &str) -> Result<ReservedResources> {
    ensure!(
        lines.len() <= 32 && lines.iter().all(|line| line.len() < 256),
        "GPU reservation projection bound"
    );
    for line in lines.iter().rev() {
        // A typed record rejects duplicate keys and unknown fields before any
        // newest-Worker selection; no JSON last-value-wins authority is accepted.
        let row: ReservationRecord = serde_json::from_str(line)?;
        if row.worker_id != worker_id {
            continue;
        }
        ensure!(
            row.schema == "gpu-lease/v1"
                && row.state == "ACTIVE"
                && row.gpu_id == gpu_id
                && row.ttl_s > 0,
            "GPU reservation inactive or rebound"
        );
        let memory_bytes = row
            .mem_mb
            .checked_mul(1024 * 1024)
            .filter(|value| *value > 0)
            .ok_or_else(|| anyhow!("GPU memory reservation missing"))?;
        ensure!(row.streams > 0, "GPU stream reservation missing");
        let streams = row.streams;
        return Ok(ReservedResources {
            memory_bytes,
            streams,
        });
    }
    bail!("GPU Worker resource reservation missing")
}

fn resources(
    transport: &mut dyn Transport,
    session: &Session,
    binding: &Binding,
) -> Result<ReservedResources> {
    parse_resources(
        &transport.read(session, &format!("/gpu/{}/lease", binding.gpu_id))?,
        &binding.gpu_id,
        &binding.worker_id,
    )
}

fn terminal(config: &ExecutorConfig, spec: &HostTicketSpec, job: &Job) -> Result<String> {
    let observed = job
        .terminal_unix_ms
        .ok_or_else(|| anyhow!("GPU child not yet reaped"))?;
    ensure!(
        job.binding == binding(spec)?,
        "GPU terminal binding changed"
    );
    let outcome = match job.state.as_str() {
        "succeeded" => cohesix_evidence::Outcome::Verified,
        "cancelled" | "revoked" => cohesix_evidence::Outcome::Cancelled,
        "failed" | "interrupted" => cohesix_evidence::Outcome::Failed,
        _ => return Err(anyhow!("invalid GPU terminal state")),
    };
    let reference = super::observation::retain_native_outcome_at(
        config,
        spec,
        job,
        &format!("gpu-job:{}:{}", job.binding.ticket_id, job.input_sha256),
        observed,
        outcome,
    )?;
    if job.state == "succeeded" {
        Ok(reference)
    } else {
        bail!("gpu_workload_{} {reference}", job.state)
    }
}

/// The bridge cancels within its finite grant TTL if this supervisor loses contact.
pub fn execute(
    transport: &mut dyn Transport,
    session: &Session,
    spec: &HostTicketSpec,
    config: &ExecutorConfig,
) -> Result<String> {
    let binding = binding(spec)?;
    endpoint(config)?;
    super::observation::prepare(config)?;
    crate::validate_ready_worker_binding(transport, session, spec)?;
    if spec.action != "gpu.workload.submit" {
        let args: WorkloadControl = serde_json::from_value(spec.args.clone())?;
        let deadline = Instant::now() + Duration::from_secs(12);
        let mut dispatched = false;
        loop {
            let command = if dispatched {
                Command::ControlStatus {
                    binding: binding.clone(),
                    job_id: args.job_id.clone(),
                }
            } else if spec.action == "gpu.workload.cancel" {
                Command::Cancel {
                    binding: binding.clone(),
                    job_id: args.job_id.clone(),
                }
            } else {
                Command::Observe {
                    binding: binding.clone(),
                    job_id: args.job_id.clone(),
                }
            };
            let job = call(config, command)?;
            dispatched = true;
            if job.terminal_unix_ms.is_some() || spec.action == "gpu.workload.observe" {
                // An observe receipt proves observation; it never changes the original job outcome.
                return control_observation(config, spec, &job);
            }
            ensure!(
                Instant::now() < deadline,
                "GPU cancellation awaiting native reaping"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    let args: WorkloadSubmit = serde_json::from_value(spec.args.clone())?;
    let root = config
        .gpu_request_root
        .as_deref()
        .ok_or_else(|| anyhow!("not_enabled GPU request CAS"))?;
    let bytes = workload::read_file(&root.join(format!("{}.json", args.request_sha256)), 8192)?;
    ensure!(
        workload::digest(&bytes) == args.request_sha256,
        "GPU request CAS digest mismatch"
    );
    let input: Input = serde_json::from_slice(&bytes)?;
    ensure!(
        serde_json::to_vec(&input)? == bytes,
        "GPU request CAS must use canonical encoding"
    );
    let sequence = check_lease(transport, session, spec, &args.lease_id, &binding)?;
    let selected_inventory = inventory(transport, session, &binding)?;
    bind_inventory(&selected_inventory, &input)?;
    let selected_resources = resources(transport, session, &binding)?;
    ensure!(
        input.request.memory_budget_bytes <= selected_resources.memory_bytes,
        "GPU workload exceeds Worker memory reservation"
    );
    let job = call(
        config,
        Command::Submit {
            binding: binding.clone(),
            lease_id: args.lease_id.clone(),
            lease_sequence: sequence,
            request_sha256: args.request_sha256,
            input: Box::new(input),
        },
    )
    .map_err(submit_error)?;
    if job.terminal_unix_ms.is_some() {
        return terminal(config, spec, &job);
    }
    let deadline = Instant::now() + Duration::from_secs(45);
    let mut serial = 0u64;
    let mut revoked = false;
    loop {
        ensure!(Instant::now() < deadline, "GPU executor terminal deadline");
        if !revoked {
            if check_lease(transport, session, spec, &args.lease_id, &binding).ok()
                != Some(sequence)
                || inventory(transport, session, &binding).ok().as_ref()
                    != Some(&selected_inventory)
                || resources(transport, session, &binding).ok().as_ref()
                    != Some(&selected_resources)
            {
                revoked = true;
                let _ = call(
                    config,
                    Command::Revoke {
                        binding: binding.clone(),
                    },
                );
            } else {
                serial = serial
                    .checked_add(1)
                    .ok_or_else(|| anyhow!("heartbeat sequence overflow"))?;
                let _ = call(
                    config,
                    Command::Renew {
                        binding: binding.clone(),
                        lease_id: args.lease_id.clone(),
                        lease_sequence: sequence,
                        serial,
                    },
                );
            }
        }
        let job = call(
            config,
            Command::Status {
                binding: binding.clone(),
            },
        )
        .map_err(|_| {
            provider_pending("GPU bridge disconnected; finite grant expires without replay")
        })?;
        if job.terminal_unix_ms.is_some() {
            return terminal(config, spec, &job);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn submit_error(error: anyhow::Error) -> anyhow::Error {
    if error
        .downcast_ref::<workload::Refusal>()
        .is_some_and(workload::Refusal::before_dispatch)
    {
        error
    } else {
        provider_pending("GPU submit outcome requires durable bridge reconciliation")
    }
}

/// Agent restart revokes an in-flight job and consumes its retained result, never resubmits.
pub fn reconcile(spec: &HostTicketSpec, config: &ExecutorConfig) -> Result<ReconcileOutcome> {
    if spec.action != "gpu.workload.submit" {
        if let Some(reference) = crate::causal::retained_success(config, spec)? {
            return Ok(ReconcileOutcome::Committed(reference));
        }
        let args: WorkloadControl = serde_json::from_value(spec.args.clone())?;
        let job = call(
            config,
            Command::ControlStatus {
                binding: binding(spec)?,
                job_id: args.job_id,
            },
        )?;
        if spec.action == "gpu.workload.cancel" && job.terminal_unix_ms.is_none() {
            return Ok(ReconcileOutcome::Ambiguous);
        }
        return Ok(ReconcileOutcome::Committed(control_observation(
            config, spec, &job,
        )?));
    }
    let job = match call(
        config,
        Command::Revoke {
            binding: binding(spec)?,
        },
    ) {
        Ok(job) => job,
        Err(_) => return Ok(ReconcileOutcome::Ambiguous),
    };
    if job.terminal_unix_ms.is_none() {
        return Ok(ReconcileOutcome::Ambiguous);
    }
    match terminal(config, spec, &job) {
        Ok(reference) => Ok(ReconcileOutcome::Committed(reference)),
        Err(error) => Ok(ReconcileOutcome::Rejected(error.to_string())),
    }
}

fn control_observation(
    config: &ExecutorConfig,
    spec: &HostTicketSpec,
    job: &Job,
) -> Result<String> {
    let args: WorkloadControl = serde_json::from_value(spec.args.clone())?;
    let admitted = binding(spec)?;
    ensure!(
        job.binding.ticket_id == args.job_id
            && job.binding.gpu_id == admitted.gpu_id
            && job.binding.worker_id == admitted.worker_id
            && job.binding.worker_slot == admitted.worker_slot
            && job.binding.lease_epoch == admitted.lease_epoch
            && job.binding.supervisor_generation == admitted.supervisor_generation
            && job.binding.cap_generation == admitted.cap_generation
            && job.binding.writer_epoch == admitted.writer_epoch,
        "GPU control observed another job or Worker generation"
    );
    // Observe succeeds by witnessing state; cancel succeeds only after reaping.
    // Neither changes the original submit operation's terminal outcome.
    ensure!(
        spec.action == "gpu.workload.observe" || job.terminal_unix_ms.is_some(),
        "GPU cancel awaiting reaping"
    );
    super::observation::retain_native_at(
        config,
        spec,
        job,
        &format!("gpu-job:{}:{}", job.binding.ticket_id, job.input_sha256),
        job.terminal_unix_ms.unwrap_or(crate::unix_time_ms_now()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unverified_errors_remain_pending_instead_of_claiming_refusal() {
        for message in [
            "GPU executor refusal: device_busy",
            "response_unauthenticated",
            "local_deadline",
            "connection reset",
        ] {
            assert!(super::super::is_provider_pending(&submit_error(anyhow!(
                message.to_owned()
            ))));
        }
    }
    #[test]
    fn worker_resource_reservations_are_exact_and_checked() {
        let row = r#"{"schema":"gpu-lease/v1","state":"ACTIVE","gpu_id":"GPU-0","worker_id":"worker-1","mem_mb":2,"streams":1,"ttl_s":3600,"priority":0}"#;
        assert_eq!(
            parse_resources(&[row.into()], "GPU-0", "worker-1").unwrap(),
            ReservedResources {
                memory_bytes: 2097152,
                streams: 1
            }
        );
        for invalid in [
            row.replace("ACTIVE", "RELEASED"),
            row.replace("\"mem_mb\":2", "\"mem_mb\":18446744073709551615"),
            row.replace("\"streams\":1", "\"streams\":0"),
            row.replace("\"streams\":1", "\"streams\":1,\"streams\":2"),
            row.replace("\"streams\":1", "\"streams\":1,\"extra\":true"),
            row.replace("\"ttl_s\":3600", "\"ttl_s\":0"),
            row.replace("\"priority\":0", "\"priority\":256"),
        ] {
            assert!(parse_resources(&[invalid], "GPU-0", "worker-1").is_err());
        }
        assert!(parse_resources(&[row.into()], "GPU-0", "worker-2").is_err());
    }
    #[test]
    fn publication_requires_native_identity_and_request_topology() {
        let mut projection = serde_json::json!({
            "id":"GPU-0", "execution_identity":{
                "schema":"cohesix-gpu-device/v1", "device_uuid":"ab".repeat(16),
                "device_ordinal":0, "topology_sha256":"cd".repeat(32),
                "helper_sha256":"ef".repeat(32), "provider_graph_sha256":"01".repeat(32),
                "source_id":"gpu-bridge-host/cuda-reference", "source_epoch":7,
                "source_mode":"production"
            }
        });
        let selected = parse_inventory(&[projection.to_string()], "GPU-0").unwrap();
        let mut input = Input {
            schema: "cohesix-gpu-workload-input/v1".into(),
            artifact_sha256: "ef".repeat(32),
            topology_sha256: "cd".repeat(32),
            expected_output_sha256: "02".repeat(32),
            request: gpu_bridge_host::reference::ReferenceRequest {
                schema: "cohesix-cuda-reference-request/v1".into(),
                ticket_id: "ticket".into(),
                entrypoint: gpu_bridge_host::reference::Entrypoint::Vadd,
                dimension: 64,
                iterations: 1,
                device_ordinal: 0,
                device_uuid: "ab".repeat(16),
                inventory_observed_unix_ms: 1000,
                provider_graph_sha256: "01".repeat(32),
                memory_budget_bytes: 1048576,
                deadline_ms: 30000,
            },
        };
        bind_inventory(&selected, &input).unwrap();
        input.topology_sha256 = "03".repeat(32);
        assert!(bind_inventory(&selected, &input).is_err());
        projection["execution_identity"]["source_mode"] = "fixture".into();
        assert!(parse_inventory(&[projection.to_string()], "GPU-0").is_err());
        assert!(parse_inventory(&[r#"{"id":"GPU-0"}"#.into()], "GPU-0").is_err());
    }

    #[test]
    fn lease_identity_and_generation_must_match_exactly() {
        let binding = Binding {
            ticket_id: "job".into(),
            idempotency_key: "once".into(),
            action: "gpu.workload.submit".into(),
            operation_id: "work".into(),
            gpu_id: "GPU-0".into(),
            worker_id: "gpu-worker-1".into(),
            worker_slot: 0,
            lease_epoch: 1,
            supervisor_generation: 1,
            cap_generation: 1,
            admission_sequence: 1,
            writer_epoch: 1,
            expires_unix_ms: 9000,
            provider_graph_sha256: "a".repeat(64),
        };
        let line =
            "id=lease-1 subject=gpu-worker-1 resource=GPU-0 ttl_s=30 priority=0 state=ACTIVE seq=7";
        assert_eq!(parse_lease(&[line.into()], "lease-1", &binding).unwrap(), 7);
        for bad in [
            line.replace("GPU-0", "GPU-1"),
            line.replace("ACTIVE", "EXPIRED"),
            line.replace("gpu-worker-1", "other"),
            format!("{line} seq=8"),
        ] {
            assert!(parse_lease(&[bad], "lease-1", &binding).is_err());
        }
        assert!(parse_lease(&[], "lease-1", &binding).is_err());
    }
}
