// CLASSIFICATION: COMMUNITY
// Filename: boot_trace_rule.rs v0.2
// Author: Cohesix Codex
// Date Modified: 2025-08-02

use cohesix::sandbox::validator::boot_must_succeed;
use std::fs;
use serial_test::serial;

#[test]
#[serial]
fn boot_trace_success() {
    let dir = std::env::temp_dir().join("boot_trace_rule");
    fs::create_dir_all(&dir)
        .expect("Could not open boot trace file -- check test sandbox permissions");
    std::env::set_var("COHESIX_TRACE_TMP", &dir);
    fs::write(dir.join("boot_trace.json"), "[{\"event\":\"boot_success\"}]")
        .expect("Could not open boot trace file -- check test sandbox permissions");
    assert!(boot_must_succeed());
}

#[test]
#[serial]
fn boot_trace_missing() {
    let dir = std::env::temp_dir().join("boot_trace_rule");
    let path = dir.join("boot_trace.json");
    let _ = fs::remove_file(&path);
    fs::create_dir_all(&dir)
        .expect("Could not open boot trace file -- check test sandbox permissions");
    std::env::set_var("COHESIX_TRACE_TMP", &dir);
    assert!(!boot_must_succeed());
}

#[test]
#[serial]
fn boot_trace_invalid_json() {
    let dir = std::env::temp_dir().join("boot_trace_rule");
    fs::create_dir_all(&dir)
        .expect("Could not open boot trace file -- check test sandbox permissions");
    std::env::set_var("COHESIX_TRACE_TMP", &dir);
    fs::write(dir.join("boot_trace.json"), "not-json")
        .expect("Could not open boot trace file -- check test sandbox permissions");
    assert!(!boot_must_succeed());
}
