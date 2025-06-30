// CLASSIFICATION: COMMUNITY
// Filename: boot_role.rs v0.1
// Author: Lukas Bower
// Date Modified: 2026-12-02

use cohesix::runtime::env::init::load_role_setting;
use tempfile::tempdir;
use std::fs;

#[test]
fn env_override() {
    std::env::set_var("CohRole", "SensorRelay");
    std::env::set_var("ROLE_CONF_PATH", "/nonexistent");
    let role = load_role_setting();
    std::env::remove_var("CohRole");
    std::env::remove_var("ROLE_CONF_PATH");
    assert_eq!(role, Some("SensorRelay".to_string()));
}

#[test]
fn file_load() {
    let dir = tempdir().unwrap();
    let conf = dir.path().join("role.conf");
    fs::write(&conf, "CohRole=KioskInteractive").unwrap();
    std::env::set_var("ROLE_CONF_PATH", &conf);
    let role = load_role_setting();
    std::env::remove_var("ROLE_CONF_PATH");
    assert_eq!(role, Some("KioskInteractive".to_string()));
}
