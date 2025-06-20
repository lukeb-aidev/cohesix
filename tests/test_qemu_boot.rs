// CLASSIFICATION: COMMUNITY
// Filename: test_qemu_boot.rs v0.5
// Author: Lukas Bower
// Date Modified: 2025-12-19

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
        assert!(Path::new("out/BOOTX64.EFI").exists(),
            "ISO not generated; out/BOOTX64.EFI missing");
        assert!(Path::new("out/cohesix.iso").exists(),
            "ISO not generated; make_iso.sh likely failed or preconditions not met");
        if Path::new("qemu_serial.log").exists() {
            let _ = fs::remove_file("qemu_serial.log");
        }

        let dry = Command::new("make")
            .arg("-n")
            .arg("qemu")
            .output()
            .expect("failed to preview qemu command");
        println!("{}", String::from_utf8_lossy(&dry.stdout));

        let status = Command::new("make")
            .arg("qemu")
            .env("TMPDIR", std::env::temp_dir())
            .status()
            .expect("failed to run make qemu");

        assert!(status.success(), "make qemu failed with {:?}", status);
        if !Path::new("out/cohesix.iso").exists() {
            eprintln!("out/cohesix.iso missing after running make qemu");
            eprintln!("make qemu preview:\n{}", String::from_utf8_lossy(&dry.stdout));
            if let Ok(log) = fs::read_to_string("qemu_serial.log") {
                eprintln!("qemu_serial.log:\n{}", log);
            }
            panic!("out/cohesix.iso missing");
        }

        let start = Instant::now();
        while !Path::new("qemu_serial.log").exists() {
            if start.elapsed() > Duration::from_secs(15) {
                eprintln!("qemu_serial.log not found after 15s");
                eprintln!("make qemu preview:\n{}", String::from_utf8_lossy(&dry.stdout));
                panic!("qemu_serial.log missing");
            }
            thread::sleep(Duration::from_millis(500));
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
            eprintln!("QEMU log:\n{}", log);
            panic!("BOOT_OK missing in log");
        }
    } else {
        eprintln!("qemu-system-x86_64 not installed; skipping test");
    }
}
