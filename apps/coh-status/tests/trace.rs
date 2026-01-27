// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate coh-status trace replay fixtures.
// Author: Lukas Bower

#[path = "../../../tests/fixtures/transcripts/support.rs"]
mod transcript_support;

use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result};
use coh_status::{trace_policy, TraceReplay};
use cohesix_ticket::Role;
use cohsh::client::{CohClient, TailEvent};
use cohsh::SECURE9P_MSIZE;
use cohsh_core::wire::{render_ack, AckLine, AckStatus, END_LINE};
use cohsh_core::{role_label, ConsoleVerb};
use secure9p_codec::OpenMode;

const SCENARIO: &str = "trace_v0";
const WORKER_ID: &str = "worker-1";
const LIST_PATH: &str = "/worker";

#[test]
fn trace_replay_matches_fixture() -> Result<()> {
    let start = Instant::now();
    let payload = load_trace_fixture()?;
    let mut replay = TraceReplay::from_bytes(&payload, Role::Queen, None)?;

    let mut transcript = Vec::new();
    let attach_detail = format!("role={}", role_label(Role::Queen));
    transcript.push(render_ack_line(
        AckStatus::Ok,
        ConsoleVerb::Attach.ack_label(),
        Some(attach_detail.as_str()),
    ));

    let list_lines = read_lines(replay.client(), LIST_PATH)?;
    let list_detail = format!("path={LIST_PATH}");
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
    let mut stream = replay.client().tail(&telemetry_path)?;
    while let Some(event) = stream.next() {
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
    format!("/worker/{}/telemetry", WORKER_ID)
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
    let payload = fs::read(&trace_path)
        .with_context(|| format!("read trace fixture {}", trace_path.display()))?;
    let policy = trace_policy();
    cohsh_core::trace::TraceLog::decode(&payload, policy).context("trace decode failed")?;
    Ok(payload)
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
