// CLASSIFICATION: COMMUNITY
// Filename: test_syscall_guard.rs v0.2
// Date Modified: 2026-09-30
// Author: Cohesix Codex

use cohesix::seL4::syscall::exec;
use cohesix::validator::{self, config::ValidatorConfig};
use env_logger;
use tempfile::tempdir;

#[test]
fn exec_denied_for_worker() {
    let _ = env_logger::builder().is_test(true).try_init();
    let dir = tempdir().unwrap();
    std::env::set_current_dir(&dir).unwrap();
    validator::config::set_config(ValidatorConfig {
        log_dir: dir.path().to_path_buf(),
        violations_dir: dir.path().to_path_buf(),
    })
    .unwrap();
    std::env::set_var("COHROLE", "DroneWorker");
    let _err = exec("echo", &["hi"]).expect_err("Worker exec was expected to fail");
}
