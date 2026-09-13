// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Protect non-authoritative case correlation, source links, scenario framing and ambiguity.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use anyhow::Result;
use coh::evidence_timeline::{write_timeline_with_scenario, Scenario};
use serde_json::{json, Value};
use std::fs;
use tempfile::TempDir;

#[test]
fn scenarios_retain_exact_links_and_never_upgrade_terminal_records() -> Result<()> {
    for scenario in [
        Scenario::Generic,
        Scenario::Incident,
        Scenario::Change,
        Scenario::Maintenance,
        Scenario::Rollout,
        Scenario::Federation,
    ] {
        let temp = TempDir::new()?;
        fs::create_dir_all(temp.path().join("host/tickets"))?;
        let identity = json!({"id":"task:1","idempotency_key":"attempt:1","action":"systemd.restart","source_hive":"a","target_hive":"b","relay_correlation_id":"relay-1","relay_hop":1});
        fs::write(temp.path().join("host/tickets/spec"), identity.to_string())?;
        let mut result = identity;
        result["state"] = json!("succeeded");
        result["message"] = json!("token=SECRET_CANARY");
        fs::write(temp.path().join("host/tickets/status"), result.to_string())?;
        write_timeline_with_scenario(temp.path(), scenario)?;
        let bytes = fs::read(temp.path().join("case.json"))?;
        let case: Value = serde_json::from_slice(&bytes)?;
        assert_eq!(case["proof"], "none");
        assert_eq!(case["chains"][0]["outcome"], "recorded-terminal");
        assert_eq!(
            case["chains"][0]["correlation"],
            json!(["task:1", "attempt:1", "a", "b"])
        );
        let timeline = fs::read_to_string(temp.path().join("timeline.ndjson"))?;
        let events: Vec<Value> = timeline
            .lines()
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()?;
        for stage in case["chains"][0]["stages"].as_array().unwrap() {
            for link in stage["sources"].as_array().unwrap() {
                let index = link["timeline_event"].as_u64().unwrap() as usize;
                assert_eq!(link["path"], events[index]["source"]);
                // Digest binds the exact canonical NDJSON event, excluding the newline.
                assert_eq!(
                    link["event_sha256"],
                    coh::operator::digest(timeline.lines().nth(index).unwrap().as_bytes())
                );
            }
        }
        for file in ["case.json", "case.md", "timeline.ndjson", "timeline.md"] {
            assert!(!fs::read_to_string(temp.path().join(file))?.contains("SECRET_CANARY"));
        }
        write_timeline_with_scenario(temp.path(), scenario)?;
        assert_eq!(bytes, fs::read(temp.path().join("case.json"))?);
        result["state"] = json!("failed");
        let prior = fs::read_to_string(temp.path().join("host/tickets/status"))?;
        fs::write(
            temp.path().join("host/tickets/status"),
            format!("{prior}\n{result}"),
        )?;
        write_timeline_with_scenario(temp.path(), scenario)?;
        let case: Value = serde_json::from_slice(&fs::read(temp.path().join("case.json"))?)?;
        assert_eq!(case["chains"][0]["outcome"], "ambiguous");
    }
    Ok(())
}

#[test]
fn canonical_case_fixture_is_shared_with_python() -> Result<()> {
    let source =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/operator/case");
    let temp = TempDir::new()?;
    fs::create_dir_all(temp.path().join("host/tickets"))?;
    for path in ["summary.json", "host/tickets/spec", "host/tickets/status"] {
        fs::copy(source.join(path), temp.path().join(path))?;
    }
    write_timeline_with_scenario(temp.path(), Scenario::Federation)?;
    for path in ["case.json", "case.md", "timeline.ndjson", "timeline.md"] {
        assert_eq!(
            fs::read(source.join(path))?,
            fs::read(temp.path().join(path))?,
            "{path}"
        );
    }
    Ok(())
}
