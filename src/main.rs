// CLASSIFICATION: COMMUNITY
// Filename: main.rs v1.0
// Date Modified: 2025-05-27
// Author: Lukas Bower
// Status: 🟢 Hydrated

//! Entry point for the Coh_CC compiler binary.

extern crate cohesix;
use cohesix::cli;
use env_logger;

fn main() {
    env_logger::init();
    if let Err(err) = cli::run() {
        eprintln!("Error: {}", err);
        std::process::exit(1);
    }
}
