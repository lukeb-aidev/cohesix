// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Provide the receipt-only no_std VM loop for AI LoRA lifecycle control.
// Author: Lukas Bower
#![cfg_attr(target_os = "none", no_std)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Receipt-only LoRA worker primitives for bounded model-adapter lifecycle control.

extern crate alloc;

/// No_std LoRA VM receipt-loop helpers.
pub mod vm;

#[cfg(target_os = "none")]
mod kernel {
    //! Kernel entrypoint is compiled from the worker binary target.
}
