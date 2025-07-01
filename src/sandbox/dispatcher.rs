// CLASSIFICATION: COMMUNITY
// Filename: dispatcher.rs v1.1
// Author: Lukas Bower
// Date Modified: 2025-07-24

use crate::prelude::*;
/// Syscall dispatcher for sandboxed workers.

use log::debug;

use crate::cohesix_types::{RoleManifest, Syscall};
use super::validator;
use super::queue::SyscallQueue;

// === SyscallDispatcher ===
/// Dispatch validated syscalls to the appropriate handler.
pub struct SyscallDispatcher;

impl SyscallDispatcher {
    /// Process a single syscall.
    pub fn dispatch(syscall: Syscall) {
        let role = RoleManifest::current_role();
        if !validator::validate("runtime", role.clone(), &syscall) {
            return;
        }
        match syscall {
            Syscall::Spawn { program, args } => {
                debug!("dispatch spawn: {} {:?}", program, args);
                // Process launcher integration pending
            }
            Syscall::CapGrant { target, capability } => {
                debug!("dispatch cap_grant: {} -> {}", target, capability);
                // Capability management not yet implemented
            }
            Syscall::Mount { src, dest } => {
                debug!("dispatch mount: {} -> {}", src, dest);
                // Mount service call not yet implemented
            }
            Syscall::Exec { path } => {
                debug!("dispatch exec: {}", path);
                // Execution in sandbox pending implementation
                // FIXME(batch5): integrate with process launcher
            }
            Syscall::ApplyNamespace => {
                debug!("dispatch apply namespace");
                // Namespace application to be handled by runtime
            }
            Syscall::Unknown => {
                debug!("unsupported syscall: Unknown");
            }
        }
    }

    /// Drain a queue and dispatch all syscalls in order.
    pub fn dispatch_queue(queue: &mut SyscallQueue) {
        while let Some(sc) = queue.dequeue() {
            Self::dispatch(sc);
        }
    }
}

