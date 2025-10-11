// Author: Lukas Bower
#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#![deny(unsafe_code)]
#![warn(missing_docs)]
#![doc = "Root task entry points for host and seL4 builds."]

#[cfg(target_os = "none")]
mod kernel;

#[cfg(not(target_os = "none"))]
mod host;

#[cfg(not(target_os = "none"))]
fn main() -> host::Result<()> {
    host::main()
}
