// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines tests for root-task sel4_syscall_guard.
// Author: Lukas Bower
//! Guard to ensure all IPC syscalls go through the tracked sel4 wrappers.

use std::process::Command;

#[test]
fn no_direct_sel4_syscall_invocations() {
    let pattern = "sel4_sys::seL4_(Send|NBSend|Call|Reply|ReplyRecv|Signal|Wait|Recv|Yield)";
    let output = Command::new("rg")
        .args(&["-n", pattern, "src"])
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
