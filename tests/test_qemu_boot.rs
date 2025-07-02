// CLASSIFICATION: COMMUNITY
// Filename: test_qemu_boot.rs v0.12
// Author: Lukas Bower
// Date Modified: 2026-09-20
// Ensure `make iso` is run before executing this test.

use std::fs;
use std::path::Path;
use std::process::Command;

fn dump_log_tail(path: &str, lines: usize) {
    if let Ok(data) = fs::read_to_string(path) {
        let tail: Vec<&str> = data.lines().rev().take(lines).collect();
        eprintln!(
            "QEMU log tail:\n{}",
            tail.into_iter().rev().collect::<Vec<_>>().join("\n")
        );
    }
}

#[test]
fn qemu_grub_boot_ok() {
    let qemu = Path::new("/usr/bin/qemu-system-x86_64");
    if !qemu.is_file() {
        eprintln!("qemu-system-x86_64 not installed; skipping test");
        return;
    }

    if !Path::new("out/cohesix.iso").exists() {
        eprintln!("ISO not present — skipping qemu_grub_boot_ok");
        return;
    }

    if !Path::new("logs").is_dir() {
        fs::create_dir_all("logs").unwrap();
    }

    let log_path = "logs/qemu_serial.log";
    if Path::new(log_path).exists() {
        let _ = fs::remove_file(log_path);
    }

    let output = Command::new("scripts/boot_qemu.sh")
        .output()
        .expect("launch qemu");

    fs::write(log_path, &output.stdout).expect("write log");

    if !output.status.success() {
        eprintln!("QEMU command: qemu-system-x86_64 -cdrom out/cohesix.iso -nographic -serial mon:stdio -m 256");
        dump_log_tail(log_path, 20);
        panic!("QEMU exited with error");
    }

    if !Path::new(log_path).is_file() {
        panic!("QEMU execution failed: log missing");
    }

    let log = fs::read_to_string(log_path).expect("read log");

    if !log.contains("COHESIX_BOOT_OK") {
        eprintln!("QEMU command: qemu-system-x86_64 -cdrom out/cohesix.iso -nographic -serial mon:stdio -m 256");
        dump_log_tail(log_path, 20);
        panic!("Boot marker COHESIX_BOOT_OK not found");
    }
}
