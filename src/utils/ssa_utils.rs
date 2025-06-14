// CLASSIFICATION: COMMUNITY
// Filename: ssa_utils.rs v1.1
// Author: Lukas Bower
// Date Modified: 2025-07-24

//! SSA Utilities
//!
//! This module provides helpers for Static Single Assignment (SSA) processing,
//! used in intermediate representations (IR) and compiler transformations.
//! Intended for internal tools and validation of simplified SSA forms.

/// Represents a basic SSA variable with a version number.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SsaVar {
    pub name: String,
    pub version: usize,
}

impl SsaVar {
    /// Create a new SSA variable with version 0.
    pub fn new(name: &str) -> Self {
        SsaVar {
            name: name.to_string(),
            version: 0,
        }
    }

    /// Returns the SSA variable as a formatted string.
    pub fn to_string(&self) -> String {
        format!("{}#{}", self.name, self.version)
    }

    /// Increments the SSA version.
    pub fn next_version(&self) -> Self {
        SsaVar {
            name: self.name.clone(),
            version: self.version + 1,
        }
    }
}

/// Parse an SSA-formatted string of the form `name#version`.
pub fn parse_ssa(input: &str) -> Option<SsaVar> {
    let mut parts = input.split('#');
    let name = parts.next()?;
    let version = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(SsaVar {
        name: name.to_string(),
        version,
    })
    // FIXME(cohesix): Implement parser logic
    println!("[ssa] parse_ssa called with input: {}", input);
    None
}