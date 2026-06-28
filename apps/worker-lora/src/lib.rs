// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide LoRA host helpers and a receipt-only no_std VM loop.
// Author: Lukas Bower
#![cfg_attr(target_os = "none", no_std)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! LoRA worker library for host tooling and no_std VM receipt loops.

extern crate alloc;

mod common;

pub use common::*;

/// No_std LoRA VM receipt-loop helpers.
pub mod vm;

#[cfg(not(target_os = "none"))]
mod host;

#[cfg(not(target_os = "none"))]
pub use host::*;

#[cfg(target_os = "none")]
mod kernel {
    //! Kernel entrypoint is compiled from the worker binary target.
}
