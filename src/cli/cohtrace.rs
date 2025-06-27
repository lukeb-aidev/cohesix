// CLASSIFICATION: COMMUNITY
// Filename: cohtrace.rs v0.3
// Author: Lukas Bower
// Date Modified: 2026-10-09

//! Minimal debug CLI for runtime trace inspection.

use std::fs;

use crate::cohesix_types::{Role, RoleManifest};
use crate::validator::{recent_syscalls, validator_running};

/// Execute `cohtrace` subcommands.
pub fn run_cohtrace(args: &[String]) -> Result<(), String> {
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("status");
    match cmd {
        "status" => {
            let running = validator_running();
            let role = RoleManifest::current_role();
            let role_name = match &role {
                Role::QueenPrimary => "QueenPrimary",
                Role::DroneWorker => "DroneWorker",
                Role::InteractiveAIBooth => "InteractiveAIBooth",
                Role::KioskInteractive => "KioskInteractive",
                Role::GlassesAgent => "GlassesAgent",
                Role::SensorRelay => "SensorRelay",
                Role::SimulatorTest => "SimulatorTest",
                Role::Other(s) => s,
            };
            let ns_map = fs::read_to_string(format!("/proc/nsmap/{role_name}")).unwrap_or_default();
            println!("Validator: {}", if running { "active" } else { "inactive" });
            println!("Role: {}", role_name);
            for m in ns_map.lines() {
                println!("Mount: {}", m);
            }
            Ok(())
        }
        "trace" => {
            let entries = recent_syscalls(10);
            if entries.is_empty() {
                println!("no recent syscalls");
            } else {
                for sc in entries.into_iter() {
                    println!("syscall: {:?}", sc);
                }
            }
            Ok(())
        }
        other => Err(format!("unknown cohtrace command: {other}")),
    }
}

/// Backwards compatibility shim for tests.
pub fn status() {
    let _ = run_cohtrace(&["status".into()]);
}
