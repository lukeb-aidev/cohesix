// CLASSIFICATION: COMMUNITY
// Filename: cohrun.rs v0.3
// Author: Lukas Bower
// Date Modified: 2025-07-11

use clap::{Parser, Subcommand};
use cohesix::queen::orchestrator::{QueenOrchestrator, SchedulePolicy};
use cohesix::sim::{physics_demo, webcam_tilt};
use cohesix::webcam::capture;

#[derive(Parser)]
#[command(name = "cohrun", about = "Run Cohesix demo scenarios", version = "0.2")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    PhysicsDemo,
    TestWebcam,
    WebcamTilt,
    KioskStart,
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
    TrustEscalate { worker_id: String },
    TrustReport,
    FederateWith { queen_url: String },
}

#[derive(Subcommand)]
enum OrchestratorCmd {
    Status,
    Assign { role: String, worker_id: String },
}

#[derive(Subcommand)]
enum GoalCmd {
    Add { json: String },
    List,
    Assign { goal_id: String, worker_id: String },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::PhysicsDemo => physics_demo::run_demo(),
        Commands::TestWebcam => {
            if capture::capture_jpeg("/srv/webcam/frame.jpg").is_ok() {
                println!("frame saved to /srv/webcam/frame.jpg");
            } else {
                println!("webcam capture failed");
            }
        }
        Commands::WebcamTilt => webcam_tilt::run(None),
        Commands::KioskStart => {
            cohesix::init::kiosk::start();
            println!("kiosk started");
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
                    .unwrap_or_else(Vec::new);
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
                    .unwrap_or_else(Vec::new);
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
            cohesix::queen::trust::escalate(&worker_id, "red");
            println!("{worker_id} escalated to red");
        }
        Commands::TrustReport => {
            for (w, lvl) in cohesix::queen::trust::list_trust() {
                println!("{}: {}", w, lvl);
            }
        }
        Commands::FederateWith { queen_url } => {
            if let Ok(mut fm) = cohesix::queen::federation::FederationManager::new(
                &hostname::get().unwrap_or_default().to_string_lossy(),
            ) {
                if let Err(e) = fm.connect(&queen_url) {
                    println!("federation failed: {e}");
                } else {
                    println!("handshake sent to {queen_url}");
                }
            }
        }
    }
}
