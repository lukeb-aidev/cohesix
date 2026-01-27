// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Compare Cohsh convergence transcript against shared fixtures.
// Author: Lukas Bower

#[path = "../../../tests/fixtures/transcripts/support.rs"]
mod transcript_support;

use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use cohesix_ticket::Role;
use cohsh::client::{CohClient, InProcessTransport, TailEvent};
use cohsh::queen;
use cohsh_core::wire::{render_ack, AckLine, AckStatus, END_LINE};
use cohsh_core::{role_label, ConsoleVerb};
use nine_door::NineDoor;
use secure9p_codec::OpenMode;

const SCENARIO: &str = "converge_v0";
const CONSOLE_ACK_FANOUT: usize = 1;
const WORKER_ID: &str = "worker-1";
const QUEEN_LOG_PATH: &str = "/log/queen.log";
const SPAWN_PAYLOAD: &str = "{\"spawn\":\"heartbeat\",\"ticks\":1,\"budget\":{\"ttl_s\":30}}";

#[test]
fn converge_transcript_matches_fixture() -> Result<()> {
    let start = Instant::now();
    let server = NineDoor::new();
    seed_worker(&server)?;

    let lines = run_converge_transcript(&server)?;
    transcript_support::compare_transcript("cohsh", SCENARIO, "cohsh.txt", &lines);
    transcript_support::write_timing(
        "cohsh",
        SCENARIO,
        "transcript",
        start.elapsed().as_millis() as u64,
    );

    Ok(())
}

fn seed_worker(server: &NineDoor) -> Result<()> {
    let connection = server.connect().context("open NineDoor session")?;
    let transport = InProcessTransport::new(connection);
    let mut client = CohClient::connect(transport, Role::Queen, None)?;
    let payload = queen::spawn("heartbeat", ["ticks=4"].iter().copied())?;
    write_payload(&mut client, queen::queen_ctl_path(), payload.as_bytes())?;
    let telemetry_path = format!("/worker/{}/telemetry", WORKER_ID);
    write_payload(&mut client, &telemetry_path, b"tick 1\n")?;
    write_payload(&mut client, &telemetry_path, b"tick 2\n")?;
    Ok(())
}

fn run_converge_transcript(server: &NineDoor) -> Result<Vec<String>> {
    let connection = server.connect().context("open NineDoor session")?;
    let transport = InProcessTransport::new(connection);
    let mut client = CohClient::connect(transport, Role::Queen, None)?;

    let mut transcript = Vec::new();
    let detail = format!("role={}", role_label(Role::Queen));
    for _ in 0..CONSOLE_ACK_FANOUT {
        transcript.push(render_ack_line(
            AckStatus::Ok,
            ConsoleVerb::Attach.ack_label(),
            Some(detail.as_str()),
        ));
    }

    append_tail(&mut transcript, &mut client, QUEEN_LOG_PATH)?;
    append_spawn(&mut transcript, &mut client)?;

    let telemetry_path = format!("/worker/{}/telemetry", WORKER_ID);
    append_tail(&mut transcript, &mut client, &telemetry_path)?;

    for _ in 0..CONSOLE_ACK_FANOUT {
        transcript.push(render_ack_line(
            AckStatus::Ok,
            ConsoleVerb::Quit.ack_label(),
            None,
        ));
    }

    Ok(transcript)
}

fn append_tail(
    transcript: &mut Vec<String>,
    client: &mut CohClient<InProcessTransport>,
    path: &str,
) -> Result<()> {
    let detail = format!("path={path}");
    transcript.push(render_ack_line(
        AckStatus::Ok,
        ConsoleVerb::Tail.ack_label(),
        Some(detail.as_str()),
    ));
    let mut stream = client.tail(path)?;
    while let Some(event) = stream.next() {
        match event? {
            TailEvent::Line(line) => transcript.push(line),
            TailEvent::End => transcript.push(END_LINE.to_owned()),
        }
    }
    Ok(())
}

fn append_spawn(
    transcript: &mut Vec<String>,
    client: &mut CohClient<InProcessTransport>,
) -> Result<()> {
    write_payload(client, queen::queen_ctl_path(), SPAWN_PAYLOAD.as_bytes())?;
    let detail = format!(
        "path={} bytes={}",
        queen::queen_ctl_path(),
        SPAWN_PAYLOAD.as_bytes().len()
    );
    transcript.push(render_ack_line(
        AckStatus::Ok,
        ConsoleVerb::Echo.ack_label(),
        Some(detail.as_str()),
    ));
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
