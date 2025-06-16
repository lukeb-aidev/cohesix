// CLASSIFICATION: COMMUNITY
// Filename: config.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-08-27
#![cfg(not(target_os = "uefi"))]

use std::fs;

/// Load a text configuration file and return its contents if readable.
pub fn load_config(path: &str) -> Option<String> {
    fs::read_to_string(path).ok()
}

