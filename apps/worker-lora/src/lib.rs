// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the worker-lora library and public module surface.
// Author: Lukas Bower
#![cfg_attr(target_os = "none", no_std)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! LoRa worker template for in-VM builds.

extern crate alloc;

mod common;

pub use common::*;

#[cfg(not(target_os = "none"))]
mod host;

#[cfg(not(target_os = "none"))]
pub use host::*;

#[cfg(target_os = "none")]
mod kernel {
    //! Stub LoRa worker module for seL4 builds.
}
