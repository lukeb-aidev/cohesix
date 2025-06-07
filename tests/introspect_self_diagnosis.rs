// CLASSIFICATION: COMMUNITY
// Filename: introspect_self_diagnosis.rs v0.1
// Date Modified: 2025-07-09
// Author: Cohesix Codex

use cohesix::agents::base::BaseAgent;
use cohesix::sim::introspect::{IntrospectionData};
use std::fs;

#[test]
fn detects_policy_failure() {
    fs::create_dir_all("/trace").unwrap();
    let mut agent = BaseAgent::new("test");
    let data = IntrospectionData::default();
    let mut triggered = false;
    for _ in 0..5 {
        if agent.tick(1.2, &data) { triggered = true; }
    }
    assert!(triggered);
    let log = fs::read_to_string("/trace/introspect_test.log").unwrap();
    assert!(!log.is_empty());
}
