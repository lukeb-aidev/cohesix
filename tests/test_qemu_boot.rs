// CLASSIFICATION: COMMUNITY
// Filename: test_qemu_boot.rs v0.9
// Author: Lukas Bower
// Date Modified: 2025-12-23

use std::fs;
use std::path::Path;
use std::process::Command;
use std::os::unix::fs::PermissionsExt;

fn dump_log_tail(path: &str, lines: usize) {
    if let Ok(data) = fs::read_to_string(path) {
        let tail: Vec<&str> = data.lines().rev().take(lines).collect();
        eprintln!("QEMU log tail:\n{}", tail.into_iter().rev().collect::<Vec<_>>().join("\n"));
    }
}

#[test]
fn debug_qemu_boot_script_runs() {
    let meta = fs::metadata("scripts/debug_qemu_boot.sh")
        .expect("debug_qemu_boot.sh missing");
    if meta.permissions().mode() & 0o111 == 0 {
        panic!("debug_qemu_boot.sh is not executable or malformed");
    }

    let run = Command::new("bash")
        .arg("scripts/debug_qemu_boot.sh")
        .output()
        .expect("failed to invoke debug script");

    if run.status.code() == Some(126) || run.status.code() == Some(127) {
        panic!("debug_qemu_boot.sh is not executable or malformed");
    }
}

#[test]
fn qemu_boot_produces_boot_ok() {
    let qemu = Path::new("/usr/bin/qemu-system-x86_64");
    if !qemu.is_file() {
        eprintln!("qemu-system-x86_64 not installed; skipping test");
        return;
    }

    if Path::new("logs").is_dir() == false {
        fs::create_dir_all("logs").unwrap();
    }

    let debug = Command::new("bash")
        .arg("scripts/debug_qemu_boot.sh")
        .output()
        .expect("run debug script");
    let dbg_out = String::from_utf8_lossy(&debug.stdout);
    println!("{}", dbg_out);
    if !debug.status.success() || !dbg_out.contains("DEBUG_BOOT_READY") {
        panic!("Preboot check failed: cohesix.iso missing or QEMU misconfigured");
    }

    if Path::new("logs/qemu_serial.log").exists() {
        let _ = fs::remove_file("logs/qemu_serial.log");
    }

    let status = Command::new(qemu)
        .args(&[
            "-cdrom",
            "out/cohesix.iso",
            "-serial",
            "file:logs/qemu_serial.log",
            "-display",
            "none",
            "-no-reboot",
            "-d",
            "int",
            "-D",
            "logs/qemu_boot_trace.txt",
        ])
        .status()
        .expect("launch qemu");

    if !status.success() {
        dump_log_tail("logs/qemu_serial.log", 40);
        if let Ok(inv) = fs::read_to_string("logs/qemu_invocation.log") {
            eprintln!("qemu_invocation.log:\n{}", inv);
        }
        panic!("QEMU exited with error");
    }

    if !Path::new("logs/qemu_serial.log").is_file() {
        panic!("QEMU execution failed before log output began");
    }

    let log = fs::read_to_string("logs/qemu_serial.log").expect("read log");

    if !log.contains("BOOT_OK") {
        eprintln!("{}", log);
        if let Ok(trace) = fs::read_to_string("logs/qemu_boot_trace.txt") {
            eprintln!("qemu_boot_trace:\n{}", trace);
        }
        panic!("QEMU booted but system did not reach OK marker. Check kernel.efi, boot script, or /init path");
    }
}
