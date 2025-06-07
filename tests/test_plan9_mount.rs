// CLASSIFICATION: COMMUNITY
// Filename: test_plan9_mount.rs v0.1
// Date Modified: 2025-06-20
// Author: Cohesix Codex

use cohesix::kernel::fs::plan9::{mount, mount_count, reset_mounts};
use serial_test::serial;

#[test]
#[serial]
fn mount_capacity_limit() {
    reset_mounts();
    for i in 0..8 {
        let path = format!("/srv/test{}", i);
        let static_path: &'static str = Box::leak(path.into_boxed_str());
        assert!(mount("/tmp", static_path));
    }
    assert!(!mount("/tmp", "/srv/overflow"));
    assert_eq!(mount_count(), 8);
}

