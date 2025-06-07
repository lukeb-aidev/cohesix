// CLASSIFICATION: COMMUNITY
// Filename: busybox_runner.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-06-18

//! Execute BusyBox commands as a fallback shell for Plan 9 interaction.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::process::{Command, Stdio};

use crate::kernel::fs::busybox;

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
