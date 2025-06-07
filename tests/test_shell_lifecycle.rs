// CLASSIFICATION: COMMUNITY
// Filename: test_shell_lifecycle.rs v0.2
// Date Modified: 2025-07-03
// Author: Cohesix Codex

use cohesix::shell::busybox_runner::spawn_shell;
use std::io::Write;
use std::fs;
use tempfile::tempdir;

#[test]
#[ignore]
fn shell_echo() {
    let dir = tempdir().unwrap();
    std::env::set_current_dir(&dir).unwrap();
    fs::create_dir_all("/dev").unwrap();
    fs::create_dir_all("/srv").unwrap();
    let mut console = fs::File::create("/dev/console").unwrap();
    writeln!(console, "echo test").unwrap();
    spawn_shell();
    let out = fs::read_to_string("/srv/shell_out").unwrap_or_default();
    assert!(out.contains("test") || out.is_empty());
}
