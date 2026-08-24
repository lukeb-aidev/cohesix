// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Execute GPU lease ticket actions using existing queen and lease control paths.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use anyhow::{anyhow, Context, Result};
use cohsh::{queen, Session, Transport, CLIENT_QUEEN_LEASE_CTL_PATH};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use super::{
    arg_str, arg_u64, provider_pending, read_last_line, target_components, ExecutorConfig,
    ReconcileOutcome,
};
use crate::{HostTicketSpec, HOST_TICKET_V2_SCHEMA};

const LEASE_REQUEST_TAG_BYTES: usize = 16;
const MAX_LEASE_ID_LEN: usize = 32;
const PROC_LEASE_BY_ID_PREFIX: &str = "/proc/lease/by-id/";

/// Execute GPU lease ticket actions.
pub fn execute(
    transport: &mut dyn Transport,
    session: &Session,
    spec: &HostTicketSpec,
    _config: &ExecutorConfig,
) -> Result<String> {
    if spec.schema == HOST_TICKET_V2_SCHEMA {
        return execute_v2(transport, session, spec);
    }
    match spec.action.as_str() {
        "gpu.lease.grant" => execute_grant(transport, session, spec),
        "gpu.lease.renew" => execute_renew(transport, session, spec),
        "gpu.lease.release" => execute_release(transport, session, spec),
        other => Err(anyhow!("unsupported gpu action {other}")),
    }
}

/// Reconcile a version-2 GPU lease operation from root-owned lease projections.
pub fn reconcile(
    transport: &mut dyn Transport,
    session: &Session,
    spec: &HostTicketSpec,
    _config: &ExecutorConfig,
) -> Result<ReconcileOutcome> {
    if spec.schema != HOST_TICKET_V2_SCHEMA {
        return Ok(ReconcileOutcome::Ambiguous);
    }
    observe_v2_operation(transport, session, spec)
}

fn execute_v2(
    transport: &mut dyn Transport,
    session: &Session,
    spec: &HostTicketSpec,
) -> Result<String> {
    let operation_id = spec
        .operation_id
        .as_deref()
        .ok_or_else(|| anyhow!("host-ticket/v2 requires operation_id"))?;
    let subject_ref = spec
        .subject_ref
        .as_deref()
        .ok_or_else(|| anyhow!("host-ticket/v2 requires subject_ref"))?;
    let receipt_worker_id = spec
        .receipt_worker_id
        .as_deref()
        .ok_or_else(|| anyhow!("host-ticket/v2 requires receipt_worker_id"))?;
    let request_tag = lease_request_tag(spec.idempotency_key.as_str());
    let payload = match spec.action.as_str() {
        "gpu.lease.grant" => {
            validate_runtime_gpu_id(transport, session, subject_ref)?;
            if find_active_lease(transport, session, operation_id)?.is_some() {
                return Err(anyhow!(
                    "gpu.lease.grant operation_id {operation_id} is already active"
                ));
            }
            let ttl_s = to_u32(arg_u64(spec, "ttl_s").unwrap_or(120), "ttl_s")?;
            let priority = arg_u64(spec, "priority").unwrap_or(0);
            json!({
                "op": "grant",
                "id": operation_id,
                "subject": receipt_worker_id,
                "resource": subject_ref,
                "ttl_s": ttl_s,
                "priority": priority
            })
        }
        "gpu.lease.renew" => {
            let ttl_s = to_u32(arg_u64(spec, "ttl_s").unwrap_or(120), "ttl_s")?;
            let priority = arg_u64(spec, "priority").unwrap_or(0);
            json!({
                "op": "renew-bound",
                "id": operation_id,
                "subject": receipt_worker_id,
                "resource": subject_ref,
                "request": request_tag.as_str(),
                "ttl_s": ttl_s,
                "priority": priority
            })
        }
        "gpu.lease.release" => {
            validate_runtime_gpu_id(transport, session, subject_ref)?;
            validate_existing_lease_binding(
                transport,
                session,
                operation_id,
                receipt_worker_id,
                subject_ref,
            )?;
            let reason = arg_str(spec, "reason").unwrap_or("host-ticket-release");
            json!({"op": "preempt", "id": operation_id, "reason": reason})
        }
        other => return Err(anyhow!("unsupported gpu action {other}")),
    };
    let encoded = serde_json::to_string(&payload).context("serialize GPU lease control")?;
    if let Err(error) = transport.write(
        session,
        CLIENT_QUEEN_LEASE_CTL_PATH,
        format!("{encoded}\n").as_bytes(),
    ) {
        return Err(provider_pending(format!(
            "GPU lease control write outcome is unknown and will be reconciled without replay: {error}"
        )));
    }
    if spec.action == "gpu.lease.renew" {
        return Ok(format!(
            "gpu.lease.renew operation_id={operation_id} observed=active request={}",
            request_tag
        ));
    }
    match observe_v2_operation(transport, session, spec)? {
        ReconcileOutcome::Committed(message) => Ok(message),
        ReconcileOutcome::Rejected(message) => Err(anyhow!(message)),
        ReconcileOutcome::Ambiguous => Err(provider_pending(
            "GPU lease control was admitted; exact terminal observation is pending",
        )),
    }
}

