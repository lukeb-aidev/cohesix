// CLASSIFICATION: COMMUNITY
// Filename: agent_identifier.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-26

use cohesix::secure9p::{policy_engine::PolicyEngine, cap_fid::Capability, sandbox::enforce};

#[test]
fn trailing_slash() {
    let mut policy = PolicyEngine::new();
    policy.allow("agent1".into(), "read:/srv/namespaces/agent1".into());
    let ns = "/srv/namespaces/agent1/";
    assert!(enforce(ns, Capability::Read, &policy));
}

#[test]
fn invalid_path() {
    let mut policy = PolicyEngine::new();
    policy.allow("agent1".into(), "read:/srv/namespaces/agent1".into());
    let ns = "/srv/namespaces/";
    assert!(!enforce(ns, Capability::Read, &policy));
}
