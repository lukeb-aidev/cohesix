// CLASSIFICATION: COMMUNITY
// Filename: test_syscalls.rs v0.5
// Date Modified: 2026-09-30
// Author: Cohesix Codex

use cohesix::cohesix_types::RoleManifest;
use cohesix::plan9::namespace::{Namespace, NsOp};
use cohesix::syscall::ns::apply_ns;
use cohesix::plan9::syscalls;
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
}

#[test]
#[serial]
fn file_rw_allowed_for_queen() {
    let prev = std::env::var("COHROLE").ok();
    let _ = std::fs::remove_file("/srv/cohrole");
    std::env::set_var("COHROLE", "QueenPrimary");

    let dir = tempdir().expect("tempdir");
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
    println!("Running as role: {:?}", role);
    match apply_ns(&mut ns) {
        Ok(_) => {}
        Err(e) => {
            match prev {
                Some(v) => std::env::set_var("COHROLE", v),
                None => std::env::remove_var("COHROLE"),
            }
            panic!("Queen namespace apply failed: {e}");
        }
    }

    let mut f = match syscalls::open(&ns, "/f") {
        Ok(f) => f,
        Err(e) => {
            match prev {
                Some(v) => std::env::set_var("COHROLE", v),
                None => std::env::remove_var("COHROLE"),
            }
            panic!("open failed: {e}");
        }
    };
    let mut buf = Vec::new();
    syscalls::read(&mut f, &mut buf).expect("read");
    assert_eq!(buf, b"hello");

    match prev {
        Some(v) => std::env::set_var("COHROLE", v),
        None => std::env::remove_var("COHROLE"),
    }
}
