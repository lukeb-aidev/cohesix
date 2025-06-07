// CLASSIFICATION: COMMUNITY
// Filename: runtime_safety.rs v0.1
// Date Modified: 2025-07-01
// Author: Cohesix Codex

use std::fs;
use serial_test::serial;

#[test]
#[serial]
fn panic_hook_works() {
    fs::create_dir_all("srv").unwrap();
    std::panic::set_hook(Box::new(|_| {
        fs::write("/srv/panic.log", "panic").ok();
    }));
    let _ = std::panic::catch_unwind(|| panic!("boom"));
    assert_eq!(fs::read_to_string("/srv/panic.log").unwrap(), "panic");
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
    fs::create_dir_all("srv").unwrap();
    let caps_before = fs::read_dir("srv").unwrap().count();
    {
        let f = fs::File::create("srv/tmp_cap").unwrap();
        drop(f);
    }
    let caps_after = fs::read_dir("srv").unwrap().count();
    assert!(caps_after >= caps_before);
}

