// CLASSIFICATION: COMMUNITY
// Filename: syscall.rs v0.3
// Author: Lukas Bower
// Date Modified: 2026-09-30

use crate::prelude::*;
/// seL4 syscall glue translating Plan 9 style calls into Cohesix runtime actions.
/// Provides minimal capability enforcement based on `ROLE_MANIFEST.md`.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::process;
use std::process::Command;

use crate::security::capabilities;
use crate::sandbox::validator;
use crate::cohesix_types::{RoleManifest, Syscall};

use crate::runtime::env::init::detect_cohrole;

fn role_allows_exec(role: &str) -> bool {
    matches!(role, "QueenPrimary" | "SimulatorTest" | "DroneWorker")
}

fn log_block(action: &str, path: &str, role: &str) {
    fs::create_dir_all("/log").ok();
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open("/log/sandbox.log") {
        let _ = writeln!(
            f,
            "blocked action={action} path={path} pid={} role={role}",
            process::id()
        );
    }
}


/// Open a file and return the handle.
pub fn open(path: &str, flags: u32) -> Result<File, std::io::Error> {
    let role = detect_cohrole();
    if std::env::var("LD_PRELOAD").is_ok() {
        log_block("open_preload", path, &role);
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "preload blocked"));
    }
    if !capabilities::role_allows(&role, "open", path) {
        log_block("open", path, &role);
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "capability denied"));
    }
    println!("[sel4:{role}] open {path} flags={flags}");
    File::options().read(true).write(flags & 1 != 0).open(path)
}

/// Read from an open file into the provided buffer.
pub fn read(file: &mut File, buf: &mut [u8]) -> Result<usize, std::io::Error> {
    file.read(buf)
}

/// Write to an open file from the buffer.
pub fn write(file: &mut File, buf: &[u8]) -> Result<usize, std::io::Error> {
    file.write(buf)
}

/// Execute a command with arguments using the host OS when allowed.
pub fn exec(cmd: &str, args: &[&str]) -> Result<(), std::io::Error> {
    let role = detect_cohrole();
    let role_enum = RoleManifest::current_role();
    println!("[sel4] exec attempt role={:?}", role_enum);
    let sc = Syscall::Exec { path: cmd.to_string() };
    let allowed = validator::validate("sel4", role_enum.clone(), &sc);
    println!("[sel4] validator result for {:?}: {}", role_enum, allowed);
    if !allowed {
        log_block("exec_validator", cmd, &role);
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "validator denied",
        ));
    }
    if std::env::var("LD_PRELOAD").is_ok() {
        log_block("exec_preload", cmd, &role);
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "preload blocked",
        ));
    }
    if !role_allows_exec(&role) || !capabilities::role_allows(&role, "exec", cmd) {
        log_block("exec", cmd, &role);
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "capability denied",
        ));
    }
    println!("[sel4:{role}] exec {cmd} {:?}", args);
    Command::new(cmd).args(args).spawn()?.wait()?;
    Ok(())
}
