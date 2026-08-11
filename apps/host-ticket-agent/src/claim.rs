// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Parse strict host ticket streams and derive idempotent claim keys.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::collections::HashSet;

use anyhow::{anyhow, Context, Result};
use serde_json::{Map, Value};

use crate::status::validate_result_digest;
use crate::{
    HostTicketResult, HostTicketSpec, ReceiptMode, HOST_TICKET_RESULT_V1_SCHEMA,
    HOST_TICKET_RESULT_V2_SCHEMA, HOST_TICKET_V1_SCHEMA, HOST_TICKET_V2_SCHEMA,
};

const TERMINAL_STATES: &[&str] = &["succeeded", "failed", "expired"];
const MAX_RELAY_HOP: u16 = 32;
const MAX_TOKEN_BYTES: usize = 128;
const MAX_REASON_BYTES: usize = 128;
const MAX_WORKER_ID_BYTES: usize = 32;

/// Maximum canonical caller-authored `host-ticket/v2` request encoded by the
/// strict field and argument bounds.
pub const HOST_TICKET_V2_MAX_ENCODED_RAW_SPEC_BYTES: usize = 1422;
/// Maximum canonical root-admitted `host-ticket/v2` snapshot encoded by the
/// strict field, argument, identity, and admission bounds.
pub const HOST_TICKET_V2_MAX_ENCODED_ADMITTED_SPEC_BYTES: usize = 1537;

/// Authority source of parsed ticket request bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecSource {
    /// Caller-authored append-only `/host/tickets/spec` input.
    RawRequest,
    /// Root-owned normalized `/host/tickets/spec.snapshot` projection.
    AdmittedSnapshot,
}

/// Stable idempotency key for host ticket processing.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TicketKey {
    /// Ticket id.
    pub id: String,
    /// Ticket idempotency key.
    pub idempotency_key: String,
}

impl TicketKey {
    /// Construct a key from borrowed parts.
    #[must_use]
    pub fn new(id: &str, idempotency_key: &str) -> Self {
        Self {
            id: id.to_owned(),
            idempotency_key: idempotency_key.to_owned(),
        }
    }

    /// Stable journal key that cannot collide through delimiter injection.
    #[must_use]
    pub fn journal_key(&self) -> String {
        format!("{}:{}:{}", self.id.len(), self.id, self.idempotency_key)
    }
}

/// Parse a legacy single-schema request stream.
pub fn parse_spec_lines(
    lines: &[String],
    request_schema: &str,
    max_line_bytes: u32,
) -> Result<Vec<HostTicketSpec>> {
    parse_spec_lines_from(
        lines,
        &[request_schema.to_owned()],
        max_line_bytes,
        SpecSource::RawRequest,
    )
}

/// Parse request lines using generated accepted schemas and their source authority.
pub fn parse_spec_lines_from(
    lines: &[String],
    accepted_schemas: &[String],
    max_line_bytes: u32,
    source: SpecSource,
) -> Result<Vec<HostTicketSpec>> {
    let mut specs = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        validate_line_bound("spec", idx, trimmed, max_line_bytes)?;
        let spec: HostTicketSpec = serde_json::from_str(trimmed)
            .with_context(|| format!("ticket spec line {} is not valid JSON", idx + 1))?;
        if !accepted_schemas.iter().any(|schema| schema == &spec.schema) {
            return Err(anyhow!(
                "ticket spec line {} schema '{}' is not in the generated accepted set",
                idx + 1,
                spec.schema
            ));
        }
        validate_spec(&spec, source)
            .with_context(|| format!("ticket spec line {} is invalid", idx + 1))?;
        specs.push(spec);
    }
    Ok(specs)
}

/// Parse `/host/tickets/status` or `/host/tickets/deadletter` JSON lines.
pub fn parse_result_lines(
    lines: &[String],
    result_schema: &str,
    max_line_bytes: u32,
) -> Result<Vec<HostTicketResult>> {
    parse_result_lines_from(lines, &[result_schema.to_owned()], max_line_bytes)
}

