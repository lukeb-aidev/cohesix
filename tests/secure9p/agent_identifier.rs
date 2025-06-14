// CLASSIFICATION: COMMUNITY
// Filename: agent_identifier.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-24

use cohesix::p9::secure::{cap_fid::Cap, policy_engine::PolicyEngine, sandbox::enforce};

#[test]
fn trailing_slash() {
    let mut policy = PolicyEngine::new();
    policy.allow("agent1".into(), Cap::READ);
    let ns = "/srv/namespaces/agent1/";
    assert!(enforce(ns, Cap::READ, &policy));
}

#[test]
fn invalid_path() {
    let mut policy = PolicyEngine::new();
    policy.allow("agent1".into(), Cap::READ);
    let ns = "/srv/namespaces/";
    assert!(!enforce(ns, Cap::READ, &policy));
}
