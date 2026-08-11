// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Guard the exact suspended-child seal and MCS activation order.
// Author: Lukas Bower

#[test]
fn qemu_children_seal_before_any_service_activation() {
    let source = include_str!("../src/kernel.rs");
    let markers = [
        "init_isolated_qemu_net_console(hal, config)",
        "construct_ninedoor_service_runtime(1)",
        "seal_target_fault_registry()",
        "activate_critical_tcb_runtime(critical_runtime)",
        "activate_worker_runtime()",
        "activate_target_service()",
        "activate_console_network_child()",
    ];
    let positions = markers.map(|marker| {
        source
            .find(marker)
            .unwrap_or_else(|| unreachable!("kernel boot path must contain {marker}"))
    });
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "MCS construction, seal, and activation order drifted: {positions:?}",
    );
}

#[test]
fn boot_logs_compiler_sized_registry_without_a_literal_source_count() {
    let source = include_str!("../src/kernel.rs");
    assert!(source.contains("worker_resource_admission_config()"));
    assert!(source.contains(".fault_registry"));
    assert!(source.contains(".capacity"));
    assert!(!source.contains("fault registry sealed sources=10"));
}
