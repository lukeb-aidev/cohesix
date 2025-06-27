// CLASSIFICATION: COMMUNITY
// Filename: cohtrace.rs v0.1
// Author: Lukas Bower
// Date Modified: 2026-10-07

//! Minimal debug CLI for runtime trace inspection.

use std::fs;
use std::path::Path;

/// Print role, namespace mounts, and validator status.
pub fn status() {
    let role = fs::read_to_string("/srv/cohrole").unwrap_or_else(|_| "Unknown".into());
    println!("role: {}", role.trim());
    let ns_path = format!("/proc/nsmap/{}", role.trim());
    if let Ok(ns) = fs::read_to_string(&ns_path) {
        println!("namespaces:\n{ns}");
    } else {
        println!("namespaces: unavailable");
    }
    let active = Path::new("/srv/validator/live.sock").exists();
    println!("validator: {}", if active { "active" } else { "inactive" });
}
