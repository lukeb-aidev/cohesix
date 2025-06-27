// CLASSIFICATION: COMMUNITY
// Filename: test_validator_matrix.rs v0.1
// Author: Lukas Bower
// Date Modified: 2026-09-30

use cohesix::cohesix_types::{Role, Syscall, VALID_ROLES};
use cohesix::validator::syscall::validate_syscall;

#[test]
fn validator_matrix_coverage() {
    let syscalls = vec![
        Syscall::Spawn {
            program: "p".into(),
            args: vec![],
        },
        Syscall::CapGrant {
            target: "t".into(),
            capability: "c".into(),
        },
        Syscall::Mount {
            src: "s".into(),
            dest: "d".into(),
        },
        Syscall::Exec {
            path: "/bin/true".into(),
        },
        Syscall::ApplyNamespace,
        Syscall::Unknown,
    ];

    for role_name in VALID_ROLES {
        let role = match *role_name {
            "QueenPrimary" => Role::QueenPrimary,
            "DroneWorker" => Role::DroneWorker,
            "InteractiveAIBooth" => Role::InteractiveAIBooth,
            "KioskInteractive" => Role::KioskInteractive,
            "GlassesAgent" => Role::GlassesAgent,
            "SensorRelay" => Role::SensorRelay,
            "SimulatorTest" => Role::SimulatorTest,
            _ => continue,
        };

        for sc in &syscalls {
            let _ = validate_syscall(role.clone(), sc);
        }
    }
}
