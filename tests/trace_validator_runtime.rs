// CLASSIFICATION: COMMUNITY
// Filename: trace_validator_runtime.rs v0.9
// Author: Lukas Bower
// Date Modified: 2026-11-12

use std::fs;
use std::io::{self, ErrorKind};

use cohesix::plan9::namespace::Namespace;
use cohesix::syscall::apply_ns;
use tempfile;
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

fn attempt_exec() -> io::Result<()> {
    let role = RoleManifest::current_role();
    let allowed = validate_syscall(role, &Syscall::Exec { path: "dummy".into() });
    if allowed {
        Ok(())
    } else {
        Err(io::Error::new(ErrorKind::PermissionDenied, "exec denied"))
    }
}

fn attempt_overlay_apply() -> io::Result<()> {
    use cohesix::plan9::namespace::{NsOp, BindFlags};
    let tmp = tempfile::tempdir()?;
    let src = tmp.path().join("src");
    fs::create_dir_all(&src)?;
    let mut ns = Namespace { ops: vec![], private: true, root: Default::default() };
    ns.add_op(NsOp::Mount { srv: src.to_string_lossy().into(), dst: "/a".into() });
    let flags = BindFlags { after: true, ..Default::default() };
    ns.add_op(NsOp::Bind { src: "/a".into(), dst: "/b".into(), flags });
    apply_ns(&mut ns)
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
    let tmp_dir = std::env::temp_dir().join("cohesix_test_violations");
    fs::create_dir_all(&tmp_dir).expect(&format!("failed to create {:?}", tmp_dir));
    std::env::set_var("COHESIX_VIOLATIONS_DIR", &tmp_dir);

    for &role in ROLES {
        std::env::set_var("COHROLE", role);
        let result = attempt_apply_namespace();
        println!("ApplyNamespace under {} -> {:?}", role, result);
    }
}

#[test]
#[serial]
fn mount_permission_matrix() {
    let tmp_dir = std::env::temp_dir().join("cohesix_test_violations");
    fs::create_dir_all(&tmp_dir).expect(&format!("failed to create {:?}", tmp_dir));
    std::env::set_var("COHESIX_VIOLATIONS_DIR", &tmp_dir);

    for &role in ROLES {
        std::env::set_var("COHROLE", role);
        let result = attempt_mount();
        println!("Mount under {} -> {:?}", role, result);
    }
}

#[test]
#[serial]
fn exec_permission_matrix() {
    let tmp_dir = std::env::temp_dir().join("cohesix_test_violations");
    fs::create_dir_all(&tmp_dir).expect(&format!("failed to create {:?}", tmp_dir));
    std::env::set_var("COHESIX_VIOLATIONS_DIR", &tmp_dir);

    for &role in ROLES {
        std::env::set_var("COHROLE", role);
        let result = attempt_exec();
        println!("Exec under {} -> {:?}", role, result);
    }
}

#[test]
#[serial]
fn overlay_apply_matrix() {
    let tmp_dir = std::env::temp_dir().join("cohesix_test_violations");
    fs::create_dir_all(&tmp_dir).expect(&format!("failed to create {:?}", tmp_dir));
    std::env::set_var("COHESIX_VIOLATIONS_DIR", &tmp_dir);

    for &role in ROLES {
        std::env::set_var("COHROLE", role);
        let result = attempt_overlay_apply();
        println!("Overlay apply under {} -> {:?}", role, result);
    }
}
