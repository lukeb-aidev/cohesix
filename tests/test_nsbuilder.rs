// CLASSIFICATION: COMMUNITY
// Filename: test_nsbuilder.rs v0.1
// Date Modified: 2025-06-17
// Author: Cohesix Codex

use cohesix::boot::plan9_ns::{BootArgs, build_namespace};

#[test]
fn namespace_includes_core_paths() {
    let args = BootArgs { rootfs: "/".into(), role: "QueenPrimary".into(), srv: vec![] };
    let ns = build_namespace(&args);
    let text = ns.to_string();
    assert!(text.contains("bind / /"));
    assert!(text.contains("bind -a /bin /bin"));
    assert!(text.contains("srv /srv"));
}

#[test]
fn bind_overlay_parsed() {
    let args = BootArgs { rootfs: "/root".into(), role: String::new(), srv: vec![] };
    let ns = build_namespace(&args);
    let parsed = cohesix::boot::plan9_ns::parse_namespace(&ns.to_string());
    assert_eq!(ns.actions(), parsed.actions());
}

#[test]
fn missing_paths_handled() {
    let args = BootArgs { rootfs: "/nonexistent".into(), role: String::new(), srv: vec![] };
    let ns = build_namespace(&args);
    // Should still include the bind entry even if the path does not exist
    assert!(ns.to_string().contains("/nonexistent"));
}

