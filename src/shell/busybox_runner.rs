// CLASSIFICATION: COMMUNITY
// Filename: busybox_runner.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-06-18

//! Execute BusyBox commands as a fallback shell for Plan 9 interaction.

use std::process::Command;

use crate::kernel::fs::busybox;

/// Spawn a BusyBox shell, piping I/O to `/dev/console` when available.
pub fn spawn_shell() {
    match Command::new("busybox").arg("sh").status() {
        Ok(status) => {
            println!("[busybox_runner] exited with {}", status);
        }
        Err(_) => {
            println!("[busybox_runner] busybox not found, using kernel stub");
            busybox::run_command("uname", &[]);
        }
    }
}
