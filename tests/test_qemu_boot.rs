// CLASSIFICATION: COMMUNITY
// Filename: test_qemu_boot.rs v0.6
// Author: Lukas Bower
// Date Modified: 2025-12-20

use std::process::Command;
use std::time::{Duration, Instant};
use std::{fs, thread};
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
        let boot = Path::new("out/BOOTX64.EFI");
        let iso = Path::new("out/cohesix.iso");
        assert!(boot.is_file(), "out/BOOTX64.EFI missing");
        assert!(iso.is_file(), "out/cohesix.iso missing");

        if Path::new("qemu_serial.log").exists() {
            let _ = fs::remove_file("qemu_serial.log");
        }

        let dry = Command::new("make")
            .arg("-n")
            .arg("qemu")
            .output()
            .expect("failed to preview qemu command");
        println!("QEMU command:\n{}", String::from_utf8_lossy(&dry.stdout));

        let run_qemu = |attempt| {
            println!("launching qemu attempt {}", attempt);
            Command::new("make")
                .arg("qemu")
                .env("TMPDIR", std::env::temp_dir())
                .status()
                .expect("failed to run make qemu")
        };

        let mut status = run_qemu(1);
        if !status.success() {
            eprintln!("make qemu exit {:?}, retrying", status);
            status = run_qemu(2);
        }
        assert!(status.success(), "make qemu failed with {:?}", status);

        if !Path::new("qemu_serial.log").exists() {
            panic!("qemu_serial.log missing after qemu run");
        }

        let log = fs::read_to_string("qemu_serial.log").expect("read log");
        let tail: Vec<&str> = log.lines().rev().take(20).collect();
        println!("QEMU log (tail):\n{}", tail.into_iter().rev().collect::<Vec<_>>().join("\n"));

        for line in log.lines() {
            if let Some(reason) = line.strip_prefix("BOOT_FAIL:") {
                panic!("BOOT_FAIL: {}", reason);
            }
        }

        if !log.contains("BOOT_OK") {
            panic!("missing BOOT_OK\nfull log:\n{}", log);
        }
    } else {
        eprintln!("qemu-system-x86_64 not installed; skipping test");
    }
}
