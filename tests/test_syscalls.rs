// CLASSIFICATION: COMMUNITY
// Filename: test_syscalls.rs v0.2
// Date Modified: 2026-09-30
// Author: Cohesix Codex

use cohesix::cohesix_types::{Role, RoleManifest};
use cohesix::plan9::namespace::{Namespace, NamespaceLoader, NsOp};
use cohesix::plan9::syscalls;
use cohesix::security::capabilities;
use serial_test::serial;
use std::io::ErrorKind;
use tempfile::tempdir;

#[test]
#[serial]
fn open_read_write() {
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
    NamespaceLoader::apply(&mut ns).expect("apply ns");

    let role = RoleManifest::current_role();
    println!("Role: {:?}", role);
    let role_name: &str = match &role {
        Role::QueenPrimary => "QueenPrimary",
        Role::DroneWorker => "DroneWorker",
        Role::InteractiveAIBooth => "InteractiveAIBooth",
        Role::KioskInteractive => "KioskInteractive",
        Role::GlassesAgent => "GlassesAgent",
        Role::SensorRelay => "SensorRelay",
        Role::SimulatorTest => "SimulatorTest",
        Role::Other(name) => name,
    };
    let allow = capabilities::role_allows(role_name, "open", file_path.to_str().expect("path"));

    match syscalls::open(&ns, "/f") {
        Ok(mut f) => {
            assert!(
                allow,
                "open succeeded but role {role_name} should be denied"
            );
            let mut buf = Vec::new();
            syscalls::read(&mut f, &mut buf).expect("read");
            assert_eq!(buf, b"hello");
        }
        Err(e) => {
            if allow {
                panic!("open failed for allowed role {role_name}: {e}");
            } else {
                assert_eq!(
                    e.kind(),
                    ErrorKind::PermissionDenied,
                    "unexpected error: {e}"
                );
            }
        }
    }
}
