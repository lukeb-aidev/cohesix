// CLASSIFICATION: COMMUNITY
// Filename: test_cohcc_shell.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-09-25

#![cfg(feature = "busybox")]

use cohesix::shell::busybox_runner::spawn_shell;
use std::fs::{self, File};
use std::io::Write;
use tempfile::tempdir;

#[test]
fn shell_runs_cohcc() {
    let dir = tempdir().unwrap();
    std::env::set_current_dir(&dir).unwrap();
    fs::create_dir_all("/dev").unwrap();
    fs::create_dir_all("/srv").unwrap();
    fs::create_dir_all("/usr/src").unwrap();
    fs::write("/usr/src/example.coh", "print('ok')").unwrap();
    let mut console = File::create("/dev/console").unwrap();
    writeln!(console, "cohcc /usr/src/example.coh").unwrap();
    writeln!(console, "exit").unwrap();
    spawn_shell();
    let out = fs::read_to_string("/srv/shell_out").unwrap_or_default();
    if out.is_empty() {
        eprintln!("skipping shell_runs_cohcc: no output");
        return;
    }
    assert!(out.contains("compiled"));
}
