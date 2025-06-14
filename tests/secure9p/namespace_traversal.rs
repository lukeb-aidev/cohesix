// CLASSIFICATION: COMMUNITY
// Filename: namespace_traversal.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-25

use cohesix::p9::secure::namespace_resolver::resolve_namespace;
use serial_test::serial;
use tempfile::tempdir;
use std::fs;
use std::env;

#[test]
#[serial]
fn namespace_traversal() {
    let dir = tempdir().unwrap();
    let cfg_dir = dir.path().join("config");
    fs::create_dir(&cfg_dir).unwrap();
    let root = dir.path().join("ns");
    fs::create_dir(&root).unwrap();
    fs::write(
        cfg_dir.join("secure9p.toml"),
        format!(
            "\n[[namespace]]\nagent='tester'\nroot='{}'\nread_only=false\n",
            root.display()
        ),
    )
    .unwrap();
    let prev = env::current_dir().unwrap();
    env::set_current_dir(&dir).unwrap();
    let ns = resolve_namespace("tester").unwrap();
    env::set_current_dir(prev).unwrap();
    assert_eq!(ns.root, root);
}
