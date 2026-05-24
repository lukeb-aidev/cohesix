// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Link the Pi 4 HDMI isolated driver runtime image.
// Author: Lukas Bower

#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#![deny(unsafe_code)]
#![warn(missing_docs)]
#![doc = "Pi 4 HDMI driver runtime image."]

#[cfg(target_os = "none")]
#[path = "../kernel.rs"]
mod kernel;

#[cfg(not(target_os = "none"))]
fn main() {
    println!("pi4-driver-hdmi");
}
