// CLASSIFICATION: COMMUNITY
// Filename: test_syscall_guard.rs v0.1
// Date Modified: 2025-06-25
// Author: Cohesix Codex

use cohesix::seL4::syscall::exec;
use std::fs;

#[test]
fn exec_denied_for_worker() {
    fs::create_dir_all("srv").unwrap();
    fs::write("/srv/cohrole", "DroneWorker").unwrap();
    let res = exec("echo", &["hi"]);
    assert!(res.is_err());
}
