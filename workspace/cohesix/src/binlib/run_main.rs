// CLASSIFICATION: COMMUNITY
// Filename: run_main.rs v0.3
// Author: Lukas Bower
// Date Modified: 2026-12-31

use crate::orchestrator::protocol::{AssignRoleRequest, ClusterStateRequest, ScheduleRequest};
use crate::queen::orchestrator::QueenOrchestrator;
#[cfg(feature = "rapier")]
use crate::sim::physics_demo;
use crate::{new_err, CohError};
#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
use clap::{Parser, Subcommand};
use std::future::Future;

/// CLI wrapper for `cohrun` utility.
#[derive(Parser)]
#[command(name = "cohrun", about = "Run Cohesix demo scenarios", version = "0.2")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

/// Top-level commands.
#[derive(Subcommand)]
pub enum Commands {
    #[cfg(feature = "rapier")]
    PhysicsDemo,
    KioskStart,
    KioskEvent {
        #[arg(long)]
        event: String,
        #[arg(long)]
        user: Option<String>,
    },
    Orchestrator {
        #[command(subcommand)]
        command: OrchestratorCmd,
    },
    GpuStatus,
    GpuDispatch {
        task: String,
    },
    Goal {
        #[command(subcommand)]
        command: GoalCmd,
    },
    TrustEscalate {
        worker_id: String,
    },
    TrustReport,
    FederateWith {
        queen_url: String,
    },
    WatchdogStatus,
    TraceReplay {
        #[arg(long, default_value = "failover")]
        context: String,
        #[arg(long, default_value_t = 5)]
        limit: u32,
    },
    InjectRule {
        from: String,
    },
}

/// Orchestrator subcommands.
#[derive(Subcommand)]
pub enum OrchestratorCmd {
    Status,
    Assign { role: String, worker_id: String },
}

/// Goal management commands.
#[derive(Subcommand)]
pub enum GoalCmd {
    Add { json: String },
    List,
    Assign { goal_id: String, worker_id: String },
}

