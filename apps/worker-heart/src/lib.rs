// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide heartbeat worker host helpers and bounded no_std loop primitives.
// Author: Lukas Bower
#![cfg_attr(target_os = "none", no_std)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Heartbeat worker library.

/// Bounded no_std worker-loop primitives.
pub mod worker_loop;

#[cfg(not(target_os = "none"))]
mod host;

#[cfg(not(target_os = "none"))]
pub use host::*;

#[cfg(target_os = "none")]
mod kernel {
    //! Kernel entrypoint is compiled from the worker binary target.
}
