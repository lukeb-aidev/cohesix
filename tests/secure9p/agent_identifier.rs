// CLASSIFICATION: COMMUNITY
// Filename: agent_identifier.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-24

use cohesix::secure9p::{policy_engine::PolicyEngine, cap_fid::Capability, sandbox::enforce};

#[test]
fn trailing_slash() {
    let mut policy = PolicyEngine::new();
    policy.allow("agent1".into(), Capability::Read);
    let ns = "/srv/namespaces/agent1/";
    assert!(enforce(ns, Capability::Read, &policy));
}

#[test]
fn invalid_path() {
    let mut policy = PolicyEngine::new();
    policy.allow("agent1".into(), Capability::Read);
    let ns = "/srv/namespaces/";
    assert!(!enforce(ns, Capability::Read, &policy));
}
