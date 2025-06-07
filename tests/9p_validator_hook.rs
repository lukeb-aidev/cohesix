// CLASSIFICATION: COMMUNITY
// Filename: 9p_validator_hook.rs v0.1
// Date Modified: 2025-07-09
// Author: Cohesix Codex

use cohesix_9p::fs::InMemoryFs;
use cohesix::validator::{log_violation, RuleViolation};
use std::fs;

#[test]
fn triggers_violations() {
    fs::create_dir_all("/log").unwrap();
    let mut fs = InMemoryFs::new();
    fn hook(ty: &'static str, file: String, agent: String, time: u64) {
        log_violation(RuleViolation { type_: ty, file, agent, time });
    }
    fs.set_validator_hook(hook);
    fs.write("/persist/secret", b"bad", "agent1");
    let log = fs::read_to_string("/log/validator_runtime.log").unwrap();
    assert!(log.contains("/persist/secret"));
}
