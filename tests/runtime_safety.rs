// CLASSIFICATION: COMMUNITY
// Filename: runtime_safety.rs v0.1
// Date Modified: 2025-07-01
// Author: Cohesix Codex

use sysinfo::{System, SystemExt};
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
    let mut sys = System::new_all();
    sys.refresh_memory();
    let before = sys.used_memory();
    let data = vec![0u8; 1024 * 10];
    drop(data);
    sys.refresh_memory();
    let after = sys.used_memory();
    assert!(after >= before);
}

