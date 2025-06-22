// CLASSIFICATION: COMMUNITY
// Filename: test_boot_build_chain.rs v0.2
// Author: Lukas Bower
// Date Modified: 2026-07-08

use std::fs;

#[test]
fn build_script_messages_present() {
    let script = fs::read_to_string("cohesix_fetch_build.sh").expect("read build script");
    for msg in [
        "kernel build complete",
        "config.yaml staged",
        "ISO build complete",
    ] {
        assert!(script.contains(msg), "missing log marker: {msg}");
    }
}
