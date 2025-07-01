// CLASSIFICATION: COMMUNITY
// Filename: self_tuning_stabilizer.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-05

use crate::prelude::*;
/// Adaptive stabilizer that learns optimal actions over time.

use super::{EnsemblePolicy, ReinforcementPolicy};

pub struct SelfTuningStabilizer {
    policy: ReinforcementPolicy,
}

impl SelfTuningStabilizer {
    /// Create a new stabilizer.
    pub fn new() -> Self {
        Self { policy: ReinforcementPolicy::new() }
    }

    /// Process input and return chosen action.
    pub fn step(&mut self, input: &str) -> String {
        EnsemblePolicy::select(input, &mut self.policy)
    }
}

impl Default for SelfTuningStabilizer {
    fn default() -> Self {
        Self::new()
    }
}
