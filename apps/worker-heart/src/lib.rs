// Author: Lukas Bower
#![cfg_attr(target_os = "none", no_std)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Heartbeat worker library.

#[cfg(not(target_os = "none"))]
mod host;

#[cfg(not(target_os = "none"))]
pub use host::*;

#[cfg(target_os = "none")]
mod kernel {
    //! Stub heartbeat worker module for seL4 builds.
}
