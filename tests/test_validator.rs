// CLASSIFICATION: COMMUNITY
// Filename: test_validator.rs v0.4
// Date Modified: 2026-11-13
// Author: Cohesix Codex

use cohesix::cohesix_types::{Role, Syscall};
use cohesix::validator::syscall::validate_syscall;
use std::fs;
use std::io;

fn run_exec_as(role: &str) -> io::Result<()> {
    fs::write("/srv/cohrole", role)?;
    let role_enum = match role {
        "QueenPrimary" => Role::QueenPrimary,
        "RegionalQueen" => Role::RegionalQueen,
        "BareMetalQueen" => Role::BareMetalQueen,
        "DroneWorker" => Role::DroneWorker,
        "InteractiveAiBooth" => Role::InteractiveAiBooth,
        "KioskInteractive" => Role::KioskInteractive,
        "GlassesAgent" => Role::GlassesAgent,
        "SensorRelay" => Role::SensorRelay,
        "SimulatorTest" => Role::SimulatorTest,
        other => Role::Other(other.to_string()),
    };
    const EXEC_PATH: &str = "/bin/sh";
    println!("Using exec path: {}", EXEC_PATH);
    if validate_syscall(role_enum, &Syscall::Exec { path: EXEC_PATH.into() }) {
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::PermissionDenied, "exec denied"))
    }
}

#[test]
fn validator_allows_worker_exec() {
    if let Err(e) = run_exec_as("DroneWorker") {
        panic!("Exec unexpectedly denied for DroneWorker: {}", e);
    }
}