fn observe_v2_operation(
    transport: &mut dyn Transport,
    session: &Session,
    spec: &HostTicketSpec,
) -> Result<ReconcileOutcome> {
    let operation_id = spec
        .operation_id
        .as_deref()
        .ok_or_else(|| anyhow!("host-ticket/v2 requires operation_id"))?;
    match spec.action.as_str() {
        "gpu.lease.grant" => {
            let worker_id = spec
                .receipt_worker_id
                .as_deref()
                .ok_or_else(|| anyhow!("host-ticket/v2 requires receipt_worker_id"))?;
            let gpu_id = spec
                .subject_ref
                .as_deref()
                .ok_or_else(|| anyhow!("host-ticket/v2 requires subject_ref"))?;
            if let Some(line) = find_active_lease(transport, session, operation_id)? {
                let subject = format!("subject={worker_id}");
                let resource = format!("resource={gpu_id}");
                if !line
                    .split_whitespace()
                    .any(|field| field == subject.as_str())
                    || !line
                        .split_whitespace()
                        .any(|field| field == resource.as_str())
                {
                    return Ok(ReconcileOutcome::Rejected(format!(
                        "{} operation_id={operation_id} observed=wrong-binding",
                        spec.action
                    )));
                }
                return Ok(ReconcileOutcome::Committed(format!(
                    "{} operation_id={operation_id} observed=active",
                    spec.action
                )));
            }
            Ok(ReconcileOutcome::Ambiguous)
        }
        "gpu.lease.renew" => {
            let worker_id = spec
                .receipt_worker_id
                .as_deref()
                .ok_or_else(|| anyhow!("host-ticket/v2 requires receipt_worker_id"))?;
            let gpu_id = spec
                .subject_ref
                .as_deref()
                .ok_or_else(|| anyhow!("host-ticket/v2 requires subject_ref"))?;
            if let Some(line) = find_active_lease(transport, session, operation_id)? {
                let subject = format!("subject={worker_id}");
                let resource = format!("resource={gpu_id}");
                if !line
                    .split_whitespace()
                    .any(|field| field == subject.as_str())
                    || !line
                        .split_whitespace()
                        .any(|field| field == resource.as_str())
                {
                    return Ok(ReconcileOutcome::Rejected(format!(
                        "gpu.lease.renew operation_id={operation_id} observed=wrong-binding"
                    )));
                }
                let request = format!(
                    "request={}",
                    lease_request_tag(spec.idempotency_key.as_str())
                );
                if line
                    .split_whitespace()
                    .any(|field| field == request.as_str())
                {
                    return Ok(ReconcileOutcome::Committed(format!(
                        "gpu.lease.renew operation_id={operation_id} observed=active request=matched"
                    )));
                }
            }
            Ok(ReconcileOutcome::Ambiguous)
        }
        "gpu.lease.release" => {
            let preemptions = transport.read(session, "/proc/lease/preemptions")?;
            let id_marker = format!("id={operation_id}");
            let worker_id = spec
                .receipt_worker_id
                .as_deref()
                .ok_or_else(|| anyhow!("host-ticket/v2 requires receipt_worker_id"))?;
            let gpu_id = spec
                .subject_ref
                .as_deref()
                .ok_or_else(|| anyhow!("host-ticket/v2 requires subject_ref"))?;
            if let Some(line) = preemptions.iter().find(|line| {
                line.split_whitespace()
                    .any(|field| field == id_marker.as_str())
            }) {
                let subject = format!("subject={worker_id}");
                let resource = format!("resource={gpu_id}");
                if !line
                    .split_whitespace()
                    .any(|field| field == subject.as_str())
                    || !line
                        .split_whitespace()
                        .any(|field| field == resource.as_str())
                {
                    return Ok(ReconcileOutcome::Rejected(format!(
                        "gpu.lease.release operation_id={operation_id} observed=wrong-binding"
                    )));
                }
                return Ok(ReconcileOutcome::Committed(format!(
                    "gpu.lease.release operation_id={operation_id} observed=preempted"
                )));
            }
            Ok(ReconcileOutcome::Ambiguous)
        }
        _ => Ok(ReconcileOutcome::Ambiguous),
    }
}

