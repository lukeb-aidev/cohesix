// CLASSIFICATION: COMMUNITY
// Filename: policy_denial.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-24

use cohesix::p9::secure::{cap_fid::Cap, policy_engine::PolicyEngine, sandbox::enforce};

#[test]
fn policy_denial() {
    let mut policy = PolicyEngine::new();
    policy.allow("agent1".into(), Cap::READ);
    let ns = format!("/srv/namespaces/{}", "agent1");
    assert!(!enforce(&ns, Cap::WRITE, &policy));
}
