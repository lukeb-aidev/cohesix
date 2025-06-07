// CLASSIFICATION: COMMUNITY
// Filename: syscall.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-06-17

//! seL4 syscall glue translating Plan 9 style calls into Cohesix runtime actions.
//! This stub routes basic file operations to the standard library while logging
//! the current Cohesix role. It is a placeholder for the real microkernel
//! integration.

use std::fs::File;
use std::io::{Read, Write};
use std::process::Command;

use crate::runtime::env::init::detect_cohrole;

/// Open a file and return the handle.
pub fn open(path: &str, flags: u32) -> Result<File, std::io::Error> {
    let role = detect_cohrole();
    println!("[sel4:{role}] open {} flags={}", path, flags);
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

/// Stub implementation of `fork` that simply logs and returns a fake pid.
pub fn fork() -> Result<u32, String> {
    let role = detect_cohrole();
    println!("[sel4:{role}] fork() -> pid 0 (stub)");
    Ok(0)
}

/// Execute a command with arguments using the host OS.
pub fn exec(cmd: &str, args: &[&str]) -> Result<(), std::io::Error> {
    println!("[sel4] exec {} {:?}", cmd, args);
    Command::new(cmd).args(args).spawn()?.wait()?;
    Ok(())
}

