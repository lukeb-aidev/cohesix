// Author: Lukas Bower
#![cfg_attr(feature = "kernel", no_std)]
#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(deprecated)]
#![warn(missing_docs)]

//! Root-task library surface exposing console and networking modules for tests.

#[cfg(all(target_os = "none", not(feature = "kernel")))]
compile_error!("enable the `kernel` feature when building root-task for seL4 targets");

pub mod console;
pub mod event;
pub mod serial;

#[cfg(feature = "net-console")]
mod net_consts;

#[cfg(all(feature = "kernel", feature = "net-console"))]
pub(crate) mod drivers;

#[cfg(feature = "net-console")]
pub mod net;

#[cfg(feature = "kernel")]
pub mod hal;

#[cfg(feature = "kernel")]
pub(crate) mod alloc;

#[cfg(feature = "kernel")]
/// Kernel entrypoints and seL4 bootstrap wiring for the in-VM root task.
pub mod kernel;

#[cfg(feature = "kernel")]
/// seL4 bootstrap tracing primitives for debug logging.
pub mod trace;

#[cfg(feature = "kernel")]
/// Runtime helpers for interrogating kernel capabilities during bootstrap.
pub mod debug;

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
pub mod sel4;

#[cfg(feature = "kernel")]
/// NineDoor 9P bridge exposed inside the VM environment.
pub mod ninedoor;

#[cfg(feature = "kernel")]
/// Guard helpers for validating indirect call targets at runtime.
pub mod guards;

pub mod platform;

#[cfg(all(not(feature = "kernel"), not(target_os = "none")))]
/// Host-mode simulation used for developer testing.
pub mod host;
