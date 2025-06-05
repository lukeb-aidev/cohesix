// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

//! Cohesix Utility Module
//!
//! This module serves as a namespace for reusable utility functions and helpers
//! used across the Cohesix platform. Submodules include constants, evaluators, formatters, and common traits.

pub mod const_eval;
pub mod format;
pub mod helpers;

/// Initializes any global utilities that require boot-time setup.
pub fn init_utils() {
    println!("[utils] initializing utility submodules...");
    // TODO(cohesix): Add any required initialization hooks here
}
