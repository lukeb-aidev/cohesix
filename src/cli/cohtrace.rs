// CLASSIFICATION: COMMUNITY
// Filename: cohtrace.rs v0.2
// Author: Lukas Bower
// Date Modified: 2026-10-08

//! Minimal debug CLI for runtime trace inspection.

use std::fs;
use std::path::Path;

/// Execute `cohtrace` subcommands.
pub fn run_cohtrace(args: &[String]) -> Result<(), String> {
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("status");
    match cmd {
        "status" => {
            let role = fs::read_to_string("/srv/cohrole").unwrap_or_else(|_| "Unknown".into());
            let role_trim = role.trim();
            let ns_map = fs::read_to_string(format!("/proc/nsmap/{role_trim}")).unwrap_or_else(|_| "/srv /home/cohesix".into());
            let ns_list = ns_map.split_whitespace().collect::<Vec<_>>().join(" ");
            let active = Path::new("/srv/validator/live.sock").exists();
            println!(
                "Validator: {} | Role: {} | Namespaces: {}",
                if active { "active" } else { "inactive" },
                role_trim,
                ns_list
            );
            Ok(())
        }
        other => Err(format!("unknown cohtrace command: {other}")),
    }
}

/// Backwards compatibility shim for tests.
pub fn status() {
    let _ = run_cohtrace(&["status".into()]);
}
