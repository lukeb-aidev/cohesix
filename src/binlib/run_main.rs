// CLASSIFICATION: COMMUNITY
// Filename: run_main.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-22

use clap::{Parser, Subcommand};
use crate::queen::orchestrator::{QueenOrchestrator, SchedulePolicy};
#[cfg(feature = "rapier")]
use crate::sim::physics_demo;
#[cfg(feature = "rapier")]
use crate::sim::webcam_tilt;
use crate::webcam::capture;

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
    TestWebcam,
    #[cfg(feature = "rapier")]
    WebcamTilt,
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
    GpuDispatch { task: String },
    Goal { #[command(subcommand)] command: GoalCmd },
    TrustEscalate { worker_id: String },
    TrustReport,
    FederateWith { queen_url: String },
    WatchdogStatus,
    TraceReplay { #[arg(long, default_value="failover")] context: String, #[arg(long, default_value_t = 5)] limit: u32 },
    InjectRule { from: String },
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
        Commands::TestWebcam => {
            if capture::capture_jpeg("/srv/webcam/frame.jpg").is_ok() {
                println!("frame saved to /srv/webcam/frame.jpg");
            } else {
                println!("webcam capture failed");
            }
        }
        #[cfg(feature = "rapier")]
        Commands::WebcamTilt => webcam_tilt::run(None),
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
                if let Ok(data) = std::fs::read_to_string("/srv/agents/active.json") {
                    println!("{data}");
                } else {
                    println!("no agents registered");
                }
            }
            OrchestratorCmd::Assign { role, worker_id } => {
                let mut orch = QueenOrchestrator::new(5, SchedulePolicy::RoundRobin);
                orch.sync_workers();
                orch.assign_role(&worker_id, &role);
                println!("assigned {role} to {worker_id}");
            }
        },
        Commands::GpuStatus => {
            if let Ok(data) = std::fs::read_to_string("/srv/gpu_registry.json") {
                println!("{data}");
            } else {
                println!("no gpu registry");
            }
        }
        Commands::GpuDispatch { task } => {
            let mut orch = QueenOrchestrator::new(5, SchedulePolicy::GpuPriority);
            orch.sync_workers();
            orch.sync_gpu_telemetry();
            if let Some(wid) = orch.schedule_gpu(&task) {
                println!("dispatched {task} to {wid}");
                orch.export_gpu_registry();
            } else {
                println!("no gpu nodes available");
            }
        }
        Commands::Goal { command } => match command {
            GoalCmd::Add { json } => {
                use std::fs;
                use serde_json::Value;
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
                fs::write("/srv/goals/active_goals.json", serde_json::to_string_pretty(&goals).unwrap()).ok();
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
                fs::write("/srv/goals/active_goals.json", serde_json::to_string_pretty(&goals).unwrap()).ok();
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
            if let Ok(mut fm) = crate::queen::federation::FederationManager::new(
                &hostname::get().unwrap_or_default().to_string_lossy(),
            ) {
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
