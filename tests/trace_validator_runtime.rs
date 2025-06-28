// CLASSIFICATION: COMMUNITY
// Filename: trace_validator_runtime.rs v0.6
// Date Modified: 2026-11-09
// Author: Lukas Bower

use std::fs;
use std::io::{self, ErrorKind};

use cohesix::plan9::namespace::Namespace;
use cohesix::syscall::apply_ns;
use cohesix::cohesix_types::{RoleManifest, Syscall};
use cohesix::validator::syscall::validate_syscall;
use serial_test::serial;

fn attempt_apply_namespace() -> std::io::Result<()> {
    let mut ns = Namespace::default();
    apply_ns(&mut ns)
}

fn attempt_mount() -> std::io::Result<()> {
    let role = RoleManifest::current_role();
    let allowed = validate_syscall(
        role,
        &Syscall::Mount {
            src: "dummy".into(),
            dest: "dummy".into(),
        },
    );
    if allowed {
        Ok(())
    } else {
        Err(io::Error::new(ErrorKind::PermissionDenied, "mount denied"))
    }
}

#[test]
#[serial]
fn mount_permission_matrix() {
    fs::create_dir_all("/srv/violations").unwrap();
    unsafe {
        std::env::set_var("COHESIX_VIOLATIONS_DIR", "/srv/violations");
    }

    for role in [
        "QueenPrimary",
        "RegionalQueen",
        "BareMetalQueen",
        "SimulatorTest",
        "SensorRelay",
    ] {
        std::env::set_var("COHROLE", role);
        let result = attempt_mount();
        println!("Mount result under {}: {:?}", role, result);

        if role.contains("Queen") || role == "SimulatorTest" {
            assert!(result.is_ok(), "Mount should succeed for {}", role);
        } else {
            assert!(
                matches!(result, Err(ref e) if e.kind() == std::io::ErrorKind::PermissionDenied),
                "Mount denied for {}",
                role
            );
        }
    }
}

#[test]
#[serial]
fn apply_namespace_permission_matrix() {
    fs::create_dir_all("/srv/violations").unwrap();
    unsafe {
        std::env::set_var("COHESIX_VIOLATIONS_DIR", "/srv/violations");
    }

    for role in [
        "QueenPrimary",
        "RegionalQueen",
        "BareMetalQueen",
        "SimulatorTest",
        "SensorRelay",
        "DroneWorker",
        "InteractiveAiBooth",
        "KioskInteractive",
        "GlassesAgent",
    ] {
        std::env::set_var("COHROLE", role);
        let result = attempt_apply_namespace();
        println!("ApplyNamespace result under {}: {:?}", role, result);

        if role.contains("Queen") {
            assert!(result.is_ok(), "ApplyNamespace should succeed for {}", role);
        } else {
            assert!(
                matches!(result, Err(ref e) if e.kind() == std::io::ErrorKind::PermissionDenied),
                "ApplyNamespace denied for {}, got {:?}",
                role,
                result
            );
        }
    }
}
