// CLASSIFICATION: COMMUNITY
// Filename: rule_based.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-05

#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
/// Simple rule based policy.
pub struct RuleBasedPolicy;

impl RuleBasedPolicy {
    /// Decide next action based on input string.
    pub fn decide(input: &str) -> String {
        if input.contains("error") {
            "recover".into()
        } else {
            "ok".into()
        }
    }
}
