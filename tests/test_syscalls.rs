// CLASSIFICATION: COMMUNITY
// Filename: test_syscalls.rs v0.1
// Date Modified: 2025-06-22
// Author: Cohesix Codex

use cohesix::plan9::namespace::{Namespace, NsOp, NamespaceLoader};
use cohesix::plan9::syscalls;
use tempfile::tempdir;
use serial_test::serial;

#[test]
#[serial]
fn open_read_write() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("file.txt");
    std::fs::write(&file_path, b"hello").unwrap();

    let mut ns = Namespace { ops: vec![], private: true, root: Default::default() };
    ns.add_op(NsOp::Mount { srv: file_path.to_str().unwrap().into(), dst: "/f".into() });
    NamespaceLoader::apply(&mut ns).unwrap();

    let mut f = syscalls::open(&ns, "/f").unwrap();
    let mut buf = Vec::new();
    syscalls::read(&mut f, &mut buf).unwrap();
    assert_eq!(buf, b"hello");
}
