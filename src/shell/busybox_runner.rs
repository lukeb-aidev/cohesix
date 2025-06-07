// CLASSIFICATION: COMMUNITY
// Filename: busybox_runner.rs v0.3
// Author: Lukas Bower
// Date Modified: 2025-06-25

//! Execute BusyBox as the interactive shell with role-based command filtering.

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use crate::runtime::env::init::detect_cohrole;
use crate::kernel::fs::busybox;

fn allowed_cmd(role: &str, cmd: &str) -> bool {
    match role {
        "QueenPrimary" => true,
        "DroneWorker" => !matches!(cmd, "wget" | "curl"),
        "KioskInteractive" => matches!(cmd, "echo" | "ls" | "mount" | "cat"),
        _ => false,
    }
}

/// Launch BusyBox shell reading from `/dev/console` and writing to `/srv/shell_out`.
pub fn spawn_shell() {
    let role = detect_cohrole();
    let console = OpenOptions::new().read(true).write(true).open("/dev/console");
    let stdin = console.as_ref().map(|f| Stdio::from(f.try_clone().unwrap())).unwrap_or(Stdio::null());
    let mut child = Command::new("/bin/busybox")
        .arg("sh")
        .stdin(stdin)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap_or_else(|_| panic!("busybox not found"));

    let mut console = console.unwrap();
    let mut reader = BufReader::new(console.try_clone().unwrap());
    fs::create_dir_all("/srv").ok();

    let mut line = String::new();
    while reader.read_line(&mut line).ok().filter(|n| *n > 0).is_some() {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.is_empty() { continue; }
        let cmd = tokens[0];
        if allowed_cmd(&role, cmd) {
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
    let _ = child.wait();
}
