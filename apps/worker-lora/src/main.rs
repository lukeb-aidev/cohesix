// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Define the WorkerLora executable entrypoint without in-VM model execution.
// Author: Lukas Bower

#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#![deny(unsafe_code)]
#![warn(missing_docs)]
#![doc = "LoRA lifecycle receipt Worker entrypoints for host and seL4 builds."]

#[cfg(target_os = "none")]
mod kernel;

#[cfg(not(target_os = "none"))]
fn main() {
    println!("worker-lora receipt projection");
}
