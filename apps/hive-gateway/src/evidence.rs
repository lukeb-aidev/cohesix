// Author: Lukas Bower
// Purpose: Sign exact delegated decisions and authenticated Root admission observations under separately enrolled gateway custody.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
#![forbid(unsafe_code)]

use crate::{authority_now_ms, AppState};
use anyhow::{anyhow, ensure, Result};
use cohesix_evidence::producer::{Custody, Operation};
use cohesix_evidence::ticket::{caller_request, matches_admission, require_binding};
use cohesix_evidence::{digest, Kind, Outcome};
use serde_json::Value;

/// Called after delegated authorization. The operator enrollment never comes from HTTP.
pub(super) fn write(
    state: &AppState,
    identity: &str,
    path: &str,
    payload: &[u8],
) -> Result<Vec<String>> {
    let Some((directory, executable)) = &state.inner.evidence_enrollment else {
        return state.write(path, payload);
    };
    if path != "/host/tickets/spec" {
        return state.write(path, payload);
    }
    let request: Value = serde_json::from_slice(payload)?;
    caller_request(&request, false)?;
    let v2 = request["schema"] == "host-ticket/v2";
    let id = field(&request, "id")?;
    let idempotency = field(&request, "idempotency_key")?;
    let action = field(&request, "action")?;
    let epoch = request["writer_epoch"]
        .as_u64()
        .ok_or_else(|| anyhow!("EPERM evidence writer epoch required"))?;
    let mut operation = Operation::open(
        directory,
        id,
        idempotency,
        Custody::GatewayAdmission,
        authority_now_ms()?,
    )?;
    operation.bind(
        action,
        epoch,
        state.inner.bounds.manifest_sha256,
        "hive-gateway",
        executable,
    )?;
    ensure!(
        operation.trust.expected.subject == identity,
        "EPERM evidence delegated identity binding"
    );
    require_binding(&operation.trust.expected, &request, false)?;
    ensure!(
        request.get("expires_unix_ms").is_none_or(|value| value
            .as_u64()
            .is_some_and(|expiry| operation.expires_unix_ms() <= expiry)),
        "EPERM evidence exceeds request expiry"
    );
    let boot = state.read_uncached("/proc/boot")?;
    let manifests: Vec<_> = boot
        .iter()
        .filter_map(|line| line.strip_prefix("manifest.sha256="))
        .collect();
    ensure!(
        manifests == [operation.trust.expected.target_manifest_sha256.as_str()],
        "EPERM evidence Root manifest measurement"
    );
    operation.emit(
        Kind::Intent,
        payload,
        None,
        Outcome::Observed,
        authority_now_ms()?,
    )?;
    if operation.record(Kind::Facts).is_none() {
        let native_facts = native_facts(state, &request)?;
        operation.emit(
            Kind::Facts,
            &serde_json::to_vec(&serde_json::json!({
                "schema":"cohesix-root-facts-witness/v1", "source":"authenticated-root-console",
                "device_attested":false, "boot":boot, "native_projections":native_facts,
            }))?,
            None,
            Outcome::Observed,
            authority_now_ms()?,
        )?;
    }
    operation.emit(
        Kind::Approval,
        &serde_json::to_vec(&serde_json::json!({
            "schema":"cohesix-gateway-decision/v1", "delegated_ticket_sha256":identity,
            "request_sha256":digest(payload), "action":action, "writer_epoch":epoch,
            "decision":"allowed", "authority_custodian":"gateway-delegation",
        }))?,
        None,
        Outcome::Admitted,
        authority_now_ms()?,
    )?;
    let snapshot_path = if v2 {
        "/host/tickets/spec.snapshot"
    } else {
        "/host/tickets/spec"
    };
    let before = state.read_uncached(snapshot_path)?;
    let already_admitted = find_admitted(&before, &request)?.is_some();
    let lines = if operation.record(Kind::Grant).is_some() || already_admitted {
        Vec::new()
    } else {
        state.write(path, payload)?
    };
    // ACK alone cannot create a grant witness. V2 additionally requires the
    // exact Root-owned resolution; caller bytes never select slot/lease/sequence.
    let snapshot = state.read_uncached(snapshot_path)?;
    let admitted = exact_admitted(&snapshot, &request)?;
    require_binding(&operation.trust.expected, &admitted, v2)?;
    // Preserve the pre-execution Worker sequence on an exact gateway retry.
    let worker_current = if operation.record(Kind::Grant).is_some() {
        operation.read_json(Kind::Grant)?["worker_current"].clone()
    } else if v2 {
        let current =
            state.read_uncached(&cohesix_evidence::ticket::current_path(id, idempotency)?)?;
        ensure!(
            current.len() == 1 && current[0].len() <= 256,
            "ELIMIT evidence Worker admission"
        );
        serde_json::to_value(current)?
    } else {
        Value::Null
    };
    operation.emit(
        Kind::Grant,
        &serde_json::to_vec(&serde_json::json!({
            "schema":"cohesix-root-admission-witness/v1", "source":"authenticated-root-console",
            "device_attested":false,
            "admission_class":if v2 { "root-resolved-worker-v2" } else { "validated-native-v1-append" },
        "request_sha256":digest(payload), "admitted":admitted,
        "worker_current":worker_current,
        }))?,
        None,
        Outcome::Admitted,
        authority_now_ms()?,
    )?;
    Ok(lines)
}

