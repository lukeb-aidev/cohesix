// CLASSIFICATION: COMMUNITY
// Filename: trace_validator_runtime.rs v0.7
// Author: Lukas Bower
// Date Modified: 2026-11-10

use std::fs;
use std::io::{self, ErrorKind};

use cohesix::plan9::namespace::Namespace;
use cohesix::syscall::apply_ns;
use cohesix::cohesix_types::{RoleManifest, Syscall};
use cohesix::validator::syscall::validate_syscall;
use serial_test::serial;

fn attempt_apply_namespace() -> io::Result<()> {
    let mut ns = Namespace::default();
    apply_ns(&mut ns)
}

fn attempt_mount() -> io::Result<()> {
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

const ROLES: &[&str] = &[
    "QueenPrimary",
    "RegionalQueen",
    "BareMetalQueen",
    "DroneWorker",
    "InteractiveAiBooth",
    "KioskInteractive",
    "GlassesAgent",
    "SensorRelay",
    "SimulatorTest",
];

fn is_queen(role: &str) -> bool {
    matches!(role, "QueenPrimary" | "RegionalQueen" | "BareMetalQueen")
}

#[test]
#[serial]
fn apply_namespace_permission_matrix() {
    let tmp_dir = "/tmp/cohesix_test_violations";
    fs::create_dir_all(tmp_dir).expect(&format!("failed to create {}", tmp_dir));
    fs::metadata(tmp_dir).expect(&format!("expected {} to exist", tmp_dir));
    std::env::set_var("COHESIX_VIOLATIONS_DIR", tmp_dir);

    for &role in ROLES {
        std::env::set_var("COHROLE", role);
        let result = attempt_apply_namespace();
        println!("ApplyNamespace under {} -> {:?}", role, result);
        if is_queen(role) {
            assert!(result.is_ok(), "ApplyNamespace should succeed for {}", role);
        } else {
            assert!(
                matches!(result, Err(ref e) if e.kind() == ErrorKind::PermissionDenied),
                "ApplyNamespace should be denied for {}: {:?}",
                role,
                result
            );
        }
    }
}

#[test]
#[serial]
fn mount_permission_matrix() {
    let tmp_dir = "/tmp/cohesix_test_violations";
    fs::create_dir_all(tmp_dir).expect(&format!("failed to create {}", tmp_dir));
    fs::metadata(tmp_dir).expect(&format!("expected {} to exist", tmp_dir));
    std::env::set_var("COHESIX_VIOLATIONS_DIR", tmp_dir);

    for &role in ROLES {
        std::env::set_var("COHROLE", role);
        let result = attempt_mount();
        println!("Mount under {} -> {:?}", role, result);
        if is_queen(role) || role == "DroneWorker" {
            assert!(result.is_ok(), "Mount should succeed for {}", role);
        } else {
            assert!(
                matches!(result, Err(ref e) if e.kind() == ErrorKind::PermissionDenied),
                "Mount should be denied for {}: {:?}",
                role,
                result
            );
        }
    }
}
