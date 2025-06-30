// CLASSIFICATION: COMMUNITY
// Filename: test_session_log.rs v0.1
// Author: Cohesix Codex
// Date Modified: 2025-07-22

#![cfg(feature = "busybox")]

use cohesix::shell::busybox_runner::spawn_shell;
use std::fs::{self, File};
use std::io::Write;
use tempfile::tempdir;

#[test]
#[ignore]
fn session_log_written() {
    let dir = tempdir().unwrap();
    std::env::set_current_dir(&dir).unwrap();
    fs::create_dir_all("/srv/trace").unwrap();
    let mut console = File::create("/srv/console").unwrap();
    writeln!(console, "echo hi").unwrap();
    writeln!(console, "exit").unwrap();
    spawn_shell();
    let log = fs::read_to_string("/srv/trace/session.log")
        .or_else(|_| fs::read_to_string("/log/session.log"))
        .unwrap();
    assert!(log.contains("CMD echo hi"));
    assert!(log.contains("SESSION START"));
    assert!(log.contains("SESSION STOP"));
}
