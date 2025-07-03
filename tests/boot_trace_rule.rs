// CLASSIFICATION: COMMUNITY
// Filename: boot_trace_rule.rs v0.4
// Author: Cohesix Codex
// Date Modified: 2026-12-31

use cohesix::sandbox::validator::boot_must_succeed;
use serial_test::serial;
use std::fs;
use tempfile::tempdir;

#[test]
#[serial]
fn boot_trace_success() {
    let dir = tempdir().expect("tempdir creation failed");
    std::env::set_var("COHESIX_TRACE_TMP", dir.path());
    fs::write(
        dir.path().join("boot_trace.json"),
        "[{\"event\":\"boot_success\"}]",
    )
    .expect("unable to write boot trace file");
    assert!(boot_must_succeed());
    std::env::remove_var("COHESIX_TRACE_TMP");
}

#[test]
#[serial]
fn boot_trace_missing() {
    let dir = tempdir().expect("tempdir creation failed");
    std::env::set_var("COHESIX_TRACE_TMP", dir.path());
    assert!(!boot_must_succeed());
    std::env::remove_var("COHESIX_TRACE_TMP");
}

#[test]
#[serial]
fn boot_trace_invalid_json() {
    let dir = tempdir().expect("tempdir creation failed");
    std::env::set_var("COHESIX_TRACE_TMP", dir.path());
    fs::write(dir.path().join("boot_trace.json"), "not-json")
        .expect("unable to write boot trace file");
    assert!(!boot_must_succeed());
    std::env::remove_var("COHESIX_TRACE_TMP");
}

#[test]
#[serial]
fn detects_policy_failure() {
    let dir = tempdir().expect("tempdir creation failed");
    std::env::set_var("COHESIX_TRACE_TMP", dir.path());
    fs::write(
        dir.path().join("boot_trace.json"),
        "[{\"event\":\"policy_failure\"}]",
    )
    .expect("unable to write boot trace file");
    assert!(!boot_must_succeed());
    std::env::remove_var("COHESIX_TRACE_TMP");
}
