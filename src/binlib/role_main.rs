// CLASSIFICATION: COMMUNITY
// Filename: role_main.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-08-16

#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
use std::env;
use std::fs;

/// Determine the current runtime role.
pub fn current_role() -> String {
    env::var("COHROLE")
        .ok()
        .or_else(|| fs::read_to_string("/srv/cohrole").ok())
        .unwrap_or_else(|| "Unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_override() {
        unsafe {
            env::set_var("COHROLE", "DroneWorker");
        }
        assert_eq!(current_role(), "DroneWorker");
    }
}
