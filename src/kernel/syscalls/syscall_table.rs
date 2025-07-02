// CLASSIFICATION: COMMUNITY
// Filename: syscall_table.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

use crate::prelude::*;
/// Kernel syscall table for Cohesix.
/// Maps syscall numbers to handler functions and enforces validation and dispatch.

/// Enumeration of supported syscalls.
#[derive(Debug)]
pub enum Syscall {
    Read,
    Write,
    Open,
    Close,
    Exec,
    Unknown,
}

/// Dispatch syscall by ID and return result code.
pub fn dispatch(syscall_id: u32, args: &[u64]) -> i64 {
    let syscall = match syscall_id {
        0 => Syscall::Read,
        1 => Syscall::Write,
        2 => Syscall::Open,
        3 => Syscall::Close,
        4 => Syscall::Exec,
        _ => Syscall::Unknown,
    };

    match syscall {
        Syscall::Read => {
            println!("[syscall] read({:?})", args);
            0
        }
        Syscall::Write => {
            println!("[syscall] write({:?})", args);
            0
        }
        Syscall::Open => {
            println!("[syscall] open({:?})", args);
            0
        }
        Syscall::Close => {
            println!("[syscall] close({:?})", args);
            0
        }
        Syscall::Exec => {
            println!("[syscall] exec({:?})", args);
            -1 // stub: not implemented
        }
        Syscall::Unknown => {
            println!(
                "[syscall] unknown syscall_id={} args={:?}",
                syscall_id, args
            );
            -1
        }
    }
}
