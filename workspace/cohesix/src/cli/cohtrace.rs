// CLASSIFICATION: COMMUNITY
// Filename: cohtrace.rs v0.5
// Author: Lukas Bower
// Date Modified: 2026-10-28

use crate::orchestrator::protocol::ClusterStateRequest;
use crate::queen::orchestrator::QueenOrchestrator;
#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
/// Minimal debug CLI for runtime trace inspection.
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::runtime::Runtime;

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
            let ns_map = fs::read_to_string(format!("/srv/nsmap/{role_name}")).unwrap_or_default();
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
            match fetch_cluster_state() {
                Ok(state) => {
                    println!("Queen ID: {}", state.queen_id);
                    println!("Connected Workers: {}", state.workers.len());
                    println!("Heartbeat Timeout: {}s", state.timeout_seconds);
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    for worker in state.workers {
                        let age = now.saturating_sub(worker.last_seen);
                        let trust = if worker.trust.is_empty() {
                            "green"
                        } else {
                            worker.trust.as_str()
                        };
                        let gpu_info = worker.gpu.map(|gpu| {
                            format!(
                                "GPU perf={} load={}/{} latency={}",
                                gpu.perf_watt,
                                gpu.current_load,
                                gpu.gpu_capacity,
                                gpu.latency_score
                            )
                        });
                        if let Some(info) = gpu_info {
                            println!(
                                "{} ({}) - {} - trust {} - last seen {}s - {}",
                                worker.worker_id, worker.role, worker.status, trust, age, info
                            );
                        } else {
                            println!(
                                "{} ({}) - {} - trust {} - last seen {}s",
                                worker.worker_id, worker.role, worker.status, trust, age
                            );
                        }
                    }
                }
                Err(err) => println!("cloud state unavailable: {err}"),
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

fn fetch_cluster_state() -> Result<crate::orchestrator::protocol::ClusterStateResponse, String> {
    let runtime = Runtime::new().map_err(|e| format!("failed to start tokio runtime: {e}"))?;
    runtime.block_on(async {
        let mut client = QueenOrchestrator::connect_default_client()
            .await
            .map_err(|e| format!("failed to connect orchestrator: {e}"))?;
        client
            .get_cluster_state(ClusterStateRequest {})
            .await
            .map_err(|e| format!("cluster state request failed: {e}"))
            .map(|resp| resp.into_inner())
    })
}
