// CLASSIFICATION: COMMUNITY
// Filename: policy_restore_test.rs v0.2
// Date Modified: 2025-08-16
// Author: Cohesix Codex

use cohesix::agent::policy_memory::PolicyMemory;
use tempfile::tempdir;
use std::env;

#[test]
fn policy_persistence_roundtrip() {
    let dir = tempdir().unwrap();
    env::set_current_dir(&dir).unwrap();
    unsafe {
        env::set_var("COHESIX_POLICY_TMP", dir.path());
    }

    let mut mem = PolicyMemory::default();
    mem.decisions.push("a".into());
    mem.q_values.push(1.0);
    mem.successes.push(true);
    PolicyMemory::save_shared(&mem).unwrap();

    let loaded = PolicyMemory::load_shared().unwrap();
    assert_eq!(loaded.decisions, mem.decisions);
    assert_eq!(loaded.q_values, mem.q_values);
    assert_eq!(loaded.successes, mem.successes);
    unsafe {
        env::remove_var("COHESIX_POLICY_TMP");
    }
}
