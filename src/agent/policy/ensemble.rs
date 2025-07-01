// CLASSIFICATION: COMMUNITY
// Filename: ensemble.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-05

use crate::prelude::*;
/// Ensemble policy combining rule based and reinforcement policies.

use super::{RuleBasedPolicy, ReinforcementPolicy};

pub struct EnsemblePolicy;

impl EnsemblePolicy {
    /// Select an action using rule based policy and update reinforcement state.
    pub fn select(input: &str, rl: &mut ReinforcementPolicy) -> String {
        let act = RuleBasedPolicy::decide(input);
        if act == "ok" {
            rl.update(1.0);
        } else {
            rl.update(-1.0);
        }
        act
    }
}
