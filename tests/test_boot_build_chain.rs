// CLASSIFICATION: COMMUNITY
// Filename: test_boot_build_chain.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-12-24

use std::fs;

#[test]
fn build_script_messages_present() {
    let script = fs::read_to_string("cohesix_fetch_build.sh").expect("read build script");
    for msg in [
        "kernel build complete",
        "EFI binary created",
        "config.yaml staged",
        "ISO successfully built",
    ] {
        assert!(script.contains(msg), "missing log marker: {msg}");
    }
}
