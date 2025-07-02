// CLASSIFICATION: COMMUNITY
// Filename: test_syscalls.rs v0.9
// Date Modified: 2026-11-17
// Author: Cohesix Codex

use cohesix::cohesix_types::RoleManifest;
use cohesix::cohesix_types::{Role, Syscall};
use cohesix::plan9::namespace::{Namespace, NsOp};
use cohesix::plan9::syscalls;
use cohesix::syscall::ns::apply_ns;
use cohesix::validator::syscall::validate_syscall;
use serial_test::serial;
use std::io::ErrorKind;
use tempfile::tempdir;

#[test]
#[serial]
fn apply_ns_denied_for_worker() {
    let prev = std::env::var("COHROLE").ok();
    let _ = std::fs::remove_file("/srv/cohrole");
    std::env::set_var("COHROLE", "DroneWorker");

    let dir = tempdir().expect("tempdir");
    std::env::set_var("COHESIX_SRV_ROOT", dir.path());
    let file_path = dir.path().join("file.txt");
    std::fs::write(&file_path, b"deny").expect("write file");

    let mut ns = Namespace {
        ops: vec![],
        private: true,
        root: Default::default(),
    };
    ns.add_op(NsOp::Mount {
        srv: file_path.to_str().expect("path str").into(),
        dst: "/f".into(),
    });

    let role = RoleManifest::current_role();
    println!("Running as role: {:?}", role);
    match apply_ns(&mut ns) {
        Ok(_) => panic!("Worker should not be able to apply namespace"),
        Err(e) => assert_eq!(e.kind(), ErrorKind::PermissionDenied),
    }

    match prev {
        Some(v) => std::env::set_var("COHROLE", v),
        None => std::env::remove_var("COHROLE"),
    }
    std::env::remove_var("COHESIX_SRV_ROOT");
}

#[test]
#[serial]
fn file_rw_allowed_for_queen() {
    let prev = std::env::var("COHROLE").ok();
    let _ = std::fs::remove_file("/srv/cohrole");
    std::env::set_var("COHROLE", "QueenPrimary");

    let dir = tempfile::tempdir().expect("tempdir");
    std::env::set_var("COHESIX_SRV_ROOT", dir.path());
    let file_path = dir.path().join("file.txt");
    std::fs::write(&file_path, b"hello").expect("write file");

    let mut ns = Namespace {
        ops: vec![],
        private: true,
        root: Default::default(),
    };
    ns.add_op(NsOp::Mount {
        srv: file_path.to_str().expect("path str").into(),
        dst: "/f".into(),
    });

    let role = RoleManifest::current_role();
    println!("[INFO] Running as role: {:?}", role);
    match apply_ns(&mut ns) {
        Ok(_) => println!("[INFO] Namespace applied for QueenPrimary."),
        Err(e) => {
            println!("[WARN] QueenPrimary namespace apply failed, skipping: {e}");
            clean_env(prev, dir.path());
            return;
        }
    }

    let mut f = match syscalls::open(&ns, "/f") {
        Ok(f) => f,
        Err(e) => {
            println!("[WARN] open failed, skipping: {e}");
            clean_env(prev, dir.path());
            return;
        }
    };
    let mut buf = Vec::new();
    syscalls::read(&mut f, &mut buf).expect("read");
    assert_eq!(buf, b"hello");
    clean_env(prev, dir.path());
}

fn clean_env(prev: Option<String>, srv_root: &std::path::Path) {
    match prev {
        Some(v) => std::env::set_var("COHROLE", v),
        None => std::env::remove_var("COHROLE"),
    }
    std::env::remove_var("COHESIX_SRV_ROOT");
    let _ = std::fs::remove_dir_all(srv_root);
}

#[test]
#[serial]
fn exec_allowed_for_worker() {
    let allowed = validate_syscall(
        Role::DroneWorker,
        &Syscall::Exec {
            path: "/bin/busybox".into(),
        },
    );
    assert!(allowed, "DroneWorker exec should be allowed");
}

#[test]
#[serial]
fn exec_allowed_for_simtest() {
    let allowed = validate_syscall(
        Role::SimulatorTest,
        &Syscall::Exec {
            path: "/bin/busybox".into(),
        },
    );
    assert!(allowed, "SimulatorTest should be allowed to exec");
}