fn validate_runtime_gpu_id(
    transport: &mut dyn Transport,
    session: &Session,
    gpu_id: &str,
) -> Result<()> {
    let entries = transport
        .list(session, "/gpu")
        .context("list generated /gpu inventory")?;
    if entries.iter().any(|entry| entry == gpu_id) {
        return Ok(());
    }
    Err(anyhow!(
        "GPU id {gpu_id} is absent from the root-owned /gpu inventory"
    ))
}

fn validate_existing_lease_binding(
    transport: &mut dyn Transport,
    session: &Session,
    operation_id: &str,
    worker_id: &str,
    gpu_id: &str,
) -> Result<()> {
    let line = find_active_lease(transport, session, operation_id)?
        .ok_or_else(|| anyhow!("lease operation_id {operation_id} is not active"))?;
    let subject = format!("subject={worker_id}");
    let resource = format!("resource={gpu_id}");
    if !line
        .split_whitespace()
        .any(|field| field == subject.as_str())
        || !line
            .split_whitespace()
            .any(|field| field == resource.as_str())
    {
        return Err(anyhow!(
            "lease operation_id {operation_id} is not pinned to Worker {worker_id} and GPU {gpu_id}"
        ));
    }
    Ok(())
}

fn find_active_lease(
    transport: &mut dyn Transport,
    session: &Session,
    operation_id: &str,
) -> Result<Option<String>> {
    if operation_id.is_empty()
        || operation_id.len() > MAX_LEASE_ID_LEN
        || !operation_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(anyhow!(
            "lease operation_id must be a 1..={MAX_LEASE_ID_LEN} byte simple token"
        ));
    }
    let path = format!("{PROC_LEASE_BY_ID_PREFIX}{operation_id}");
    let lines = transport.read(session, path.as_str())?;
    if lines.len() > 1 {
        return Err(anyhow!(
            "exact lease lookup for operation_id {operation_id} returned multiple records"
        ));
    }
    let id = format!("id={operation_id}");
    let Some(line) = lines.into_iter().next() else {
        return Ok(None);
    };
    if !line.split_whitespace().any(|field| field == id.as_str()) {
        return Err(anyhow!(
            "exact lease lookup for operation_id {operation_id} returned a mismatched record"
        ));
    }
    Ok(Some(line))
}

fn lease_request_tag(idempotency_key: &str) -> String {
    let digest = Sha256::digest(idempotency_key.as_bytes());
    hex::encode(&digest[..LEASE_REQUEST_TAG_BYTES])
}

