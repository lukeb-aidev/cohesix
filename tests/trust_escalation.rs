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
    fs::write("/srv/cohrole", "DroneWorker").unwrap();
    std::env::set_var("LD_PRELOAD", "evil.so");
    let res = open("/tmp/ok", 0);
    assert!(res.is_err());
    let log = fs::read_to_string("/log/sandbox.log").unwrap();
    assert!(log.contains("open_preload"));
}
