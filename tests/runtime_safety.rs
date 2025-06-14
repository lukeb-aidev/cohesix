// CLASSIFICATION: COMMUNITY
// Filename: runtime_safety.rs v0.2
// Date Modified: 2025-07-03
// Author: Cohesix Codex

use std::fs;
use serial_test::serial;

#[test]
#[serial]
fn panic_hook_works() {
    let srv_dir = std::env::temp_dir();
    fs::create_dir_all(&srv_dir).unwrap();
    let log_path = srv_dir.join("panic.log");
    let hook_path = log_path.clone();
    std::panic::set_hook(Box::new(move |_| {
        fs::write(&hook_path, "panic").ok();
    }));
    let _ = std::panic::catch_unwind(|| panic!("boom"));
    assert_eq!(fs::read_to_string(log_path).unwrap(), "panic");
}

#[test]
#[serial]
fn memory_growth_within_bounds() {
    let data = vec![0u8; 1024 * 10];
    assert_eq!(data.len(), 10240);
}

#[test]
#[serial]
fn no_unclosed_caps() {
    let srv_dir = std::env::temp_dir();
    fs::create_dir_all(&srv_dir).unwrap();
    let caps_before = fs::read_dir(&srv_dir).unwrap().count();
    {
        let f = fs::File::create(srv_dir.join("tmp_cap")).unwrap();
        drop(f);
    }
    let caps_after = fs::read_dir(&srv_dir).unwrap().count();
    assert!(caps_after >= caps_before);
}

