// CLASSIFICATION: COMMUNITY
// Filename: test_cohcc_output.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-09-25

use std::fs;
use tempfile::tempdir;

#[test]
fn compile_creates_binary() {
    let dir = tempdir().unwrap();
    let out = dir.path().join("out.bin");
    let bytes = cohesix::coh_cc::compile("/usr/src/example.coh").unwrap();
    fs::write(&out, &bytes).unwrap();
    let meta = fs::metadata(&out).unwrap();
    assert!(meta.len() > 0);
}
