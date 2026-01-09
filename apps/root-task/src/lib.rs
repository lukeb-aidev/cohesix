// Author: Lukas Bower
// Purpose: Root-task library surface exposing console and networking modules for tests.
#![cfg_attr(feature = "kernel", no_std)]
#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(deprecated)]
#![warn(missing_docs)]
#![allow(
    dead_code,
    missing_docs,
    non_snake_case,
    non_upper_case_globals,
    static_mut_refs,
    unused_must_use
)]

//! Root-task library surface exposing console and networking modules for tests.

#[cfg(all(target_os = "none", not(feature = "kernel")))]
compile_error!("enable the `kernel` feature when building root-task for seL4 targets");

#[cfg(all(feature = "kernel", target_arch = "aarch64"))]
pub mod arch;
#[cfg(feature = "kernel")]
/// Audit helpers for boot sequencing and readiness checkpoints.
pub mod audit;
pub mod console;
pub mod event;
/// Compile-time profile surface exposed to kernels and tests.
pub mod profile;
/// Compile-time feature manifest and guard rails.
pub mod features;
pub mod serial;

#[cfg(feature = "net-console")]
mod net_consts;

#[cfg(all(feature = "kernel", feature = "net-console"))]
pub(crate) mod drivers;

#[cfg(feature = "net-console")]
pub mod net;

pub mod hal;

#[cfg(feature = "kernel")]
pub(crate) mod alloc;

#[cfg(feature = "kernel")]
/// Kernel entrypoints and seL4 bootstrap wiring for the in-VM root task.
pub mod kernel;

#[cfg(feature = "kernel")]
pub mod built_info {
    include!(concat!(env!("OUT_DIR"), "/built_info.rs"));
}

#[cfg(feature = "kernel")]
mod panic;

#[cfg(any(feature = "kernel", feature = "bootstrap-trace", feature = "net-console", test))]
/// seL4 bootstrap tracing primitives for debug logging.
pub mod trace;

#[cfg(feature = "kernel")]
/// Userland entrypoints exposed after the bootstrap hand-off.
pub mod userland;

#[cfg(feature = "kernel")]
/// Runtime helpers for interrogating kernel capabilities during bootstrap.
pub mod debug;

#[cfg(feature = "kernel")]
/// UART-only debug helpers for scenarios where the logger path is unreliable.
pub mod debug_uart;

/// Shared bootinfo layout helpers used by snapshot verification logic.
pub mod bootinfo_layout;

#[cfg(feature = "kernel")]
/// Capability operations and debug helpers built atop seL4 syscalls.
pub mod caps;

#[cfg(feature = "kernel")]
/// Minimal capability-space view abstraction over seL4 boot info records.
pub mod cspace;

#[cfg(feature = "kernel")]
/// Helpers for addressing slots within the init CSpace.
pub mod cspace_view;

#[cfg(feature = "kernel")]
/// Early bootstrap helpers for capability allocation.
pub mod bootstrap;

#[cfg(feature = "kernel")]
/// Endpoint bootstrap primitives used during kernel bring-up.
pub mod boot;

#[cfg(feature = "kernel")]
/// Runtime readiness flags shared between transports.
pub mod readiness;

#[cfg(feature = "kernel")]
/// Bounded log buffer backing /log/queen.log after console handoff.
pub mod log_buffer;

/// IPC helpers that guard direct syscalls during early bring-up.
pub mod ipc;

#[cfg(feature = "kernel")]
pub mod sel4;

#[cfg(feature = "kernel")]
pub mod sel4_view;

#[cfg(feature = "kernel")]
/// Static manifest entry points for tickets and namespace mounts.
pub mod manifest;

#[cfg(feature = "kernel")]
/// NineDoor 9P bridge exposed inside the VM environment.
pub mod ninedoor;

#[cfg(feature = "kernel")]
/// Guard helpers for validating indirect call targets at runtime.
pub mod guards;

#[cfg(feature = "kernel")]
/// Minimal UART helpers used by the early bootstrap console.
pub mod uart;

#[cfg(feature = "kernel")]
/// Re-export canonical init CNode tuple helpers.
pub use crate::cspace::tuples;

pub mod platform;

#[cfg(all(not(feature = "kernel"), not(target_os = "none")))]
/// Host-mode simulation used for developer testing.
pub mod host;
