// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Build, hash, and append bounded versioned host ticket result receipts.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use anyhow::{anyhow, Result};
use cohsh::{Session, Transport};
use sha2::{Digest, Sha256};

use crate::text::truncate_utf8;
use crate::{
    HostTicketResult, HostTicketSpec, HOST_TICKET_RESULT_V1_SCHEMA, HOST_TICKET_RESULT_V2_SCHEMA,
    HOST_TICKET_V1_SCHEMA, HOST_TICKET_V2_SCHEMA,
};

/// Maximum compact JSON bytes for a valid version-2 result at current field bounds.
pub const HOST_TICKET_V2_MAX_ENCODED_RESULT_BYTES: usize = 1467;

/// Render a result receipt line for `/host/tickets/status` or `/host/tickets/deadletter`.
pub fn build_result_line(
    spec: &HostTicketSpec,
    result_schema: &str,
    state: &str,
    message: Option<&str>,
    max_line_bytes: u32,
) -> Result<String> {
    validate_schema_pair(spec.schema.as_str(), result_schema)?;
    let cleaned_message = message
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(sanitize_message);
    let mut result = HostTicketResult {
        schema: result_schema.to_owned(),
        id: spec.id.clone(),
        idempotency_key: spec.idempotency_key.clone(),
        action: spec.action.clone(),
        state: state.to_owned(),
        message: cleaned_message,
        source_hive: spec.source_hive.clone(),
        target_hive: spec.target_hive.clone(),
        relay_hop: spec.relay_hop,
        relay_correlation_id: spec.relay_correlation_id.clone(),
        receipt_mode: spec.receipt_mode,
        operation_id: spec.operation_id.clone(),
        subject_ref: spec.subject_ref.clone(),
        receipt_worker_role: spec.receipt_worker_role.clone(),
        receipt_worker_id: spec.receipt_worker_id.clone(),
        receipt_supervisor_generation: spec.receipt_supervisor_generation,
        receipt_cap_generation: spec.receipt_cap_generation,
        resolved_worker_slot: spec.resolved_worker_slot,
        resolved_lease_epoch: spec.resolved_lease_epoch,
        admission_sequence: spec.admission_sequence,
        result_digest: None,
    };
    refresh_v2_digest(&mut result)?;
    if let Some(line) = encode_if_within_limit(&result, max_line_bytes)? {
        return Ok(line);
    }

    if result.schema == HOST_TICKET_RESULT_V1_SCHEMA {
        result.relay_correlation_id = None;
        if let Some(line) = encode_if_within_limit(&result, max_line_bytes)? {
            return Ok(line);
        }
    }

    if let Some(message) = result.message.as_deref() {
        result.message = Some(truncate_utf8(message, 96));
        refresh_v2_digest(&mut result)?;
    }
    if let Some(line) = encode_if_within_limit(&result, max_line_bytes)? {
        return Ok(line);
    }

    result.message = None;
    refresh_v2_digest(&mut result)?;
    if let Some(line) = encode_if_within_limit(&result, max_line_bytes)? {
        return Ok(line);
    }

    if result.schema == HOST_TICKET_RESULT_V1_SCHEMA {
        result.source_hive = None;
        result.target_hive = None;
        result.relay_hop = None;
        if let Some(line) = encode_if_within_limit(&result, max_line_bytes)? {
            return Ok(line);
        }
    }

    Err(anyhow!(
        "ticket result line exceeds max_line_bytes {}",
        max_line_bytes
    ))
}

/// Append a rendered receipt line to the supplied path.
pub fn append_result_line(
    transport: &mut dyn Transport,
    session: &Session,
    path: &str,
    line: &str,
) -> Result<()> {
    let mut payload = line.as_bytes().to_vec();
    payload.push(b'\n');
    transport.write(session, path, payload.as_slice())?;
    Ok(())
}

