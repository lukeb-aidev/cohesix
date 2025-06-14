// CLASSIFICATION: COMMUNITY
// Filename: policy_denial.rs v0.3
// Author: Lukas Bower
// Date Modified: 2025-07-26

use cohesix::secure9p::policy_engine::PolicyEngine;
use serial_test::serial;

#[test]
#[serial]
fn policy_denial() {
    let mut pe = PolicyEngine::new();
    pe.allow("tester".into(), "read:/data".into());
    assert!(pe.allows("tester", "read", "/data/file"));
    assert!(!pe.allows("tester", "write", "/data/file"));
}
