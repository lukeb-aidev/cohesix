// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate telemetry ingest envelope size enforcement.
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
    client
        .attach(1, cohesix_ticket::Role::Queen)
        .expect("attach");
    client
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
        .write(2, b"{\"new\":\"segment\",\"mime\":\"text/plain\"}\n")
        .expect("create segment");
    client.clunk(2).expect("clunk ctl");

    let latest_path = vec![
        "queen".to_owned(),
        "telemetry".to_owned(),
        device_id.to_owned(),
        "latest".to_owned(),
    ];
    client.walk(1, 3, &latest_path).expect("walk latest");
    client.open(3, OpenMode::read_only()).expect("open latest");
    let data = client.read(3, 0, MAX_MSIZE).expect("read latest");
    client.clunk(3).expect("clunk latest");
    String::from_utf8(data).expect("utf8").trim().to_owned()
}

#[test]
fn telemetry_ingest_rejects_oversize_record() {
    let ingest = TelemetryIngestConfig {
        max_segments_per_device: 1,
        max_bytes_per_segment: 8192,
        max_total_bytes_per_device: 8192,
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

    let device_id = "device-5";
    let seg_id = create_segment(&mut client, device_id);
    let seg_path = vec![
        "queen".to_owned(),
        "telemetry".to_owned(),
        device_id.to_owned(),
        "seg".to_owned(),
        seg_id,
    ];
    client.walk(1, 4, &seg_path).expect("walk seg");
    client.open(4, OpenMode::write_append()).expect("open seg");

    let oversize = vec![b'X'; 4097];
    let err = client.write(4, &oversize).expect_err("reject oversize");
    match err {
        NineDoorError::Protocol { code, .. } => assert_eq!(code, ErrorCode::TooBig),
        other => panic!("unexpected error: {other:?}"),
    }
}
