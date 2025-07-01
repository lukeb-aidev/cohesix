// CLASSIFICATION: COMMUNITY
// Filename: chain.rs v0.3
// Author: Lukas Bower
// Date Modified: 2026-10-28

use crate::prelude::*;
/// Execute syscall chains within the sandbox environment.
//
/// Chains are executed in FIFO order using the [`SyscallQueue`] and
/// [`SyscallDispatcher`]. Role permissions are verified via
/// `/srv/cohrole` before any syscalls are dispatched.

use log::debug;

use super::{dispatcher::SyscallDispatcher, queue::SyscallQueue};
use crate::cohesix_types::{Role, RoleManifest, Syscall};

/// Trait for types that can execute a syscall chain.
pub trait SandboxChainExecutor {
    /// Execute all syscalls in the provided chain.
    fn execute_chain(&self, chain: Vec<Syscall>);
}

/// Basic chain executor used by shell commands.
pub struct DefaultChainExecutor;

impl SandboxChainExecutor for DefaultChainExecutor {
    fn execute_chain(&self, chain: Vec<Syscall>) {
        let role = RoleManifest::current_role();
        if !matches!(
            role,
            Role::DroneWorker
                | Role::InteractiveAiBooth
                | Role::SimulatorTest
                | Role::QueenPrimary
                | Role::RegionalQueen
                | Role::BareMetalQueen
        ) {
            debug!("chain execution blocked for role: {:?}", role);
            return;
        }
        let mut queue = SyscallQueue::new();
        for sc in chain {
            queue.enqueue(sc);
        }
        SyscallDispatcher::dispatch_queue(&mut queue);
    }
}