/// Parse result lines using the generated accepted result schema set.
pub fn parse_result_lines_from(
    lines: &[String],
    accepted_schemas: &[String],
    max_line_bytes: u32,
) -> Result<Vec<HostTicketResult>> {
    let mut results = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        validate_line_bound("result", idx, trimmed, max_line_bytes)?;
        let result: HostTicketResult = serde_json::from_str(trimmed)
            .with_context(|| format!("ticket result line {} is not valid JSON", idx + 1))?;
        if !accepted_schemas
            .iter()
            .any(|schema| schema == &result.schema)
        {
            return Err(anyhow!(
                "ticket result line {} schema '{}' is not in the generated accepted set",
                idx + 1,
                result.schema
            ));
        }
        validate_result(&result)
            .with_context(|| format!("ticket result line {} is invalid", idx + 1))?;
        results.push(result);
    }
    Ok(results)
}

/// Validate an already-deserialized request against the versioned field matrix.
pub fn validate_spec(spec: &HostTicketSpec, source: SpecSource) -> Result<()> {
    validate_token("id", spec.id.as_str())?;
    validate_token("idempotency_key", spec.idempotency_key.as_str())?;
    validate_token("action", spec.action.as_str())?;
    if spec.expires_unix_ms == Some(0) {
        return Err(anyhow!("expires_unix_ms must be nonzero when present"));
    }
    match spec.schema.as_str() {
        HOST_TICKET_V1_SCHEMA => validate_v1_spec(spec),
        HOST_TICKET_V2_SCHEMA => validate_v2_spec(spec, source),
        other => Err(anyhow!("unsupported ticket request schema {other}")),
    }
}

/// Validate strict version-2 action arguments before any executor is invoked.
pub fn validate_v2_action_args(spec: &HostTicketSpec) -> Result<()> {
    let args = spec
        .args
        .as_object()
        .ok_or_else(|| anyhow!("host-ticket/v2 args must be a JSON object"))?;
    match spec.action.as_str() {
        "gpu.lease.grant" => {
            validate_exact_keys(args, &["priority", "ttl_s"])?;
            validate_optional_u64(args, "ttl_s", 1, u32::MAX as u64)?;
            validate_optional_u64(args, "priority", 0, u8::MAX as u64)?;
        }
        "gpu.lease.renew" => {
            validate_exact_keys(args, &["priority", "ttl_s"])?;
            validate_optional_u64(args, "ttl_s", 1, u32::MAX as u64)?;
            validate_optional_u64(args, "priority", 0, u8::MAX as u64)?;
        }
        "gpu.lease.release" => {
            validate_exact_keys(args, &["reason"])?;
            if let Some(reason) = args.get("reason") {
                validate_bounded_text_value(reason, "reason", MAX_REASON_BYTES)?;
            }
        }
        "peft.export" | "peft.activate" | "peft.rollback" => {
            validate_exact_keys(args, &[])?;
        }
        "peft.import" => {
            validate_exact_keys(
                args,
                &[
                    "adapter_ref",
                    "adapter_sha256",
                    "job_id",
                    "lora_sha256",
                    "metrics_sha256",
                ],
            )?;
            validate_required_token_value(args, "adapter_ref")?;
            validate_required_token_value(args, "job_id")?;
            validate_optional_sha256(args, "adapter_sha256")?;
            validate_optional_sha256(args, "lora_sha256")?;
            validate_optional_sha256(args, "metrics_sha256")?;
        }
        other => return Err(anyhow!("action {other} is not a version-2 receipt action")),
    }
    Ok(())
}

/// Derive terminal ticket keys from parsed result entries.
#[must_use]
pub fn terminal_keys(results: &[HostTicketResult]) -> HashSet<TicketKey> {
    let mut terminal = HashSet::new();
    for result in results {
        if TERMINAL_STATES.contains(&result.state.as_str()) {
            terminal.insert(TicketKey::new(&result.id, &result.idempotency_key));
        }
    }
    terminal
}

