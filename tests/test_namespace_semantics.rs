// CLASSIFICATION: COMMUNITY
// Filename: test_namespace_semantics.rs v0.5
// Date Modified: 2026-12-31
// Author: Cohesix Codex

use cohesix::plan9::namespace::Namespace;
use cohesix::plan9::namespace::{BindFlags, NsOp};
use cohesix::plan9::srv_registry::{lookup_srv, register_srv};
use cohesix::syscall::apply_ns;
use serial_test::serial;

#[test]
#[serial]
fn bind_overlay_order() {
    use std::fs::{self, File};
    use std::path::Path;
    use std::{env, io::ErrorKind};

    let srv_path = Path::new("/srv");
    if !srv_path.exists() {
        if let Err(e) = fs::create_dir_all(srv_path) {
            eprintln!(
                "[bind_overlay_order] skipping: cannot create /srv: {}",
                e
            );
            return;
        }
    }
    let test_file = srv_path.join("cohesix_test_write");
    match File::create(&test_file) {
        Ok(_) => {
            let _ = fs::remove_file(&test_file);
        }
        Err(e) => {
            eprintln!(
                "[bind_overlay_order] skipping: /srv not writable: {}",
                e
            );
            return;
        }
    }

    let dir = tempfile::tempdir().expect("create temp dir");
    let src = dir.path().join("src");
    fs::create_dir_all(&src).expect("create src dir");

    let mut ns = Namespace {
        ops: vec![],
        private: true,
        root: Default::default(),
    };
    ns.add_op(NsOp::Mount {
        srv: src.to_str().expect("utf8 path").into(),
        dst: "/a".into(),
    });
    let f = BindFlags {
        after: true,
        ..Default::default()
    };
    ns.add_op(NsOp::Bind {
        src: "/a".into(),
        dst: "/b".into(),
        flags: f,
    });

    // set role to QueenPrimary and expect success
    let prev = env::var("COHROLE").ok();
    env::set_var("COHROLE", "QueenPrimary");
    fs::write("/srv/cohrole", "QueenPrimary").ok();
    match apply_ns(&mut ns) {
        Ok(_) => assert_eq!(ns.root.get_or_create("/b").mounts.len(), 1),
        Err(e) => panic!("apply_ns failed on {:?}: {}", srv_path, e),
    }

    // subtest: DroneWorker should be denied
    env::set_var("COHROLE", "DroneWorker");
    fs::write("/srv/cohrole", "DroneWorker").ok();
    let mut ns_denied = ns.clone();
    match apply_ns(&mut ns_denied) {
        Err(ref e) if e.kind() == ErrorKind::PermissionDenied => (),
        Err(e) => panic!("unexpected error: {}", e),
        Ok(_) => panic!("expected PermissionDenied for DroneWorker"),
    }

    match prev {
        Some(v) => {
            env::set_var("COHROLE", &v);
            fs::write("/srv/cohrole", v).ok();
        }
        None => {
            env::remove_var("COHROLE");
            let _ = fs::remove_file("/srv/cohrole");
        }
    }
}

#[test]
#[serial]
fn srv_registration() {
    register_srv("telemetry", 1);
    assert!(lookup_srv("telemetry").is_some());
}
