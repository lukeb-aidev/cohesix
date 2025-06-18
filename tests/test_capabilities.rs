// CLASSIFICATION: COMMUNITY
// Filename: test_capabilities.rs v0.2
// Date Modified: 2025-09-16
// Author: Cohesix Codex

use cohesix::seL4::syscall::{exec, open};
use std::fs::{self, File, OpenOptions};
use std::path::Path;

#[test]
fn open_denied_logs_violation() -> std::io::Result<()> {
    let log_path = Path::new("/log/sandbox.log");
    if OpenOptions::new().create(true).append(true).open(log_path).is_err() {
        eprintln!("Skipping test: cannot access log path {:?}", log_path);
        return Ok(());
    }

    let cohcap_path = Path::new("/etc/cohcap.json");
    if File::create(cohcap_path).is_err() {
        eprintln!("Skipping test: cannot write to {:?}", cohcap_path);
        return Ok(());
    }

    fs::create_dir_all("/etc")
        .unwrap_or_else(|e| panic!("open_denied_logs_violation failed: {}", e));
    unsafe { std::env::set_var("COHESIX_LOG_DIR", "/log"); }
    let log_dir = std::path::PathBuf::from("/log");
    fs::create_dir_all(&log_dir)
        .unwrap_or_else(|e| panic!("open_denied_logs_violation failed: {}", e));
    fs::write(
        cohcap_path,
        r#"{"DroneWorker":{"verbs":["open"],"paths":["/tmp"]}}"#,
    )
    .unwrap_or_else(|e| panic!("open_denied_logs_violation failed: {}", e));
    let srv_dir = std::env::temp_dir();
    fs::write(srv_dir.join("cohrole"), "DroneWorker")
        .unwrap_or_else(|e| panic!("open_denied_logs_violation failed: {}", e));
    let res = open("/secret/data", 0);
    assert!(res.is_err());
    let log = fs::read_to_string(log_dir.join("sandbox.log"))
        .unwrap_or_else(|e| panic!("open_denied_logs_violation failed: {}", e));
    assert!(log.contains("blocked action=open"));
    unsafe { std::env::remove_var("COHESIX_LOG_DIR"); }
    Ok(())
}

#[test]
fn exec_denied_for_worker() {
    let srv_dir = std::env::temp_dir();
    fs::write(srv_dir.join("cohrole"), "DroneWorker")
        .unwrap_or_else(|e| panic!("exec_denied_for_worker failed: {}", e));
    let res = exec("/bin/echo", &["hi"]);
    assert!(res.is_err());
}
