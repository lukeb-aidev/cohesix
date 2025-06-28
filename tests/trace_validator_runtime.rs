// CLASSIFICATION: COMMUNITY
// Filename: trace_validator_runtime.rs v0.3
// Date Modified: 2026-11-06
// Author: Lukas Bower

use std::fs;

use cohesix::plan9::namespace::Namespace;
use cohesix::syscall::apply_ns;
use serial_test::serial;

fn attempt_mount_or_namespace_op() -> std::io::Result<()> {
    let mut ns = Namespace::default();
    apply_ns(&mut ns)
}

#[test]
#[serial]
fn mount_permission_policy_matrix() {
    fs::create_dir_all("/srv/violations").unwrap();
    unsafe {
        std::env::set_var("COHESIX_VIOLATIONS_DIR", "/srv/violations");
    }

    for role in [
        "QueenPrimary",
        "RegionalQueen",
        "BareMetalQueen",
        "SensorRelay",
        "SimulatorTest",
    ] {
        std::env::set_var("COHROLE", role);
        let result = attempt_mount_or_namespace_op();
        println!("Mount result under {}: {:?}", role, result);
        if role.contains("Queen") {
            assert!(result.is_ok(), "Expected success for role {}", role);
        } else {
            assert!(
                matches!(result, Err(ref e) if e.kind() == std::io::ErrorKind::PermissionDenied),
                "Expected PermissionDenied for role {}",
                role
            );
        }
    }
}
