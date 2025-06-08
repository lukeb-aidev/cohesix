// CLASSIFICATION: COMMUNITY
// Filename: cohrun.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-11

use clap::{Parser, Subcommand};
use cohesix::sim::{physics_demo, webcam_tilt};
use cohesix::webcam::capture;

#[derive(Parser)]
#[command(name = "cohrun", about = "Run Cohesix demo scenarios", version = "0.1")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    PhysicsDemo,
    TestWebcam,
    WebcamTilt,
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
    }
}
