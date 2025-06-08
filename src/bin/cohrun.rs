// CLASSIFICATION: COMMUNITY
// Filename: cohrun.rs v0.2
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
}

#[derive(Subcommand)]
enum OrchestratorCmd {
    Status,
    Assign { role: String, worker_id: String },
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
    }
}
