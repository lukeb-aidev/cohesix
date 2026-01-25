// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Compare SwarmUI console prompt output against cohsh transcripts.
// Author: Lukas Bower

#[path = "../../../tests/fixtures/transcripts/support.rs"]
mod transcript_support;

use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use cohsh::client::{CohClient, InProcessTransport};
use cohsh::queen;
use cohsh::trace::{TraceAckMode, TraceShellTransport};
use cohesix_ticket::Role;
use nine_door::NineDoor;
use secure9p_codec::OpenMode;
use swarmui::{SwarmUiConsoleBackend, SwarmUiConfig};

const SCENARIO: &str = "converge_v0";
const WORKER_ID: &str = "worker-1";
#[test]
fn console_prompt_matches_cohsh_transcript() -> Result<()> {
    let start = Instant::now();
    let server = NineDoor::new();
    seed_worker(&server)?;

    let mut backend = build_backend(&server);
    let mut lines = Vec::new();
    let commands = [
        "help",
        "attach queen",
        "log",
        "spawn heartbeat ticks=1 ttl_s=30",
        "tail /worker/worker-1/telemetry",
        "quit",
    ];

    for command in commands {
        let transcript = backend.console_command(command);
        lines.extend(transcript.lines);
    }

    transcript_support::compare_transcript("swarmui", SCENARIO, "cohsh.txt", &lines);
    transcript_support::write_timing(
        "swarmui",
        SCENARIO,
        "console_parity",
        start.elapsed().as_millis() as u64,
    );

    Ok(())
}

fn build_backend(server: &NineDoor) -> SwarmUiConsoleBackend<TraceShellTransport<InProcessTransport>> {
    let data_dir = std::env::temp_dir();
    let config = SwarmUiConfig::from_generated(data_dir);
    let server = server.clone();
    let transport = TraceShellTransport::new(
        Box::new(move || {
            let connection = server.connect().context("open NineDoor session")?;
            Ok(InProcessTransport::new(connection))
        }),
        TraceAckMode::None,
        "trace",
    );
    SwarmUiConsoleBackend::with_transport(config, transport)
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