fn execute_grant(
    transport: &mut dyn Transport,
    session: &Session,
    spec: &HostTicketSpec,
) -> Result<String> {
    let gpu_id = resolve_gpu_id(spec)?;
    let mem_mb = to_u32(arg_u64(spec, "mem_mb").unwrap_or(4096), "mem_mb")?;
    let streams = to_u8(arg_u64(spec, "streams").unwrap_or(1), "streams")?;
    let ttl_s = to_u32(arg_u64(spec, "ttl_s").unwrap_or(120), "ttl_s")?;
    let priority = arg_u64(spec, "priority")
        .map(|value| to_u8(value, "priority"))
        .transpose()?;

    let mut args = vec![
        format!("gpu_id={gpu_id}"),
        format!("mem_mb={mem_mb}"),
        format!("streams={streams}"),
        format!("ttl_s={ttl_s}"),
    ];
    if let Some(priority) = priority {
        args.push(format!("priority={priority}"));
    }
    if let Some(budget_ttl_s) = arg_u64(spec, "budget_ttl_s") {
        args.push(format!("budget_ttl_s={budget_ttl_s}"));
    }
    if let Some(budget_ops) = arg_u64(spec, "budget_ops") {
        args.push(format!("budget_ops={budget_ops}"));
    }

    let payload = queen::spawn("gpu", args.iter().map(String::as_str))?;
    transport
        .write(session, queen::queen_ctl_path(), payload.as_bytes())
        .context("write gpu lease grant to /queen/ctl")?;

    let lease_path = format!("/gpu/{gpu_id}/lease");
    let verification = read_last_line(transport, session, lease_path.as_str())
        .ok()
        .flatten()
        .filter(|line| line.contains("\"state\":\"ACTIVE\"") && line.contains(&gpu_id))
        .map(|_| "verified")
        .unwrap_or("queued");
    Ok(format!(
        "gpu lease grant gpu_id={gpu_id} mem_mb={mem_mb} streams={streams} ttl_s={ttl_s} status={verification}"
    ))
}

fn execute_renew(
    transport: &mut dyn Transport,
    session: &Session,
    spec: &HostTicketSpec,
) -> Result<String> {
    let lease_id = spec
        .args
        .get("lease_id")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(spec.id.as_str());
    let ttl_s = to_u32(arg_u64(spec, "ttl_s").unwrap_or(120), "ttl_s")?;
    let mut payload = json!({
        "op": "renew",
        "id": lease_id,
        "ttl_s": ttl_s
    });
    if let Some(priority) = arg_u64(spec, "priority") {
        payload["priority"] = serde_json::Value::from(priority);
    }
    let encoded = serde_json::to_string(&payload).context("serialize renew payload")?;
    transport
        .write(
            session,
            CLIENT_QUEEN_LEASE_CTL_PATH,
            format!("{encoded}\n").as_bytes(),
        )
        .context("write gpu lease renew to /queen/lease/ctl")?;
    Ok(format!(
        "gpu lease renew id={lease_id} ttl_s={ttl_s} queued"
    ))
}

fn execute_release(
    transport: &mut dyn Transport,
    session: &Session,
    spec: &HostTicketSpec,
) -> Result<String> {
    let gpu_id = resolve_gpu_id(spec)?;
    let worker_id = spec
        .args
        .get("worker_id")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            active_worker_for_gpu(transport, session, gpu_id.as_str())
                .ok()
                .flatten()
        })
        .ok_or_else(|| {
            anyhow!("gpu release requires worker_id or active /gpu/{gpu_id}/lease entry")
        })?;

    let payload = queen::kill(worker_id.as_str())?;
    transport
        .write(session, queen::queen_ctl_path(), payload.as_bytes())
        .context("write gpu release kill to /queen/ctl")?;

    let lease_path = format!("/gpu/{gpu_id}/lease");
    let verification = read_last_line(transport, session, lease_path.as_str())
        .ok()
        .flatten()
        .filter(|line| line.contains("\"state\":\"RELEASED\""))
        .map(|_| "released")
        .unwrap_or("queued");
    Ok(format!(
        "gpu lease release gpu_id={gpu_id} worker_id={worker_id} status={verification}"
    ))
}

