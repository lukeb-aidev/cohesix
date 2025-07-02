// CLASSIFICATION: COMMUNITY
// Filename: introspect_self_diagnosis.rs v0.2
// Date Modified: 2026-09-15
// Author: Cohesix Codex

use cohesix::agents::base::BaseAgent;
use cohesix::sim::introspect::IntrospectionData;
use std::fs;
use std::io::ErrorKind;

#[test]
fn detects_policy_failure() {
    let trace_root = std::env::temp_dir().join("trace");
    if let Err(e) = fs::create_dir_all(&trace_root) {
        eprintln!("\u{1F512} Skipping test: cannot create trace dir: {e}");
        return;
    }

    let mut agent = BaseAgent::new("test");
    let data = IntrospectionData::default();
    let mut triggered = false;
    for _ in 0..5 {
        if agent.tick(1.2, &data) {
            triggered = true;
        }
    }
    assert!(triggered);

    let log = match fs::read_to_string(trace_root.join("introspect_test.log")) {
        Ok(v) => v,
        Err(e) if e.kind() == ErrorKind::PermissionDenied => {
            eprintln!(
                "\u{1F512} Skipping test: insufficient permissions to run detects_policy_failure"
            );
            return;
        }
        Err(e) => panic!("failed to read introspect_test.log: {}", e),
    };
    assert!(!log.is_empty());
}
