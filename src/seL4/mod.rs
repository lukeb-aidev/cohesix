// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v0.3
// Author: Lukas Bower
// Date Modified: 2026-11-23

use crate::prelude::*;
/// seL4 integration module root.
pub mod syscall;

#[cfg(target_os = "none")]
extern "C" {
    /// Architecture-specific routine to drop from kernel mode to user mode.
    ///
    /// # Safety
    /// Invokes `eret` or `iretq` and does not return.
    pub fn switch_to_user(entry: usize, stack_top: usize) -> !;
}

/// Compiles only on bare-metal (target_os = "none"), safe stub otherwise.
#[cfg(not(target_os = "none"))]
#[no_mangle]
pub extern "C" fn switch_to_user(_entry: usize, _stack_top: usize) -> ! {
    panic!("switch_to_user attempted on non-bare-metal target");
}
