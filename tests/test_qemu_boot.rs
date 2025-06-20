// CLASSIFICATION: COMMUNITY
// Filename: test_qemu_boot.rs v0.7
// Author: Lukas Bower
// Date Modified: 2025-12-21

use std::fs;
use std::path::Path;
use std::process::Command;

fn dump_log_tail(path: &str, lines: usize) {
    if let Ok(data) = fs::read_to_string(path) {
        let tail: Vec<&str> = data.lines().rev().take(lines).collect();
        eprintln!("QEMU log tail:\n{}", tail.into_iter().rev().collect::<Vec<_>>().join("\n"));
    }
}

#[test]
fn qemu_boot_produces_boot_ok() {
    let qemu = Path::new("/usr/bin/qemu-system-x86_64");
    if !qemu.is_file() {
        eprintln!("qemu-system-x86_64 not installed; skipping test");
        return;
    }

    let version = Command::new(qemu)
        .arg("--version")
        .output()
        .expect("check qemu version");
    println!("{}", String::from_utf8_lossy(&version.stdout));

    let iso = Path::new("out/cohesix.iso");
    if !iso.is_file() {
        panic!("QEMU ISO missing: ISO was not built or misplaced");
    }
    let boot = Path::new("out/BOOTX64.EFI");
    assert!(boot.is_file(), "out/BOOTX64.EFI missing");

    if Path::new("qemu_serial.log").exists() {
        let _ = fs::remove_file("qemu_serial.log");
    }

    let status = Command::new(qemu)
        .args(&[
            "-cdrom",
            "out/cohesix.iso",
            "-serial",
            "file:qemu_serial.log",
            "-display",
            "none",
            "-no-reboot",
        ])
        .status()
        .expect("launch qemu");

    if !status.success() {
        dump_log_tail("qemu_serial.log", 40);
        panic!("QEMU exited with error");
    }

    if !Path::new("qemu_serial.log").is_file() {
        panic!("QEMU execution failed before log output began");
    }

    let log = fs::read_to_string("qemu_serial.log").expect("read log");

    if !log.contains("BOOT_OK") {
        eprintln!("{}", log);
        panic!("QEMU booted but system did not reach OK marker. Check kernel.efi, boot script, or /init path");
    }
}
