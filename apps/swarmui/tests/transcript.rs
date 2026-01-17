// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Compare SwarmUI telemetry transcript against CLI golden output.
// Author: Lukas Bower

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use cohsh::client::{CohClient, InProcessTransport, TailEvent};
use cohsh::queen;
use cohsh_core::wire::{render_ack, AckLine, AckStatus, END_LINE};
use cohsh_core::{role_label, ConsoleVerb};
use cohesix_ticket::Role;
use nine_door::NineDoor;
use secure9p_codec::OpenMode;
use swarmui::{SwarmUiBackend, SwarmUiConfig, SwarmUiTransportFactory};

const WORKER_ID: &str = "worker-1";

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
fn telemetry_transcript_matches_cli() -> Result<()> {
    let server = NineDoor::new();
    seed_worker(&server)?;

    let cli_lines = run_cli_transcript(&server)?;
    let ui_lines = run_ui_transcript(&server)?;

    let transcripts_root = transcripts_root();
    let cli_path = write_transcript(&transcripts_root, "cli.txt", &cli_lines)?;
    let ui_path = write_transcript(&transcripts_root, "ui.txt", &ui_lines)?;

    assert_eq!(
        cli_lines, ui_lines,
        "cli vs ui transcript mismatch: {} vs {}",
        cli_path.display(),
        ui_path.display()
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

fn run_cli_transcript(server: &NineDoor) -> Result<Vec<String>> {
    let connection = server.connect().context("open NineDoor session")?;
    let transport = InProcessTransport::new(connection);
    let mut client = CohClient::connect(transport, Role::Queen, None)?;
    let mut transcript = Vec::new();
    let detail = format!("role={}", role_label(Role::Queen));
    transcript.push(render_ack_line(
        AckStatus::Ok,
        ConsoleVerb::Attach.ack_label(),
        Some(detail.as_str()),
    ));
    let path = format!("/worker/{}/telemetry", WORKER_ID);
    let detail = format!("path={path}");
    transcript.push(render_ack_line(
        AckStatus::Ok,
        ConsoleVerb::Tail.ack_label(),
        Some(detail.as_str()),
    ));
    let mut stream = client.tail(&path)?;
    while let Some(event) = stream.next() {
        match event? {
            TailEvent::Line(line) => transcript.push(line),
            TailEvent::End => transcript.push(END_LINE.to_owned()),
        }
    }
    Ok(transcript)
}

fn run_ui_transcript(server: &NineDoor) -> Result<Vec<String>> {
    let data_dir = std::env::temp_dir();
    let config = SwarmUiConfig::from_generated(data_dir);
    let factory = InProcessFactory {
        server: server.clone(),
    };
    let mut backend = SwarmUiBackend::new(config, factory);
    let mut lines = Vec::new();
    let attach = backend.attach(Role::Queen, None);
    lines.extend(attach.lines);
    let tail = backend.tail_telemetry(Role::Queen, None, WORKER_ID);
    lines.extend(tail.lines);
    assert_eq!(backend.active_tails(), 0);
    Ok(lines)
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
    let ack = AckLine { status, verb, detail };
    let mut line = String::new();
    render_ack(&mut line, &ack).expect("render ack line");
    line
}

fn write_transcript(root: &Path, name: &str, lines: &[String]) -> Result<PathBuf> {
    fs::create_dir_all(root).with_context(|| format!("create {}", root.display()))?;
    let path = root.join(name);
    let mut payload = lines.join("\n");
    payload.push('\n');
    fs::write(&path, payload).with_context(|| format!("write {}", path.display()))?;
    Ok(path)
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target")
}

fn transcripts_root() -> PathBuf {
    std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root())
        .join("swarmui-transcripts")
}
