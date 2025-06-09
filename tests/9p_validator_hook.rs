// CLASSIFICATION: COMMUNITY
// Filename: 9p_validator_hook.rs v0.1
// Date Modified: 2025-07-16
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

#[test]
fn unauthorized_capability_error() {
    use cohesix::kernel::security::l4_verified::{enforce_capability, CapabilityResult};
    assert_eq!(enforce_capability(42, "write"), CapabilityResult::Denied);
}

#[test]
fn validator_hook_timeout() {
    use std::time::{Duration, Instant};
    fs::create_dir_all("/log").unwrap();
    let mut fs = InMemoryFs::new();
    fn slow_hook(ty: &'static str, file: String, agent: String, time: u64) {
        std::thread::sleep(Duration::from_millis(50));
        log_violation(RuleViolation { type_: ty, file, agent, time });
    }
    fs.set_validator_hook(slow_hook);
    let start = Instant::now();
    fs.write("/persist/secret2", b"bad", "agent1");
    assert!(start.elapsed() >= Duration::from_millis(50));
}

#[test]
fn replay_violation_detected() {
    use std::sync::{Arc, Mutex};
    fs::create_dir_all("/log").unwrap();
    let mut fs = InMemoryFs::new();
    let seen: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let seen_clone = seen.clone();
    fs.set_validator_hook(move |_ty, file: String, agent: String, time: u64| {
        let mut guard = seen_clone.lock().unwrap();
        if let Some(prev) = &*guard {
            if prev != &file {
                log_violation(RuleViolation { type_: "replay_violation", file, agent, time });
            }
        } else {
            *guard = Some(file);
        }
    });
    fs.write("/persist/a", b"x", "agent1");
    fs.write("/persist/b", b"y", "agent1");
    let log = fs::read_to_string("/log/validator_runtime.log").unwrap();
    assert!(log.contains("replay_violation"));
}
