// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v0.2
// Author: Lukas Bower
// Date Modified: 2026-11-20

//! seL4 integration module root.

pub mod syscall;

extern "C" {
    /// Architecture-specific routine to drop from kernel mode to user mode.
    ///
    /// # Safety
    /// Invokes `eret` or `iretq` and does not return.
    pub fn switch_to_user(entry: usize, stack_top: usize) -> !;
}
