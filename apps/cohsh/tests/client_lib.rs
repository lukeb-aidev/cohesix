// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Compare CohClient 9P replay output against console semantics.
// Author: Lukas Bower

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use cohesix_ticket::Role;
use cohsh::client::{CohClient, InProcessTransport, TailEvent};
use cohsh::queen;
use cohsh::{NineDoorTransport, Session, Transport};
use cohsh_core::wire::{render_ack, AckLine, AckStatus, END_LINE};
use cohsh_core::{parse_role, role_label, ConsoleVerb, RoleParseMode};
use nine_door::NineDoor;
use secure9p_codec::OpenMode;

const QUEEN_LOG_PATH: &str = "/log/queen.log";

#[test]
fn client_replay_matches_console() -> Result<()> {
    let script_path = repo_root()
        .join("scripts")
        .join("cohsh")
        .join("session_pool.coh");
    let contents = fs::read_to_string(&script_path)
        .with_context(|| format!("read script {}", script_path.display()))?;
    let commands = parse_replay_commands(&contents)?;

    let console_lines = run_console_replay(&commands)?;
    let client_lines = run_client_replay(&commands)?;

    let transcripts_root = transcripts_root();
    let console_path = write_transcript(&transcripts_root, "console.txt", &console_lines)?;
    let client_path = write_transcript(&transcripts_root, "client.txt", &client_lines)?;

    assert_eq!(
        console_lines,
        client_lines,
        "console vs client transcript mismatch: {} vs {}",
        console_path.display(),
        client_path.display()
    );

    Ok(())
}

fn parse_replay_commands(contents: &str) -> Result<Vec<String>> {
    let mut commands = Vec::new();
    let mut after_quit = false;
    for raw_line in contents.lines() {
        let trimmed = raw_line.trim_end();
        let without_comment = trimmed
            .split_once('#')
            .map(|(before, _)| before)
            .unwrap_or(trimmed);
        let text = without_comment.trim();
        if text.is_empty() {
            continue;
        }
        let keyword = text.split_whitespace().next().unwrap_or("");
        if !after_quit {
            if keyword.eq_ignore_ascii_case("quit") {
                after_quit = true;
            }
            continue;
        }
        if keyword.eq_ignore_ascii_case("expect") || keyword.eq_ignore_ascii_case("wait") {
            continue;
        }
        commands.push(translate_command(text));
    }
    if !after_quit {
        return Err(anyhow!("session_pool.coh must include a quit delimiter"));
    }
    if commands.is_empty() {
        return Err(anyhow!("session_pool.coh replay section is empty"));
    }
    Ok(commands)
}

fn translate_command(line: &str) -> String {
    let mut parts = line.split_whitespace();
    let Some(keyword) = parts.next() else {
        return line.to_owned();
    };
    if keyword.eq_ignore_ascii_case("log") && parts.next().is_none() {
        return format!("tail {QUEEN_LOG_PATH}");
    }
    line.to_owned()
}

fn run_console_replay(commands: &[String]) -> Result<Vec<String>> {
    let server = NineDoor::new();
    let mut transport = NineDoorTransport::new(server);
    let mut session: Option<Session> = None;
    let mut transcript = Vec::new();
    for line in commands {
        let (command, args) = split_command(line)?;
        match command.as_str() {
            "attach" => {
                let role = parse_role_arg(args.get(0))?;
                let ticket = args.get(1).map(String::as_str);
                let result = transport.attach(role, ticket);
                transcript.extend(transport.drain_acknowledgements());
                session = Some(result?);
            }
            "spawn" => {
                let session = session.as_ref().context("attach before spawn")?;
                let role = args.get(0).context("spawn requires a role")?;
                let payload = queen::spawn(role, args.iter().skip(1).map(String::as_str))?;
                let result = transport.write(session, queen::queen_ctl_path(), payload.as_bytes());
                transcript.extend(transport.drain_acknowledgements());
                result?;
            }
            "kill" => {
                let session = session.as_ref().context("attach before kill")?;
                let worker_id = args.get(0).context("kill requires a worker id")?;
                let payload = queen::kill(worker_id)?;
                let result = transport.write(session, queen::queen_ctl_path(), payload.as_bytes());
                transcript.extend(transport.drain_acknowledgements());
                result?;
            }
            "tail" => {
                let session = session.as_ref().context("attach before tail")?;
                let path = args.get(0).context("tail requires a path")?;
                let result = transport.tail(session, path);
                transcript.extend(transport.drain_acknowledgements());
                match result {
                    Ok(lines) => {
                        transcript.extend(lines);
                        transcript.push(END_LINE.to_owned());
                    }
                    Err(err) => {
                        if !is_abuse_path(path) {
                            return Err(err);
                        }
                    }
                }
            }
            other => {
                return Err(anyhow!("unsupported replay command '{other}'"));
            }
        }
    }
    Ok(transcript)
}

