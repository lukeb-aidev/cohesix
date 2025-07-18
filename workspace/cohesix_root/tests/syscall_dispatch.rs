// CLASSIFICATION: COMMUNITY
// Filename: syscall_dispatch.rs v0.1
// Author: Cohesix Codex
// Date Modified: 2028-01-21

const DEFINED_SYSCALLS: &[i64] = &[-9, -3, -5, -7, -11];

#[test]
fn syscall_coverage() {
    let src = include_str!("../src/exception.rs");
    for num in DEFINED_SYSCALLS {
        assert!(src.contains(&format!("{num} =>")), "syscall {num} missing");
    }
}