fn resolve_gpu_id(spec: &HostTicketSpec) -> Result<String> {
    if let Some(gpu_id) = spec
        .args
        .get("gpu_id")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(gpu_id.to_owned());
    }
    let target = target_components(spec);
    if target.len() >= 2 && target[0] == "gpu" {
        return Ok(target[1].to_owned());
    }
    Err(anyhow!(
        "gpu action {} requires args.gpu_id or target /gpu/<id>/...",
        spec.action
    ))
}

fn active_worker_for_gpu(
    transport: &mut dyn Transport,
    session: &Session,
    gpu_id: &str,
) -> Result<Option<String>> {
    let path = format!("/gpu/{gpu_id}/lease");
    let lines = transport
        .read(session, path.as_str())
        .with_context(|| format!("read {path}"))?;
    for line in lines.iter().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let entry: GpuLeaseLine = match serde_json::from_str(trimmed) {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if entry.gpu_id == gpu_id && entry.state == "ACTIVE" {
            return Ok(Some(entry.worker_id));
        }
    }
    Ok(None)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GpuLeaseLine {
    gpu_id: String,
    worker_id: String,
    state: String,
}

fn to_u32(value: u64, field: &str) -> Result<u32> {
    let converted = u32::try_from(value).map_err(|_| anyhow!("{field} exceeds u32"))?;
    if converted == 0 {
        return Err(anyhow!("{field} must be > 0"));
    }
    Ok(converted)
}

fn to_u8(value: u64, field: &str) -> Result<u8> {
    let converted = u8::try_from(value).map_err(|_| anyhow!("{field} exceeds u8"))?;
    if converted == 0 && field != "priority" {
        return Err(anyhow!("{field} must be > 0"));
    }
    Ok(converted)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use cohesix_ticket::Role;
    use serde_json::Value;

    fn v2_gpu_spec(action: &str) -> HostTicketSpec {
        HostTicketSpec {
            schema: HOST_TICKET_V2_SCHEMA.to_owned(),
            id: "ticket-v2".to_owned(),
            idempotency_key: "idem-v2".to_owned(),
            action: action.to_owned(),
            args: if action == "gpu.lease.release" {
                serde_json::json!({"reason": "test"})
            } else {
                serde_json::json!({"ttl_s": 30})
            },
            receipt_mode: Some(crate::ReceiptMode::Worker),
            operation_id: Some("lease-1".to_owned()),
            subject_ref: Some("GPU-0".to_owned()),
            receipt_worker_role: Some("worker-gpu".to_owned()),
            receipt_worker_id: Some("gpu-worker-1".to_owned()),
            receipt_supervisor_generation: Some(2),
            receipt_cap_generation: Some(3),
            resolved_worker_slot: Some(0),
            resolved_lease_epoch: Some(4),
            admission_sequence: Some(5),
            ..HostTicketSpec::default()
        }
    }

    #[test]
    fn resolve_gpu_from_target_components() {
        let spec = HostTicketSpec {
            schema: "host-ticket/v1".to_owned(),
            id: "t1".to_owned(),
            idempotency_key: "k1".to_owned(),
            action: "gpu.lease.grant".to_owned(),
            target: Some("/gpu/GPU-0/lease".to_owned()),
            args: Value::Null,
            expires_unix_ms: None,
            source_hive: None,
            target_hive: None,
            relay_hop: None,
            relay_correlation_id: None,
            ..HostTicketSpec::default()
        };
        let gpu_id = resolve_gpu_id(&spec).expect("gpu id");
        assert_eq!(gpu_id, "GPU-0");
    }

    #[test]
    fn resolve_gpu_prefers_args() {
        let spec = HostTicketSpec {
            schema: "host-ticket/v1".to_owned(),
            id: "t1".to_owned(),
            idempotency_key: "k1".to_owned(),
            action: "gpu.lease.grant".to_owned(),
            target: Some("/gpu/GPU-0/lease".to_owned()),
            args: serde_json::json!({ "gpu_id": "GPU-9" }),
            expires_unix_ms: None,
            source_hive: None,
            target_hive: None,
            relay_hop: None,
            relay_correlation_id: None,
            ..HostTicketSpec::default()
        };
        let gpu_id = resolve_gpu_id(&spec).expect("gpu id");
        assert_eq!(gpu_id, "GPU-9");
    }

    #[test]
    fn v2_gpu_grant_uses_only_lease_control_and_exact_binding() {
        let mut transport = GpuTransport::new(true);
        let session = Session::new(1.into(), Role::Queen);
        let result = execute(
            &mut transport,
            &session,
            &v2_gpu_spec("gpu.lease.grant"),
            &ExecutorConfig::default(),
        )
        .expect("grant");
        assert!(result.contains("observed=active"));
        assert_eq!(transport.writes.len(), 1);
        assert_eq!(transport.writes[0].0, CLIENT_QUEEN_LEASE_CTL_PATH);
        assert!(transport.writes[0].1.contains("\"op\":\"grant\""));
        assert!(transport.writes[0]
            .1
            .contains("\"subject\":\"gpu-worker-1\""));
        assert!(transport.writes[0].1.contains("\"resource\":\"GPU-0\""));
        assert!(!transport
            .writes
            .iter()
            .any(|(path, _payload)| path == queen::queen_ctl_path()));
    }

    #[test]
    fn v2_gpu_ambiguous_admission_is_pending_not_rejected() {
        let mut transport = GpuTransport::new(false);
        let error = execute(
            &mut transport,
            &Session::new(1.into(), Role::Queen),
            &v2_gpu_spec("gpu.lease.grant"),
            &ExecutorConfig::default(),
        )
        .expect_err("missing observation is pending");
        assert!(super::super::is_provider_pending(&error));
        assert_eq!(transport.writes.len(), 1);
    }

    #[test]
    fn v2_gpu_renew_uses_one_atomic_correlated_round_trip() {
        let mut transport = GpuTransport::new(true);
        let spec = v2_gpu_spec("gpu.lease.renew");
        let result = execute(
            &mut transport,
            &Session::new(1.into(), Role::Queen),
            &spec,
            &ExecutorConfig::default(),
        )
        .expect("renew");

        assert!(result.contains("observed=active"));
        assert_eq!(transport.writes.len(), 1);
        assert!(transport.writes[0].1.contains("\"op\":\"renew-bound\""));
        assert!(transport.writes[0]
            .1
            .contains("\"subject\":\"gpu-worker-1\""));
        assert!(transport.writes[0].1.contains("\"resource\":\"GPU-0\""));
        assert!(transport.writes[0].1.contains(&format!(
            "\"request\":\"{}\"",
            lease_request_tag(spec.idempotency_key.as_str())
        )));
        assert!(transport.reads.is_empty());
        assert!(transport.lists.is_empty());
    }

    #[test]
    fn v2_gpu_renew_reconciliation_requires_exact_request_tag() {
        let mut transport = GpuTransport::new(false);
        let spec = v2_gpu_spec("gpu.lease.renew");
        transport.files.insert(
            "/proc/lease/by-id/lease-1".to_owned(),
            vec![format!(
                "id=lease-1 subject=gpu-worker-1 resource=GPU-0 request={}",
                lease_request_tag(spec.idempotency_key.as_str())
            )],
        );
        let outcome = reconcile(
            &mut transport,
            &Session::new(1.into(), Role::Queen),
            &spec,
            &ExecutorConfig::default(),
        )
        .expect("reconcile exact request");
        assert!(matches!(outcome, ReconcileOutcome::Committed(_)));

        transport.files.insert(
            "/proc/lease/by-id/lease-1".to_owned(),
            vec!["id=lease-1 subject=gpu-worker-1 resource=GPU-0 request=00000000000000000000000000000000".to_owned()],
        );
        let outcome = reconcile(
            &mut transport,
            &Session::new(1.into(), Role::Queen),
            &spec,
            &ExecutorConfig::default(),
        )
        .expect("reconcile stale request");
        assert!(matches!(outcome, ReconcileOutcome::Ambiguous));
    }

    #[test]
    fn v2_gpu_release_preempts_without_killing_receipt_worker() {
        let mut transport = GpuTransport::new(true);
        transport.files.insert(
            "/proc/lease/by-id/lease-1".to_owned(),
            vec!["id=lease-1 subject=gpu-worker-1 resource=GPU-0".to_owned()],
        );
        let result = execute(
            &mut transport,
            &Session::new(1.into(), Role::Queen),
            &v2_gpu_spec("gpu.lease.release"),
            &ExecutorConfig::default(),
        )
        .expect("release");
        assert!(result.contains("observed=preempted"));
        assert_eq!(transport.writes.len(), 1);
        assert_eq!(transport.writes[0].0, CLIENT_QUEEN_LEASE_CTL_PATH);
        assert!(transport.writes[0].1.contains("\"op\":\"preempt\""));
    }

    #[derive(Debug)]
    struct GpuTransport {
        files: BTreeMap<String, Vec<String>>,
        writes: Vec<(String, String)>,
        reads: Vec<String>,
        lists: Vec<String>,
        publish_observation: bool,
    }

    impl GpuTransport {
        fn new(publish_observation: bool) -> Self {
            let mut files = BTreeMap::new();
            files.insert("/gpu".to_owned(), vec!["GPU-0".to_owned()]);
            files.insert("/proc/lease/by-id/lease-1".to_owned(), Vec::new());
            files.insert("/proc/lease/preemptions".to_owned(), Vec::new());
            Self {
                files,
                writes: Vec::new(),
                reads: Vec::new(),
                lists: Vec::new(),
                publish_observation,
            }
        }
    }

    impl Transport for GpuTransport {
        fn attach(&mut self, _role: Role, _ticket: Option<&str>) -> Result<Session> {
            Ok(Session::new(1.into(), Role::Queen))
        }

        fn ping(&mut self, _session: &Session) -> Result<String> {
            Ok("pong".to_owned())
        }

        fn tail(
            &mut self,
            session: &Session,
            path: &str,
            _lines: Option<u16>,
        ) -> Result<Vec<String>> {
            self.read(session, path)
        }

        fn read(&mut self, _session: &Session, path: &str) -> Result<Vec<String>> {
            self.reads.push(path.to_owned());
            Ok(self.files.get(path).cloned().unwrap_or_default())
        }

        fn list(&mut self, _session: &Session, path: &str) -> Result<Vec<String>> {
            self.lists.push(path.to_owned());
            Ok(self.files.get(path).cloned().unwrap_or_default())
        }

        fn write(&mut self, _session: &Session, path: &str, payload: &[u8]) -> Result<()> {
            let payload = std::str::from_utf8(payload)?.trim().to_owned();
            self.writes.push((path.to_owned(), payload.clone()));
            if self.publish_observation && path == CLIENT_QUEEN_LEASE_CTL_PATH {
                let value: Value = serde_json::from_str(&payload)?;
                if value.get("op").and_then(Value::as_str) == Some("grant") {
                    let id = value.get("id").and_then(Value::as_str).unwrap_or_default();
                    self.files.insert(
                        format!("{PROC_LEASE_BY_ID_PREFIX}{id}"),
                        vec![format!("id={id} subject=gpu-worker-1 resource=GPU-0")],
                    );
                } else if value.get("op").and_then(Value::as_str) == Some("renew-bound") {
                    let id = value.get("id").and_then(Value::as_str).unwrap_or_default();
                    let subject = value
                        .get("subject")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let resource = value
                        .get("resource")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let request = value
                        .get("request")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    self.files.insert(
                        format!("{PROC_LEASE_BY_ID_PREFIX}{id}"),
                        vec![format!(
                            "id={id} subject={subject} resource={resource} request={request}"
                        )],
                    );
                } else if value.get("op").and_then(Value::as_str) == Some("preempt") {
                    let id = value.get("id").and_then(Value::as_str).unwrap_or_default();
                    self.files
                        .remove(format!("{PROC_LEASE_BY_ID_PREFIX}{id}").as_str());
                    self.files.insert(
                        "/proc/lease/preemptions".to_owned(),
                        vec!["id=lease-1 subject=gpu-worker-1 resource=GPU-0".to_owned()],
                    );
                }
            }
            Ok(())
        }
    }
}
