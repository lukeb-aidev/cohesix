// CLASSIFICATION: COMMUNITY
// Filename: main.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-08-02

#![cfg(feature = "kernel_bin")]

#[cfg(not(target_os = "uefi"))]
use cohesix::kernel::config;

fn main() {
    println!("Cohesix kernel stub");
    #[cfg(not(target_os = "uefi"))]
    if let Some(cfg) = config::load_config("/etc/init.conf") {
        println!("[kernel] init config:\n{}", cfg);
    }
}
