// CLASSIFICATION: COMMUNITY
// Filename: role_config.rs v0.1
// Author: Lukas Bower
// Date Modified: 2025-08-30

use cohesix::runtime::role_config::{load_active, RoleConfig};
use serial_test::serial;
use std::fs;
use tempfile::tempdir;

#[test]
#[serial]
fn load_specific_role() {
    let dir = tempdir().unwrap();
    let etc = dir.path().join("etc_roles");
    let srv = dir.path().join("srv");
    fs::create_dir_all(&etc).unwrap();
    fs::create_dir_all(&srv).unwrap();
    fs::write(srv.join("cohrole"), "QueenPrimary").unwrap();
    fs::write(
        etc.join("QueenPrimary.yaml"),
        "telemetry_interval: 5\ntrace_policy: q\nvalidator: true\n",
    )
    .unwrap();
    fs::write(
        etc.join("default.yaml"),
        "telemetry_interval: 1\ntrace_policy: d\nvalidator: false\n",
    )
    .unwrap();
    std::env::set_var("COHROLE_PATH", srv.join("cohrole"));
    std::env::set_var("ROLE_CONFIG_DIR", &etc);
    let cfg = load_active();
    std::env::remove_var("COHROLE_PATH");
    std::env::remove_var("ROLE_CONFIG_DIR");
    assert_eq!(cfg.telemetry_interval, Some(5));
    assert_eq!(cfg.validator, Some(true));
}

#[test]
#[serial]
fn fallback_to_default() {
    let dir = tempdir().unwrap();
    let etc = dir.path().join("etc_roles");
    let srv = dir.path().join("srv");
    fs::create_dir_all(&etc).unwrap();
    fs::create_dir_all(&srv).unwrap();
    fs::write(srv.join("cohrole"), "UnknownRole").unwrap();
    fs::write(
        etc.join("default.yaml"),
        "telemetry_interval: 7\ntrace_policy: def\nvalidator: false\n",
    )
    .unwrap();
    std::env::set_var("COHROLE_PATH", srv.join("cohrole"));
    std::env::set_var("ROLE_CONFIG_DIR", &etc);
    let cfg = load_active();
    std::env::remove_var("COHROLE_PATH");
    std::env::remove_var("ROLE_CONFIG_DIR");
    assert_eq!(cfg.telemetry_interval, Some(7));
    assert_eq!(cfg.validator, Some(false));
}
