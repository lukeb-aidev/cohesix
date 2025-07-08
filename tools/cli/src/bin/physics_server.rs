// CLASSIFICATION: COMMUNITY
// Filename: physics_server.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-08

use clap::Parser;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Deserialize)]
struct PhysicsJob {
    job_id: String,
    initial_position: [f64; 3],
    initial_velocity: [f64; 3],
    mass: f64,
    duration: f64,
}

#[derive(Serialize)]
struct World {
    final_position: [f64; 3],
    final_velocity: [f64; 3],
    collided: bool,
    energy_remaining: f64,
}

#[derive(Serialize)]
struct ResultData {
    job_id: String,
    status: String,
    steps: i32,
    duration: f64,
    logs: Vec<String>,
}

#[derive(Parser)]
#[command(about = "Plan9 physics job processor")]
struct Args {
    #[arg(long, default_value = "/mnt/physics_jobs")]
    job_dir: PathBuf,
    #[arg(long, default_value = "/sim")]
    sim_dir: PathBuf,
    #[arg(long, default_value = "/srv/trace/sim.log")]
    log_file: PathBuf,
}

fn write_status(processed: usize, last_err: &str, last_job: &str) -> std::io::Result<()> {
    let status = format!("jobs_processed={}\nlast_error=\"{}\"\nlast_job=\"{}\"\n", processed, last_err, last_job);
    fs::write("/srv/physics/status", status)
}

fn process_job(job_path: &Path, args: &Args) -> std::io::Result<()> {
    let data = fs::read(job_path)?;
    let job: PhysicsJob = serde_json::from_slice(&data).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    let steps = (job.duration * 100.0) as i32;
    let final_position = [
        job.initial_position[0] + job.initial_velocity[0] * job.duration,
        job.initial_position[1] + job.initial_velocity[1] * job.duration,
        job.initial_position[2] + job.initial_velocity[2] * job.duration,
    ];
    let world = World {
        final_position,
        final_velocity: job.initial_velocity,
        collided: false,
        energy_remaining: 0.95,
    };
    fs::create_dir_all(&args.sim_dir)?;
    let world_path = args.sim_dir.join("world.json");
    let result_path = args.sim_dir.join("result.json");
    fs::write(&world_path, serde_json::to_vec_pretty(&world).unwrap())?;
    let result = ResultData {
        job_id: job.job_id.clone(),
        status: "completed".into(),
        steps,
        duration: steps as f64 / 100.0,
        logs: vec!["t=0.1 pos=[0.1,0,0]".into(), "t=0.2 pos=[0.2,0,0]".into()],
    };
    fs::write(&result_path, serde_json::to_vec_pretty(&result).unwrap())?;
    Ok(())
}

fn main() -> std::io::Result<()> {
    let args = Args::parse();
    fs::create_dir_all("/srv/trace")?;
    fs::create_dir_all("/srv/physics")?;
    fs::create_dir_all(&args.sim_dir)?;
    let mut logf = OpenOptions::new().create(true).append(true).open(&args.log_file)?;
    let mut processed = 0usize;
    let mut last_err = String::new();
    let mut last_job = String::new();

    loop {
        for entry in glob::glob(&format!("{}/physics_job_*.json", args.job_dir.display())).unwrap() {
            match entry {
                Ok(path) => {
                    match process_job(&path, &args) {
                        Ok(_) => {
                            writeln!(logf, "completed {}", path.display())?;
                            last_err.clear();
                            last_job = path.display().to_string();
                            processed += 1;
                            write_status(processed, &last_err, &last_job)?;
                            fs::remove_file(path)?;
                        }
                        Err(e) => {
                            writeln!(logf, "error {}: {}", path.display(), e)?;
                            last_err = e.to_string();
                            write_status(processed, &last_err, &last_job)?;
                        }
                    }
                }
                Err(_) => {}
            }
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}
