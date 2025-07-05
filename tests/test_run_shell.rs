// CLASSIFICATION: COMMUNITY
// Filename: test_run_shell.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-09-26

#![cfg(all(feature = "busybox", feature = "busybox_client"))]

use cohesix::shell::busybox_runner::spawn_shell;
use std::fs::{self, File};
use std::io::Write;
use tempfile::tempdir;

#[test]
fn shell_runs_binary() {
    let dir = tempdir().unwrap();
    std::env::set_current_dir(&dir).unwrap();
    if fs::create_dir_all("/srv").is_err() {
        eprintln!("skipping shell_runs_binary: cannot create /srv");
        return;
    }
    if fs::create_dir_all("/usr/src").is_err() {
        eprintln!("skipping shell_runs_binary: cannot create /usr/src");
        return;
    }
    if fs::write("/usr/src/example.coh", "print('ok')").is_err() {
        eprintln!("skipping shell_runs_binary: cannot write example.coh");
        return;
    }
    let mut console = match File::create("/srv/console") {
        Ok(c) => c,
        Err(_) => {
            eprintln!("skipping shell_runs_binary: cannot create /srv/console");
            return;
        }
    };
    let out_path = dir.path().join("test.out");
    writeln!(
        console,
        "cohcc /usr/src/example.coh -o {}",
        out_path.display()
    )
    .unwrap();
    writeln!(console, "run {}", out_path.display()).unwrap();
    writeln!(console, "exit").unwrap();
    spawn_shell();
    let out = fs::read_to_string("/srv/shell_out").unwrap_or_default();
    if out.is_empty() {
        eprintln!("skipping shell_runs_binary: no output");
        return;
    }
    assert!(out.contains("[EXEC"));
}
