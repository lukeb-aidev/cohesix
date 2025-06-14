// CLASSIFICATION: COMMUNITY
// Filename: boot_trace_rule.rs v0.1
// Author: Cohesix Codex
// Date Modified: 2025-07-23

use cohesix::sandbox::validator::boot_must_succeed;
use std::fs;
use serial_test::serial;

#[test]
#[serial]
fn boot_trace_success() {
    fs::create_dir_all("/trace").unwrap();
    fs::write("/trace/boot_trace.json", "[{\"event\":\"boot_success\"}]").unwrap();
    assert!(boot_must_succeed());
}

#[test]
#[serial]
fn boot_trace_missing() {
    let _ = fs::remove_file("/trace/boot_trace.json");
    fs::create_dir_all("/trace").unwrap();
    assert!(!boot_must_succeed());
}

#[test]
#[serial]
fn boot_trace_invalid_json() {
    fs::create_dir_all("/trace").unwrap();
    fs::write("/trace/boot_trace.json", "not-json").unwrap();
    assert!(!boot_must_succeed());
}
