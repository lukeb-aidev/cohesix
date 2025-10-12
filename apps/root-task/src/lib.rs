// Author: Lukas Bower
#![cfg_attr(target_os = "none", no_std)]
#![deny(unsafe_code)]
#![warn(missing_docs)]

//! Root-task library surface exposing console and networking modules for tests.

pub mod console;
pub mod event;
pub mod serial;

#[cfg(feature = "net")]
pub mod net;

#[cfg(target_os = "none")]
pub mod kernel;

#[cfg(target_os = "none")]
pub mod sel4;

#[cfg(target_os = "none")]
pub mod ninedoor;

#[cfg(not(target_os = "none"))]
/// Host-mode simulation used for developer testing.
pub mod host;
