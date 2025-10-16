// Author: Lukas Bower
#![cfg_attr(feature = "kernel", no_std)]
#![deny(unsafe_code)]
#![warn(missing_docs)]

//! Root-task library surface exposing console and networking modules for tests.

pub mod console;
pub mod event;
pub mod serial;

#[cfg(feature = "net")]
pub mod net;

#[cfg(feature = "kernel")]
/// Kernel entrypoints and seL4 bootstrap wiring for the in-VM root task.
pub mod kernel;

#[cfg(feature = "kernel")]
pub mod sel4;

#[cfg(feature = "kernel")]
/// NineDoor 9P bridge exposed inside the VM environment.
pub mod ninedoor;

pub mod platform;

#[cfg(not(feature = "kernel"))]
/// Host-mode simulation used for developer testing.
pub mod host;