/// Return whether an action belongs to the GPU receipt role.
#[must_use]
pub fn expected_receipt_role(action: &str) -> Option<&'static str> {
    if matches!(
        action,
        "gpu.lease.grant" | "gpu.lease.renew" | "gpu.lease.release"
    ) {
        Some("worker-gpu")
    } else if matches!(
        action,
        "peft.export" | "peft.import" | "peft.activate" | "peft.rollback"
    ) {
        Some("worker-lora")
    } else {
        None
    }
}

fn validate_v1_spec(spec: &HostTicketSpec) -> Result<()> {
    if !matches!(spec.receipt_mode, None | Some(ReceiptMode::None)) {
        return Err(anyhow!("host-ticket/v1 permits only receipt_mode=none"));
    }
    if has_worker_binding(spec) {
        return Err(anyhow!(
            "host-ticket/v1 must not contain version-2 Worker binding or admission fields"
        ));
    }
    validate_federation_fields(
        spec.source_hive.as_deref(),
        spec.target_hive.as_deref(),
        spec.relay_hop,
        spec.relay_correlation_id.as_deref(),
    )
}

fn validate_v2_spec(spec: &HostTicketSpec, source: SpecSource) -> Result<()> {
    if spec.receipt_mode != Some(ReceiptMode::Worker) {
        return Err(anyhow!("host-ticket/v2 requires receipt_mode=worker"));
    }
    if spec.source_hive.is_some()
        || spec.target_hive.is_some()
        || spec.relay_hop.is_some()
        || spec.relay_correlation_id.is_some()
    {
        return Err(anyhow!(
            "host-ticket/v2 is local-only and forbids federation/relay fields"
        ));
    }
    if spec.target.is_some() {
        return Err(anyhow!(
            "host-ticket/v2 forbids free-form target paths; use subject_ref"
        ));
    }
    validate_token(
        "operation_id",
        required_str(spec.operation_id.as_deref(), "operation_id")?,
    )?;
    validate_token(
        "subject_ref",
        required_str(spec.subject_ref.as_deref(), "subject_ref")?,
    )?;
    let role = required_str(spec.receipt_worker_role.as_deref(), "receipt_worker_role")?;
    validate_token("receipt_worker_role", role)?;
    validate_token(
        "receipt_worker_id",
        required_str(spec.receipt_worker_id.as_deref(), "receipt_worker_id")?,
    )?;
    if spec
        .receipt_worker_id
        .as_deref()
        .is_some_and(|worker_id| worker_id.len() > MAX_WORKER_ID_BYTES)
    {
        return Err(anyhow!(
            "receipt_worker_id exceeds {MAX_WORKER_ID_BYTES} bytes"
        ));
    }
    require_nonzero(
        spec.receipt_supervisor_generation,
        "receipt_supervisor_generation",
    )?;
    require_nonzero(spec.receipt_cap_generation, "receipt_cap_generation")?;
    let expected = expected_receipt_role(spec.action.as_str())
        .ok_or_else(|| anyhow!("action {} is not receipt-bearing", spec.action))?;
    if role != expected {
        return Err(anyhow!(
            "action {} requires receipt_worker_role={expected}, got {role}",
            spec.action
        ));
    }
    match source {
        SpecSource::RawRequest => {
            if spec.resolved_worker_slot.is_some()
                || spec.resolved_lease_epoch.is_some()
                || spec.admission_sequence.is_some()
            {
                return Err(anyhow!(
                    "caller-authored host-ticket/v2 must not contain root-owned admission fields"
                ));
            }
        }
        SpecSource::AdmittedSnapshot => {
            spec.resolved_worker_slot
                .ok_or_else(|| anyhow!("admitted host-ticket/v2 requires resolved_worker_slot"))?;
            require_nonzero(spec.resolved_lease_epoch, "resolved_lease_epoch")?;
            require_nonzero(spec.admission_sequence, "admission_sequence")?;
        }
    }
    validate_v2_action_args(spec)
}

