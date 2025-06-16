// CLASSIFICATION: COMMUNITY
// Filename: queue.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-06-17

//! Syscall queue for sandbox mediation.

use std::collections::VecDeque;
use log::debug;

use crate::cohesix_types::{Role, RoleManifest, Syscall};

// === SyscallQueue Struct ===
/// Simple FIFO queue of syscalls pending validation and dispatch.
pub struct SyscallQueue {
    buffer: VecDeque<Syscall>,
}

impl Default for SyscallQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl SyscallQueue {
    /// Create an empty queue.
    pub fn new() -> Self {
        Self { buffer: VecDeque::new() }
    }

    /// Enqueue a syscall for later dispatch.
    pub fn enqueue(&mut self, sc: Syscall) {
        self.buffer.push_back(sc);
    }

    /// Dequeue the next syscall if the current role is `DroneWorker`.
    pub fn dequeue(&mut self) -> Option<Syscall> {
        match RoleManifest::current_role() {
            Role::DroneWorker => self.buffer.pop_front(),
            role => {
                debug!("syscall dequeue blocked for role: {:?}", role);
                None
            }
        }
    }
}

