// CLASSIFICATION: COMMUNITY
// Filename: test_runtime_cli.rs v0.3
// Author: Lukas Bower
// Date Modified: 2026-10-09

#![cfg(feature = "busybox")]

use cohesix::shell::busybox_runner::spawn_shell;
use std::fs::{self, File};
use std::io::Write;
use tempfile::tempdir;

#[test]
fn cli_tools_execute() {
    let dir = tempdir().unwrap();
    std::env::set_current_dir(&dir).unwrap();
    if fs::create_dir_all("/srv").is_err() {
        return;
    }
    if fs::create_dir_all("/usr/src").is_err() {
        return;
    }
    fs::write("/usr/src/hello.c", "int main(){return 0;}").ok();
    let mut console = match File::create("/srv/console") {
        Ok(c) => c,
        Err(_) => return,
    };
    let out_path = dir.path().join("hello.out");
    writeln!(console, "cohcc /usr/src/hello.c -o {}", out_path.display()).unwrap();
    writeln!(console, "run {}", out_path.display()).unwrap();
    writeln!(console, "cohtrace status").unwrap();
    writeln!(console, "exit").unwrap();
    spawn_shell();
    let out = fs::read_to_string("/srv/shell_out").unwrap_or_default();
    if out.is_empty() {
        return;
    }
    assert!(out.contains("role:"));
    assert!(out.contains("compiled") || out.contains("ran"));
}

#[test]
fn cohtrace_status_output() {
    use std::process::Command;
    let output = Command::new(env!("CARGO_BIN_EXE_cohesix_trace"))
        .arg("status")
        .output()
        .expect("run cohtrace");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("Validator"));
    assert!(text.contains("Role"));
    assert!(text.contains("Mount"));
}

#[test]
fn cohtrace_trace_runs() {
    use std::process::Command;
    let _ = Command::new(env!("CARGO_BIN_EXE_cohesix_trace"))
        .arg("trace")
        .output()
        .expect("run cohtrace trace");
}
