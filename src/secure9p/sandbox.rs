// CLASSIFICATION: COMMUNITY
// Filename: sandbox.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-24

use super::{policy_engine::PolicyEngine, cap_fid::Capability};
use std::path::Path;

pub fn enforce(ns: &str, cap: Capability, policy: &PolicyEngine) -> bool {
    let ns = ns.trim_end_matches('/');
    let agent = Path::new(ns)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    policy.check(agent, cap)
}
