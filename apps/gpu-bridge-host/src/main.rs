// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: CLI entry point for the host-side GPU bridge; prints mirrored namespace metadata.
// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! CLI entry point for the host-side GPU bridge. The binary prints discovered
//! GPU information as JSON, enabling integration tests to synchronise the
//! NineDoor namespace with host state.

use anyhow::{anyhow, Context, Result};
use clap::{ArgAction, Parser};
use cohsh_core::wire::{parse_ack, AckStatus};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use gpu_bridge_host::{
    auto_bridge_with_registry, build_publish_lines, namespace_to_json_pretty, GpuNamespaceSnapshot,
};

const DEFAULT_TCP_PORT: u16 = 31337;
const ACK_TIMEOUT: Duration = Duration::from_secs(5);

/// CLI arguments for the GPU bridge host tool.
#[derive(Debug, Parser)]
#[command(author, version, about = "Cohesix GPU bridge host utilities")]
struct Args {
    /// Use the deterministic mock backend instead of NVML.
    #[arg(long, action = ArgAction::SetTrue)]
    mock: bool,
    /// Host registry root containing available model manifests.
    #[arg(long, value_name = "DIR")]
    registry: Option<PathBuf>,
    /// Print GPU namespace JSON to stdout.
    #[arg(long, action = ArgAction::SetTrue)]
    list: bool,
    /// Publish the GPU namespace into /gpu/bridge/ctl on a live Queen.
    #[arg(long, action = ArgAction::SetTrue)]
    publish: bool,
    /// Interval in milliseconds between publish snapshots (requires --publish).
    #[arg(long, value_name = "MS")]
    interval_ms: Option<u64>,
    /// TCP host for the live console publish mode.
    #[arg(long, default_value = "127.0.0.1")]
    tcp_host: String,
    /// TCP port for the live console publish mode.
    #[arg(long, default_value_t = DEFAULT_TCP_PORT)]
    tcp_port: u16,
    /// Authentication token for the live console publish mode.
    #[arg(long)]
    auth_token: Option<String>,
    /// Optional ticket payload when attaching to the console.
    #[arg(long)]
    ticket: Option<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let bridge = auto_bridge_with_registry(args.mock, args.registry.as_deref())?;
    let namespace: GpuNamespaceSnapshot = bridge.serialise_namespace()?;
    if args.list {
        println!("{}", namespace_to_json_pretty(&namespace));
    }
    if args.publish {
        let auth_token = resolve_auth_token(args.auth_token.as_deref());
        let mut client = ConsoleClient::connect(
            &args.tcp_host,
            args.tcp_port,
            auth_token.as_str(),
            args.ticket.as_deref(),
        )
        .context("connect to live console")?;
        let interval = args.interval_ms.map(Duration::from_millis);
        loop {
            let snapshot = bridge.serialise_namespace()?;
            let publish = build_publish_lines(&snapshot)?;
            client
                .publish_lines(&publish.lines)
                .context("publish gpu bridge snapshot")?;
            if let Some(delay) = interval {
                thread::sleep(delay);
            } else {
                break;
            }
        }
    }
    Ok(())
}

fn resolve_auth_token(cli_token: Option<&str>) -> String {
    if let Some(token) = cli_token {
        let trimmed = token.trim();
        if !trimmed.is_empty() {
            return trimmed.to_owned();
        }
    }
    if let Ok(value) = std::env::var("COH_AUTH_TOKEN") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return trimmed.to_owned();
        }
    }
    if let Ok(value) = std::env::var("COHSH_AUTH_TOKEN") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return trimmed.to_owned();
        }
    }
    "changeme".to_owned()
}

struct ConsoleClient {
    stream: TcpStream,
}

impl ConsoleClient {
    fn connect(host: &str, port: u16, auth_token: &str, ticket: Option<&str>) -> Result<Self> {
        let stream = TcpStream::connect((host, port)).with_context(|| format!("{host}:{port}"))?;
        stream.set_read_timeout(Some(Duration::from_millis(200)))?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        let mut client = Self { stream };
        client.send_line(&format!("AUTH {auth_token}"))?;
        client.wait_ack("AUTH")?;
        let ticket = ticket.unwrap_or("");
        client.send_line(&format!("ATTACH queen {ticket}"))?;
        client.wait_ack("ATTACH")?;
        Ok(client)
    }

    fn publish_lines(&mut self, lines: &[String]) -> Result<()> {
        for line in lines {
            self.send_line(&format!("ECHO /gpu/bridge/ctl {line}"))?;
            self.wait_ack("ECHO")?;
        }
        Ok(())
    }

    fn send_line(&mut self, line: &str) -> Result<()> {
        let total_len = line
            .len()
            .checked_add(4)
            .ok_or_else(|| anyhow!("console frame length overflow"))?;
        let len_bytes = (total_len as u32).to_le_bytes();
        self.stream.write_all(&len_bytes)?;
        self.stream.write_all(line.as_bytes())?;
        Ok(())
    }

    fn read_frame(&mut self) -> Result<Option<String>> {
        let mut len_buf = [0u8; 4];
        match self.stream.read_exact(&mut len_buf) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::TimedOut => return Ok(None),
            Err(err) => return Err(err.into()),
        }
        let total_len = u32::from_le_bytes(len_buf) as usize;
        if total_len < 4 {
            return Err(anyhow!("invalid frame length {total_len}"));
        }
        let payload_len = total_len.saturating_sub(4);
        let mut payload = vec![0u8; payload_len];
        self.stream.read_exact(&mut payload)?;
        let line =
            String::from_utf8(payload).map_err(|_| anyhow!("console frame is not UTF-8"))?;
        Ok(Some(line))
    }

    fn wait_ack(&mut self, verb: &str) -> Result<()> {
        let start = Instant::now();
        loop {
            if start.elapsed() > ACK_TIMEOUT {
                return Err(anyhow!("timeout waiting for {verb} ack"));
            }
            let Some(line) = self.read_frame()? else {
                continue;
            };
            let Some(ack) = parse_ack(line.trim()) else {
                continue;
            };
            if !ack.verb.eq_ignore_ascii_case(verb) {
                continue;
            }
            if matches!(ack.status, AckStatus::Ok) {
                return Ok(());
            }
            let detail = ack
                .detail
                .map(|value| value.to_owned())
                .unwrap_or_else(|| "unknown".to_owned());
            return Err(anyhow!("{verb} failed: {detail}"));
        }
    }
}
