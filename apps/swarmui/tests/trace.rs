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
use cohesix_ticket::Role;
use cohsh_core::command::MAX_LINE_LEN;
use cohsh_core::trace::{TraceError, TraceLog, TracePolicy};
use swarmui::{
    SwarmUiBackend, SwarmUiConfig, SwarmUiHiveAgent, SwarmUiHiveEvent, SwarmUiHiveEventKind,
    SwarmUiHiveSnapshot, TraceTransportFactory,
};

const TRACE_ENV: &str = "COHESIX_WRITE_TRACE";
const SCENARIO: &str = "trace_v0";
const WORKER_ID: &str = "worker-1";
const HIVE_CREATED_MS: u64 = 1_735_000_000_000;
const HIVE_EVENT_TARGET: usize = 3600;

#[test]
fn trace_hive_fixture_matches_payload() -> Result<()> {
    let snapshot = trace_hive_snapshot();
    let payload = serde_cbor::to_vec(&snapshot).context("encode hive snapshot")?;
    let path = trace_hive_fixture_path();
    if std::env::var(TRACE_ENV).is_ok() {
        fs::create_dir_all(path.parent().unwrap()).context("create hive fixture dir")?;
        fs::write(&path, &payload).context("write hive fixture")?;
    }
    let fixture = fs::read(&path).context("read hive fixture")?;
    assert_eq!(
        payload, fixture,
        "hive fixture mismatch: regenerate with {TRACE_ENV}=1"
    );
    Ok(())
}

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
    fs::read(&trace_path).with_context(|| format!("read trace fixture {}", trace_path.display()))
}

fn trace_hive_fixture_path() -> PathBuf {
    transcript_support::repo_root()
        .join("tests")
        .join("fixtures")
        .join("traces")
        .join(format!("{SCENARIO}.hive.cbor"))
}

fn trace_hive_snapshot() -> SwarmUiHiveSnapshot {
    let queen = SwarmUiHiveAgent {
        id: "queen".to_owned(),
        role: "queen".to_owned(),
        namespace: "/queen".to_owned(),
    };
    let workers = vec![
        SwarmUiHiveAgent {
            id: "worker-heart-1".to_owned(),
            role: "worker-heartbeat".to_owned(),
            namespace: "/worker/worker-heart-1".to_owned(),
        },
        SwarmUiHiveAgent {
            id: "worker-heart-2".to_owned(),
            role: "worker-heartbeat".to_owned(),
            namespace: "/worker/worker-heart-2".to_owned(),
        },
        SwarmUiHiveAgent {
            id: "worker-heart-3".to_owned(),
            role: "worker-heartbeat".to_owned(),
            namespace: "/worker/worker-heart-3".to_owned(),
        },
        SwarmUiHiveAgent {
            id: "worker-gpu-1".to_owned(),
            role: "worker-gpu".to_owned(),
            namespace: "/worker/worker-gpu-1".to_owned(),
        },
        SwarmUiHiveAgent {
            id: "worker-gpu-2".to_owned(),
            role: "worker-gpu".to_owned(),
            namespace: "/worker/worker-gpu-2".to_owned(),
        },
        SwarmUiHiveAgent {
            id: "worker-gpu-3".to_owned(),
            role: "worker-gpu".to_owned(),
            namespace: "/worker/worker-gpu-3".to_owned(),
        },
        SwarmUiHiveAgent {
            id: "worker-heart-4".to_owned(),
            role: "worker-heartbeat".to_owned(),
            namespace: "/worker/worker-heart-4".to_owned(),
        },
        SwarmUiHiveAgent {
            id: "worker-heart-5".to_owned(),
            role: "worker-heartbeat".to_owned(),
            namespace: "/worker/worker-heart-5".to_owned(),
        },
    ];

    let mut events = Vec::with_capacity(HIVE_EVENT_TARGET);
    let mut seq = 0u64;
    'outer: for cycle in 0..800u64 {
        for (idx, agent) in workers.iter().enumerate() {
            let detail = format!("tick {cycle}.{idx}");
            events.push(hive_event(
                seq,
                SwarmUiHiveEventKind::Telemetry,
                agent,
                &detail,
            ));
            seq += 1;
            if events.len() >= HIVE_EVENT_TARGET {
                break 'outer;
            }
        }
        if cycle % 9 == 0 {
            events.push(hive_event(
                seq,
                SwarmUiHiveEventKind::Error,
                &workers[3],
                "ERR lease expired",
            ));
            seq += 1;
            if events.len() >= HIVE_EVENT_TARGET {
                break 'outer;
            }
        }
        if cycle % 11 == 0 {
            events.push(hive_event(
                seq,
                SwarmUiHiveEventKind::Error,
                &workers[1],
                "ERR heartbeat drift",
            ));
            seq += 1;
            if events.len() >= HIVE_EVENT_TARGET {
                break 'outer;
            }
        }
        if cycle % 17 == 0 {
            events.push(hive_event(
                seq,
                SwarmUiHiveEventKind::Error,
                &workers[4],
                "ERR thermal spike",
            ));
            seq += 1;
            if events.len() >= HIVE_EVENT_TARGET {
                break 'outer;
            }
        }
    }

    let mut agents = Vec::with_capacity(workers.len() + 1);
    agents.push(queen);
    agents.extend(workers);

    SwarmUiHiveSnapshot {
        version: 1,
        created_ms: HIVE_CREATED_MS,
        agents,
        events,
    }
}

fn hive_event(
    seq: u64,
    kind: SwarmUiHiveEventKind,
    agent: &SwarmUiHiveAgent,
    detail: &str,
) -> SwarmUiHiveEvent {
    SwarmUiHiveEvent {
        seq,
        kind,
        agent: agent.id.clone(),
        role: Some(agent.role.clone()),
        namespace: agent.namespace.clone(),
        detail: Some(detail.to_owned()),
        reason: None,
    }
}
