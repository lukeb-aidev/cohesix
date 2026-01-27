// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate SwarmUI hive replay determinism and snapshot expiry handling.
// Author: Lukas Bower

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use tempfile::TempDir;

use cohesix_ticket::Role;
use swarmui::{
    SwarmUiBackend, SwarmUiConfig, SwarmUiHiveAgent, SwarmUiHiveEvent, SwarmUiHiveEventKind,
    SwarmUiHiveSnapshot, SwarmUiTransportFactory,
};

struct NoConnectFactory {
    calls: Arc<Mutex<usize>>,
}

impl SwarmUiTransportFactory for NoConnectFactory {
    type Transport = cohsh::client::InProcessTransport;

    fn connect(&self) -> Result<Self::Transport, swarmui::SwarmUiError> {
        let mut guard = self.calls.lock().unwrap();
        *guard += 1;
        Err(swarmui::SwarmUiError::Transport(
            "network disabled".to_owned(),
        ))
    }
}

const DEMO_CREATED_MS: u64 = 1_725_000_000_000;
const DEMO_DIGEST: u64 = 0x8ca29cb91bf32073;

#[test]
fn demo_snapshot_fixture_matches_payload() -> Result<()> {
    let snapshot = demo_snapshot();
    let payload = serde_cbor::to_vec(&snapshot).context("encode demo snapshot")?;
    let path = demo_snapshot_path();
    if std::env::var("SWARMUI_WRITE_DEMO").is_ok() {
        fs::create_dir_all(path.parent().unwrap()).context("create fixtures dir")?;
        fs::write(&path, &payload).context("write demo fixture")?;
    }
    let fixture = fs::read(&path).context("read demo fixture")?;
    assert_eq!(
        payload, fixture,
        "demo snapshot mismatch: regenerate with SWARMUI_WRITE_DEMO=1"
    );
    Ok(())
}

#[test]
fn demo_snapshot_replay_is_deterministic() -> Result<()> {
    let payload = fs::read(&demo_snapshot_path()).context("read demo fixture")?;
    let digest = replay_digest(&payload)?;
    assert_eq!(digest, DEMO_DIGEST, "demo digest mismatch");
    Ok(())
}

#[test]
fn expired_hive_snapshot_is_rejected() -> Result<()> {
    let temp_dir = TempDir::new().context("tempdir")?;
    let mut config = SwarmUiConfig::from_generated(temp_dir.path().to_path_buf());
    config.cache.enabled = true;
    config.cache.max_bytes = 65536;
    config.cache.ttl = Duration::from_millis(1);
    let calls = Arc::new(Mutex::new(0usize));
    let factory = NoConnectFactory {
        calls: Arc::clone(&calls),
    };
    let mut backend = SwarmUiBackend::new(config, factory);
    let payload = serde_cbor::to_vec(&demo_snapshot()).context("encode demo snapshot")?;
    backend
        .cache_write("hive:demo", &payload)
        .context("cache write")?;
    std::thread::sleep(Duration::from_millis(4));
    backend.set_offline(true);
    let err = backend
        .hive_bootstrap(Role::Queen, None, Some("demo"))
        .unwrap_err();
    assert!(
        err.to_string().contains("snapshot expired"),
        "unexpected error: {err}"
    );
    Ok(())
}

fn replay_digest(payload: &[u8]) -> Result<u64> {
    let temp_dir = TempDir::new().context("tempdir")?;
    let config = SwarmUiConfig::from_generated(temp_dir.path().to_path_buf());
    let calls = Arc::new(Mutex::new(0usize));
    let factory = NoConnectFactory {
        calls: Arc::clone(&calls),
    };
    let mut backend = SwarmUiBackend::new(config, factory);
    backend.load_hive_replay(payload)?;
    backend.hive_bootstrap(Role::Queen, None, None)?;
    let mut events = Vec::new();
    loop {
        let batch = backend.hive_poll(Role::Queen, None)?;
        events.extend(batch.events);
        if batch.done {
            break;
        }
    }
    Ok(hash_events(&events))
}

fn hash_events(events: &[SwarmUiHiveEvent]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for event in events {
        hash = fnv1a(hash, &event.seq.to_le_bytes());
        hash = fnv1a(hash, event.agent.as_bytes());
        hash = fnv1a(hash, event.namespace.as_bytes());
        hash = fnv1a(hash, event_kind_id(&event.kind).as_bytes());
        if let Some(detail) = event.detail.as_ref() {
            hash = fnv1a(hash, detail.as_bytes());
        }
    }
    hash
}

fn event_kind_id(kind: &SwarmUiHiveEventKind) -> &'static str {
    match kind {
        SwarmUiHiveEventKind::Telemetry => "telemetry",
        SwarmUiHiveEventKind::Error => "error",
    }
}

fn fnv1a(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x00000100000001b3);
    }
    hash
}

fn demo_snapshot() -> SwarmUiHiveSnapshot {
    let queen = SwarmUiHiveAgent {
        id: "queen".to_owned(),
        role: "queen".to_owned(),
        namespace: "/queen".to_owned(),
    };
    let worker = SwarmUiHiveAgent {
        id: "worker-1".to_owned(),
        role: "worker".to_owned(),
        namespace: "/worker/worker-1".to_owned(),
    };
    let gpu = SwarmUiHiveAgent {
        id: "worker-gpu-1".to_owned(),
        role: "worker".to_owned(),
        namespace: "/worker/worker-gpu-1".to_owned(),
    };
    let events = vec![
        event(0, SwarmUiHiveEventKind::Telemetry, &worker, "tick 1"),
        event(1, SwarmUiHiveEventKind::Telemetry, &worker, "tick 2"),
        event(2, SwarmUiHiveEventKind::Telemetry, &gpu, "lease ok"),
        event(3, SwarmUiHiveEventKind::Error, &gpu, "ERR lease expired"),
        event(4, SwarmUiHiveEventKind::Telemetry, &worker, "tick 3"),
        event(5, SwarmUiHiveEventKind::Telemetry, &gpu, "recovered"),
    ];
    SwarmUiHiveSnapshot {
        version: 1,
        created_ms: DEMO_CREATED_MS,
        agents: vec![queen, worker, gpu],
        events,
    }
}

fn event(
    seq: u64,
    kind: SwarmUiHiveEventKind,
    agent: &SwarmUiHiveAgent,
    detail: &str,
) -> SwarmUiHiveEvent {
    SwarmUiHiveEvent {
        seq,
        kind,
        agent: agent.id.clone(),
        namespace: agent.namespace.clone(),
        detail: Some(detail.to_owned()),
        reason: None,
    }
}

fn demo_snapshot_path() -> PathBuf {
    repo_root().join("apps/swarmui/tests/fixtures/demo.cbor")
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}
