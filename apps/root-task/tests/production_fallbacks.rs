// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Prove selected root-task source contains no silent production mock or stub fallback.
// Author: Lukas Bower

#[test]
fn operational_userland_requires_real_serial() {
    let source = include_str!("../src/userland/mod.rs");
    assert!(!source.contains("KernelSerialDriver::null()"));
    assert!(source.contains("operational serial driver missing"));
}

#[test]
fn target_namespace_starts_without_fabricated_host_or_gpu_state() {
    let source = include_str!("../src/ninedoor.rs");
    for forbidden in ["Mock 4090", "Mock 4060", "42C", "state=idle"] {
        assert!(
            !source.contains(forbidden),
            "fabricated production seed remains: {forbidden}"
        );
    }
    assert!(source.contains("state=unavailable source=none"));
    assert!(source.contains("unavailable source=none"));
}

#[test]
fn legacy_console_and_unwired_trace_alias_are_retired() {
    let uart = include_str!("../src/uart/pl011.rs");
    let trace = include_str!("../src/trace.rs");
    assert!(!uart.contains("console_main()"));
    assert!(!uart.contains("(stub) reboot not implemented"));
    assert!(!trace.contains("TraceSink::Ipc"));
}

#[test]
fn early_reboot_is_a_typed_authorization_refusal() {
    let console = include_str!("../src/console/mod.rs");
    assert!(console.contains("Command::Reboot => self.emit_refusal"));
    assert!(console.contains("reason=unauthorized"));
    assert!(console.contains("authenticated-event-pump-required"));
}
