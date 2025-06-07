// CLASSIFICATION: COMMUNITY
// Filename: test_namespace_semantics.rs v0.1
// Date Modified: 2025-06-22
// Author: Cohesix Codex

use cohesix::plan9::namespace::{NamespaceLoader, NsOp, BindFlags};
use cohesix::plan9::namespace::Namespace;
use cohesix::plan9::srv_registry::{register_srv, lookup_srv};
use serial_test::serial;

#[test]
#[serial]
fn bind_overlay_order() {
    let mut ns = Namespace { ops: vec![], private: true, root: Default::default() };
    ns.add_op(NsOp::Mount { srv: "/tmp/src".into(), dst: "/a".into() });
    let mut f = BindFlags::default();
    f.after = true;
    ns.add_op(NsOp::Bind { src: "/a".into(), dst: "/b".into(), flags: f });
    NamespaceLoader::apply(&mut ns).unwrap();
    assert_eq!(ns.root.get_or_create("/b").mounts.len(), 1);
}

#[test]
#[serial]
fn srv_registration() {
    register_srv("telemetry", 1);
    assert!(lookup_srv("telemetry").is_some());
}