fn validate_result(result: &HostTicketResult) -> Result<()> {
    validate_token("id", result.id.as_str())?;
    validate_token("idempotency_key", result.idempotency_key.as_str())?;
    validate_token("action", result.action.as_str())?;
    validate_token("state", result.state.as_str())?;
    match result.schema.as_str() {
        HOST_TICKET_RESULT_V1_SCHEMA => {
            validate_federation_fields(
                result.source_hive.as_deref(),
                result.target_hive.as_deref(),
                result.relay_hop,
                result.relay_correlation_id.as_deref(),
            )?;
            if result.receipt_mode == Some(ReceiptMode::Worker)
                || result.operation_id.is_some()
                || result.subject_ref.is_some()
                || result.receipt_worker_role.is_some()
                || result.receipt_worker_id.is_some()
                || result.receipt_supervisor_generation.is_some()
                || result.receipt_cap_generation.is_some()
                || result.resolved_worker_slot.is_some()
                || result.resolved_lease_epoch.is_some()
                || result.admission_sequence.is_some()
                || result.result_digest.is_some()
            {
                return Err(anyhow!("host-ticket-result/v1 contains version-2 fields"));
            }
        }
        HOST_TICKET_RESULT_V2_SCHEMA => {
            if result.receipt_mode != Some(ReceiptMode::Worker) {
                return Err(anyhow!(
                    "host-ticket-result/v2 requires receipt_mode=worker"
                ));
            }
            if result.source_hive.is_some()
                || result.target_hive.is_some()
                || result.relay_hop.is_some()
                || result.relay_correlation_id.is_some()
            {
                return Err(anyhow!("host-ticket-result/v2 is local-only"));
            }
            if result.message.as_deref().is_some_and(|message| {
                message.is_empty() || message.len() > 192 || message.chars().any(char::is_control)
            }) {
                return Err(anyhow!(
                    "host-ticket-result/v2 message must be control-free and at most 192 bytes"
                ));
            }
            let operation = required_str(result.operation_id.as_deref(), "operation_id")?;
            let subject = required_str(result.subject_ref.as_deref(), "subject_ref")?;
            let role = required_str(result.receipt_worker_role.as_deref(), "receipt_worker_role")?;
            validate_token("operation_id", operation)?;
            validate_token("subject_ref", subject)?;
            validate_token("receipt_worker_role", role)?;
            validate_token(
                "receipt_worker_id",
                required_str(result.receipt_worker_id.as_deref(), "receipt_worker_id")?,
            )?;
            if result
                .receipt_worker_id
                .as_deref()
                .is_some_and(|worker_id| worker_id.len() > MAX_WORKER_ID_BYTES)
            {
                return Err(anyhow!(
                    "receipt_worker_id exceeds {MAX_WORKER_ID_BYTES} bytes"
                ));
            }
            require_nonzero(
                result.receipt_supervisor_generation,
                "receipt_supervisor_generation",
            )?;
            require_nonzero(result.receipt_cap_generation, "receipt_cap_generation")?;
            result
                .resolved_worker_slot
                .ok_or_else(|| anyhow!("host-ticket-result/v2 requires resolved_worker_slot"))?;
            require_nonzero(result.resolved_lease_epoch, "resolved_lease_epoch")?;
            require_nonzero(result.admission_sequence, "admission_sequence")?;
            let expected_role = expected_receipt_role(result.action.as_str())
                .ok_or_else(|| anyhow!("result action {} is not receipt-bearing", result.action))?;
            if role != expected_role {
                return Err(anyhow!(
                    "result action {} requires receipt_worker_role={expected_role}",
                    result.action
                ));
            }
            validate_result_digest(result)?;
        }
        other => return Err(anyhow!("unsupported ticket result schema {other}")),
    }
    Ok(())
}

fn validate_line_bound(kind: &str, idx: usize, line: &str, max_line_bytes: u32) -> Result<()> {
    if line.len() > max_line_bytes as usize {
        return Err(anyhow!(
            "ticket {kind} line {} exceeds max_line_bytes {}",
            idx + 1,
            max_line_bytes
        ));
    }
    Ok(())
}

fn validate_token(label: &str, value: &str) -> Result<()> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("ticket {label} must not be empty"));
    }
    if trimmed.len() > MAX_TOKEN_BYTES {
        return Err(anyhow!("ticket {label} exceeds {MAX_TOKEN_BYTES} bytes"));
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | ':'))
    {
        return Err(anyhow!("ticket {label} contains invalid characters"));
    }
    if trimmed.starts_with('-') {
        return Err(anyhow!("ticket {label} must not start with '-'"));
    }
    Ok(())
}

