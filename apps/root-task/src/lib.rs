// Author: Lukas Bower
#![cfg_attr(target_os = "none", no_std)]
#![deny(unsafe_code)]
#![warn(missing_docs)]

//! Root-task library surface exposing console and networking modules for tests.

pub mod console;

#[cfg(feature = "net")]
pub mod net;

#[cfg(target_os = "none")]
pub mod kernel;

#[cfg(not(target_os = "none"))]
/// Host-mode simulation used for developer testing.
pub mod host;
