// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Compare SwarmUI convergence transcript against shared fixtures.
// Author: Lukas Bower

#[path = "../../../tests/fixtures/transcripts/support.rs"]
mod transcript_support;

use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use cohesix_ticket::Role;
use cohsh::client::{CohClient, InProcessTransport, TailEvent};
use cohsh::queen;
use cohsh_core::wire::{render_ack, AckLine, AckStatus, END_LINE};
use cohsh_core::ConsoleVerb;
use nine_door::NineDoor;
use secure9p_codec::OpenMode;
use swarmui::{SwarmUiBackend, SwarmUiConfig, SwarmUiTransportFactory};

const SCENARIO: &str = "converge_v0";
const CONSOLE_ACK_FANOUT: usize = 1;
const WORKER_ID: &str = "worker-1";
const QUEEN_LOG_PATH: &str = "/log/queen.log";
const SPAWN_PAYLOAD: &str = "{\"spawn\":\"heartbeat\",\"ticks\":1,\"budget\":{\"ttl_s\":30}}";

struct InProcessFactory {
    server: NineDoor,
}

impl SwarmUiTransportFactory for InProcessFactory {
    type Transport = InProcessTransport;

    fn connect(&self) -> Result<Self::Transport, swarmui::SwarmUiError> {
        let connection = self
            .server
            .connect()
            .map_err(|err| swarmui::SwarmUiError::Transport(err.to_string()))?;
        Ok(InProcessTransport::new(connection))
    }
}

#[test]
fn converge_transcript_matches_fixture() -> Result<()> {
    let start = Instant::now();
    let server = NineDoor::new();
    seed_worker(&server)?;

    let lines = run_converge_transcript(&server)?;
    transcript_support::compare_transcript("swarmui", SCENARIO, "swarmui.txt", &lines);
    transcript_support::write_timing(
        "swarmui",
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
    let data_dir = std::env::temp_dir();
    let config = SwarmUiConfig::from_generated(data_dir);
    let factory = InProcessFactory {
        server: server.clone(),
    };
    let mut backend = SwarmUiBackend::new(config, factory);

    let mut transcript = Vec::new();
    let attach = backend.attach(Role::Queen, None);
    for _ in 0..CONSOLE_ACK_FANOUT {
        transcript.extend(attach.lines.iter().cloned());
    }

    let mut client = connect_client(server)?;
    append_tail(&mut transcript, &mut client, QUEEN_LOG_PATH)?;
    append_spawn(&mut transcript, &mut client)?;

    let telemetry = backend.tail_telemetry(Role::Queen, None, WORKER_ID);
    transcript.extend(telemetry.lines);
    assert_eq!(backend.active_tails(), 0);

    for _ in 0..CONSOLE_ACK_FANOUT {
        transcript.push(render_ack_line(
            AckStatus::Ok,
            ConsoleVerb::Quit.ack_label(),
            None,
        ));
    }

    Ok(transcript)
}

fn connect_client(server: &NineDoor) -> Result<CohClient<InProcessTransport>> {
    let connection = server.connect().context("open NineDoor session")?;
    let transport = InProcessTransport::new(connection);
    CohClient::connect(transport, Role::Queen, None).context("connect CohClient")
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
