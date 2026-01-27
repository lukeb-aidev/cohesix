// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate SwarmUI snapshot cache bounds and offline behavior.
// Author: Lukas Bower

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tempfile::TempDir;

use swarmui::{
    CacheError, SnapshotCache, SwarmUiBackend, SwarmUiConfig, SwarmUiTranscript,
    SwarmUiTransportFactory,
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

#[test]
fn snapshot_cache_bounds_and_expiry() {
    let temp_dir = TempDir::new().expect("tempdir");
    let cache = SnapshotCache::new(
        temp_dir.path().join("snapshots"),
        128,
        Duration::from_millis(1),
    );
    let payload = vec![1u8, 2u8, 3u8, 4u8];
    let record = cache.write("fleet:ingest", &payload).expect("write");
    assert_eq!(record.payload, payload);

    let read = cache.read("fleet:ingest").expect("read");
    assert_eq!(read.payload, payload);

    std::thread::sleep(Duration::from_millis(4));
    let expired = cache.read("fleet:ingest").unwrap_err();
    assert!(matches!(expired, CacheError::Expired));

    let oversized = vec![0u8; 256];
    let err = cache.write("fleet:oversize", &oversized).unwrap_err();
    assert!(matches!(err, CacheError::TooLarge { .. }));
}

#[test]
fn offline_mode_blocks_network_and_allows_cache_read() {
    let temp_dir = TempDir::new().expect("tempdir");
    let mut config = SwarmUiConfig::from_generated(temp_dir.path().to_path_buf());
    config.cache.enabled = true;
    config.cache.max_bytes = 256;
    config.cache.ttl = Duration::from_secs(60);
    let calls = Arc::new(Mutex::new(0usize));
    let factory = NoConnectFactory {
        calls: Arc::clone(&calls),
    };
    let mut backend = SwarmUiBackend::new(config, factory);
    backend.set_offline(true);

    let attach = backend.attach(cohesix_ticket::Role::Queen, None);
    assert!(!attach.ok);
    assert_eq!(backend.active_tails(), 0);

    let write_err = backend
        .cache_write("fleet:offline", b"payload")
        .unwrap_err();
    assert_eq!(
        write_err.to_string(),
        "offline mode prohibits network access"
    );

    backend.set_offline(false);
    backend
        .cache_write("fleet:offline", b"payload")
        .expect("cache write");
    backend.set_offline(true);
    let record = backend.cache_read("fleet:offline").expect("cache read");
    assert_eq!(record.payload, b"payload");

    let call_count = *calls.lock().unwrap();
    assert_eq!(call_count, 0);
}

#[test]
fn offline_tail_reads_cached_snapshot_only() {
    let temp_dir = TempDir::new().expect("tempdir");
    let mut config = SwarmUiConfig::from_generated(temp_dir.path().to_path_buf());
    config.cache.enabled = true;
    config.cache.max_bytes = 1024;
    config.cache.ttl = Duration::from_secs(60);
    let calls = Arc::new(Mutex::new(0usize));
    let factory = NoConnectFactory {
        calls: Arc::clone(&calls),
    };
    let mut backend = SwarmUiBackend::new(config, factory);

    let transcript = SwarmUiTranscript {
        ok: true,
        lines: vec![
            "OK TAIL path=/worker/worker-1/telemetry".to_owned(),
            "tick 1".to_owned(),
            "END".to_owned(),
        ],
    };
    let payload = serde_cbor::to_vec(&transcript).expect("encode snapshot");
    backend
        .cache_write("telemetry:worker-1", &payload)
        .expect("cache write");

    backend.set_offline(true);
    let result = backend.tail_telemetry(cohesix_ticket::Role::Queen, None, "worker-1");
    assert!(result.ok);
    assert_eq!(result.lines, transcript.lines);

    let call_count = *calls.lock().unwrap();
    assert_eq!(call_count, 0);
}
