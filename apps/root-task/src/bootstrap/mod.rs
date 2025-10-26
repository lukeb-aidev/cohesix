// Author: Lukas Bower
#![allow(dead_code)]

/// Capability-space helpers extracted from the seL4 boot info structure.
pub mod cspace;
/// Syscall wrappers for capability operations using invocation addressing only.
pub mod cspace_sys;
/// ABI guard helpers ensuring the seL4 FFI signatures remain pinned.
pub mod ffi;
/// IPC buffer bring-up helpers with deterministic logging waypoints.
pub mod ipcbuf;
/// Early boot logging backends.
pub mod log;
/// Thin wrapper around `seL4_Untyped_Retype` tailored for the init CSpace policy.
pub mod retype;
/// Helpers for selecting RAM-backed untyped capabilities during bootstrap.
pub mod untyped_pick;

pub use untyped_pick::pick_untyped;

#[macro_export]
macro_rules! bp {
    ($name:expr) => {
        log::info!(concat!("[boot] ", $name));
    };
}

#[inline(always)]
pub fn ktry(step: &str, rc: i32) -> Result<(), i32> {
    if rc != sel4_sys::seL4_NoError as i32 {
        log::error!("[boot] {step}: seL4 err={rc}");
        return Err(rc);
    }
    Ok(())
}