/// Append bounded result records in order through one transport activation.
pub fn append_result_lines(
    transport: &mut dyn Transport,
    session: &Session,
    path: &str,
    lines: &[String],
) -> Result<()> {
    if lines.is_empty() {
        return Err(anyhow!("result batch requires at least one line"));
    }
    let payloads = lines
        .iter()
        .map(|line| {
            let mut payload = line.as_bytes().to_vec();
            payload.push(b'\n');
            payload
        })
        .collect::<Vec<_>>();
    let written = transport.write_batch(session, path, payloads.as_slice())?;
    if written != payloads.len() {
        return Err(anyhow!(
            "result batch wrote {written} of {} records",
            payloads.len()
        ));
    }
    Ok(())
}

/// Return the canonical compact JSON bytes hashed by a version-2 result.
pub fn canonical_result_bytes(result: &HostTicketResult) -> Result<Vec<u8>> {
    let mut canonical = result.clone();
    canonical.result_digest = None;
    serde_json::to_vec(&canonical).map_err(Into::into)
}

/// Compute the lowercase SHA-256 digest for a version-2 result.
pub fn canonical_result_digest(result: &HostTicketResult) -> Result<String> {
    let canonical = canonical_result_bytes(result)?;
    let mut hasher = Sha256::new();
    hasher.update(&canonical);
    Ok(hex::encode(hasher.finalize()))
}

/// Validate that a version-2 result carries its exact canonical digest.
pub fn validate_result_digest(result: &HostTicketResult) -> Result<()> {
    let supplied = result
        .result_digest
        .as_deref()
        .ok_or_else(|| anyhow!("host-ticket-result/v2 requires result_digest"))?;
    if supplied.len() != 64
        || !supplied.bytes().all(|byte| byte.is_ascii_hexdigit())
        || supplied.bytes().any(|byte| byte.is_ascii_uppercase())
    {
        return Err(anyhow!(
            "host-ticket-result/v2 result_digest must be lowercase SHA-256 hex"
        ));
    }
    let expected = canonical_result_digest(result)?;
    if supplied != expected {
        return Err(anyhow!(
            "host-ticket-result/v2 result_digest mismatch: expected {expected}"
        ));
    }
    Ok(())
}

fn refresh_v2_digest(result: &mut HostTicketResult) -> Result<()> {
    if result.schema == HOST_TICKET_RESULT_V2_SCHEMA {
        result.result_digest = Some(canonical_result_digest(result)?);
    }
    Ok(())
}

fn validate_schema_pair(request: &str, result: &str) -> Result<()> {
    let valid = matches!(
        (request, result),
        (HOST_TICKET_V1_SCHEMA, HOST_TICKET_RESULT_V1_SCHEMA)
            | (HOST_TICKET_V2_SCHEMA, HOST_TICKET_RESULT_V2_SCHEMA)
    );
    if !valid {
        return Err(anyhow!(
            "ticket request/result schema mismatch: {request} -> {result}"
        ));
    }
    Ok(())
}

fn sanitize_message(input: &str) -> String {
    input
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect()
}

