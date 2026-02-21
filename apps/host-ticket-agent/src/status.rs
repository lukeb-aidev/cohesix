// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Build and append bounded host ticket status receipts.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use anyhow::{anyhow, Result};
use cohsh::{Session, Transport};

use crate::{HostTicketResult, HostTicketSpec};

/// Render a result receipt line for `/host/tickets/status` or `/host/tickets/deadletter`.
pub fn build_result_line(
    spec: &HostTicketSpec,
    result_schema: &str,
    state: &str,
    message: Option<&str>,
    max_line_bytes: u32,
) -> Result<String> {
    let cleaned_message = message
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(sanitize_message);
    let result = HostTicketResult {
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
    };
    let mut line = serde_json::to_string(&result)?;
    if line.len() > max_line_bytes as usize {
        let fallback = truncate_to_bytes(
            result
                .message
                .as_deref()
                .unwrap_or("status exceeded max_line_bytes"),
            96,
        );
        let compact = HostTicketResult {
            schema: result.schema,
            id: result.id,
            idempotency_key: result.idempotency_key,
            action: result.action,
            state: result.state,
            message: Some(fallback),
            source_hive: result.source_hive,
            target_hive: result.target_hive,
            relay_hop: result.relay_hop,
            relay_correlation_id: result.relay_correlation_id,
        };
        line = serde_json::to_string(&compact)?;
    }
    if line.len() > max_line_bytes as usize {
        return Err(anyhow!(
            "ticket result line exceeds max_line_bytes {}",
            max_line_bytes
        ));
    }
    Ok(line)
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

fn sanitize_message(input: &str) -> String {
    input
        .chars()
        .map(|ch| if ch == '\n' || ch == '\r' { ' ' } else { ch })
        .collect()
}

fn truncate_to_bytes(input: &str, max_bytes: usize) -> String {
    if input.len() <= max_bytes {
        return input.to_owned();
    }
    let mut out = String::new();
    for ch in input.chars() {
        if out.len().saturating_add(ch.len_utf8()) > max_bytes {
            break;
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn line_builder_enforces_bounds() {
        let spec = HostTicketSpec {
            schema: "host-ticket/v1".to_owned(),
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
        };
        let line = build_result_line(
            &spec,
            "host-ticket-result/v1",
            "failed",
            Some("line one\nline two"),
            512,
        )
        .unwrap_or_else(|err| unreachable!("build line: {err}"));
        assert!(line.contains("line one line two"));

        let err = build_result_line(
            &spec,
            "host-ticket-result/v1",
            "failed",
            Some(&"x".repeat(3000)),
            64,
        )
        .unwrap_err();
        assert!(err.to_string().contains("max_line_bytes"));
    }
}
