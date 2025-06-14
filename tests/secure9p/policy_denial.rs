// CLASSIFICATION: COMMUNITY
// Filename: policy_denial.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-25

use cohesix::p9::secure::policy_engine::PolicyEngine;
use serial_test::serial;
use tempfile::tempdir;
use std::fs;

#[test]
#[serial]
fn policy_denial() {
    let dir = tempdir().unwrap();
    let policy_path = dir.path().join("policy.json");
    fs::write(
        &policy_path,
        "{\"policy\":[{\"agent\":\"tester\",\"allow\":[\"read:/data\"]}]}",
    )
    .unwrap();
    let pe = PolicyEngine::load(&policy_path).unwrap();
    assert!(pe.allows("tester", "read", "/data/file"));
    assert!(!pe.allows("tester", "write", "/data/file"));
}
