// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate SwarmUI trace replay fixtures.
// Author: Lukas Bower

#[path = "../../../tests/fixtures/transcripts/support.rs"]
mod transcript_support;

use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result};
use cohsh_core::command::MAX_LINE_LEN;
use cohsh_core::trace::{TraceError, TraceLog, TracePolicy};
use cohesix_ticket::Role;
use swarmui::{SwarmUiBackend, SwarmUiConfig, TraceTransportFactory};

const SCENARIO: &str = "trace_v0";
const WORKER_ID: &str = "worker-1";

#[test]
fn trace_replay_matches_fixture() -> Result<()> {
    let start = Instant::now();
    let payload = load_trace_fixture()?;
    let data_dir = std::env::temp_dir();
    let config = SwarmUiConfig::from_generated(data_dir);
    let policy = TracePolicy::new(
        config.trace_max_bytes as u32,
        swarmui::SECURE9P_MSIZE,
        MAX_LINE_LEN as u32,
    );
    let trace = TraceLog::decode(&payload, policy).context("decode trace fixture")?;
    let factory = TraceTransportFactory::new(trace.frames);
    let mut backend = SwarmUiBackend::new(config, factory);

    let mut transcript = Vec::new();
    let attach = backend.attach(Role::Queen, None);
    transcript.extend(attach.lines);

    let list = backend.list_namespace(Role::Queen, None, "/worker");
    transcript.extend(list.lines);

    let tail = backend.tail_telemetry(Role::Queen, None, WORKER_ID);
    transcript.extend(tail.lines);
    assert_eq!(backend.active_tails(), 0);

    transcript_support::compare_transcript("swarmui", SCENARIO, "swarmui.txt", &transcript);
    transcript_support::write_timing(
        "swarmui",
        SCENARIO,
        "trace-replay",
        start.elapsed().as_millis() as u64,
    );
    Ok(())
}

#[test]
fn trace_tamper_is_rejected() -> Result<()> {
    let mut payload = load_trace_fixture()?;
    let last = payload.len().saturating_sub(1);
    payload[last] ^= 0xff;
    let data_dir = std::env::temp_dir();
    let config = SwarmUiConfig::from_generated(data_dir);
    let policy = TracePolicy::new(
        config.trace_max_bytes as u32,
        swarmui::SECURE9P_MSIZE,
        MAX_LINE_LEN as u32,
    );
    let err = TraceLog::decode(&payload, policy).expect_err("tampered trace should fail");
    assert_eq!(err, TraceError::HashMismatch);
    Ok(())
}

fn trace_fixture_path() -> PathBuf {
    transcript_support::repo_root()
        .join("tests")
        .join("fixtures")
        .join("traces")
        .join(format!("{SCENARIO}.trace"))
}

fn load_trace_fixture() -> Result<Vec<u8>> {
    let trace_path = trace_fixture_path();
    fs::read(&trace_path)
        .with_context(|| format!("read trace fixture {}", trace_path.display()))
}
