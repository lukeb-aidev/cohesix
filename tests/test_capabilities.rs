// CLASSIFICATION: COMMUNITY
// Filename: test_capabilities.rs v0.1
// Date Modified: 2025-07-13
// Author: Cohesix Codex

use cohesix::seL4::syscall::{exec, open};
use std::fs;

#[test]
fn open_denied_logs_violation() {
    fs::create_dir_all("/etc").unwrap();
    let log_dir = std::env::var("COHESIX_LOG_DIR").map(std::path::PathBuf::from).unwrap_or_else(|_| std::env::temp_dir());
    fs::create_dir_all(&log_dir).unwrap();
    fs::write(
        "/etc/cohcap.json",
        r#"{"DroneWorker":{"verbs":["open"],"paths":["/tmp"]}}"#,
    )
    .unwrap();
    let srv_dir = std::env::temp_dir();
    fs::write(srv_dir.join("cohrole"), "DroneWorker").unwrap();
    let res = open("/secret/data", 0);
    assert!(res.is_err());
    let log = fs::read_to_string(log_dir.join("sandbox.log")).unwrap();
    assert!(log.contains("blocked action=open"));
}

#[test]
fn exec_denied_for_worker() {
    let srv_dir = std::env::temp_dir();
    fs::write(srv_dir.join("cohrole"), "DroneWorker").unwrap();
    let res = exec("/bin/echo", &["hi"]);
    assert!(res.is_err());
}
