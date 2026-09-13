// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate evidence timeline generation behavior.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use anyhow::{Context, Result};
use coh::evidence_timeline::write_timeline;
use serde_json::Value;
use tempfile::TempDir;

#[test]
fn evidence_timeline_preserves_structured_and_legacy_audit_errors() -> Result<()> {
    let temp = TempDir::new()?;
    std::fs::create_dir(temp.path().join("audit"))?;
    let records = [
        serde_json::json!({"code": "Permission", "message": "EPERM"}),
        serde_json::json!("legacy refusal"),
        Value::Null,
    ];
    let journal = records
        .into_iter()
        .enumerate()
        .map(|(index, error)| {
            serde_json::json!({
                "seq": index + 1, "kind": "host-control",
                "path": "/queen/lifecycle/ctl", "payload": "drain",
                "outcome": "err", "error": error, "role": "queen",
                "ticket": "sha256:fixture"
            })
            .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(temp.path().join("audit/journal"), journal)?;
    let summary = write_timeline(temp.path())?;
    let output = std::fs::read_to_string(summary.ndjson_path)?;
    let events = output
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(events.len(), 3);
    assert_eq!(events[0]["error"], "Permission: EPERM");
    assert_eq!(events[1]["error"], "legacy refusal");
    assert!(events[2].get("error").is_none());
    assert_eq!(events[0]["outcome"], "err");
    Ok(())
}

#[test]
fn evidence_timeline_rejects_malformed_structured_audit_error() -> Result<()> {
    let temp = TempDir::new()?;
    std::fs::create_dir(temp.path().join("audit"))?;
    std::fs::write(
        temp.path().join("audit/journal"),
        r#"{"seq":1,"kind":"host-control","path":"/queen/lifecycle/ctl","payload":"drain","outcome":"err","error":{"code":1,"message":"EPERM"},"role":"queen","ticket":"sha256:fixture"}"#,
    )?;
    assert!(write_timeline(temp.path()).is_err());
    assert!(!temp.path().join("timeline.ndjson").exists());
    Ok(())
}

#[test]
fn evidence_timeline_is_deterministic_for_fixed_pack() -> Result<()> {
    let temp = TempDir::new().expect("tempdir");
    let pack = temp.path();

    std::fs::create_dir_all(pack.join("audit"))?;
    std::fs::create_dir_all(pack.join("proc").join("lease"))?;

    // Out-of-order seq to validate sorting.
    let journal = concat!(
        "{\"seq\":2,\"kind\":\"queen-ctl\",\"path\":\"/queen/ctl\",\"payload\":\"{}\",\"outcome\":\"ok\",\"error\":null,\"role\":\"queen\",\"ticket\":\"sha256:dead\"}\n",
        "{\"seq\":1,\"kind\":\"queen-ctl\",\"path\":\"/queen/ctl\",\"payload\":\"{}\",\"outcome\":\"ok\",\"error\":null,\"role\":\"queen\",\"ticket\":\"sha256:beef\"}\n"
    );
    std::fs::write(pack.join("audit").join("journal"), journal)?;

    let decisions = "{\"seq\":3,\"kind\":\"policy-gate\",\"outcome\":\"approve\",\"id\":\"a1\",\"target\":\"/queen/ctl\",\"path\":\"/queen/ctl\",\"role\":\"queen\",\"ticket\":\"sha256:cafe\"}\n";
    std::fs::write(pack.join("audit").join("decisions"), decisions)?;

    let lease_active =
        "id=lease-1 subject=queen resource=gpu0 ttl_s=60 priority=1 state=ACTIVE seq=7\n";
    std::fs::write(pack.join("proc").join("lease").join("active"), lease_active)?;

    let summary = write_timeline(pack)?;
    let ndjson = std::fs::read_to_string(&summary.ndjson_path)
        .with_context(|| format!("read {}", summary.ndjson_path.display()))?;
    let lines = ndjson
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), 4);

    let first: Value = serde_json::from_str(lines[0]).context("parse first ndjson")?;
    assert_eq!(first.get("seq").and_then(Value::as_u64), Some(1));

    let second: Value = serde_json::from_str(lines[1]).context("parse second ndjson")?;
    assert_eq!(second.get("seq").and_then(Value::as_u64), Some(2));

    Ok(())
}

#[test]
fn evidence_timeline_correlates_host_ticket_streams() -> Result<()> {
    let temp = TempDir::new().expect("tempdir");
    let pack = temp.path();

    std::fs::create_dir_all(pack.join("host").join("tickets"))?;

    let spec = "{\"schema\":\"host-ticket/v1\",\"id\":\"ticket-1\",\"idempotency_key\":\"idem-1\",\"action\":\"systemd.restart\",\"target\":\"/host/systemd/cohesix-agent.service/restart\",\"source_hive\":\"hive-a\",\"target_hive\":\"hive-b\",\"relay_hop\":1,\"relay_correlation_id\":\"ticket-1:idem-1:hive-a:hive-b\"}\n";
    std::fs::write(pack.join("host").join("tickets").join("spec"), spec)?;
    let status = "{\"schema\":\"host-ticket-result/v1\",\"id\":\"ticket-1\",\"idempotency_key\":\"idem-1\",\"action\":\"systemd.restart\",\"state\":\"succeeded\",\"message\":\"ok\",\"source_hive\":\"hive-a\",\"target_hive\":\"hive-b\",\"relay_hop\":2,\"relay_correlation_id\":\"ticket-1:idem-1:hive-a:hive-b\"}\n";
    std::fs::write(pack.join("host").join("tickets").join("status"), status)?;

    let summary = write_timeline(pack)?;
    let ndjson = std::fs::read_to_string(&summary.ndjson_path)
        .with_context(|| format!("read {}", summary.ndjson_path.display()))?;
    assert!(ndjson.contains("\"kind\":\"host-ticket.spec\""));
    assert!(ndjson.contains("\"kind\":\"host-ticket.status\""));
    assert!(ndjson.contains("\"correlation_key\":\"ticket-1:idem-1:hive-a:hive-b\""));
    assert!(ndjson.contains("\"relay_hop\":2"));

    let markdown = std::fs::read_to_string(&summary.markdown_path)
        .with_context(|| format!("read {}", summary.markdown_path.display()))?;
    assert!(markdown.contains("ticket=ticket-1:idem-1:hive-a:hive-b"));
    assert!(markdown.contains("source_hive=hive-a"));
    assert!(markdown.contains("target_hive=hive-b"));

    Ok(())
}