fn native_facts(state: &AppState, request: &Value) -> Result<Value> {
    let mut facts = serde_json::Map::new();
    if field(request, "action")?.starts_with("gpu.workload.") {
        let gpu = field(request, "subject_ref")?;
        cohesix_authority::validate_id(gpu).map_err(|_| anyhow!("EPERM evidence GPU id"))?;
        let mut paths = vec![format!("/gpu/{gpu}/info")];
        if request["action"] == "gpu.workload.submit" {
            let lease = field(&request["args"], "lease_id")?;
            cohesix_authority::validate_id(lease)
                .map_err(|_| anyhow!("EPERM evidence lease id"))?;
            paths.push(format!("/gpu/{gpu}/lease"));
            paths.push(format!("/proc/lease/by-id/{lease}"));
        }
        for path in paths {
            let lines = state.read_uncached(&path)?;
            ensure!(
                !lines.is_empty()
                    && lines.len() <= 64
                    && lines.iter().map(String::len).sum::<usize>() <= 8192,
                "ELIMIT evidence native facts"
            );
            facts.insert(path, serde_json::to_value(lines)?);
        }
    }
    Ok(facts.into())
}

fn field<'a>(request: &'a Value, name: &str) -> Result<&'a str> {
    request[name]
        .as_str()
        .ok_or_else(|| anyhow!("EPERM evidence request identity"))
}

fn exact_admitted(snapshot: &[String], request: &Value) -> Result<Value> {
    find_admitted(snapshot, request)?
        .ok_or_else(|| anyhow!("unconfirmed evidence Root admission not observed"))
}

fn find_admitted(snapshot: &[String], request: &Value) -> Result<Option<Value>> {
    ensure!(
        snapshot.len() <= 1024
            && snapshot.iter().map(String::len).sum::<usize>() <= 2 * 1024 * 1024,
        "ELIMIT evidence Root snapshot"
    );
    let mut matched = None;
    for line in snapshot {
        let row: Value = serde_json::from_str(line)?;
        if row["id"] == request["id"] && row["idempotency_key"] == request["idempotency_key"] {
            ensure!(
                matches_admission(&row, request)? && matched.is_none(),
                "EPERM evidence ambiguous Root admission"
            );
            matched = Some(row);
        }
    }
    Ok(matched)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn worker_admission_requires_root_resolution_and_refuses_duplicates() {
        let request = serde_json::json!({"schema":"host-ticket/v2", "id":"one",
            "idempotency_key":"once", "action":"gpu.workload.submit", "writer_epoch":7});
        assert!(exact_admitted(&[request.to_string()], &request).is_err());
        let mut admitted = request.clone();
        admitted["resolved_worker_slot"] = 0.into();
        admitted["resolved_lease_epoch"] = 2.into();
        admitted["admission_sequence"] = 3.into();
        assert_eq!(
            exact_admitted(&[admitted.to_string()], &request).unwrap(),
            admitted
        );
        assert!(exact_admitted(&[admitted.to_string(), admitted.to_string()], &request).is_err());
        admitted["receipt_cap_generation"] = 99.into();
        assert!(exact_admitted(&[admitted.to_string()], &request).is_err());
    }
    #[test]
    fn admission_witness_requires_exact_root_fields_and_one_record() {
        let request = serde_json::json!({"schema":"host-ticket/v1", "id":"one", "idempotency_key":"once", "action":"systemd.status-check", "args":{"unit":"ssh.service"}, "writer_epoch":7});
        let line = request.to_string();
        assert_eq!(
            exact_admitted(std::slice::from_ref(&line), &request).unwrap(),
            request
        );
        assert!(exact_admitted(&[], &request).is_err());
        assert!(exact_admitted(&[line.clone(), line], &request).is_err());
        let mut changed = request.clone();
        changed["writer_epoch"] = serde_json::json!(6);
        assert!(exact_admitted(&[changed.to_string()], &request).is_err());
        changed = request.clone();
        changed["args"]["unit"] = serde_json::json!("other.service");
        assert!(exact_admitted(&[changed.to_string()], &request).is_err());
    }
}
