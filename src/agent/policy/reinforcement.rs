// CLASSIFICATION: COMMUNITY
// Filename: reinforcement.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-05

/// Reinforcement learning style policy tracker.
pub struct ReinforcementPolicy {
    reward: f32,
}

impl ReinforcementPolicy {
    /// Create a new policy with zero reward.
    pub fn new() -> Self {
        Self { reward: 0.0 }
    }

    /// Update the reward value.
    pub fn update(&mut self, reward: f32) {
        self.reward += reward;
    }

    /// Current reward total.
    pub fn reward(&self) -> f32 {
        self.reward
    }
}

impl Default for ReinforcementPolicy {
    fn default() -> Self {
        Self::new()
    }
}
