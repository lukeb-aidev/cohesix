// CLASSIFICATION: COMMUNITY
// Filename: main.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-09-05

#[cfg(not(target_os = "uefi"))]
fn main() {
    use cohesix::runtime::env::init::initialize_runtime_env;
    use cohesix::runtime::role_config::load_active;
    println!("[init] starting user init");
    initialize_runtime_env();
    let role = std::env::var("cohrole").unwrap_or_else(|_| "unknown".into());
    let cfg = load_active();
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

#[cfg(target_os = "uefi")]
fn main() {
    // Minimal UEFI init stub
}
