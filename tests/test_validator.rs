// CLASSIFICATION: COMMUNITY
// Filename: test_validator.rs v0.1
// Date Modified: 2025-07-04
// Author: Cohesix Codex

use cohesix::sandbox::dispatcher::SyscallDispatcher;
use cohesix::cohesix_types::Syscall;
use std::fs;

#[test]
fn validator_blocks_non_worker_spawn() {
    fs::create_dir_all("/srv/violations").unwrap();
    fs::write("/srv/cohrole", "KioskInteractive").unwrap();
    SyscallDispatcher::dispatch(Syscall::Spawn { program: "echo".into(), args: vec!["hi".into()] });
    let viol = fs::read_to_string("/srv/violations/runtime.json").unwrap();
    assert!(viol.contains("denied"));
}
