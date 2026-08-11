// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate coh-status trace replay fixtures.
// Author: Lukas Bower

#[path = "../../../tests/fixtures/transcripts/support.rs"]
mod transcript_support;

use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use coh_status::{trace_policy, TraceReplay};
use cohesix_ticket::Role;
use cohsh::client::{CohClient, InProcessTransport, TailEvent};
use cohsh::queen;
use cohsh::SECURE9P_MSIZE;
use cohsh_core::trace::{TraceLogBuilder, TraceTransportRecorder};
use cohsh_core::wire::{render_ack, AckLine, AckStatus, END_LINE};
use cohsh_core::{role_label, ConsoleVerb};
use nine_door::{NineDoor, ShardLayout};
use secure9p_codec::OpenMode;

const SCENARIO: &str = "trace_v0";
const WORKER_ID: &str = "worker-1";

#[test]
fn trace_replay_matches_fixture() -> Result<()> {
    let start = Instant::now();
    let payload = read_only_trace_fixture()?;
    let mut replay = TraceReplay::from_bytes(&payload, Role::Queen, None)?;

    let mut transcript = Vec::new();
    let attach_detail = format!("role={}", role_label(Role::Queen));
    transcript.push(render_ack_line(
        AckStatus::Ok,
        ConsoleVerb::Attach.ack_label(),
        Some(attach_detail.as_str()),
    ));

    let list_path = worker_list_path();
    let list_lines = read_lines(replay.client(), &list_path)?;
    let list_detail = format!("path={list_path}");
    transcript.push(render_ack_line(
        AckStatus::Ok,
        ConsoleVerb::Ls.ack_label(),
        Some(list_detail.as_str()),
    ));
    transcript.extend(list_lines);

    let telemetry_path = telemetry_path();
    let tail_detail = format!("path={telemetry_path}");
    transcript.push(render_ack_line(
        AckStatus::Ok,
        ConsoleVerb::Tail.ack_label(),
        Some(tail_detail.as_str()),
    ));
    let stream = replay.client().tail(&telemetry_path)?;
    for event in stream {
        match event? {
            TailEvent::Line(line) => transcript.push(line),
            TailEvent::End => transcript.push(END_LINE.to_owned()),
        }
    }

    transcript_support::compare_transcript("coh-status", SCENARIO, "coh-status.txt", &transcript);
    transcript_support::write_timing(
        "coh-status",
        SCENARIO,
        "trace-replay",
        start.elapsed().as_millis() as u64,
    );
    Ok(())
}

fn read_lines<T: cohsh_core::Secure9pTransport>(
    client: &mut CohClient<T>,
    path: &str,
) -> Result<Vec<String>> {
    let fid = client.open(path, OpenMode::read_only())?;
    let mut offset = 0u64;
    let mut buffer = Vec::new();
    loop {
        let chunk = client.read(fid, offset, SECURE9P_MSIZE)?;
        if chunk.is_empty() {
            break;
        }
        offset = offset
            .checked_add(chunk.len() as u64)
            .context("offset overflow during read")?;
        buffer.extend_from_slice(&chunk);
        if chunk.len() < SECURE9P_MSIZE as usize {
            break;
        }
    }
    let _ = client.clunk(fid);
    let text = String::from_utf8(buffer).context("log is not valid UTF-8")?;
    Ok(text.lines().map(|line| line.to_owned()).collect())
}

fn telemetry_path() -> String {
    format!(
        "/{}",
        worker_shards().worker_telemetry_path(WORKER_ID).join("/")
    )
}

fn worker_list_path() -> String {
    format!("/{}", worker_shards().worker_parent(WORKER_ID).join("/"))
}

fn worker_shards() -> ShardLayout {
    ShardLayout::enabled(8, true)
}

fn read_only_trace_fixture() -> Result<Vec<u8>> {
    let server = NineDoor::new_with_shard_layout(worker_shards());
    seed_worker(&server)?;

    let builder = TraceLogBuilder::shared(trace_policy());
    let connection = server.connect().context("open read-only trace session")?;
    let transport =
        TraceTransportRecorder::new(InProcessTransport::new(connection), builder.clone());
    let mut client = CohClient::connect(transport, Role::Queen, None)?;
    let _ = read_lines(&mut client, &worker_list_path())?;
    let stream = client.tail(&telemetry_path())?;
    for event in stream {
        if matches!(event?, TailEvent::End) {
            break;
        }
    }
    let encoded = builder
        .borrow()
        .snapshot()
        .encode(trace_policy())
        .context("encode read-only canonical Worker trace")?;
    Ok(encoded)
}

fn seed_worker(server: &NineDoor) -> Result<()> {
    let connection = server.connect().context("open seed session")?;
    let transport = InProcessTransport::new(connection);
    let mut client = CohClient::connect(transport, Role::Queen, None)?;
    let payload = queen::spawn("heartbeat", ["ticks=4"].iter().copied())?;
    write_payload(&mut client, queen::queen_ctl_path(), payload.as_bytes())?;
    write_payload(&mut client, &telemetry_path(), b"tick 1\ntick 2\n")
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
        return Err(anyhow!(
            "short write to {path}: expected {} bytes, wrote {written}",
            payload.len()
        ));
    }
    clunk_result?;
    Ok(())
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