fn required_str<'a>(value: Option<&'a str>, label: &str) -> Result<&'a str> {
    value.ok_or_else(|| anyhow!("host-ticket/v2 requires {label}"))
}

fn require_nonzero(value: Option<u64>, label: &str) -> Result<u64> {
    match value {
        Some(value) if value > 0 => Ok(value),
        _ => Err(anyhow!("host-ticket/v2 requires nonzero {label}")),
    }
}

fn has_worker_binding(spec: &HostTicketSpec) -> bool {
    spec.operation_id.is_some()
        || spec.subject_ref.is_some()
        || spec.receipt_worker_role.is_some()
        || spec.receipt_worker_id.is_some()
        || spec.receipt_supervisor_generation.is_some()
        || spec.receipt_cap_generation.is_some()
        || spec.resolved_worker_slot.is_some()
        || spec.resolved_lease_epoch.is_some()
        || spec.admission_sequence.is_some()
}

fn validate_exact_keys(args: &Map<String, Value>, allowed: &[&str]) -> Result<()> {
    for key in args.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(anyhow!("action args contain unsupported field {key}"));
        }
    }
    Ok(())
}

fn validate_optional_u64(args: &Map<String, Value>, key: &str, min: u64, max: u64) -> Result<()> {
    let Some(value) = args.get(key) else {
        return Ok(());
    };
    let parsed = value
        .as_u64()
        .ok_or_else(|| anyhow!("args.{key} must be an unsigned integer"))?;
    if !(min..=max).contains(&parsed) {
        return Err(anyhow!("args.{key} must be in range {min}..={max}"));
    }
    Ok(())
}

fn validate_bounded_text_value(value: &Value, key: &str, max_bytes: usize) -> Result<()> {
    let text = value
        .as_str()
        .ok_or_else(|| anyhow!("args.{key} must be a string"))?;
    if text.is_empty() || text.len() > max_bytes || text.chars().any(char::is_control) {
        return Err(anyhow!(
            "args.{key} must be non-empty, control-free, and at most {max_bytes} bytes"
        ));
    }
    Ok(())
}

fn validate_required_token_value(args: &Map<String, Value>, key: &str) -> Result<()> {
    let value = args
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("args.{key} must be a string"))?;
    validate_token(key, value)
}

fn validate_optional_sha256(args: &Map<String, Value>, key: &str) -> Result<()> {
    let Some(value) = args.get(key) else {
        return Ok(());
    };
    let value = value
        .as_str()
        .ok_or_else(|| anyhow!("args.{key} must be a string"))?;
    if value.len() != 64
        || value.bytes().any(|byte| {
            !byte.is_ascii_hexdigit() || (byte.is_ascii_alphabetic() && byte.is_ascii_uppercase())
        })
    {
        return Err(anyhow!(
            "args.{key} must be 64 lowercase hexadecimal characters"
        ));
    }
    Ok(())
}

