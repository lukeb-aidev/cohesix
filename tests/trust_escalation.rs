// CLASSIFICATION: COMMUNITY
// Filename: trust_escalation.rs v0.1
// Date Modified: 2025-07-13
// Author: Cohesix Codex

use cohesix::seL4::syscall::open;
use std::fs;

#[test]
fn ld_preload_blocked() {
    fs::create_dir_all("/etc").unwrap();
    let tmpdir = std::env::temp_dir();
    fs::write(
        "/etc/cohcap.json",
        format!(
            r#"{{"DroneWorker":{{"verbs":["open"],"paths":["{}"]}}}}"#,
            tmpdir.display()
        ),
    )
    .unwrap();
    fs::write("/srv/cohrole", "DroneWorker").unwrap();
    std::env::set_var("LD_PRELOAD", "evil.so");
    let path = tmpdir.join("ok");
    let res = open(path.to_str().unwrap(), 0);
    assert!(res.is_err());
    let log_dir = std::path::PathBuf::from("/log");
    fs::create_dir_all(&log_dir).unwrap();
    std::env::set_var("COHESIX_LOG_DIR", "/log");
    let log = fs::read_to_string(log_dir.join("sandbox.log")).unwrap();
    assert!(log.contains("open_preload"));
}
