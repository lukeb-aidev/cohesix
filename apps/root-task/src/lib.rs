// Author: Lukas Bower
#![cfg_attr(feature = "kernel", no_std)]
#![deny(unsafe_code)]
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
/// Kernel entrypoints and seL4 bootstrap wiring for the in-VM root task.
pub mod kernel;

#[cfg(feature = "kernel")]
pub mod cspace;

#[cfg(feature = "kernel")]
/// Early bootstrap helpers for capability allocation.
pub mod bootstrap;

#[cfg(feature = "kernel")]
pub mod sel4;

#[cfg(feature = "kernel")]
/// NineDoor 9P bridge exposed inside the VM environment.
pub mod ninedoor;

pub mod platform;

#[cfg(all(not(feature = "kernel"), not(target_os = "none")))]
/// Host-mode simulation used for developer testing.
pub mod host;
