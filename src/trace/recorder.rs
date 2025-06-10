// CLASSIFICATION: COMMUNITY
// Filename: recorder.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-06-21

//! Syscall and agent event recorder.
//!
//! Logs spawn, exec, capability grants and read/write operations into
//! `/srv/trace/live.log` with simple JSON lines. Supports replay of a
//! trace file to re-execute scenarios.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::net::UnixDatagram;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct TraceEvent {
    ts: u64,
    agent: String,
    event: String,
    detail: String,
    ok: bool,
}

/// Record a syscall-like event.
fn record(agent: &str, event: &str, detail: &str, ok: bool) {
    fs::create_dir_all("/srv/trace").ok();
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/srv/trace/live.log")
        .unwrap();
    let ev = TraceEvent {
        ts: now(),
        agent: agent.into(),
        event: event.into(),
        detail: detail.into(),
        ok,
    };
    let line = serde_json::to_string(&ev).unwrap();
    let _ = writeln!(f, "{}", line);
    if let Ok(sock) = UnixDatagram::unbound() {
        if sock.connect("/srv/validator/live.sock").is_ok() {
            let _ = sock.send(line.as_bytes());
        }
    }
}

/// Spawn a process while recording the event.
pub fn spawn(agent: &str, cmd: &str, args: &[&str]) -> std::io::Result<()> {
    let result = Command::new(cmd)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    record(agent, "spawn", cmd, result.as_ref().map(|s| s.success()).unwrap_or(false));
    result.map(|_| ())
}

pub fn exec(agent: &str, cmd: &str) -> std::io::Result<()> {
    let result = Command::new(cmd).status();
    record(agent, "exec", cmd, result.as_ref().map(|s| s.success()).unwrap_or(false));
    result.map(|_| ())
}

pub fn cap_grant(agent: &str, target: &str, cap: &str) {
    record(agent, "cap_grant", &format!("{} -> {}", cap, target), true);
}

pub fn read(agent: &str, path: &str) -> std::io::Result<String> {
    let res = fs::read_to_string(path);
    record(agent, "read", path, res.is_ok());
    res
}

pub fn write(agent: &str, path: &str, data: &str) -> std::io::Result<()> {
    let res = fs::write(path, data);
    record(agent, "write", path, res.is_ok());
    res
}

/// Record a generic event without side effects.
pub fn event(agent: &str, event: &str, detail: &str) {
    record(agent, event, detail, true);
}

/// Replay events from a trace file.
pub fn replay(file: &str) -> anyhow::Result<()> {
    let data = fs::read_to_string(file)?;
    for line in data.lines() {
        let ev: TraceEvent = serde_json::from_str(line)?;
        match ev.event.as_str() {
            "spawn" => {
                let _ = Command::new(&ev.detail).status();
            }
            "exec" => {
                let _ = Command::new(&ev.detail).status();
            }
            _ => {}
        }
        // record replayed event for validator hooks
        event(&ev.agent, "replay", &format!("{} {}", ev.event, ev.detail));
    }
    Ok(())
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
