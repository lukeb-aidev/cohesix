// CLASSIFICATION: COMMUNITY
// Filename: main.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-08-02

#![no_std]
extern crate alloc;

#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
#[cfg(feature = "kernel_bin")]
use cohesix::kernel::config;

fn main() {
    println!("Cohesix kernel stub");

    if let Some(cfg) = config::load_config("/etc/init.conf") {
        println!("[kernel] init config:\n{}", cfg);
    }
}
