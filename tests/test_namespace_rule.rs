// CLASSIFICATION: COMMUNITY
// Filename: test_namespace_rule.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-23

use cohesix::services::{sandbox::SandboxService, Service};
use cohesix::validator::{self, config::ValidatorConfig, RuleViolation};
use tempfile::tempdir;

#[test]
fn validator_denies_forbidden_path() {
    let dir = tempdir().unwrap();
    std::env::set_current_dir(&dir).unwrap();
    let log_dir = dir.path().join("log");
    let viol_dir = dir.path().join("viol");
    validator::config::set_config(ValidatorConfig {
        log_dir: log_dir.clone(),
        violations_dir: viol_dir,
    });
    std::fs::create_dir_all(&log_dir).unwrap();

    let mut svc = SandboxService::default();
    svc.init();
    let allowed = svc.enforce("write", "/secret/data", "DroneWorker");
    svc.shutdown();
    assert!(!allowed);

    let log = std::fs::read_to_string(log_dir.join("validator_runtime.log")).unwrap();
    assert!(log.contains("/secret/data"));
}
