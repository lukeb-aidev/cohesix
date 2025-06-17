// CLASSIFICATION: COMMUNITY
// Filename: init.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-09-02

//! Runtime environment initialization for Cohesix.
//! Sets up runtime globals, telemetry, role configuration, and system entropy.

/// Initialize the runtime environment at startup.
pub fn initialize_runtime_env() {
    println!("[env] Initializing runtime environment...");
    load_config();
    let role = detect_cohrole();
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
    std::env::var("COH_ROLE").unwrap_or_else(|_| "Unknown".into())
}

