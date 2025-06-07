// CLASSIFICATION: COMMUNITY
// Filename: busybox_runner.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-06-19

//! Execute BusyBox commands as a fallback shell for Plan 9 interaction.
//!
//! Commands are read from `/dev/console` and executed within the sandbox.
//! Output is written back to the console and to `/srv/shell_out`.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader, Write};
use std::process::Command;

use crate::cohesix_types::Syscall;
use crate::kernel::fs::busybox;
use crate::sandbox::chain::{DefaultChainExecutor, SandboxChainExecutor};

/// Spawn a BusyBox shell, piping I/O to `/dev/console` when available.
pub fn spawn_shell() {
    let console = OpenOptions::new().read(true).write(true).open("/dev/console");
    let stdin = console
        .map(|f| Stdio::from(f))
        .unwrap_or(Stdio::null());
    let mut child = match Command::new("busybox")
        .arg("sh")
        .stdin(stdin)
        .stdout(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => {
            println!("[busybox_runner] busybox not found, using kernel stub");
            busybox::run_command("uname", &[]);
            return;
    let executor = DefaultChainExecutor;
    let console = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/console")
        .or_else(|_| OpenOptions::new().write(true).open("/dev/tty"));

    let mut console = match console {
        Ok(f) => f,
        Err(_) => {
            println!("[busybox_runner] console unavailable");
            return;
        }
    };

    let mut reader = BufReader::new(console.try_clone().unwrap());
    writeln!(console, "[busybox_runner] ready").ok();
    let mut line = String::new();
    while reader
        .read_line(&mut line)
        .ok()
        .filter(|n| *n > 0)
        .is_some()
    {
        let tokens: Vec<String> = line.split_whitespace().map(|s| s.to_string()).collect();
        line.clear();
        if tokens.is_empty() {
            continue;
        }
        executor.execute_chain(vec![Syscall::Spawn {
            program: tokens[0].clone(),
            args: tokens[1..].to_vec(),
        }]);
        let output = Command::new("busybox").args(&tokens).output();
        if let Ok(out) = output {
            let _ = fs::write("/srv/shell_out", &out.stdout);
            let _ = console.write_all(&out.stdout);
        } else {
            busybox::run_command(
                &tokens[0],
                &tokens[1..].iter().map(|s| s.as_str()).collect::<Vec<_>>(),
            );
        }
    };

    if let Some(mut out) = child.stdout.take() {
        let mut buf = Vec::new();
        let _ = out.read_to_end(&mut buf);
        fs::create_dir_all("/srv").ok();
        let mut f = OpenOptions::new().create(true).append(true).open("/srv/shell_out").unwrap();
        let _ = f.write_all(&buf);
    }

    let _ = child.wait();
}
