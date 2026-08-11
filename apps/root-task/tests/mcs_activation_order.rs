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
        "complete_bootstrap_ipc_trace()",
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

#[test]
fn bootstrap_ipc_trace_finishes_before_restricted_children_run() {
    let source = include_str!("../src/kernel.rs");
    let seal = source
        .find("seal_target_fault_registry()")
        .expect("kernel boot path must seal the exact fault registry");
    let trace_complete = source
        .find("complete_bootstrap_ipc_trace()")
        .expect("kernel boot path must close the synchronous bootstrap IPC trace");
    let restricted_activation = source
        .find("activate_critical_tcb_runtime(critical_runtime)")
        .expect("kernel boot path must activate the restricted critical children");

    assert_eq!(
        source.matches("complete_bootstrap_ipc_trace()").count(),
        1,
        "bootstrap IPC trace completion must remain a single boot boundary",
    );
    assert!(
        seal < trace_complete && trace_complete < restricted_activation,
        "bootstrap IPC trace must finish after registry seal and before restricted activation: \
         seal={seal}, trace_complete={trace_complete}, activation={restricted_activation}",
    );
}

#[test]
fn root_control_temporal_activation_exists_only_at_userland_loop_seams() {
    const ACTIVATE_CALL: &str = "activate_root_control_temporal_or_fail(ctx);";
    let kernel = include_str!("../src/kernel.rs");
    let userland = include_str!("../src/userland/mod.rs");

    assert!(
        !kernel.contains("activate_root_control_temporal_runtime("),
        "kernel bootstrap must retain the initial SC instead of arming root-control temporal policy",
    );
    assert_eq!(
        userland
            .matches("activate_root_control_temporal_runtime(")
            .count(),
        1,
        "only the userland activation guard may invoke the HAL temporal transition",
    );
    assert_eq!(
        userland
            .matches("activate_root_control_temporal_or_fail(")
            .count(),
        4,
        "the guard definition plus the three selected loop-entry calls must remain exact",
    );
    assert!(
        userland.contains(
            "boot_log::allow_ep_only_transport();\n        \
             activate_root_control_temporal_or_fail(&ctx);\n        pump.run();"
        ),
        "the non-serial pump must arm root-control immediately at its run boundary",
    );

    let normal_loop_start = userland
        .find("fn enter_root_console_loop<'a")
        .expect("serial root-console loop entry must exist");
    let normal_loop_end = userland[normal_loop_start..]
        .find("fn run_root_console_pump<'a")
        .map(|offset| normal_loop_start + offset)
        .expect("serial root-console loop entry must have a bounded source section");
    let normal_loop = &userland[normal_loop_start..normal_loop_end];
    let normal_activation = normal_loop
        .find(ACTIVATE_CALL)
        .expect("serial root-console loop must arm root-control");
    let normal_poll_loop = normal_loop
        .find("loop {\n        pump.poll();")
        .expect("serial root-console poll loop must exist");
    assert_eq!(normal_loop.matches(ACTIVATE_CALL).count(), 1);
    assert!(
        normal_activation < normal_poll_loop,
        "serial root-control policy must be armed immediately before steady polling",
    );

    let deferred_loop_start = userland
        .find("fn enter_root_console_loop_with_deferred_net_supervisor<")
        .expect("deferred-network supervisor loop entry must exist");
    let deferred_loop = &userland[deferred_loop_start..];
    let deferred_activation = deferred_loop
        .find(ACTIVATE_CALL)
        .expect("deferred-network supervisor loop must arm root-control");
    let supervisor_loop = deferred_loop
        .find("'supervisor: loop {")
        .expect("deferred-network steady supervisor loop must exist");
    assert_eq!(deferred_loop.matches(ACTIVATE_CALL).count(), 1);
    assert!(
        deferred_activation < supervisor_loop,
        "deferred-network root-control policy must be armed at its supervisor-loop seam",
    );
}