fn run_client_replay(commands: &[String]) -> Result<Vec<String>> {
    let server = NineDoor::new();
    let mut client: Option<CohClient<InProcessTransport>> = None;
    let mut transcript = Vec::new();
    for line in commands {
        let (command, args) = split_command(line)?;
        match command.as_str() {
            "attach" => {
                let role = parse_role_arg(args.get(0))?;
                let ticket = args.get(1).map(String::as_str);
                let connection = server.connect().context("open NineDoor session")?;
                let transport = InProcessTransport::new(connection);
                match CohClient::connect(transport, role, ticket) {
                    Ok(next) => {
                        let detail = format!("role={}", role_label(role));
                        transcript.push(render_ack_line(
                            AckStatus::Ok,
                            ConsoleVerb::Attach.ack_label(),
                            Some(detail.as_str()),
                        ));
                        client = Some(next);
                    }
                    Err(err) => {
                        let detail = format!("reason={err}");
                        transcript.push(render_ack_line(
                            AckStatus::Err,
                            ConsoleVerb::Attach.ack_label(),
                            Some(detail.as_str()),
                        ));
                        return Err(err);
                    }
                }
            }
            "spawn" => {
                let client = client.as_mut().context("attach before spawn")?;
                let role = args.get(0).context("spawn requires a role")?;
                let payload = queen::spawn(role, args.iter().skip(1).map(String::as_str))?;
                write_payload(client, queen::queen_ctl_path(), payload.as_bytes())
                    .with_context(|| "spawn write failed")?;
                let detail = format!(
                    "path={} bytes={}",
                    queen::queen_ctl_path(),
                    payload.as_bytes().len()
                );
                transcript.push(render_ack_line(
                    AckStatus::Ok,
                    ConsoleVerb::Spawn.ack_label(),
                    Some(detail.as_str()),
                ));
            }
            "kill" => {
                let client = client.as_mut().context("attach before kill")?;
                let worker_id = args.get(0).context("kill requires a worker id")?;
                let payload = queen::kill(worker_id)?;
                write_payload(client, queen::queen_ctl_path(), payload.as_bytes())
                    .with_context(|| "kill write failed")?;
                let detail = format!(
                    "path={} bytes={}",
                    queen::queen_ctl_path(),
                    payload.as_bytes().len()
                );
                transcript.push(render_ack_line(
                    AckStatus::Ok,
                    ConsoleVerb::Kill.ack_label(),
                    Some(detail.as_str()),
                ));
            }
            "tail" => {
                let client = client.as_mut().context("attach before tail")?;
                let path = args.get(0).context("tail requires a path")?;
                match client.tail(path) {
                    Ok(mut stream) => {
                        let detail = format!("path={path}");
                        transcript.push(render_ack_line(
                            AckStatus::Ok,
                            ConsoleVerb::Tail.ack_label(),
                            Some(detail.as_str()),
                        ));
                        while let Some(event) = stream.next() {
                            match event? {
                                TailEvent::Line(line) => transcript.push(line),
                                TailEvent::End => transcript.push(END_LINE.to_owned()),
                            }
                        }
                    }
                    Err(err) => {
                        let detail = format!("path={path} reason={err}");
                        transcript.push(render_ack_line(
                            AckStatus::Err,
                            ConsoleVerb::Tail.ack_label(),
                            Some(detail.as_str()),
                        ));
                        if !is_abuse_path(path) {
                            return Err(err);
                        }
                    }
                }
            }
            other => {
                return Err(anyhow!("unsupported replay command '{other}'"));
            }
        }
    }
    Ok(transcript)
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

fn split_command(line: &str) -> Result<(String, Vec<String>)> {
    let mut parts = line.split_whitespace();
    let Some(command) = parts.next() else {
        return Err(anyhow!("empty command"));
    };
    Ok((command.to_owned(), parts.map(str::to_owned).collect()))
}

fn parse_role_arg(input: Option<&String>) -> Result<Role> {
    let Some(value) = input else {
        return Err(anyhow!("attach requires a role"));
    };
    parse_role(value, RoleParseMode::Strict).ok_or_else(|| anyhow!("unknown role '{value}'"))
}

fn is_abuse_path(path: &str) -> bool {
    path.split('/').any(|component| component == "..")
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
        .join("..")
        .join("..")
}

fn target_root() -> PathBuf {
    std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repo_root().join("target"))
}

fn transcripts_root() -> PathBuf {
    target_root().join("cohsh-client-transcripts")
}
