// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-11

use crate::prelude::*;
/// Minimal POSIX compatibility helpers for Cohesix.
/// Provides simple syscall name translations to allow
/// refactoring legacy code.

use std::collections::HashMap;

/// Translate a POSIX syscall name to a Cohesix shim.
pub fn translate_syscall(name: &str) -> Option<&'static str> {
    let table: HashMap<&str, &str> = HashMap::from([
        ("open", "coh_open"),
        ("read", "coh_read"),
        ("write", "coh_write"),
    ]);
    table.get(name).copied()
}
