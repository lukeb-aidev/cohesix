// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines tests for root-task bootinfo_snapshot_layout.
// Author: Lukas Bower

use root_task::bootinfo_layout::{post_canary_offset, POST_CANARY_BYTES};
use std::fs;
use std::path::Path;

fn align_up(value: usize, align: usize) -> usize {
    (value + (align - 1)) & !(align - 1)
}

fn parse_hex_assignment(source: &str, marker: &str) -> usize {
    let line = source
        .lines()
        .find(|line| line.contains(marker))
        .unwrap_or_else(|| panic!("missing assignment for {marker}"));
    let (_, value) = line
        .split_once('=')
        .unwrap_or_else(|| panic!("assignment for {marker} must contain '='"));
    let literal = value
        .split(';')
        .next()
        .expect("assignment must end before a semicolon")
        .trim()
        .replace('_', "");
    let digits = literal
        .strip_prefix("0x")
        .unwrap_or_else(|| panic!("assignment for {marker} must be hexadecimal"));
    usize::from_str_radix(digits, 16)
        .unwrap_or_else(|_| panic!("assignment for {marker} must be valid hexadecimal"))
}

#[test]
fn post_canary_respects_unpadded_snapshot_length() {
    let payload_len = 0x1800usize;
    let base_addr = 0x3000_0000usize;
    let full_len = payload_len + POST_CANARY_BYTES;

    let post_addr = base_addr + post_canary_offset(payload_len);
    assert_eq!(post_addr, base_addr + full_len - POST_CANARY_BYTES);

    let padded_len = align_up(full_len, 0x1000);
    assert_ne!(
        post_addr,
        base_addr + padded_len - POST_CANARY_BYTES,
        "post-canary must stay outside padding spans",
    );
}

#[test]
fn runtime_entry_uses_linker_stack_top() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = crate_dir
        .parent()
        .and_then(Path::parent)
        .expect("root-task crate lives under apps/root-task");
    let runtime = fs::read_to_string(workspace.join("crates/sel4-runtime/src/lib.rs"))
        .expect("read sel4 runtime source");
    let linker = fs::read_to_string(crate_dir.join("sel4.ld")).expect("read root-task linker");

    assert!(
        runtime.contains("static __stack_top"),
        "runtime entry must import the linker-provided stack top"
    );
    assert!(
        runtime.contains("stack_top = sym __stack_top"),
        "runtime entry must seed SP from __stack_top"
    );
    assert!(
        !runtime.contains("static mut BOOT_STACK") && !runtime.contains("struct BootStack"),
        "runtime must not reserve a second data-adjacent stack"
    );
    assert!(
        linker.contains("__stack_top = .;"),
        "linker script must export the stack top consumed by the runtime"
    );
}

#[test]
fn linker_stack_size_matches_bootstrap_policy_guard() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let linker = fs::read_to_string(crate_dir.join("sel4.ld")).expect("read root-task linker");
    let layout = fs::read_to_string(crate_dir.join("src/bootstrap/layout.rs"))
        .expect("read bootstrap layout guard");

    let linker_size = parse_hex_assignment(&linker, "__stack_size");
    let guard_size = parse_hex_assignment(&layout, "const EXPECTED_STACK_SIZE");
    assert_eq!(
        guard_size,
        1024 * 1024,
        "root stack policy must admit the measured boot chain with service headroom"
    );
    assert_eq!(
        linker_size, guard_size,
        "sel4.ld and the independent boot layout guard must agree"
    );
}
