// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate telemetry ingest quota enforcement and eviction behaviour.
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

fn create_segment(client: &mut InProcessConnection, device_id: &str) -> String {
    let ctl_path = vec![
        "queen".to_owned(),
        "telemetry".to_owned(),
        device_id.to_owned(),
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
        device_id.to_owned(),
        "latest".to_owned(),
    ];
    read_text(client, 3, &latest_path).trim().to_owned()
}

#[test]
fn telemetry_ingest_refuses_segment_quota() {
    let ingest = TelemetryIngestConfig {
        max_segments_per_device: 1,
        max_bytes_per_segment: 64,
        max_total_bytes_per_device: 64,
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

    let device_id = "device-2";
    let _first = create_segment(&mut client, device_id);

    let ctl_path = vec![
        "queen".to_owned(),
        "telemetry".to_owned(),
        device_id.to_owned(),
        "ctl".to_owned(),
    ];
    client.walk(1, 4, &ctl_path).expect("walk ctl");
    client.open(4, OpenMode::write_append()).expect("open ctl");
    let err = client
        .write(4, br#"{"new":"segment","mime":"text/plain"}\n"#)
        .expect_err("quota exceeded");
    match err {
        NineDoorError::Protocol { code, .. } => assert_eq!(code, ErrorCode::TooBig),
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn telemetry_ingest_evicts_oldest_segment() {
    let ingest = TelemetryIngestConfig {
        max_segments_per_device: 2,
        max_bytes_per_segment: 64,
        max_total_bytes_per_device: 128,
        eviction_policy: TelemetryIngestEvictionPolicy::EvictOldest,
    };
    let server = NineDoor::new_with_limits_and_telemetry_manifest(
        Arc::new(TestClock::default()),
        Default::default(),
        TelemetryConfig::default(),
        ingest,
        TelemetryManifestStore::default(),
    );
    let mut client = attach_queen(&server);

    let device_id = "device-3";
    let first = create_segment(&mut client, device_id);
    let second = create_segment(&mut client, device_id);
    let third = create_segment(&mut client, device_id);

    let seg_dir = vec![
        "queen".to_owned(),
        "telemetry".to_owned(),
        device_id.to_owned(),
        "seg".to_owned(),
    ];
    let listing = read_text(&mut client, 4, &seg_dir);
    assert!(!listing.contains(first.as_str()));
    assert!(listing.contains(second.as_str()));
    assert!(listing.contains(third.as_str()));

    let latest_path = vec![
        "queen".to_owned(),
        "telemetry".to_owned(),
        device_id.to_owned(),
        "latest".to_owned(),
    ];
    let latest = read_text(&mut client, 5, &latest_path);
    assert_eq!(latest.trim(), third);
}

#[test]
fn telemetry_ingest_evicts_on_total_bytes() {
    let ingest = TelemetryIngestConfig {
        max_segments_per_device: 2,
        max_bytes_per_segment: 32,
        max_total_bytes_per_device: 32,
        eviction_policy: TelemetryIngestEvictionPolicy::EvictOldest,
    };
    let server = NineDoor::new_with_limits_and_telemetry_manifest(
        Arc::new(TestClock::default()),
        Default::default(),
        TelemetryConfig::default(),
        ingest,
        TelemetryManifestStore::default(),
    );
    let mut client = attach_queen(&server);

    let device_id = "device-4";
    let seg_one = create_segment(&mut client, device_id);
    let seg_two = create_segment(&mut client, device_id);

    let seg_one_path = vec![
        "queen".to_owned(),
        "telemetry".to_owned(),
        device_id.to_owned(),
        "seg".to_owned(),
        seg_one.clone(),
    ];
    client.walk(1, 6, &seg_one_path).expect("walk seg one");
    client.open(6, OpenMode::write_append()).expect("open seg one");
    client
        .write(6, b"1234567890abcdef")
        .expect("write seg one");
    client.clunk(6).expect("clunk seg one");

    let seg_two_path = vec![
        "queen".to_owned(),
        "telemetry".to_owned(),
        device_id.to_owned(),
        "seg".to_owned(),
        seg_two.clone(),
    ];
    client.walk(1, 7, &seg_two_path).expect("walk seg two");
    client.open(7, OpenMode::write_append()).expect("open seg two");
    client
        .write(7, b"abcdefghijABCDEF")
        .expect("write seg two");
    client.clunk(7).expect("clunk seg two");

    client.walk(1, 8, &seg_two_path).expect("walk seg two again");
    client.open(8, OpenMode::write_append()).expect("open seg two again");
    client
        .write(8, b"klmnopqrstUVWXy1")
        .expect("write seg two again");
    client.clunk(8).expect("clunk seg two again");

    let err = client.walk(1, 9, &seg_one_path).expect_err("seg one evicted");
    match err {
        NineDoorError::Protocol { code, .. } => assert_eq!(code, ErrorCode::NotFound),
        other => panic!("unexpected error: {other:?}"),
    }
}
