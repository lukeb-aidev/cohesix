// CLASSIFICATION: COMMUNITY
// Filename: syscall.rs v1.1
// Author: Lukas Bower
// Date Modified: 2025-07-20

//! Kernel syscall interface layer for Cohesix.
//! Provides syscall entry point, argument validation, and dispatch wiring.

use super::syscall_table::dispatch;
use crate::kernel::security::l4_verified::{enforce_capability, CapabilityResult};
use std::fs::OpenOptions;
use std::io::Write;

/// Entry point invoked by the trap handler or syscall instruction.
pub fn handle_syscall(syscall_id: u32, args: &[u64]) -> i64 {
    println!("[syscall] Handling syscall_id={} with args={:?}", syscall_id, args);
    crate::kernel::kernel_trace::log_syscall(&format!("{}", syscall_id));
    std::fs::create_dir_all("/log").ok();
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open("/log/syscall.log") {
        let _ = writeln!(f, "id={syscall_id} args={:?}", args);
    }
    if enforce_capability(syscall_id, "syscall") != CapabilityResult::Allowed {
        return -1;
    }
    if args.len() > 8 {
        return -1;
    }
    dispatch(syscall_id, args)
}
