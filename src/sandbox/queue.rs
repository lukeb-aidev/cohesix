// CLASSIFICATION: COMMUNITY
// Filename: queue.rs v1.1
// Author: Lukas Bower
// Date Modified: 2026-09-30

extern crate alloc;
use alloc::collections::VecDeque;
/// Syscall queue for sandbox mediation.
use log::{debug, info};

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
        Self {
            buffer: VecDeque::new(),
        }
    }

    /// Enqueue a syscall for later dispatch.
    pub fn enqueue(&mut self, sc: Syscall) {
        self.buffer.push_back(sc);
    }

    /// Dequeue the next syscall if the current role is `DroneWorker`.
    pub fn dequeue(&mut self) -> Option<Syscall> {
        match RoleManifest::current_role() {
            Role::DroneWorker => {
                let sc = self.buffer.pop_front();
                info!("Role {:?} attempted dequeue: {:?}", Role::DroneWorker, sc);
                sc
            }
            role => {
                info!("Role {:?} attempted dequeue: PermissionDenied", role);
                debug!("syscall dequeue blocked for role: {:?}", role);
                None
            }
        }
    }
}
