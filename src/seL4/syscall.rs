// CLASSIFICATION: COMMUNITY
// Filename: syscall.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-06-25

//! seL4 syscall glue translating Plan 9 style calls into Cohesix runtime actions.
//! Provides minimal capability enforcement based on `ROLE_MANIFEST.md`.

use std::fs::File;
use std::io::{Read, Write};
use std::process::Command;

use crate::runtime::env::init::detect_cohrole;

fn role_allows_exec(role: &str) -> bool {
    matches!(role, "QueenPrimary" | "SimulatorTest")
}

/// Open a file and return the handle.
pub fn open(path: &str, flags: u32) -> Result<File, std::io::Error> {
    let role = detect_cohrole();
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
    if !role_allows_exec(&role) {
        println!("[sel4:{role}] exec denied: {cmd}");
        return Ok(());
    }
    println!("[sel4:{role}] exec {cmd} {:?}", args);
    Command::new(cmd).args(args).spawn()?.wait()?;
    Ok(())
}
