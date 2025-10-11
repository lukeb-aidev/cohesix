// Author: Lukas Bower
#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#![deny(unsafe_code)]
#![warn(missing_docs)]
#![doc = "NineDoor binary entry points for host and seL4 targets."]

#[cfg(target_os = "none")]
mod kernel;

#[cfg(not(target_os = "none"))]
use anyhow::Result;

#[cfg(not(target_os = "none"))]
use nine_door::NineDoor;

/// Host entry point wiring the NineDoor library into a placeholder binary.
#[cfg(not(target_os = "none"))]
fn main() -> Result<()> {
    let _server = NineDoor::new();
    println!("NineDoor host binary initialised (stub).");
    Ok(())
}
