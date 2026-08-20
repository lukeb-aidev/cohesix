// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines tests for root-task sel4_syscall_guard.
// Author: Lukas Bower
//! Guard to ensure all IPC syscalls go through the tracked sel4 wrappers.

use std::fs;
use std::process::Command;

#[test]
fn no_direct_sel4_syscall_invocations() {
    let pattern =
        "sel4_sys::seL4_(Send|NBSend|Call|CallWithMRs|Reply|ReplyRecv|Signal|Wait|Recv|NBRecv|Poll|Yield)\\s*\\(";
    let output = Command::new("rg")
        .args(["-n", pattern, "src"])
        .output()
        .expect("rg must be available to enforce sel4 syscall routing");

    if !(output.status.success() || output.status.code() == Some(1)) {
        panic!(
            "rg returned non-zero exit code {}",
            output.status.code().unwrap_or(-1)
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let path = line.split(':').next().unwrap_or("");
        if path.is_empty() {
            continue;
        }
        if path != "src/sel4.rs" && path != "src/sel4/syscall.rs" {
            panic!("direct seL4 syscall found outside sel4 wrappers: {line}");
        }
    }
}

#[test]
fn debug_syscalls_use_selected_sel4_sys_abi() {
    let source = fs::read_to_string("src/sel4.rs")
        .expect("root sel4 wrapper source must be readable for ABI guard");

    for forbidden in [
        "core::arch::asm",
        "SYS_DEBUG_PUT_CHAR",
        "SYS_DEBUG_HALT",
        "wrapping_sub(8)",
        "wrapping_sub(10)",
    ] {
        assert!(
            !source.contains(forbidden),
            "root debug wrapper must not encode a profile-specific syscall ABI: {forbidden}"
        );
    }

    assert!(source.contains("sel4_sys::debug_put_char(byte)"));
    assert!(source.contains("sel4_sys::debug_halt()"));
}