/// Execute the run commands.
pub fn run(cli: Cli) {
    match cli.command {
        #[cfg(feature = "rapier")]
        Commands::PhysicsDemo => physics_demo::run_demo(),
        Commands::KioskStart => {
            crate::init::kiosk::start();
            println!("kiosk started");
        }
        Commands::KioskEvent { event, user } => {
            crate::init::kiosk::emit_event(&event, user.as_deref());
            println!("event logged");
        }
        Commands::Orchestrator { command: cmd } => match cmd {
            OrchestratorCmd::Status => {
                match with_runtime(async move {
                    let mut client = QueenOrchestrator::connect_default_client().await?;
                    client
                        .get_cluster_state(ClusterStateRequest {})
                        .await
                        .map_err(|e| new_err(format!("cluster state request failed: {e}")))
                        .map(|resp| resp.into_inner())
                }) {
                    Ok(state) => {
                        if state.workers.is_empty() {
                            println!("no agents registered");
                        } else {
                            for worker in state.workers {
                                println!(
                                    "{} {} {} {}",
                                    worker.worker_id, worker.role, worker.status, worker.ip
                                );
                            }
                        }
                    }
                    Err(err) => {
                        println!("failed to query orchestrator: {err}");
                    }
                }
            }
            OrchestratorCmd::Assign { role, worker_id } => {
                let worker_display = worker_id.clone();
                let role_display = role.clone();
                let worker_request = worker_id.clone();
                let role_request = role.clone();
                match with_runtime(async move {
                    let mut client = QueenOrchestrator::connect_default_client().await?;
                    client
                        .assign_role(AssignRoleRequest {
                            worker_id: worker_request,
                            role: role_request,
                        })
                        .await
                        .map_err(|e| new_err(format!("assign role failed: {e}")))
                        .map(|resp| resp.into_inner().updated)
                }) {
                    Ok(true) => println!("assigned {role_display} to {worker_display}"),
                    Ok(false) => println!("worker {worker_display} not registered"),
                    Err(err) => println!("assignment failed: {err}"),
                }
            }
        },
        Commands::GpuStatus => {
            match with_runtime(async move {
                let mut client = QueenOrchestrator::connect_default_client().await?;
                client
                    .get_cluster_state(ClusterStateRequest {})
                    .await
                    .map_err(|e| new_err(format!("cluster state request failed: {e}")))
                    .map(|resp| resp.into_inner())
            }) {
                Ok(state) => {
                    let mut printed = false;
                    for worker in state.workers {
                        if let Some(gpu) = worker.gpu {
                            println!(
                                "{} perf_watt={} load={}/{} latency={}",
                                worker.worker_id,
                                gpu.perf_watt,
                                gpu.current_load,
                                gpu.gpu_capacity,
                                gpu.latency_score
                            );
                            printed = true;
                        }
                    }
                    if !printed {
                        println!("no gpu nodes available");
                    }
                }
                Err(err) => println!("failed to query GPU state: {err}"),
            }
        }
        Commands::GpuDispatch { task } => {
            let task_display = task.clone();
            let task_request = task.clone();
            match with_runtime(async move {
                let mut client = QueenOrchestrator::connect_default_client().await?;
                client
                    .request_schedule(ScheduleRequest {
                        agent_id: task_request,
                        require_gpu: true,
                    })
                    .await
                    .map_err(|e| new_err(format!("schedule request failed: {e}")))
                    .map(|resp| resp.into_inner())
            }) {
                Ok(resp) if resp.assigned => {
                    println!("dispatched {task_display} to {}", resp.worker_id)
                }
                Ok(_) => println!("no gpu nodes available"),
                Err(err) => println!("failed to dispatch {task_display}: {err}"),
            }
        }
        Commands::Goal { command } => match command {
            GoalCmd::Add { json } => {
                use serde_json::Value;
                use std::fs;
                let mut goals: Vec<Value> = fs::read_to_string("/srv/goals/active_goals.json")
                    .ok()
                    .and_then(|d| serde_json::from_str(&d).ok())
                    .unwrap_or_default();
                let mut val: Value = serde_json::from_str(&json).unwrap_or_default();
                let id = format!("g{}", goals.len() + 1);
                if let Some(obj) = val.as_object_mut() {
                    obj.insert("id".into(), Value::String(id.clone()));
                }
                goals.push(val);
                fs::create_dir_all("/srv/goals").ok();
                fs::write(
                    "/srv/goals/active_goals.json",
                    serde_json::to_string_pretty(&goals).unwrap(),
                )
                .ok();
                println!("goal {id} added");
            }
            GoalCmd::List => {
                if let Ok(data) = std::fs::read_to_string("/srv/goals/active_goals.json") {
                    println!("{data}");
                } else {
                    println!("no goals");
                }
            }
            GoalCmd::Assign { goal_id, worker_id } => {
                use serde_json::Value;
                use std::fs;
                let mut goals: Vec<Value> = fs::read_to_string("/srv/goals/active_goals.json")
                    .ok()
                    .and_then(|d| serde_json::from_str(&d).ok())
                    .unwrap_or_default();
                for g in &mut goals {
                    if g["id"] == goal_id {
                        g["assigned_worker"] = Value::String(worker_id.clone());
                    }
                }
                fs::write(
                    "/srv/goals/active_goals.json",
                    serde_json::to_string_pretty(&goals).unwrap(),
                )
                .ok();
                println!("goal {goal_id} assigned to {worker_id}");
            }
        },
        Commands::TrustEscalate { worker_id } => {
            crate::queen::trust::escalate(&worker_id, "red");
            println!("{worker_id} escalated to red");
        }
        Commands::TrustReport => {
            for (w, lvl) in crate::queen::trust::list_trust() {
                println!("{}: {}", w, lvl);
            }
        }
        Commands::FederateWith { queen_url } => {
            let hostname = "cohesix-uefi";
            if let Ok(mut fm) = crate::queen::federation::FederationManager::new(hostname) {
                if let Err(e) = fm.connect(&queen_url) {
                    println!("federation failed: {e}");
                } else {
                    println!("handshake sent to {queen_url}");
                }
            }
        }
        Commands::WatchdogStatus => {
            if let Ok(log) = std::fs::read_to_string("/srv/logs/watchdog.log") {
                println!("{log}");
            } else {
                println!("no watchdog log");
            }
        }
        Commands::TraceReplay { context, limit } => {
            if context == "failover" {
                crate::worker::role_memory::RoleMemory::replay_last(limit as usize);
            } else {
                println!("unknown context");
            }
        }
        Commands::InjectRule { from } => {
            if let Ok(data) = std::fs::read_to_string(&from) {
                std::fs::create_dir_all("/srv/validator").ok();
                std::fs::write("/srv/validator/inject_rule", data).ok();
                println!("rule injected from {from}");
            } else {
                println!("failed to read rule file");
            }
        }
    }
}

fn with_runtime<F, T>(future: F) -> Result<T, CohError>
where
    F: Future<Output = Result<T, CohError>> + Send + 'static,
    T: Send + 'static,
{
    tokio::runtime::Runtime::new()
        .map_err(|e| new_err(format!("failed to start tokio runtime: {e}")))?
        .block_on(future)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_gpu_status() {
        let cli = Cli::parse_from(["cohrun", "gpu-status"]);
        match cli.command {
            Commands::GpuStatus => (),
            _ => panic!("unexpected parse"),
        }
    }
}