fn encode_if_within_limit(
    result: &HostTicketResult,
    max_line_bytes: u32,
) -> Result<Option<String>> {
    let line = serde_json::to_string(result)?;
    if line.len() > max_line_bytes as usize {
        return Ok(None);
    }
    Ok(Some(line))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ReceiptMode;
    use serde_json::Value;

    fn v1_spec() -> HostTicketSpec {
        HostTicketSpec {
            schema: HOST_TICKET_V1_SCHEMA.to_owned(),
            id: "ticket-1".to_owned(),
            idempotency_key: "k1".to_owned(),
            action: "systemd.restart".to_owned(),
            target: None,
            args: Value::Null,
            expires_unix_ms: None,
            source_hive: Some("hive-a".to_owned()),
            target_hive: Some("hive-b".to_owned()),
            relay_hop: Some(1),
            relay_correlation_id: Some("ticket-1:k1:hive-a:hive-b".to_owned()),
            receipt_mode: Some(ReceiptMode::None),
            operation_id: None,
            subject_ref: None,
            receipt_worker_role: None,
            receipt_worker_id: None,
            receipt_supervisor_generation: None,
            receipt_cap_generation: None,
            resolved_worker_slot: None,
            resolved_lease_epoch: None,
            admission_sequence: None,
        }
    }

    fn v2_spec() -> HostTicketSpec {
        HostTicketSpec {
            schema: HOST_TICKET_V2_SCHEMA.to_owned(),
            id: "ticket-v2".to_owned(),
            idempotency_key: "idem-v2".to_owned(),
            action: "gpu.lease.grant".to_owned(),
            target: None,
            args: serde_json::json!({"ttl_s": 30}),
            expires_unix_ms: None,
            source_hive: None,
            target_hive: None,
            relay_hop: None,
            relay_correlation_id: None,
            receipt_mode: Some(ReceiptMode::Worker),
            operation_id: Some("lease-1".to_owned()),
            subject_ref: Some("GPU-0".to_owned()),
            receipt_worker_role: Some("worker-gpu".to_owned()),
            receipt_worker_id: Some("worker-gpu-1".to_owned()),
            receipt_supervisor_generation: Some(2),
            receipt_cap_generation: Some(3),
            resolved_worker_slot: Some(0),
            resolved_lease_epoch: Some(4),
            admission_sequence: Some(5),
        }
    }

    #[test]
    fn line_builder_enforces_bounds_and_multibyte_safety() {
        let line = build_result_line(
            &v1_spec(),
            HOST_TICKET_RESULT_V1_SCHEMA,
            "failed",
            Some("🔥 line one\nline two"),
            512,
        )
        .expect("build line");
        assert!(line.contains("🔥 line one line two"));

        let err = build_result_line(
            &v1_spec(),
            HOST_TICKET_RESULT_V1_SCHEMA,
            "failed",
            Some(&"🙂".repeat(1000)),
            64,
        )
        .expect_err("tiny bound must fail");
        assert!(err.to_string().contains("max_line_bytes"));
    }

    #[test]
    fn v2_result_echoes_binding_and_verifies_digest() {
        let line = build_result_line(
            &v2_spec(),
            HOST_TICKET_RESULT_V2_SCHEMA,
            "succeeded",
            Some("committed"),
            2048,
        )
        .expect("v2 line");
        let result: HostTicketResult = serde_json::from_str(&line).expect("parse result");
        validate_result_digest(&result).expect("digest");
        assert_eq!(line.len(), 506);
        assert_eq!(
            result.result_digest.as_deref(),
            Some("730b822b8b3497f4ac21e3aaddf3d5f89411b95ddc05083268725dad2fb620b0")
        );
        assert_eq!(result.resolved_worker_slot, Some(0));
        assert_eq!(result.resolved_lease_epoch, Some(4));
        assert_eq!(result.admission_sequence, Some(5));

        let mut tampered = result;
        tampered.state = "failed".to_owned();
        assert!(validate_result_digest(&tampered).is_err());
    }

    #[test]
    fn v2_maximal_valid_result_has_a_stable_transport_bound() {
        let mut spec = v2_spec();
        spec.id = "i".repeat(128);
        spec.idempotency_key = "k".repeat(128);
        spec.action = "gpu.lease.release".to_owned();
        spec.args = serde_json::json!({"reason": "bounded"});
        spec.operation_id = Some("o".repeat(128));
        spec.subject_ref = Some("s".repeat(128));
        spec.receipt_worker_id = Some("w".repeat(32));
        spec.receipt_supervisor_generation = Some(u64::MAX);
        spec.receipt_cap_generation = Some(u64::MAX);
        spec.resolved_worker_slot = Some(u16::MAX);
        spec.resolved_lease_epoch = Some(u64::MAX);
        spec.admission_sequence = Some(u64::MAX);

        let line = build_result_line(
            &spec,
            HOST_TICKET_RESULT_V2_SCHEMA,
            "succeeded",
            Some(&"\\".repeat(192)),
            2048,
        )
        .expect("maximal result");
        assert_eq!(line.len(), HOST_TICKET_V2_MAX_ENCODED_RESULT_BYTES);
        let parsed = crate::claim::parse_result_lines_from(
            &[line],
            &[HOST_TICKET_RESULT_V2_SCHEMA.to_owned()],
            2048,
        )
        .expect("strict maximal result");
        assert_eq!(parsed.len(), 1);
    }
}
