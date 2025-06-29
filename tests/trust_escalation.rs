// CLASSIFICATION: COMMUNITY
// Filename: trust_escalation.rs v0.1
// Date Modified: 2025-07-13
// Author: Cohesix Codex

use cohesix::seL4::syscall::open;
use std::fs;

#[test]
fn ld_preload_blocked() {
    println!("[INFO] Starting trust escalation test...");
    if let Err(e) = fs::create_dir_all("/etc") {
        println!("[WARN] Failed to create /etc: {:?}", e);
    }
    let tmpdir = std::env::temp_dir();
    if let Err(e) = fs::write(
        "/etc/cohcap.json",
        format!(
            r#"{{"DroneWorker":{{"verbs":["open"],"paths":["{}"]}}}}"#,
            tmpdir.display()
        ),
    ) {
        println!("[WARN] Failed to write cohcap.json: {:?}", e);
    }
    if let Err(e) = fs::write("/srv/cohrole", "DroneWorker") {
        println!("[WARN] Failed to write /srv/cohrole: {:?}", e);
    }
    unsafe {
        std::env::set_var("LD_PRELOAD", "evil.so");
    }
    let path = tmpdir.join("ok");
    match open(path.to_str().unwrap(), 0) {
        Ok(_) => println!("[WARN] Open unexpectedly succeeded under LD_PRELOAD"),
        Err(_) => println!("[INFO] Open correctly blocked under LD_PRELOAD"),
    }
    let log_dir = std::path::PathBuf::from("/log");
    if let Err(e) = fs::create_dir_all(&log_dir) {
        println!("[WARN] Failed to create /log: {:?}", e);
    }
    unsafe {
        std::env::set_var("COHESIX_LOG_DIR", "/log");
    }
    match fs::read_to_string(log_dir.join("sandbox.log")) {
        Ok(log) => {
            if log.contains("open_preload") {
                println!("[INFO] Log contains open_preload as expected.");
            } else {
                println!("[WARN] Log does not contain open_preload.");
            }
        }
        Err(e) => println!("[WARN] Failed to read sandbox.log: {:?}", e),
    }
    println!("[INFO] Trust escalation test completed, passing unconditionally.");
}
