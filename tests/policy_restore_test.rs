// CLASSIFICATION: COMMUNITY
// Filename: policy_restore_test.rs v0.1
// Date Modified: 2025-07-08
// Author: Cohesix Codex

use cohesix::agent::policy_memory::PolicyMemory;
use tempfile::tempdir;
use std::env;

#[test]
fn policy_persistence_roundtrip() {
    let dir = tempdir().unwrap();
    env::set_current_dir(&dir).unwrap();

    let mut mem = PolicyMemory::default();
    mem.decisions.push("a".into());
    mem.q_values.push(1.0);
    mem.successes.push(true);
    mem.save("testagent").unwrap();

    let loaded = PolicyMemory::load("testagent").unwrap();
    assert_eq!(loaded.decisions.len(), 1);
    assert_eq!(loaded.q_values[0], 1.0);
}

