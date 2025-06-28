// CLASSIFICATION: COMMUNITY
// Filename: test_validator.rs v0.2
// Date Modified: 2026-10-29
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
    if validate_syscall(role_enum, &Syscall::Exec { path: "/bin/true".into() }) {
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::PermissionDenied, "exec denied"))
    }
}

#[test]
fn validator_blocks_non_worker_spawn() {
    assert!(matches!(
        run_exec_as("DroneWorker"),
        Err(ref e) if e.kind() == std::io::ErrorKind::PermissionDenied
    ));

    if let Err(e) = run_exec_as("DroneWorker") {
        println!("Got expected error: {:?}", e);
    } else {
        panic!("Exec unexpectedly succeeded for DroneWorker");
    }
}
