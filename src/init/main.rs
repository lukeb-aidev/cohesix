// CLASSIFICATION: COMMUNITY
// Filename: main.rs v0.3
// Author: Lukas Bower
// Date Modified: 2026-12-31
// UEFI-specific stub removed; full init runs on UEFI by default.

#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
fn main() {
    use cohesix::runtime::env::init::{initialize_runtime_env, parse_boot_args};
    use cohesix::runtime::role_config::load_active;
    use cohesix::runtime::ServiceRegistry;
    use std::process::Command;
    println!("[init] starting user init");
    initialize_runtime_env();
    let boot = parse_boot_args();
    println!("[init] secure9p: {}", boot.secure9p);
    println!("[init] busybox: {}", boot.busybox);
    let role = std::env::var("cohrole").unwrap_or_else(|_| "unknown".into());
    let cfg = load_active();
    match Command::new("coh-9p-helper").spawn() {
        Ok(_) => {
            let _ = ServiceRegistry::register_service("coh9p", "/srv/coh9p");
            println!("[init] coh-9p-helper started");
        }
        Err(e) => eprintln!("[init] coh-9p-helper start failed: {e}"),
    }
    match Command::new("cohtrace").arg("status").spawn() {
        Ok(_) => println!("[init] cohtrace service started"),
        Err(e) => eprintln!("[init] cohtrace start failed: {e}"),
    }
    if cfg.validator.unwrap_or(true) {
        match std::process::Command::new("python3")
            .arg("python/validator.py")
            .arg("--live")
            .spawn()
        {
            Ok(_) => println!("Validator initialized for role: {}", role),
            Err(e) => eprintln!("[init] validator failed to start: {e}"),
        }
    }
    if let Err(e) = cohesix::cli::run() {
        eprintln!("[init] cli error: {e}");
    }
    cohesix::sh_loop::run();
}
