// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines tests for root-task sel4_syscall_guard.
// Author: Lukas Bower
//! Guard to ensure all IPC syscalls go through the tracked sel4 wrappers.

use std::fs;
use std::path::{Path, PathBuf};

const TRACKED_SYSCALLS: [&str; 12] = [
    "Send",
    "NBSend",
    "Call",
    "CallWithMRs",
    "Reply",
    "ReplyRecv",
    "Signal",
    "Wait",
    "Recv",
    "NBRecv",
    "Poll",
    "Yield",
];

fn collect_rust_sources(directory: &Path, sources: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", directory.display()));

    for entry in entries {
        let entry = entry.unwrap_or_else(|error| {
            panic!(
                "failed to read an entry under {}: {error}",
                directory.display()
            )
        });
        let path = entry.path();
        let file_type = entry
            .file_type()
            .unwrap_or_else(|error| panic!("failed to inspect {}: {error}", path.display()));

        if file_type.is_dir() {
            collect_rust_sources(&path, sources);
        } else if file_type.is_file() && path.extension().is_some_and(|extension| extension == "rs")
        {
            sources.push(path);
        }
    }
}

fn direct_syscall(source: &str) -> Option<(usize, &'static str)> {
    for syscall in TRACKED_SYSCALLS {
        let needle = format!("sel4_sys::seL4_{syscall}");
        for (offset, _) in source.match_indices(&needle) {
            let suffix = &source[offset + needle.len()..];
            if suffix.trim_start().starts_with('(') {
                let line = source[..offset]
                    .bytes()
                    .filter(|byte| *byte == b'\n')
                    .count()
                    + 1;
                return Some((line, syscall));
            }
        }
    }
    None
}

#[test]
fn no_direct_sel4_syscall_invocations() {
    let mut sources = Vec::new();
    collect_rust_sources(Path::new("src"), &mut sources);
    sources.sort();

    for path in sources {
        if path == Path::new("src/sel4.rs") || path == Path::new("src/sel4/syscall.rs") {
            continue;
        }

        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        if let Some((line, syscall)) = direct_syscall(&source) {
            panic!(
                "direct seL4 syscall found outside sel4 wrappers: {}:{line}: seL4_{syscall}",
                path.display()
            );
        }
    }
}

#[test]
fn syscall_scanner_detects_only_direct_invocations() {
    assert_eq!(
        direct_syscall("fn bad() {\n    sel4_sys::seL4_Call \n        (endpoint);\n}"),
        Some((2, "Call"))
    );
    assert_eq!(
        direct_syscall("fn helper() { sel4_sys::seL4_CallWithMRs_helper(); }"),
        None
    );
}

#[test]
fn debug_syscalls_use_selected_sel4_sys_abi() {
    let source = fs::read_to_string("src/sel4.rs")
        .expect("root sel4 wrapper source must be readable for ABI guard");
    let (_, debug_wrappers_and_after) = source
        .split_once("pub unsafe extern \"C\" fn seL4_DebugPutChar(byte: u8)")
        .expect("root DebugPutChar wrapper must remain present");
    let (debug_wrappers, _) = debug_wrappers_and_after
        .split_once("pub unsafe fn seL4_DebugCapIdentify")
        .expect("root debug wrapper region must remain bounded");

    for forbidden in [
        "core::arch::asm",
        "SYS_DEBUG_PUT_CHAR",
        "SYS_DEBUG_HALT",
        "wrapping_sub(8)",
        "wrapping_sub(10)",
    ] {
        assert!(
            !debug_wrappers.contains(forbidden),
            "root debug wrapper must not encode a profile-specific syscall ABI: {forbidden}"
        );
    }

    assert!(debug_wrappers.contains("sel4_sys::debug_put_char(byte)"));
    assert!(debug_wrappers.contains("sel4_sys::debug_halt()"));
}
