// CLASSIFICATION: COMMUNITY
// Filename: test_qemu_boot.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-23

use std::process::Command;
use std::{thread, time::Duration};
use std::time::Instant;
use std::fs;
use std::path::Path;

#[test]
fn qemu_boot_produces_boot_ok() {
    if Command::new("sh")
        .arg("-c")
        .arg("command -v qemu-system-x86_64")
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        let tmpdir = tempfile::tempdir().expect("tmpdir");
        let status = Command::new("make")
            .arg("qemu")
            .env("TMPDIR", tmpdir.path())
            .env("QEMU_ENV", "1")
            .status()
            .expect("failed to run make qemu");
        assert!(status.success(), "make qemu failed");

        let start = Instant::now();
        while !Path::new("qemu_serial.log").exists() {
            if start.elapsed() > Duration::from_secs(15) {
                panic!("qemu_serial.log not found after 15s");
            }
            thread::sleep(Duration::from_millis(500));
        }
        let log = fs::read_to_string("qemu_serial.log").expect("read log");
        for line in log.lines() {
            if let Some(rest) = line.strip_prefix("BOOT_FAIL:") {
                println!("BOOT_FAIL:{}", rest);
            }
        }
        assert!(log.contains("BOOT_OK"), "BOOT_OK missing in log");
        assert!(log.contains("/srv/cuda"), "cuda service not initialized");
        assert!(log.contains("QEMU_ENV=1"), "QEMU_ENV traces missing");
    } else {
        eprintln!("qemu-system-x86_64 not installed; skipping test");
    }
}
