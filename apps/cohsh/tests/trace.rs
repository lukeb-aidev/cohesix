// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate cohsh trace record/replay determinism and fixtures.
// Author: Lukas Bower

#[path = "../../../tests/fixtures/transcripts/support.rs"]
mod transcript_support;

use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use anyhow::{Context, Result};
use cohesix_ticket::Role;
use cohsh::client::{CohClient, InProcessTransport};
use cohsh::policy::CohshPolicy;
use cohsh::trace::{TraceAckMode, TraceShellTransport};
use cohsh::Transport;
use cohsh::SECURE9P_MSIZE;
use cohsh_core::command::MAX_LINE_LEN;
use cohsh_core::trace::{
    TraceError, TraceLog, TraceLogBuilder, TracePolicy, TraceReplayTransport,
    TraceTransportRecorder,
};
use cohsh_core::wire::{render_ack, AckLine, AckStatus};
use cohsh_core::{role_label, ConsoleVerb};
use nine_door::NineDoor;
use secure9p_codec::OpenMode;

const SCENARIO: &str = "trace_v0";
const TRACE_ENV: &str = "COHESIX_WRITE_TRACE";
const WORKER_ID: &str = "worker-1";
const LIST_PATH: &str = "/worker";

#[test]
fn trace_record_matches_fixture() -> Result<()> {
    let start = Instant::now();
    let server = NineDoor::new();
    seed_worker(&server)?;

    let (trace, transcript) = record_trace(&server)?;
    assert_eq!(trace.ack_lines, expected_ack_lines());

    let encoded = trace.encode(trace_policy())?;
    let trace_path = trace_fixture_path();
    if std::env::var(TRACE_ENV).is_ok() {
        fs::create_dir_all(trace_path.parent().unwrap()).context("create trace fixture dir")?;
        fs::write(&trace_path, &encoded).context("write trace fixture")?;
    }
    let fixture = fs::read(&trace_path).context("read trace fixture")?;
    assert_eq!(
        encoded, fixture,
        "trace fixture mismatch: regenerate with {TRACE_ENV}=1"
    );

    transcript_support::compare_transcript("cohsh", SCENARIO, "cohsh.txt", &transcript);
    transcript_support::write_timing(
        "cohsh",
        SCENARIO,
        "trace-record",
        start.elapsed().as_millis() as u64,
    );
    Ok(())
}

#[test]
fn trace_replay_matches_fixture() -> Result<()> {
    let trace = decode_trace_fixture()?;
    let transcript = replay_trace(trace)?;
    transcript_support::compare_transcript("cohsh", SCENARIO, "cohsh.txt", &transcript);
    Ok(())
}

#[test]
fn trace_tamper_is_rejected() -> Result<()> {
    let mut payload = load_trace_fixture()?;
    let last = payload.len().saturating_sub(1);
    payload[last] ^= 0xff;
    let err = TraceLog::decode(&payload, trace_policy()).expect_err("tampered trace should fail");
    assert_eq!(err, TraceError::HashMismatch);
    Ok(())
}

fn record_trace(server: &NineDoor) -> Result<(TraceLog, Vec<String>)> {
    let policy = trace_policy();
    let builder = TraceLogBuilder::shared(policy);
    let builder_for_transport = Rc::clone(&builder);
    let builder_for_ack = Rc::clone(&builder);
    let server = server.clone();
    let factory = Box::new(move || {
        let connection = server.connect().context("open NineDoor session")?;
        let transport = InProcessTransport::new(connection);
        Ok(TraceTransportRecorder::new(
            transport,
            Rc::clone(&builder_for_transport),
        ))
    });
    let mut transport = TraceShellTransport::new(
        factory,
        TraceAckMode::Record(builder_for_ack),
        "trace-record",
    );

    let mut transcript = Vec::new();
    let session = transport.attach(Role::Queen, None)?;
    transcript.extend(transport.drain_acknowledgements());

    let list_lines = transport.list(&session, LIST_PATH)?;
    transcript.extend(transport.drain_acknowledgements());
    transcript.extend(list_lines);

    let telemetry_path = telemetry_path();
    let tail_lines = transport.tail(&session, &telemetry_path)?;
    transcript.extend(transport.drain_acknowledgements());
    transcript.extend(tail_lines);

    transport.finish()?;
    let trace = builder.borrow().snapshot();
    Ok((trace, transcript))
}

