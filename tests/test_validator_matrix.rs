// CLASSIFICATION: COMMUNITY
// Filename: test_validator_matrix.rs v0.2
// Author: Lukas Bower
// Date Modified: 2026-11-02

use cohesix::cohesix_types::{Role, Syscall, VALID_ROLES};
use cohesix::syscall::guard::{SyscallOp, PERMISSIONS};
use cohesix::validator::syscall::validate_syscall;
use std::io::ErrorKind;

#[test]
fn validator_matrix_coverage() {
    let syscalls = vec![
        (
            SyscallOp::Spawn,
            Syscall::Spawn {
                program: "p".into(),
                args: vec![],
            },
        ),
        (
            SyscallOp::CapGrant,
            Syscall::CapGrant {
                target: "t".into(),
                capability: "c".into(),
            },
        ),
        (
            SyscallOp::Mount,
            Syscall::Mount {
                src: "s".into(),
                dest: "d".into(),
            },
        ),
        (
            SyscallOp::Exec,
            Syscall::Exec {
                path: "/bin/true".into(),
            },
        ),
        (SyscallOp::ApplyNs, Syscall::ApplyNamespace),
    ];

    for role_name in VALID_ROLES {
        let role = match *role_name {
            "QueenPrimary" => Role::QueenPrimary,
            "RegionalQueen" => Role::RegionalQueen,
            "BareMetalQueen" => Role::BareMetalQueen,
            "DroneWorker" => Role::DroneWorker,
            "InteractiveAiBooth" => Role::InteractiveAiBooth,
            "KioskInteractive" => Role::KioskInteractive,
            "GlassesAgent" => Role::GlassesAgent,
            "SensorRelay" => Role::SensorRelay,
            "SimulatorTest" => Role::SimulatorTest,
            other => Role::Other(other.into()),
        };

        for (op, sc) in &syscalls {
            let expect_allowed = PERMISSIONS.get(&role).map_or(false, |set| set.contains(op));
            let result = if validate_syscall(role.clone(), sc) {
                Ok(())
            } else {
                Err(std::io::Error::new(ErrorKind::PermissionDenied, "denied"))
            };
            if expect_allowed {
                assert!(result.is_ok(), "Expected Ok(()) for {:?} {:?}", role, op);
            } else {
                assert!(
                    matches!(result, Err(ref e) if e.kind() == ErrorKind::PermissionDenied),
                    "Expected PermissionDenied for {:?} {:?}",
                    role,
                    op
                );
            }
        }
    }
}
