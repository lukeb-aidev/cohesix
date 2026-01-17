// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate /proc observability providers for NineDoor host metrics.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use cohesix_ticket::Role;
use nine_door::{Clock, InProcessConnection, NineDoor};
use log::{Level, LevelFilter, Log, Metadata, Record};
use secure9p_codec::{OpenMode, Request, RequestBody, MAX_MSIZE};
use secure9p_core::{SessionLimits, ShortWritePolicy};

struct FixedClock {
    now: Instant,
}

impl FixedClock {
    fn new() -> Self {
        Self { now: Instant::now() }
    }
}

impl Clock for FixedClock {
    fn now(&self) -> Instant {
        self.now
    }
}

fn setup_session(server: &NineDoor) -> InProcessConnection {
    let mut client = server.connect().expect("create session");
    client.version(MAX_MSIZE).expect("version handshake");
    client.attach(1, Role::Queen).expect("attach");
    let log_path = vec!["log".to_owned(), "queen.log".to_owned()];
    client.walk(1, 2, &log_path).expect("walk /log/queen.log");
    client
        .open(2, OpenMode::write_append())
        .expect("open /log/queen.log");
    client
}

fn read_proc_text(
    client: &mut InProcessConnection,
    path: &[&str],
    fid: u32,
) -> String {
    let components = path.iter().map(|seg| seg.to_string()).collect::<Vec<_>>();
    client
        .walk(1, fid, &components)
        .expect("walk proc path");
    client
        .open(fid, OpenMode::read_only())
        .expect("open proc path");
    let data = client.read(fid, 0, MAX_MSIZE).expect("read proc path");
    client.clunk(fid).expect("clunk proc fid");
    String::from_utf8(data).expect("proc output should be utf8")
}

fn parse_kv(line: &str, key: &str) -> u64 {
    line.split_whitespace()
        .find_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            if k == key {
                v.parse::<u64>().ok()
            } else {
                None
            }
        })
        .unwrap_or(0)
}

struct CaptureLogger;

static LOGS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
static LOGGER: CaptureLogger = CaptureLogger;

impl Log for CaptureLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        if let Some(logs) = LOGS.get() {
            let mut guard = logs.lock().expect("log lock");
            guard.push(format!("{}", record.args()));
        }
    }

    fn flush(&self) {}
}

fn init_logger() -> &'static Mutex<Vec<String>> {
    let logs = LOGS.get_or_init(|| Mutex::new(Vec::new()));
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(LevelFilter::Info);
    logs
}

#[test]
fn proc_metrics_track_pipeline_state() {
    let limits = SessionLimits {
        tags_per_session: 4,
        batch_frames: 1,
        short_write_policy: ShortWritePolicy::Reject,
    };
    let server = NineDoor::new_with_limits(Arc::new(FixedClock::new()), limits);
    let mut client = setup_session(&server);

    let request_a = Request {
        tag: 10,
        body: RequestBody::Write {
            fid: 2,
            offset: u64::MAX,
            data: b"observe-a".to_vec(),
        },
    };
    let request_b = Request {
        tag: 11,
        body: RequestBody::Write {
            fid: 2,
            offset: u64::MAX,
            data: b"observe-b".to_vec(),
        },
    };
    let mut batch = Vec::new();
    let codec = secure9p_codec::Codec;
    batch.extend_from_slice(&codec.encode_request(&request_a).expect("encode a"));
    batch.extend_from_slice(&codec.encode_request(&request_b).expect("encode b"));
    let _ = client.exchange_batch(&batch).expect("batch exchange");

    let metrics = server.pipeline_metrics();
    let outstanding = read_proc_text(&mut client, &["proc", "9p", "outstanding"], 3);
    let short_writes = read_proc_text(&mut client, &["proc", "9p", "short_writes"], 4);
    let ingest_backpressure = read_proc_text(&mut client, &["proc", "ingest", "backpressure"], 5);
    let ingest_p50 = read_proc_text(&mut client, &["proc", "ingest", "p50_ms"], 6);
    let ingest_p95 = read_proc_text(&mut client, &["proc", "ingest", "p95_ms"], 7);
    let ingest_queued = read_proc_text(&mut client, &["proc", "ingest", "queued"], 8);

    assert_eq!(parse_kv(&outstanding, "current"), metrics.queue_depth as u64);
    assert_eq!(parse_kv(&outstanding, "limit"), metrics.queue_limit as u64);
    assert_eq!(parse_kv(&short_writes, "total"), metrics.short_writes);
    assert_eq!(parse_kv(&short_writes, "retries"), metrics.short_write_retries);
    assert_eq!(
        parse_kv(&ingest_backpressure, "backpressure"),
        metrics.backpressure_events
    );
    assert_eq!(parse_kv(&ingest_p50, "p50_ms"), 0);
    assert_eq!(parse_kv(&ingest_p95, "p95_ms"), 0);
    assert_eq!(parse_kv(&ingest_queued, "queued"), metrics.queue_depth as u64);
}

#[test]
fn ingest_watch_throttles_with_fixed_timebase() {
    let limits = SessionLimits {
        tags_per_session: 4,
        batch_frames: 1,
        short_write_policy: ShortWritePolicy::Reject,
    };
    let server = NineDoor::new_with_limits(Arc::new(FixedClock::new()), limits);
    let mut client = setup_session(&server);

    for idx in 0..3 {
        let request = Request {
            tag: 100 + idx,
            body: RequestBody::Write {
                fid: 2,
                offset: u64::MAX,
                data: format!("watch-{idx}").into_bytes(),
            },
        };
        let frame = secure9p_codec::Codec
            .encode_request(&request)
            .expect("encode request");
        let _ = client.exchange_batch(&frame).expect("batch exchange");
    }

    let watch = read_proc_text(&mut client, &["proc", "ingest", "watch"], 9);
    let lines: Vec<&str> = watch.lines().collect();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].starts_with("watch ts_ms=0 "));
}

#[test]
fn ingest_watch_throttle_emits_log_line() {
    let logs = init_logger();
    logs.lock().expect("log lock").clear();

    let limits = SessionLimits {
        tags_per_session: 4,
        batch_frames: 1,
        short_write_policy: ShortWritePolicy::Reject,
    };
    let server = NineDoor::new_with_limits(Arc::new(FixedClock::new()), limits);
    let mut client = setup_session(&server);

    for idx in 0..3 {
        let request = Request {
            tag: 200 + idx,
            body: RequestBody::Write {
                fid: 2,
                offset: u64::MAX,
                data: format!("throttle-{idx}").into_bytes(),
            },
        };
        let frame = secure9p_codec::Codec
            .encode_request(&request)
            .expect("encode request");
        let _ = client.exchange_batch(&frame).expect("batch exchange");
    }

    let captured = logs.lock().expect("log lock");
    assert!(
        captured
            .iter()
            .any(|line| line.contains("ingest watch throttled")),
        "missing throttle log line: {captured:?}"
    );
}
