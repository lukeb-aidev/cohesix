// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

use crate::prelude::*;
//! Cohesix Utility Module
//!
//! This module serves as a namespace for reusable utility functions and helpers
//! used across the Cohesix platform. Submodules include constants, evaluators, formatters, and common traits.

/// Constant expression evaluation helpers.
pub mod const_eval;
/// Formatting helpers.
pub mod format;
/// Miscellaneous helper utilities.
pub mod helpers;
/// Simple deterministic RNG.
pub mod tiny_rng;
/// Lightweight Ed25519 implementation.
pub mod tiny_ed25519;
/// GPU runtime helpers.
#[cfg(feature = "cuda")]
pub mod gpu;

/// Initializes any global utilities that require boot-time setup.
pub fn init_utils() {
    println!("[utils] initializing utility submodules...");
}
