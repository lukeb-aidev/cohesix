// CLASSIFICATION: COMMUNITY
// Filename: busybox_runner.rs v0.8
// Author: Lukas Bower
// Date Modified: 2026-10-28

use crate::prelude::*;
/// Execute BusyBox as the interactive shell with role-based command filtering.

use chrono::Utc;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::process::{Command, Stdio};

use crate::runtime::env::init::detect_cohrole;
use crate::runtime::loader;

fn allowed_cmd(role: &str, cmd: &str) -> bool {
    match role {
        "QueenPrimary" => true,
        "DroneWorker" => !matches!(cmd, "wget" | "curl"),
        "InteractiveAiBooth" => matches!(cmd, "echo" | "ls" | "mount" | "cat" | "cohcc" | "run"),
        "KioskInteractive" => matches!(cmd, "echo" | "ls" | "mount" | "cat" | "cohcc" | "run"),
        _ => false,
    }
}

fn log_event(log: &mut std::fs::File, event: &str) {
    let ts = Utc::now().to_rfc3339();
    let pid = process::id();
    let _ = writeln!(log, "[{} pid={}] {}", ts, pid, event);
}

/// Launch BusyBox shell reading from `/srv/console` and writing to `/srv/shell_out`.
pub fn spawn_shell() {
    let role = detect_cohrole();
    println!("[shell] starting BusyBox shell");
    std::fs::write("/log/shell_start", role.as_bytes()).ok();
    let console = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/srv/console");
    let stdin = console
        .as_ref()
        .map(|f| Stdio::from(f.try_clone().unwrap()))
        .unwrap_or(Stdio::null());
    let mut child = match Command::new("/bin/busybox")
        .arg("sh")
        .stdin(stdin)
        .stdout(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            crate::coh_cc::logging::log(
                "ERROR",
                "shell",
                Path::new("/bin/busybox"),
                Path::new("/srv/console"),
                &[],
                &format!("spawn failed: {e}"),
            );
            return;
        }
    };

    let mut console = console.unwrap();
    let mut reader = BufReader::new(console.try_clone().unwrap());
    fs::create_dir_all("/srv").ok();
    fs::create_dir_all("/log").ok();
    let mut log = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/log/session.log")
        .or_else(|_| {
            fs::create_dir_all("/srv/trace").ok();
            OpenOptions::new()
                .create(true)
                .append(true)
                .open("/srv/trace/session.log")
        })
        .unwrap();
    log_event(&mut log, &format!("SESSION START {}", role));

    if std::path::Path::new("/etc/test_boot.sh").exists() {
        let _ = Command::new("/bin/busybox")
            .arg("sh")
            .arg("/etc/test_boot.sh")
            .status();
    }

    let mut line = String::new();
    while reader
        .read_line(&mut line)
        .ok()
        .filter(|n| *n > 0)
        .is_some()
    {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.is_empty() {
            line.clear();
            continue;
        }
        let cmd = tokens[0];
        log_event(&mut log, &format!("CMD {}", line.trim_end()));
        if cmd == "cohcc" {
            if let Some(src) = tokens.get(1) {
                let mut out_path = Path::new("/srv/a.out").to_path_buf();
                if tokens.len() >= 4 && tokens[2] == "-o" {
                    out_path = PathBuf::from(tokens[3]);
                }
                if !(out_path.starts_with("/srv/") || out_path.starts_with("/usr/bin/")) {
                    let msg = b"output must be under /srv or /usr/bin\n";
                    let _ = console.write_all(msg);
                    let _ = fs::write("/srv/shell_out", msg);
                    line.clear();
                    continue;
                }
                if src.starts_with("/usr/src/") || src.starts_with("/srv/") {
                    match crate::coh_cc::compile(src) {
                        Ok(bytes) => {
                            if let Some(parent) = out_path.parent() {
                                fs::create_dir_all(parent).ok();
                            }
                            if fs::write(&out_path, &bytes).is_ok() {
                                let msg = format!("compiled {}\n", out_path.display());
                                let _ = console.write_all(msg.as_bytes());
                                let _ = fs::write("/srv/shell_out", msg.as_bytes());
                            } else {
                                let msg = b"write failed\n";
                                let _ = console.write_all(msg);
                                let _ = fs::write("/srv/shell_out", msg);
                            }
                        }
                        Err(e) => {
                            let msg = format!("compile failed: {}\n", e);
                            let _ = console.write_all(msg.as_bytes());
                            let _ = fs::write("/srv/shell_out", msg.as_bytes());
                        }
                    }
                } else {
                    let msg = b"path must be under /usr/src or /srv\n";
                    let _ = console.write_all(msg);
                    let _ = fs::write("/srv/shell_out", msg);
                }
            } else {
                let msg = b"usage: cohcc <file> -o <out>\n";
                let _ = console.write_all(msg);
                let _ = fs::write("/srv/shell_out", msg);
            }
        } else if cmd == "run" {
            if let Some(bin) = tokens.get(1) {
                match loader::load_and_run(bin) {
                    Ok(_) => {
                        let msg = format!("ran {}\n", bin);
                        let _ = console.write_all(msg.as_bytes());
                        let _ = fs::write("/srv/shell_out", msg.as_bytes());
                    }
                    Err(e) => {
                        let msg = format!("run failed: {}\n", e);
                        let _ = console.write_all(msg.as_bytes());
                        let _ = fs::write("/srv/shell_out", msg.as_bytes());
                    }
                }
            } else {
                let msg = b"usage: run <file>\n";
                let _ = console.write_all(msg);
                let _ = fs::write("/srv/shell_out", msg);
            }
        } else if allowed_cmd(&role, cmd) {
            let output = Command::new("/bin/busybox").args(&tokens).output();
            if let Ok(out) = output {
                let _ = console.write_all(&out.stdout);
                let _ = fs::write("/srv/shell_out", &out.stdout);
            }
        } else {
            let msg = format!("command {cmd} not allowed for role {role}\n");
            let _ = console.write_all(msg.as_bytes());
        }
        line.clear();
    }
    log_event(&mut log, &format!("SESSION STOP {}", role));
    let _ = child.wait();
}
