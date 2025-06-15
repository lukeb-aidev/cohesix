// CLASSIFICATION: COMMUNITY
// Filename: trace_validator_runtime.rs v0.1
// Date Modified: 2025-07-08
// Author: Lukas Bower

use std::fs;

use cohesix::sandbox::dispatcher::SyscallDispatcher;
use cohesix::cohesix_types::Syscall;

#[test]
fn validator_logs_mount_violation() {
    fs::create_dir_all("/srv/violations").unwrap();
    unsafe {
        std::env::set_var("COHESIX_VIOLATIONS_DIR", "/srv/violations");
    }
    fs::write("/srv/cohrole", "KioskInteractive").unwrap();
    SyscallDispatcher::dispatch(Syscall::Mount { src: "foo".into(), dest: "bar".into() });
    let log = fs::read_to_string("/srv/violations/runtime.json").unwrap();
    assert!(log.contains("Mount"));
}
