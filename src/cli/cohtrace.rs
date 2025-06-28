// CLASSIFICATION: COMMUNITY
// Filename: cohtrace.rs v0.5
// Author: Lukas Bower
// Date Modified: 2026-10-28

//! Minimal debug CLI for runtime trace inspection.

use std::fs;
use serde_json;

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
                Role::RegionalQueen => "RegionalQueen",
                Role::BareMetalQueen => "BareMetalQueen",
                Role::DroneWorker => "DroneWorker",
                Role::InteractiveAiBooth => "InteractiveAiBooth",
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
        "cloud" => {
            let data = fs::read_to_string("/srv/cloud/state.json").unwrap_or_default();
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) {
                let queen = v
                    .get("queen_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let ts = v.get("ts").and_then(|v| v.as_u64()).unwrap_or(0);
                let workers = v
                    .get("worker_count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                println!("Queen ID: {}", queen);
                println!("Last heartbeat: {}", ts);
                println!("Connected Workers: {}", workers);
            } else {
                println!("cloud state unavailable");
            }

            if let Ok(active) = fs::read_to_string("/srv/agents/active.json") {
                if let Ok(entries) = serde_json::from_str::<serde_json::Value>(&active) {
                    if let Some(arr) = entries.as_array() {
                        for entry in arr {
                            let id = entry
                                .get("worker_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown");
                            let role = entry
                                .get("role")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown");
                            println!("Worker {id}: {role}");
                        }
                    }
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
