// CLASSIFICATION: COMMUNITY
// Filename: test_capabilities.rs v0.1
// Date Modified: 2025-07-13
// Author: Cohesix Codex

use cohesix::seL4::syscall::{exec, open};
use std::fs;

#[test]
fn open_denied_logs_violation() {
    fs::create_dir_all("/etc").unwrap();
    fs::create_dir_all("/log").unwrap();
    fs::write(
        "/etc/cohcap.json",
        r#"{"DroneWorker":{"verbs":["open"],"paths":["/tmp"]}}"#,
    )
    .unwrap();
    fs::write("/srv/cohrole", "DroneWorker").unwrap();
    let res = open("/secret/data", 0);
    assert!(res.is_err());
    let log = fs::read_to_string("/log/sandbox.log").unwrap();
    assert!(log.contains("blocked action=open"));
}

#[test]
fn exec_denied_for_worker() {
    fs::write("/srv/cohrole", "DroneWorker").unwrap();
    let res = exec("/bin/echo", &["hi"]);
    assert!(res.is_err());
}
