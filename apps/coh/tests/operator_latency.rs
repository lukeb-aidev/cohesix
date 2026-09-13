// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Bound representative operator artifact work and report host-only latency samples.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use anyhow::Result;
use coh::operator::{self, Availability, Observation, Snapshot, SNAPSHOT_SCHEMA};
use std::time::Instant;

#[test]
fn representative_readonly_artifacts_have_finite_work_and_latency_samples() -> Result<()> {
    let mut samples = Vec::new();
    for size in [0, 4096, 65536, 524288] {
        let content = "x".repeat(size);
        let snapshot = Snapshot {
            schema: SNAPSHOT_SCHEMA.to_owned(),
            source_class: "offline-pack".to_owned(),
            observations: vec![Observation {
                path: "/proc/boot".to_owned(),
                status: Availability::Observed,
                content: Some(content),
                reason: None,
            }],
            violations: vec![],
        };
        let pack = tempfile::TempDir::new()?;
        std::fs::write(
            pack.path().join("boot"),
            snapshot.observations[0].content.as_deref().unwrap(),
        )?;
        std::fs::write(
            pack.path().join("summary.json"),
            serde_json::to_vec(&serde_json::json!({
                "schema":"cohesix-evidence-pack/summary-v1", "captured":1, "missing":0, "errors":0,
                "items":[{"path":"/proc/boot","saved_as":"boot","status":"captured"}]
            }))?,
        )?;
        for _ in 0..8 {
            let start = Instant::now();
            let snapshot = operator::inspect_pack(pack.path())?;
            assert!(snapshot.render()?.len() <= operator::MAX_BYTES);
            assert!(operator::diff(&snapshot, &snapshot)?.is_empty());
            assert_ne!(operator::attest(&snapshot).verdict, "PASS");
            samples.push(start.elapsed().as_micros());
        }
    }
    samples.sort_unstable();
    // Correctness gates use finite input/output bounds; timings are measurements,
    // not wall-clock assertions or substitutes for a same-harness baseline.
    println!("class=host-operator-latency samples={} p50_us={} p95_us={} max_input_bytes=524288 target_proof=none", samples.len(), samples[15], samples[30]);
    let oversized = "x".repeat(operator::MAX_BYTES + 1);
    let temp = tempfile::TempDir::new()?;
    std::fs::write(temp.path().join("oversized"), oversized)?;
    assert!(operator::read_bounded(&temp.path().join("oversized"), operator::MAX_BYTES).is_err());
    Ok(())
}
