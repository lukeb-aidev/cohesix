// CLASSIFICATION: COMMUNITY
// Filename: syscall.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

//! Kernel syscall interface layer for Cohesix.
//! Provides syscall entry point, argument validation, and dispatch wiring.

use super::syscall_table::dispatch;

/// Entry point invoked by the trap handler or syscall instruction.
pub fn handle_syscall(syscall_id: u32, args: &[u64]) -> i64 {
    println!("[syscall] Handling syscall_id={} with args={:?}", syscall_id, args);

    // TODO(cohesix): Insert tracing, access validation, and sandbox boundary enforcement here

    dispatch(syscall_id, args)
}
