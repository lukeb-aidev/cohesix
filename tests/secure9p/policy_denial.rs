// CLASSIFICATION: COMMUNITY
// Filename: policy_denial.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-24

use cohesix::secure9p::{policy_engine::PolicyEngine, cap_fid::Capability, sandbox::enforce, namespace_resolver::resolve};

#[test]
fn policy_denial() {
    let mut policy = PolicyEngine::new();
    policy.allow("agent1".into(), Capability::Read);
    let ns = resolve("agent1");
    assert!(!enforce(&ns, Capability::Write, &policy));
}
