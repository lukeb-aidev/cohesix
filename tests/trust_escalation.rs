// CLASSIFICATION: COMMUNITY
// Filename: trust_escalation.rs v0.1
// Date Modified: 2025-07-13
// Author: Cohesix Codex

use cohesix::seL4::syscall::open;
use std::fs;

#[test]
fn ld_preload_blocked() {
    fs::create_dir_all("/etc").unwrap();
    fs::write(
        "/etc/cohcap.json",
        r#"{"DroneWorker":{"verbs":["open"],"paths":["/tmp"]}}"#,
    )
    .unwrap();
    let srv_dir = std::env::temp_dir();
    fs::write(srv_dir.join("cohrole"), "DroneWorker").unwrap();
    std::env::set_var("LD_PRELOAD", "evil.so");
    let res = open("/tmp/ok", 0);
    assert!(res.is_err());
    let log_dir = std::env::var("COHESIX_LOG_DIR").map(std::path::PathBuf::from).unwrap_or_else(|_| std::env::temp_dir());
    fs::create_dir_all(&log_dir).unwrap();
    let log = fs::read_to_string(log_dir.join("sandbox.log")).unwrap();
    assert!(log.contains("open_preload"));
}
