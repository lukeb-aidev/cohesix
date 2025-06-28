// CLASSIFICATION: COMMUNITY
// Filename: trace_validator_runtime.rs v0.2
// Date Modified: 2026-11-01
// Author: Lukas Bower

use std::fs;
use std::io::ErrorKind;

use cohesix::plan9::namespace::Namespace;
use cohesix::syscall::apply_ns;
use serial_test::serial;

#[test]
#[serial]
fn mount_permission_matrix() {
    fs::create_dir_all("/srv/violations").unwrap();
    unsafe {
        std::env::set_var("COHESIX_VIOLATIONS_DIR", "/srv/violations");
    }

    for role in ["QueenPrimary", "SensorRelay", "GlassesAgent"] {
        std::env::set_var("COHROLE", role);
        let mut ns = Namespace::default();
        let result = apply_ns(&mut ns);
        println!("Result under {}: {:?}", role, result);
        if role == "QueenPrimary" {
            assert!(result.is_ok(), "Expected success for {}", role);
        } else {
            assert!(
                matches!(result, Err(ref e) if e.kind() == ErrorKind::PermissionDenied),
                "Expected PermissionDenied for {}",
                role
            );
        }
    }
}