fn validate_federation_fields(
    source_hive: Option<&str>,
    target_hive: Option<&str>,
    relay_hop: Option<u16>,
    relay_correlation_id: Option<&str>,
) -> Result<()> {
    if source_hive.is_some() != target_hive.is_some() {
        return Err(anyhow!(
            "ticket federation fields must set both source_hive and target_hive"
        ));
    }
    if let Some(source_hive) = source_hive {
        validate_token("source_hive", source_hive)?;
    }
    if let Some(target_hive) = target_hive {
        validate_token("target_hive", target_hive)?;
    }
    if let Some(relay_hop) = relay_hop {
        if relay_hop == 0 || relay_hop > MAX_RELAY_HOP {
            return Err(anyhow!(
                "ticket relay_hop must be in range 1..={MAX_RELAY_HOP}"
            ));
        }
    }
    if let Some(correlation_id) = relay_correlation_id {
        validate_token("relay_correlation_id", correlation_id)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v2_request_json(extra: &str) -> String {
        format!(
            "{{\"schema\":\"host-ticket/v2\",\"id\":\"a\",\"idempotency_key\":\"k1\",\"action\":\"gpu.lease.grant\",\"args\":{{\"ttl_s\":30}},\"receipt_mode\":\"worker\",\"operation_id\":\"lease-1\",\"subject_ref\":\"GPU-0\",\"receipt_worker_role\":\"worker-gpu\",\"receipt_worker_id\":\"gpu-worker-1\",\"receipt_supervisor_generation\":2,\"receipt_cap_generation\":3{extra}}}"
        )
    }

    #[test]
    fn parse_specs_and_terminal_dedupe() {
        let specs = vec![
            "{\"schema\":\"host-ticket/v1\",\"id\":\"a\",\"idempotency_key\":\"k1\",\"action\":\"systemd.restart\"}".to_owned(),
            "{\"schema\":\"host-ticket/v1\",\"id\":\"b\",\"idempotency_key\":\"k2\",\"action\":\"docker.stop\"}".to_owned(),
        ];
        let parsed = parse_spec_lines(&specs, HOST_TICKET_V1_SCHEMA, 2048)
            .unwrap_or_else(|err| unreachable!("parse specs: {err}"));
        assert_eq!(parsed.len(), 2);

        let status = vec![
            "{\"schema\":\"host-ticket-result/v1\",\"id\":\"a\",\"idempotency_key\":\"k1\",\"action\":\"systemd.restart\",\"state\":\"running\"}".to_owned(),
            "{\"schema\":\"host-ticket-result/v1\",\"id\":\"a\",\"idempotency_key\":\"k1\",\"action\":\"systemd.restart\",\"state\":\"succeeded\"}".to_owned(),
        ];
        let parsed_status = parse_result_lines(&status, HOST_TICKET_RESULT_V1_SCHEMA, 2048)
            .unwrap_or_else(|err| unreachable!("parse status: {err}"));
        let terminal = terminal_keys(&parsed_status);
        assert!(terminal.contains(&TicketKey::new("a", "k1")));
        assert!(!terminal.contains(&TicketKey::new("b", "k2")));
    }

    #[test]
    fn v2_raw_and_admitted_field_matrices_are_distinct() {
        let raw = vec![v2_request_json("")];
        parse_spec_lines_from(
            &raw,
            &[HOST_TICKET_V2_SCHEMA.to_owned()],
            2048,
            SpecSource::RawRequest,
        )
        .expect("valid raw v2 request");

        let admitted = vec![v2_request_json(
            ",\"resolved_worker_slot\":0,\"resolved_lease_epoch\":7,\"admission_sequence\":9",
        )];
        parse_spec_lines_from(
            &admitted,
            &[HOST_TICKET_V2_SCHEMA.to_owned()],
            2048,
            SpecSource::AdmittedSnapshot,
        )
        .expect("valid admitted v2 request");

        let forged = parse_spec_lines_from(
            &admitted,
            &[HOST_TICKET_V2_SCHEMA.to_owned()],
            2048,
            SpecSource::RawRequest,
        )
        .expect_err("raw root fields must fail");
        assert!(format!("{forged:#}").contains("root-owned"));
    }

    #[test]
    fn v2_rejects_cross_role_federation_and_unknown_args() {
        for line in [
            v2_request_json(",\"source_hive\":\"a\",\"target_hive\":\"b\""),
            v2_request_json("").replace("worker-gpu", "worker-lora"),
            v2_request_json("").replace("\"ttl_s\":30", "\"ttl_s\":30,\"path\":\"/tmp/x\""),
        ] {
            assert!(parse_spec_lines_from(
                &[line],
                &[HOST_TICKET_V2_SCHEMA.to_owned()],
                2048,
                SpecSource::RawRequest,
            )
            .is_err());
        }
    }

    #[test]
    fn v2_rejects_noncanonical_uppercase_hashes() {
        let mut spec: HostTicketSpec =
            serde_json::from_str(&v2_request_json("")).expect("parse request fixture");
        spec.action = "peft.import".to_owned();
        spec.receipt_worker_role = Some("worker-lora".to_owned());
        spec.args = serde_json::json!({
            "adapter_ref": "adapter-1",
            "adapter_sha256": "A".repeat(64),
            "job_id": "job-1",
        });
        assert!(validate_v2_action_args(&spec).is_err());
    }

    #[test]
    fn v2_maximal_spec_bounds_are_stable_and_parseable() {
        let mut spec = HostTicketSpec {
            schema: HOST_TICKET_V2_SCHEMA.to_owned(),
            id: "i".repeat(MAX_TOKEN_BYTES),
            idempotency_key: "k".repeat(MAX_TOKEN_BYTES),
            action: "peft.import".to_owned(),
            args: serde_json::json!({
                "adapter_ref": "a".repeat(MAX_TOKEN_BYTES),
                "adapter_sha256": "a".repeat(64),
                "job_id": "j".repeat(MAX_TOKEN_BYTES),
                "lora_sha256": "b".repeat(64),
                "metrics_sha256": "c".repeat(64),
            }),
            expires_unix_ms: Some(u64::MAX),
            receipt_mode: Some(ReceiptMode::Worker),
            operation_id: Some("o".repeat(MAX_TOKEN_BYTES)),
            subject_ref: Some("s".repeat(MAX_TOKEN_BYTES)),
            receipt_worker_role: Some("worker-lora".to_owned()),
            receipt_worker_id: Some("w".repeat(MAX_WORKER_ID_BYTES)),
            receipt_supervisor_generation: Some(u64::MAX),
            receipt_cap_generation: Some(u64::MAX),
            ..HostTicketSpec::default()
        };
        let raw = serde_json::to_string(&spec).expect("encode maximal raw request");
        assert_eq!(raw.len(), HOST_TICKET_V2_MAX_ENCODED_RAW_SPEC_BYTES);
        parse_spec_lines_from(
            &[raw],
            &[HOST_TICKET_V2_SCHEMA.to_owned()],
            2048,
            SpecSource::RawRequest,
        )
        .expect("parse maximal raw request");

        spec.resolved_worker_slot = Some(u16::MAX);
        spec.resolved_lease_epoch = Some(u64::MAX);
        spec.admission_sequence = Some(u64::MAX);
        let admitted = serde_json::to_string(&spec).expect("encode maximal admitted request");
        assert_eq!(
            admitted.len(),
            HOST_TICKET_V2_MAX_ENCODED_ADMITTED_SPEC_BYTES
        );
        parse_spec_lines_from(
            &[admitted],
            &[HOST_TICKET_V2_SCHEMA.to_owned()],
            2048,
            SpecSource::AdmittedSnapshot,
        )
        .expect("parse maximal admitted request");
    }

    #[test]
    fn v2_rejects_oversized_identity_and_wrong_arg_types() {
        let oversized = v2_request_json("").replace("lease-1", &"x".repeat(129));
        let wrong_type = v2_request_json("").replace("\"ttl_s\":30", "\"ttl_s\":\"30\"");
        for line in [oversized, wrong_type] {
            assert!(parse_spec_lines_from(
                &[line],
                &[HOST_TICKET_V2_SCHEMA.to_owned()],
                2048,
                SpecSource::RawRequest,
            )
            .is_err());
        }
    }

    #[test]
    fn v1_forbids_worker_receipt_fields_but_preserves_federation() {
        let federated = vec![
            "{\"schema\":\"host-ticket/v1\",\"id\":\"a\",\"idempotency_key\":\"k1\",\"action\":\"systemd.restart\",\"receipt_mode\":\"none\",\"source_hive\":\"hive-a\",\"target_hive\":\"hive-b\",\"relay_hop\":1,\"relay_correlation_id\":\"a:k1:hive-a:hive-b\"}".to_owned(),
        ];
        assert!(parse_spec_lines(&federated, HOST_TICKET_V1_SCHEMA, 2048).is_ok());

        let invalid = vec![federated[0].replace(
            "\"receipt_mode\":\"none\"",
            "\"receipt_mode\":\"worker\",\"operation_id\":\"op-1\"",
        )];
        assert!(parse_spec_lines(&invalid, HOST_TICKET_V1_SCHEMA, 2048).is_err());
    }
}
