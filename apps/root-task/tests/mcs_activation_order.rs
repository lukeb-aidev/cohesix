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
        .find("loop {")
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

#[test]
fn root_control_containment_serializes_before_the_ordinary_pump_turn() {
    let userland = include_str!("../src/userland/mod.rs");
    let loop_start = userland
        .find("fn enter_root_console_loop<'a")
        .expect("serial root-console loop entry must exist");
    let loop_end = userland[loop_start..]
        .find("fn run_root_console_pump<'a")
        .map(|offset| loop_start + offset)
        .expect("serial root-console loop entry must have a bounded source section");
    let root_loop = &userland[loop_start..loop_end];

    let console = root_loop
        .find("pump.contain_faulted_console_network(hal)")
        .expect("console-network containment probe must remain first");
    let console_assignment = root_loop[..console]
        .rfind("recovery_turn =")
        .expect("console-network containment result must own recovery_turn");
    let ninedoor_guard = root_loop
        .find("if !recovery_turn && hal_ptr != 0 {")
        .expect("NineDoor containment must be guarded by the console result");
    let ninedoor = root_loop
        .find("pump.contain_faulted_ninedoor(hal)")
        .expect("NineDoor containment probe must remain present");
    let ninedoor_assignment = root_loop[ninedoor_guard..ninedoor]
        .find("recovery_turn =")
        .map(|offset| ninedoor_guard + offset)
        .expect("NineDoor containment result must own recovery_turn");
    let pump_guard = root_loop
        .find("if !recovery_turn {")
        .expect("ordinary pump must be guarded by both containment results");
    let pump = root_loop
        .find("pump.poll();")
        .expect("ordinary pump call must remain present");
    let outer_yield = root_loop
        .find("sel4::yield_now();")
        .expect("root-control loop must retain its sole outer yield");

    assert!(
        console_assignment < console
            && console < ninedoor_guard
            && ninedoor_guard < ninedoor_assignment
            && ninedoor_assignment < ninedoor
            && ninedoor < pump_guard
            && pump_guard < pump
            && pump < outer_yield,
        "containment/pump/yield source order drifted: \
         console_assignment={console_assignment}, console={console}, \
         ninedoor_guard={ninedoor_guard}, ninedoor_assignment={ninedoor_assignment}, \
         ninedoor={ninedoor}, \
         pump_guard={pump_guard}, pump={pump}, yield={outer_yield}",
    );
    assert_eq!(root_loop.matches("pump.poll();").count(), 1);
    assert_eq!(root_loop.matches("sel4::yield_now();").count(), 1);
    assert_eq!(
        root_loop
            .matches("pump.contain_faulted_console_network(hal)")
            .count(),
        1,
    );
    assert_eq!(
        root_loop
            .matches("pump.contain_faulted_ninedoor(hal)")
            .count(),
        1,
    );
    assert_eq!(
        root_loop.matches(".unwrap_or(false);").count(),
        2,
        "both optional HAL probes must preserve false-on-no-probe semantics",
    );
    assert!(
        root_loop.contains("if !recovery_turn {\n            pump.poll();\n        }"),
        "either containment result must exclude the ordinary pump from that turn",
    );
}

#[test]
fn ninedoor_attachment_does_not_attempt_classic_tcb_affinity_under_mcs() {
    let userland = include_str!("../src/userland/mod.rs");
    let attach_start = userland
        .find("fn attach_ninedoor_bridge<'a")
        .expect("target NineDoor attachment helper must exist");
    let attach_end = userland[attach_start..]
        .find("#[cfg(not(feature = \"kernel\"))]")
        .map(|offset| attach_start + offset)
        .expect("target NineDoor attachment helper must have a bounded source section");
    let attach = &userland[attach_start..attach_end];

    assert!(attach.contains("pump.attach_ninedoor(ninedoor);"));
    assert!(!attach.contains("with_role_affinity"));
    assert!(!attach.contains("set_tcb_affinity"));
}
