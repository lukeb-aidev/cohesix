// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate telemetry ingest namespace creation and OS-named segments.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::sync::Arc;

use nine_door::{
    Clock, InProcessConnection, NineDoor, NineDoorError, TelemetryConfig, TelemetryIngestConfig,
    TelemetryIngestEvictionPolicy, TelemetryManifestStore,
};
use secure9p_codec::{ErrorCode, OpenMode, MAX_MSIZE};

#[derive(Debug, Default)]
struct TestClock;

impl Clock for TestClock {
    fn now(&self) -> std::time::Instant {
        std::time::Instant::now()
    }
}

fn attach_queen(server: &NineDoor) -> InProcessConnection {
    let mut client = server.connect().expect("connect");
    client.version(MAX_MSIZE).expect("version");
    client.attach(1, cohesix_ticket::Role::Queen).expect("attach");
    client
}

fn read_text(client: &mut InProcessConnection, fid: u32, path: &[String]) -> String {
    client.walk(1, fid, path).expect("walk");
    client.open(fid, OpenMode::read_only()).expect("open");
    let data = client.read(fid, 0, MAX_MSIZE).expect("read");
    client.clunk(fid).expect("clunk");
    String::from_utf8(data).expect("utf8")
}

#[test]
fn telemetry_ingest_disabled_hides_namespace() {
    let ingest = TelemetryIngestConfig {
        max_segments_per_device: 0,
        max_bytes_per_segment: 0,
        max_total_bytes_per_device: 0,
        eviction_policy: TelemetryIngestEvictionPolicy::Refuse,
    };
    let server = NineDoor::new_with_limits_and_telemetry_manifest(
        Arc::new(TestClock::default()),
        Default::default(),
        TelemetryConfig::default(),
        ingest,
        TelemetryManifestStore::default(),
    );
    let mut client = attach_queen(&server);
    let path = vec!["queen".to_owned(), "telemetry".to_owned()];
    let err = client.walk(1, 2, &path).expect_err("telemetry disabled");
    match err {
        NineDoorError::Protocol { code, .. } => assert_eq!(code, ErrorCode::NotFound),
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn telemetry_ingest_allocates_segment_and_latest() {
    let ingest = TelemetryIngestConfig {
        max_segments_per_device: 2,
        max_bytes_per_segment: 256,
        max_total_bytes_per_device: 512,
        eviction_policy: TelemetryIngestEvictionPolicy::Refuse,
    };
    let server = NineDoor::new_with_limits_and_telemetry_manifest(
        Arc::new(TestClock::default()),
        Default::default(),
        TelemetryConfig::default(),
        ingest,
        TelemetryManifestStore::default(),
    );
    let mut client = attach_queen(&server);

    let ctl_path = vec![
        "queen".to_owned(),
        "telemetry".to_owned(),
        "device-1".to_owned(),
        "ctl".to_owned(),
    ];
    client.walk(1, 2, &ctl_path).expect("walk ctl");
    client.open(2, OpenMode::write_append()).expect("open ctl");
    client
        .write(2, br#"{"new":"segment","mime":"text/plain"}\n"#)
        .expect("create segment");
    client.clunk(2).expect("clunk ctl");

    let latest_path = vec![
        "queen".to_owned(),
        "telemetry".to_owned(),
        "device-1".to_owned(),
        "latest".to_owned(),
    ];
    let latest = read_text(&mut client, 3, &latest_path);
    let latest = latest.trim();
    assert_eq!(latest, "seg-000001");

    let seg_dir = vec![
        "queen".to_owned(),
        "telemetry".to_owned(),
        "device-1".to_owned(),
        "seg".to_owned(),
    ];
    let listing = read_text(&mut client, 4, &seg_dir);
    assert!(listing.contains("seg-000001"));

    let bad_seg = vec![
        "queen".to_owned(),
        "telemetry".to_owned(),
        "device-1".to_owned(),
        "seg".to_owned(),
        "custom".to_owned(),
    ];
    let err = client.walk(1, 5, &bad_seg).expect_err("reject client segment path");
    match err {
        NineDoorError::Protocol { code, .. } => assert_eq!(code, ErrorCode::NotFound),
        other => panic!("unexpected error: {other:?}"),
    }

    let seg_path = vec![
        "queen".to_owned(),
        "telemetry".to_owned(),
        "device-1".to_owned(),
        "seg".to_owned(),
        "seg-000001".to_owned(),
    ];
    client.walk(1, 6, &seg_path).expect("walk seg");
    client
        .open(6, OpenMode::write_append())
        .expect("open seg append");
    client.write(6, b"hello").expect("append seg");
    client.clunk(6).expect("clunk seg append");

    let readback = read_text(&mut client, 7, &seg_path);
    assert_eq!(readback, "hello");
}
