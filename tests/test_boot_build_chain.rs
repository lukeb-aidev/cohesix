// CLASSIFICATION: COMMUNITY
// Filename: test_boot_build_chain.rs v0.4
// Author: Lukas Bower
// Date Modified: 2027-02-01

use std::fs;

#[test]
fn build_script_messages_present() {
    let script = fs::read_to_string("cohesix_fetch_build.sh").expect("read build script");
    for msg in [
        "== Rust build ==",
        "== Go build ==",
        "BUILD AND STAGING COMPLETE",
    ] {
        assert!(script.contains(msg), "missing log marker: {msg}");
    }
}
