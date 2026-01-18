// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate telemetry ring quota enforcement and cursor resumption.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use cohesix_ticket::{BudgetSpec, MountSpec, Role, TicketClaims, TicketIssuer};
use secure9p_codec::{ErrorCode, OpenMode, MAX_MSIZE};
use std::time::{SystemTime, UNIX_EPOCH};

use nine_door::{
    Clock, NineDoor, ShardLayout, TelemetryConfig, TelemetryCursorConfig, TelemetryFrameSchema,
    TelemetryManifestStore,
};

const METRICS_ENV: &str = "COHESIX_LATENCY_METRICS_PATH";

#[derive(Debug, Default)]
struct TestClock;

impl Clock for TestClock {
    fn now(&self) -> std::time::Instant {
        std::time::Instant::now()
    }
}

fn issue_ticket(secret: &str, role: Role, subject: &str) -> String {
    let budget = match role {
        Role::Queen => BudgetSpec::unbounded(),
        Role::WorkerHeartbeat => BudgetSpec::default_heartbeat(),
        Role::WorkerGpu => BudgetSpec::default_gpu(),
        Role::WorkerBus | Role::WorkerLora => BudgetSpec::default_heartbeat(),
    };
    let issuer = TicketIssuer::new(secret);
    let claims = TicketClaims::new(
        role,
        budget,
        Some(subject.to_owned()),
        MountSpec::empty(),
        unix_time_ms(),
    );
    issuer.issue(claims).unwrap().encode().unwrap()
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn worker_telemetry_path(worker_id: &str) -> Vec<String> {
    ShardLayout::default().worker_telemetry_path(worker_id)
}

#[test]
fn telemetry_ring_enforces_quota_and_resumes_cursor() {
    let telemetry = TelemetryConfig {
        ring_bytes_per_worker: 64,
        frame_schema: TelemetryFrameSchema::LegacyPlaintext,
        cursor: TelemetryCursorConfig {
            retain_on_boot: true,
        },
    };
    let manifest = TelemetryManifestStore::new();
    let clock = std::sync::Arc::new(TestClock::default());
    let payload_one = b"one\n";
    let payload_two = b"two\n";
    let payload_three = b"three\n";
    let resume_offset = (payload_one.len() + payload_two.len()) as u64;
    let mut latencies_ms = Vec::new();

    {
        let server = NineDoor::new_with_limits_and_telemetry_manifest(
            clock.clone(),
            Default::default(),
            telemetry,
            manifest.clone(),
        );
        server.register_ticket_secret(Role::WorkerHeartbeat, "worker-secret");

        let mut queen = server.connect().expect("queen session");
        queen.version(MAX_MSIZE).expect("version");
        queen.attach(1, Role::Queen).expect("attach queen");

        let queen_ctl = vec!["queen".to_owned(), "ctl".to_owned()];
        queen.walk(1, 2, &queen_ctl).expect("walk /queen/ctl");
        queen.open(2, OpenMode::write_append()).expect("open ctl");
        queen
            .write(2, b"{\"spawn\":\"heartbeat\",\"ticks\":100}\n")
            .expect("spawn worker");
        queen.clunk(2).expect("clunk ctl");

        let mut worker = server.connect().expect("worker session");
        worker.version(MAX_MSIZE).expect("version");
        worker
            .attach_with_identity(
                1,
                Role::WorkerHeartbeat,
                Some("worker-1"),
                Some(issue_ticket("worker-secret", Role::WorkerHeartbeat, "worker-1").as_str()),
            )
            .expect("attach worker");

        let telemetry_path = worker_telemetry_path("worker-1");
        worker
            .walk(1, 2, &telemetry_path)
            .expect("walk telemetry");
        worker
            .open(2, OpenMode::write_append())
            .expect("open telemetry");

        let start = std::time::Instant::now();
        worker.write(2, payload_one).expect("write one");
        latencies_ms.push(start.elapsed().as_secs_f64() * 1_000.0);
        let start = std::time::Instant::now();
        worker.write(2, payload_two).expect("write two");
        latencies_ms.push(start.elapsed().as_secs_f64() * 1_000.0);

        queen
            .walk(1, 3, &telemetry_path)
            .expect("queen walk telemetry");
        queen
            .open(3, OpenMode::read_only())
            .expect("open telemetry read");
        let first = queen.read(3, 0, 4).expect("read first chunk");
        let second = queen
            .read(3, first.len() as u64, MAX_MSIZE)
            .expect("read second chunk");
        let mut combined = Vec::new();
        combined.extend_from_slice(&first);
        combined.extend_from_slice(&second);
        let combined_text = String::from_utf8(combined).expect("utf8");
        assert!(combined_text.contains("one"));
        assert!(combined_text.contains("two"));
        assert!(combined_text.find("one").unwrap() < combined_text.find("two").unwrap());
    }

    let server = NineDoor::new_with_limits_and_telemetry_manifest(
        clock.clone(),
        Default::default(),
        telemetry,
        manifest,
    );
    server.register_ticket_secret(Role::WorkerHeartbeat, "worker-secret");

    let mut queen = server.connect().expect("queen session");
    queen.version(MAX_MSIZE).expect("version");
    queen.attach(1, Role::Queen).expect("attach queen");

    let queen_ctl = vec!["queen".to_owned(), "ctl".to_owned()];
    queen.walk(1, 2, &queen_ctl).expect("walk /queen/ctl");
    queen.open(2, OpenMode::write_append()).expect("open ctl");
    queen
        .write(2, b"{\"spawn\":\"heartbeat\",\"ticks\":100}\n")
        .expect("spawn worker");
    queen.clunk(2).expect("clunk ctl");

    let mut worker = server.connect().expect("worker session");
    worker.version(MAX_MSIZE).expect("version");
    worker
        .attach_with_identity(
            1,
            Role::WorkerHeartbeat,
            Some("worker-1"),
            Some(issue_ticket("worker-secret", Role::WorkerHeartbeat, "worker-1").as_str()),
        )
        .expect("attach worker");

    let telemetry_path = worker_telemetry_path("worker-1");
    worker
        .walk(1, 2, &telemetry_path)
        .expect("walk telemetry");
    worker
        .open(2, OpenMode::write_append())
        .expect("open telemetry");

    queen
        .walk(1, 3, &telemetry_path)
        .expect("queen walk telemetry");
    queen
        .open(3, OpenMode::read_only())
        .expect("open telemetry read");

    let replay = queen.read(3, 0, MAX_MSIZE).expect("replay read");
    let replay_text = String::from_utf8(replay).expect("replay utf8");
    assert!(replay_text.contains("one"));
    assert!(replay_text.contains("two"));

    let start = std::time::Instant::now();
    worker.write(2, payload_three).expect("write three");
    latencies_ms.push(start.elapsed().as_secs_f64() * 1_000.0);

    let resumed = queen
        .read(3, resume_offset, MAX_MSIZE)
        .expect("resume read");
    let resumed_text = String::from_utf8(resumed).expect("resume utf8");
    assert!(resumed_text.contains("three"));

    let oversize = vec![b'X'; 128];
    let err = worker.write(2, &oversize).expect_err("reject oversize");
    match err {
        nine_door::NineDoorError::Protocol { code, .. } => {
            assert_eq!(code, ErrorCode::TooBig)
        }
        other => panic!("unexpected error: {other:?}"),
    }

    let wrap_payload = vec![b'W'; 48];
    for _ in 0..4 {
        let start = std::time::Instant::now();
        worker.write(2, &wrap_payload).expect("wrap write");
        latencies_ms.push(start.elapsed().as_secs_f64() * 1_000.0);
    }

    let stale = queen.read(3, 0, MAX_MSIZE).expect_err("stale cursor");
    match stale {
        nine_door::NineDoorError::Protocol { code, .. } => {
            assert_eq!(code, ErrorCode::Invalid)
        }
        other => panic!("unexpected error: {other:?}"),
    }

    let log_path = vec!["log".to_owned(), "queen.log".to_owned()];
    queen.walk(1, 4, &log_path).expect("walk log");
    queen.open(4, OpenMode::read_only()).expect("open log");
    let log_text = String::from_utf8(queen.read(4, 0, MAX_MSIZE).expect("read log"))
        .expect("log utf8");
    assert!(log_text.contains("telemetry quota reject"));
    assert!(log_text.contains("telemetry ring wrap"));
    assert!(log_text.contains("telemetry cursor rewind"));
    assert!(log_text.contains("telemetry cursor stale"));

    if let Ok(path) = std::env::var(METRICS_ENV) {
        if !latencies_ms.is_empty() {
            let mut samples = latencies_ms.clone();
            let p50 = percentile_ms(&mut samples, 0.50);
            let p95 = percentile_ms(&mut samples, 0.95);
            let payload = serde_json::json!({
                "suite": "nine-door/telemetry_ring",
                "unit": "ms",
                "samples": samples.len(),
                "p50_ms": p50,
                "p95_ms": p95,
            });
            if let Some(parent) = std::path::Path::new(&path).parent() {
                std::fs::create_dir_all(parent).expect("metrics dir");
            }
            std::fs::write(&path, serde_json::to_vec_pretty(&payload).expect("json"))
                .expect("write metrics");
        }
    }
}

fn percentile_ms(samples: &mut [f64], pct: f64) -> f64 {
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let rank = ((samples.len() - 1) as f64 * pct).round() as usize;
    samples[rank]
}
