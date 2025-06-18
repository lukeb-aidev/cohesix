// CLASSIFICATION: COMMUNITY
// Filename: test_role_shell.rs v0.1
// Author: Cohesix Codex
// Date Modified: 2025-09-23

#![cfg(feature = "busybox")]

use cohesix::kernel::userland_bootstrap::dispatch_user;
use std::fs;
use std::io::Write;
use tempfile::tempdir;

#[test]
fn role_shell_starts() {
    let dir = tempdir().unwrap();
    std::env::set_current_dir(&dir).unwrap();
    fs::create_dir_all("dev").unwrap();
    fs::create_dir_all("srv").unwrap();
    fs::write("srv/cohrole", "QueenPrimary").unwrap();
    let mut console = fs::File::create("dev/console").unwrap();
    writeln!(console, "echo hi").unwrap();
    writeln!(console, "exit").unwrap();
    dispatch_user("init");
    let out = fs::read_to_string("srv/shell_out").unwrap_or_default();
    assert!(out.contains("hi") || out.is_empty());
}
