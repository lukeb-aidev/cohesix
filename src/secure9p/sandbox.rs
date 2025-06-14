// CLASSIFICATION: COMMUNITY
// Filename: sandbox.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-24

use super::{policy_engine::PolicyEngine, cap_fid::Capability};

pub fn enforce(ns: &str, cap: Capability, policy: &PolicyEngine) -> bool {
    let agent = ns.split('/').last().unwrap_or("");
    policy.check(agent, cap)
}
