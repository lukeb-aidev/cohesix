// CLASSIFICATION: COMMUNITY
// Filename: test_boot_build_chain.rs v0.3
// Author: Lukas Bower
// Date Modified: 2026-12-31

use std::fs;

#[test]
fn build_script_messages_present() {
    let script = fs::read_to_string("cohesix_fetch_build.sh").expect("read build script");
    for msg in [
        "Kernel ELF staged",
        "Elfloader staged",
        "Booting elfloader + kernel in QEMU",
    ] {
        assert!(script.contains(msg), "missing log marker: {msg}");
    }
}
