// CLASSIFICATION: COMMUNITY
// Filename: syscall.rs v0.5
// Author: Lukas Bower
// Date Modified: 2026-11-11

use crate::prelude::*;
use crate::cohesix_types::{Role, Syscall};
use crate::syscall::guard::check_permission;
use crate::validator::record_syscall;

/// Validate a syscall based on static role rules and guard defaults.
pub fn validate_syscall(role: Role, sc: &Syscall) -> bool {
    use Role::*;
    use Syscall::*;

    println!("Validator received syscall: {:?}", sc);

    let allowed = match (role.clone(), sc) {
        (
            QueenPrimary
            | RegionalQueen
            | BareMetalQueen,
            ApplyNamespace,
        ) => true,
        (
            DroneWorker
            | InteractiveAiBooth
            | KioskInteractive
            | GlassesAgent
            | SensorRelay
            | SimulatorTest,
            ApplyNamespace,
        ) => false,

        (
            QueenPrimary
            | RegionalQueen
            | BareMetalQueen
            | DroneWorker
            | InteractiveAiBooth
            | KioskInteractive
            | GlassesAgent
            | SimulatorTest,
            Mount { .. },
        ) => true,

        (
            QueenPrimary
            | RegionalQueen
            | BareMetalQueen
            | DroneWorker
            | InteractiveAiBooth
            | KioskInteractive
            | GlassesAgent
            | SimulatorTest,
            Exec { .. },
        ) => true,
        (SensorRelay, Exec { .. }) => false,

        _ => check_permission(role.clone(), sc),
    };

    if !allowed {
        println!(
            "Validator fallback deny: syscall {:?} not recognized for role {:?}",
            sc, role
        );
        log::warn!("syscall {:?} denied for {:?}", sc, role);
    }
    println!(
        "Validator: role={:?}, syscall={:?} -> {}",
        role, sc, allowed
    );
    record_syscall(sc);
    allowed
}
