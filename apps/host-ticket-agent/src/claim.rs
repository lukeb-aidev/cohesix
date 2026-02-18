// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Parse host ticket streams and derive idempotent claim keys.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::collections::HashSet;

use anyhow::{anyhow, Context, Result};

use crate::{HostTicketResult, HostTicketSpec};

const TERMINAL_STATES: &[&str] = &["succeeded", "failed", "expired"];

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
}

/// Parse `/host/tickets/spec` JSON lines.
pub fn parse_spec_lines(
    lines: &[String],
    request_schema: &str,
    max_line_bytes: u32,
) -> Result<Vec<HostTicketSpec>> {
    let mut specs = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.as_bytes().len() > max_line_bytes as usize {
            return Err(anyhow!(
                "ticket spec line {} exceeds max_line_bytes {}",
                idx + 1,
                max_line_bytes
            ));
        }
        let spec: HostTicketSpec = serde_json::from_str(trimmed).with_context(|| {
            format!("ticket spec line {} is not valid JSON", idx + 1)
        })?;
        if spec.schema != request_schema {
            return Err(anyhow!(
                "ticket spec line {} schema '{}' does not match '{}'",
                idx + 1,
                spec.schema,
                request_schema
            ));
        }
        validate_token("id", spec.id.as_str())?;
        validate_token("idempotency_key", spec.idempotency_key.as_str())?;
        validate_token("action", spec.action.as_str())?;
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
    let mut results = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.as_bytes().len() > max_line_bytes as usize {
            return Err(anyhow!(
                "ticket result line {} exceeds max_line_bytes {}",
                idx + 1,
                max_line_bytes
            ));
        }
        let result: HostTicketResult = serde_json::from_str(trimmed).with_context(|| {
            format!("ticket result line {} is not valid JSON", idx + 1)
        })?;
        if result.schema != result_schema {
            return Err(anyhow!(
                "ticket result line {} schema '{}' does not match '{}'",
                idx + 1,
                result.schema,
                result_schema
            ));
        }
        validate_token("id", result.id.as_str())?;
        validate_token("idempotency_key", result.idempotency_key.as_str())?;
        validate_token("state", result.state.as_str())?;
        results.push(result);
    }
    Ok(results)
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

fn validate_token(label: &str, value: &str) -> Result<()> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("ticket {label} must not be empty"));
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | ':'))
    {
        return Err(anyhow!("ticket {label} contains invalid characters"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_specs_and_terminal_dedupe() {
        let specs = vec![
            "{\"schema\":\"host-ticket/v1\",\"id\":\"a\",\"idempotency_key\":\"k1\",\"action\":\"systemd.restart\"}".to_owned(),
            "{\"schema\":\"host-ticket/v1\",\"id\":\"b\",\"idempotency_key\":\"k2\",\"action\":\"docker.stop\"}".to_owned(),
        ];
        let parsed = parse_spec_lines(&specs, "host-ticket/v1", 2048).expect("parse specs");
        assert_eq!(parsed.len(), 2);

        let status = vec![
            "{\"schema\":\"host-ticket-result/v1\",\"id\":\"a\",\"idempotency_key\":\"k1\",\"action\":\"systemd.restart\",\"state\":\"running\"}".to_owned(),
            "{\"schema\":\"host-ticket-result/v1\",\"id\":\"a\",\"idempotency_key\":\"k1\",\"action\":\"systemd.restart\",\"state\":\"succeeded\"}".to_owned(),
        ];
        let parsed_status =
            parse_result_lines(&status, "host-ticket-result/v1", 2048).expect("parse status");
        let terminal = terminal_keys(&parsed_status);
        assert!(terminal.contains(&TicketKey::new("a", "k1")));
        assert!(!terminal.contains(&TicketKey::new("b", "k2")));
    }

    #[test]
    fn schema_mismatch_is_rejected() {
        let specs = vec![
            "{\"schema\":\"wrong\",\"id\":\"a\",\"idempotency_key\":\"k1\",\"action\":\"systemd.restart\"}".to_owned(),
        ];
        let err = parse_spec_lines(&specs, "host-ticket/v1", 2048).unwrap_err();
        assert!(err.to_string().contains("does not match"));
    }
}