fn replay_trace(trace: TraceLog) -> Result<Vec<String>> {
    let expected = trace.ack_lines;
    let frames = Rc::new(RefCell::new(Some(trace.frames)));
    let factory = Box::new(move || {
        let frames = frames
            .borrow_mut()
            .take()
            .context("trace replay already consumed")?;
        Ok(TraceReplayTransport::new(frames))
    });
    let mut transport = TraceShellTransport::new(
        factory,
        TraceAckMode::Verify { expected, index: 0 },
        "trace-replay",
    );

    let mut transcript = Vec::new();
    let session = transport.attach(Role::Queen, None)?;
    transcript.extend(transport.drain_acknowledgements());

    let list_lines = transport.list(&session, LIST_PATH)?;
    transcript.extend(transport.drain_acknowledgements());
    transcript.extend(list_lines);

    let telemetry_path = telemetry_path();
    let tail_lines = transport.tail(&session, &telemetry_path)?;
    transcript.extend(transport.drain_acknowledgements());
    transcript.extend(tail_lines);

    transport.finish()?;
    Ok(transcript)
}

fn seed_worker(server: &NineDoor) -> Result<()> {
    let connection = server.connect().context("open NineDoor session")?;
    let transport = InProcessTransport::new(connection);
    let mut client = CohClient::connect(transport, Role::Queen, None)?;
    let payload = cohsh::queen::spawn("heartbeat", ["ticks=4"].iter().copied())?;
    write_payload(
        &mut client,
        cohsh::queen::queen_ctl_path(),
        payload.as_bytes(),
    )?;
    let telemetry_path = telemetry_path();
    write_payload(&mut client, &telemetry_path, b"tick 1\n")?;
    write_payload(&mut client, &telemetry_path, b"tick 2\n")?;
    Ok(())
}

fn write_payload<T: cohsh_core::Secure9pTransport>(
    client: &mut CohClient<T>,
    path: &str,
    payload: &[u8],
) -> Result<()> {
    let fid = client.open(path, OpenMode::write_append())?;
    let written = client.write(fid, u64::MAX, payload)?;
    let clunk_result = client.clunk(fid);
    if written as usize != payload.len() {
        return Err(anyhow::anyhow!(
            "short write to {path}: expected {} bytes, wrote {written}",
            payload.len()
        ));
    }
    clunk_result?;
    Ok(())
}

fn trace_policy() -> TracePolicy {
    let policy = CohshPolicy::from_generated();
    TracePolicy::new(policy.trace.max_bytes, SECURE9P_MSIZE, MAX_LINE_LEN as u32)
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
    if std::env::var(TRACE_ENV).is_ok() {
        let _guard = trace_fixture_lock().lock().expect("lock trace fixture");
        let server = NineDoor::new();
        seed_worker(&server)?;
        let (trace, _) = record_trace(&server)?;
        let payload = trace.encode(trace_policy())?;
        if let Some(parent) = trace_path.parent() {
            fs::create_dir_all(parent).context("create trace fixture dir")?;
        }
        fs::write(&trace_path, &payload).context("write trace fixture")?;
    }
    fs::read(&trace_path).with_context(|| format!("read trace fixture {}", trace_path.display()))
}

fn trace_fixture_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn decode_trace_fixture() -> Result<TraceLog> {
    let payload = load_trace_fixture()?;
    TraceLog::decode(&payload, trace_policy()).context("decode trace fixture")
}

fn telemetry_path() -> String {
    format!("/worker/{}/telemetry", WORKER_ID)
}

fn expected_ack_lines() -> Vec<String> {
    let attach_detail = format!("role={}", role_label(Role::Queen));
    let list_detail = format!("path={}", LIST_PATH);
    let tail_detail = format!("path={}", telemetry_path());
    vec![
        render_ack_line(
            AckStatus::Ok,
            ConsoleVerb::Attach.ack_label(),
            Some(attach_detail.as_str()),
        ),
        render_ack_line(
            AckStatus::Ok,
            ConsoleVerb::Ls.ack_label(),
            Some(list_detail.as_str()),
        ),
        render_ack_line(
            AckStatus::Ok,
            ConsoleVerb::Tail.ack_label(),
            Some(tail_detail.as_str()),
        ),
    ]
}

fn render_ack_line(status: AckStatus, verb: &str, detail: Option<&str>) -> String {
    let ack = AckLine {
        status,
        verb,
        detail,
    };
    let mut line = String::new();
    render_ack(&mut line, &ack).expect("render ack line");
    line
}
