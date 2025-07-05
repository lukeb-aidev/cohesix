// CLASSIFICATION: COMMUNITY
// Filename: main.rs v0.3
// Author: Lukas Bower
// Date Modified: 2027-02-02

#![no_std]
extern crate alloc;

#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
#[cfg(feature = "kernel_bin")]
use cohesix::kernel::config;

fn main() {
    coherr!("Cohesix kernel stub");

    if let Some(cfg) = config::load_config("/etc/init.conf") {
        coherr!("[kernel] init config:\n{}", cfg);
    }
}
