// CLASSIFICATION: COMMUNITY
// Filename: test_namespace_semantics.rs v0.2
// Date Modified: 2025-12-15
// Author: Cohesix Codex

use cohesix::plan9::namespace::{NamespaceLoader, NsOp, BindFlags};
use cohesix::plan9::namespace::Namespace;
use cohesix::plan9::srv_registry::{register_srv, lookup_srv};
use serial_test::serial;

#[test]
#[serial]
fn bind_overlay_order() {
    use std::fs::{self, File};
    use std::path::Path;
    let uid = unsafe { libc::geteuid() };

    let srv_path = Path::new("/srv");
    if !srv_path.exists() {
        if let Err(e) = fs::create_dir_all(srv_path) {
            eprintln!(
                "[bind_overlay_order] skipping: cannot create /srv for uid {}: {}",
                uid, e
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
                "[bind_overlay_order] skipping: /srv not writable for uid {}: {}",
                uid, e
            );
            return;
        }
    }

    let dir = tempfile::tempdir().expect("create temp dir");
    let src = dir.path().join("src");
    fs::create_dir_all(&src).expect("create src dir");

    let mut ns = Namespace { ops: vec![], private: true, root: Default::default() };
    ns.add_op(NsOp::Mount {
        srv: src.to_str().expect("utf8 path").into(),
        dst: "/a".into(),
    });
    let f = BindFlags { after: true, ..Default::default() };
    ns.add_op(NsOp::Bind { src: "/a".into(), dst: "/b".into(), flags: f });
    NamespaceLoader::apply(&mut ns).unwrap_or_else(|_| panic!(
        "apply failed for uid {} on {:?}",
        uid, srv_path
    ));
    assert_eq!(ns.root.get_or_create("/b").mounts.len(), 1);
}

#[test]
#[serial]
fn srv_registration() {
    register_srv("telemetry", 1);
    assert!(lookup_srv("telemetry").is_some());
}
