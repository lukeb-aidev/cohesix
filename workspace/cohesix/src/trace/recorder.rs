// CLASSIFICATION: COMMUNITY
// Filename: recorder.rs v0.1
// Author: Lukas Bower
// Date Modified: 2029-02-15

use crate::CohError;
#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
/// Syscall and agent event recorder.
//
/// Logs spawn, exec, capability grants and read/write operations into
/// `/log/trace/live.log` with simple JSON lines. Supports replay of a
/// trace file to re-execute scenarios.
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
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
    let dir = trace_directory();
    let path = dir.join("live.log");
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .unwrap_or_else(|_| {
            let tmp_dir = std::env::temp_dir().join("cohesix_trace");
            fs::create_dir_all(&tmp_dir).ok();
            let tmp_path = tmp_dir.join("live.log");
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&tmp_path)
                .unwrap_or_else(|e| panic!("trace record failed: {}", e))
        });
    let ev = TraceEvent {
        ts: now(),
        agent: agent.into(),
        event: event.into(),
        detail: detail.into(),
        ok,
    };
    let line = serde_json::to_string(&ev).unwrap();
    let _ = writeln!(f, "{}", line);
}

/// Spawn a process while recording the event.
pub fn spawn(agent: &str, cmd: &str, args: &[&str]) -> std::io::Result<()> {
    let dir = trace_directory();
    fs::create_dir_all(&dir).ok();
    let result = Command::new(cmd)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    record(
        agent,
        "spawn",
        cmd,
        result.as_ref().map(|s| s.success()).unwrap_or(false),
    );
    result.map(|_| ())
}

pub fn exec(agent: &str, cmd: &str) -> std::io::Result<()> {
    let result = Command::new(cmd).status();
    record(
        agent,
        "exec",
        cmd,
        result.as_ref().map(|s| s.success()).unwrap_or(false),
    );
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
pub fn replay(file: &str) -> Result<(), CohError> {
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

fn trace_directory() -> PathBuf {
    if let Ok(dir) = std::env::var("TRACE_OUT") {
        let custom = PathBuf::from(dir);
        fs::create_dir_all(&custom).ok();
        return custom;
    }

    let primary = PathBuf::from("/log/trace");
    if fs::create_dir_all(&primary).is_ok() {
        ensure_compat_symlink(&primary);
        return primary;
    }

    let legacy = PathBuf::from("/srv/trace");
    if fs::create_dir_all(&legacy).is_ok() {
        return legacy;
    }

    let fallback = std::env::temp_dir().join("cohesix_trace");
    fs::create_dir_all(&fallback).ok();
    fallback
}

fn ensure_compat_symlink(primary: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let link = Path::new("/srv/trace");
        match fs::symlink_metadata(link) {
            Ok(meta) if meta.file_type().is_symlink() => {
                if let Ok(existing) = fs::read_link(link) {
                    if existing == primary {
                        return;
                    }
                }
                let _ = fs::remove_file(link);
            }
            Ok(meta) if meta.is_dir() => {
                return;
            }
            Ok(_) => {
                return;
            }
            Err(_) => {}
        }
        let _ = symlink(primary, link);
    }
}

#[cfg(not(unix))]
fn ensure_compat_symlink(_primary: &Path) {}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
