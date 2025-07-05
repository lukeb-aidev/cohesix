// CLASSIFICATION: COMMUNITY
// Filename: init.rs v1.2
// Author: Lukas Bower
// Date Modified: 2026-12-02

#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
/// Runtime environment initialization for Cohesix.
/// Sets up runtime globals, telemetry, role configuration, and system entropy.
use std::fs;

#[derive(Debug, Default, Clone)]
pub struct BootArgs {
    pub cohrole: Option<String>,
    pub secure9p: bool,
    pub busybox: bool,
}

/// Load the Cohesix role from `CohRole` environment variable or `/etc/role.conf`.
/// Returns `Some(role)` if detected.
pub fn load_role_setting() -> Option<String> {
    if let Ok(env_role) = std::env::var("CohRole") {
        println!("[BOOT] Loaded role: {env_role} from environment");
        return Some(env_role);
    }
    let path = std::env::var("ROLE_CONF_PATH").unwrap_or_else(|_| "/etc/role.conf".into());
    if let Ok(data) = fs::read_to_string(&path) {
        for line in data.lines() {
            if let Some(v) = line.strip_prefix("CohRole=") {
                let role = v.trim().to_string();
                println!("[BOOT] Loaded role: {} from {}", role, path);
                return Some(role);
            }
        }
    }
    None
}

pub fn parse_boot_args() -> BootArgs {
    let mut args = BootArgs::default();
    if let Some(role) = load_role_setting() {
        args.cohrole = Some(role);
    } else if let Ok(role) = std::env::var("COHROLE") {
        args.cohrole = Some(role);
    } else if let Ok(role) = fs::read_to_string("/srv/cohrole") {
        args.cohrole = Some(role.trim().to_string());
    }
    args.secure9p = std::env::var("secure9p").ok().as_deref() == Some("1");
    args.busybox = std::env::var("busybox").map(|v| v != "0").unwrap_or(true);
    args
}

/// Initialize the runtime environment at startup.
pub fn initialize_runtime_env() {
    println!("[env] Initializing runtime environment...");
    load_config();
    let boot = parse_boot_args();
    if let Some(role) = &boot.cohrole {
        std::env::set_var("cohrole", role);
        fs::write("/srv/cohrole", role).ok();
    }
    println!("[env] boot args: {:?}", boot);
    let role = boot.cohrole.clone().unwrap_or_else(detect_cohrole);
    println!("[env] running as role: {}", role);
    // In the future this will also seed entropy and launch telemetry threads.
}

/// Load configuration file or fallback defaults.
pub fn load_config() {
    println!("[env] Loading configuration...");
    let path = "/etc/cohesix/config.yaml";
    match std::fs::read_to_string(path) {
        Ok(cfg) => println!("[env] loaded {} bytes of config", cfg.len()),
        Err(_) => println!("[env] using default configuration"),
    }
}

/// Detect and expose the Cohesix role (e.g., QueenPrimary, DroneWorker).
pub fn detect_cohrole() -> String {
    println!("[env] Detecting Cohesix role...");
    match crate::cohesix_types::RoleManifest::current_role() {
        crate::cohesix_types::Role::Other(name) => name,
        r => format!("{:?}", r),
    }
}
