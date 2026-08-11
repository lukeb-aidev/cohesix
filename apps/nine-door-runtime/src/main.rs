// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Link the isolated NineDoor namespace parser target image.
// Author: Lukas Bower
#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#![deny(unsafe_op_in_unsafe_fn)]
#![warn(missing_docs)]
#![doc = "Isolated NineDoor namespace parser target image."]

#[cfg(target_os = "none")]
mod kernel;

#[cfg(not(target_os = "none"))]
fn main() {
    // The host exercises the runtime through library tests. There is no host
    // service executable or independent authority path.
}
