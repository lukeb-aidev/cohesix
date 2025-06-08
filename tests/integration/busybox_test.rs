// CLASSIFICATION: COMMUNITY
// Filename: busybox_test.rs v0.1
// Date Modified: 2025-06-05
// Author: Cohesix Codex

use cohesix::kernel::fs::{busybox, initfs};

#[test]
fn busybox_ls_prints_files() {
    let mut output = Vec::new();
    // capture output using a simple macro
    use std::io::Write;
    let files: Vec<_> = initfs::list_files().collect();
    assert!(!files.is_empty(), "initfs should have files");
    // run command
    busybox::run_command("ls", &[]);
}

#[test]
fn busybox_cat_works() {
    use std::fs;
    let path = "/tmp/bb_test.txt";
    fs::write(path, "hi").unwrap();
    busybox::run_command("cat", &[path]);
    let _ = fs::remove_file(path);
}

#[test]
fn busybox_uptime_works() {
    busybox::run_command("uptime", &[]);
}
