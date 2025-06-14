// CLASSIFICATION: COMMUNITY
// Filename: validator_hook.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-24

pub type ValidatorHook = std::sync::Arc<dyn Fn(&str, String, u64) + Send + Sync>;
