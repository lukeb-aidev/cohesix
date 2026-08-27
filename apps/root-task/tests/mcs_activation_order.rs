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
fn wired_console_child_registers_before_seal_and_hands_off_only_after_dhcp() {
    let kernel = include_str!("../src/kernel.rs");
    let requires = kernel
        .find("requires_preseal_console_network_runtime")
        .expect("wired stack must declare its pre-seal console child");
    let construct = kernel[requires..]
        .find("construct_direct_genet_console_network_runtime_shell(1)")
        .map(|offset| requires + offset)
        .expect("wired console child must be constructed before seal");
    let attach = kernel[construct..]
        .find("attach_preseal_console_network_runtime(runtime)")
        .map(|offset| construct + offset)
        .expect("wired stack must retain the constructed child");
    let ninedoor = kernel[attach..]
        .find("construct_ninedoor_service_runtime(1)")
        .map(|offset| attach + offset)
        .expect("NineDoor must remain the final child constructor");
    let seal = kernel[ninedoor..]
        .find("seal_target_fault_registry()")
        .map(|offset| ninedoor + offset)
        .expect("exact registry seal must follow every child constructor");
    assert!(requires < construct && construct < attach && attach < ninedoor && ninedoor < seal);

    let network = include_str!("../src/net/stack.rs");
    let defer = network
        .find("fn attach_console_runtime(")
        .expect("GENET must accept the pre-seal runtime");
    let dhcp = network[defer..]
        .find("fn transition_ready(&self)")
        .map(|offset| defer + offset)
        .expect("GENET handoff must retain an address-readiness gate");
    let finalize = network[dhcp..]
        .find("runtime.finalize_descriptor(")
        .map(|offset| dhcp + offset)
        .expect("GENET must finalize the descriptor after DHCP");
    let activate = network[finalize..]
        .find("runtime.activate()")
        .map(|offset| finalize + offset)
        .expect("GENET must activate only after descriptor finalization");
    let move_device = network[activate..]
        .find("IsolatedNetworkConsole::from_existing(")
        .map(|offset| activate + offset)
        .expect("GENET device must move into the isolated console adapter");
    assert!(defer < dhcp && dhcp < finalize && finalize < activate && activate < move_device);
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
fn steady_ipc_bypasses_shared_bootstrap_diagnostics() {
    let source = include_str!("../src/sel4.rs");
    assert!(
        source.contains(
            "#[inline(always)]\nfn ipc_bootstrap_trap(kind: IpcSyscallKind, dest: seL4_CPtr, location: &Location)"
        ),
        "the four-read steady IPC gate must remain available for syscall-wrapper optimization",
    );
    assert!(
        source.contains("#[cold]\n#[inline(never)]\nfn ipc_bootstrap_trap_slow("),
        "bootstrap diagnostics must remain a cold outlined function",
    );
    let fast_start = source
        .find("fn ipc_bootstrap_trap(kind:")
        .expect("IPC wrappers must retain the bootstrap readiness guard");
    let slow_start = source[fast_start..]
        .find("fn ipc_bootstrap_trap_slow(")
        .map(|offset| fast_start + offset)
        .expect("bootstrap diagnostics must live in a separate slow path");
    let fast = &source[fast_start..slow_start];
    let slow_end = source[slow_start..]
        .find("\n#[inline(never)]\nfn ensure_endpoint(")
        .map(|offset| slow_start + offset)
        .expect("bootstrap diagnostics must end before the endpoint helper");
    let slow = &source[slow_start..slow_end];

    let ready = fast
        .find("if ready && validated && unlocked && post_commit")
        .expect("steady IPC must revalidate all four readiness guards");
    let dispatch = fast
        .find("ipc_bootstrap_trap_slow(")
        .expect("pre-readiness IPC must retain the diagnostic trap");
    assert!(
        ready < dispatch,
        "the steady readiness return must precede slow diagnostic dispatch",
    );
    assert!(
        !fast.contains("BOOTSTRAP_SEND_INSTRUMENT_COUNT.fetch_add")
            && !fast.contains("boot_tracer().snapshot()")
            && !fast.contains("HeaplessString")
            && !fast.contains("write!(")
            && !fast.contains("emit_illegal_send_line"),
        "steady IPC must not contend on bootstrap trace state",
    );
    assert!(
        slow.contains("BOOTSTRAP_SEND_INSTRUMENT_COUNT.fetch_add")
            && slow.contains("boot_tracer().snapshot()"),
        "pre-readiness diagnostics must retain the bounded trace and snapshot",
    );
    assert_eq!(
        slow.matches("ep_ready()").count()
            + slow.matches("ep_validated()").count()
            + slow.matches("ipc_send_unlocked()").count()
            + slow.matches("post_commit_ipc_unlocked()").count(),
        0,
        "the cold path must diagnose the exact readiness values selected by the caller",
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

    let guard_start = userland
        .find("fn activate_root_control_temporal_or_fail(ctx: &BootContext)")
        .expect("root-control activation guard must exist");
    let guard_end = userland[guard_start..]
        .find("#[cfg(all(feature = \"serial-console\", feature = \"kernel\"))]")
        .map(|offset| guard_start + offset)
        .expect("root-control activation guard must have a bounded source section");
    let guard = &userland[guard_start..guard_end];
    let attach = guard
        .find("crate::hal::critical_tcb::activate_root_control_temporal_runtime(")
        .expect("activation guard must attach the generated root-control SC");
    let activation_yield = guard
        .find("Ok(()) => sel4::yield_now(),")
        .expect("successful activation must immediately surrender the activation-seam remainder");
    let failure = guard
        .find("Err(error) =>")
        .expect("activation failure must remain fail-stop");
    assert!(attach < activation_yield && activation_yield < failure);
    assert_eq!(
        guard.matches("Ok(()) => sel4::yield_now(),").count(),
        1,
        "Milestone 26e has one universal success-only activation-seam yield",
    );
    let post_attach = &guard[attach..activation_yield];
    for forbidden in [
        "boot_log::",
        "log::",
        "pump.",
        "contain_faulted_",
        "allow_ep_only_transport",
    ] {
        assert!(
            !post_attach.contains(forbidden),
            "no output, containment, or EventPump work may run between steady-SC attach and the activation yield: {forbidden}",
        );
    }

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
    assert_eq!(
        normal_loop.matches("Ok(()) => sel4::yield_now(),").count(),
        0,
        "the normal caller must use the universal guard rather than minting another seam yield",
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
    assert_eq!(
        deferred_loop.matches("Ok(()) => sel4::yield_now(),").count(),
        0,
        "the deferred supervisor must use the universal guard rather than minting another seam yield",
    );
}

#[test]
fn root_control_natural_postpone_keeps_exact_target_budgets_and_fault_routes() {
    fn root_control_section(manifest: &str) -> &str {
        const START: &str = "[[temporal_authority.tasks]]\nid = \"root-control\"";
        let start = manifest.find(START).expect("root-control manifest record");
        let tail = &manifest[start..];
        let end = tail[START.len()..]
            .find("\n[[temporal_authority.tasks]]")
            .map_or(tail.len(), |offset| START.len() + offset);
        &tail[..end]
    }

    fn assert_exact_line(section: &str, line: &str) {
        assert_eq!(
            section
                .lines()
                .filter(|candidate| *candidate == line)
                .count(),
            1,
            "missing or duplicate exact manifest line: {line}",
        );
    }

    let qemu = include_str!("../../../configs/root_task.toml");
    let regression = include_str!("../../../configs/root_task_regression.toml");
    let pi4 = include_str!("../../../configs/root_task_pi4_uboot_aarch64.toml");
    for (manifest, budget, provenance) in [
        (
            qemu,
            9_000,
            "m26e-qemu-root-dedicated-core-bounded-quantum-v1",
        ),
        (
            regression,
            9_000,
            "m26e-qemu-root-dedicated-core-bounded-quantum-v1",
        ),
        (
            pi4,
            2_750,
            "m26e-pi4-root-adjacent-refill-natural-postpone-candidate-v24",
        ),
    ] {
        let root = root_control_section(manifest);
        assert_exact_line(root, &format!("budget_us = {budget}"));
        assert_exact_line(root, "period_us = 10000");
        assert_exact_line(root, "max_refills = 2");
        assert_exact_line(root, "timeout_policy = \"natural-postpone\"");
        assert_exact_line(root, &format!("wcet_provenance = \"{provenance}\""));
    }
    assert!(qemu.contains("timer_clock_hz = 24000000"));
    assert!(regression.contains("timer_clock_hz = 24000000"));
    assert!(pi4.contains("timer_clock_hz = 54000000"));

    let source = include_str!("../src/hal/critical_tcb.rs");
    let configure_start = source
        .find("fn configure_active_sc_with_sched_control(")
        .expect("critical SC configuration helper");
    let configure_end = source[configure_start..]
        .find("\nfn allocate_stack(")
        .map(|offset| configure_start + offset)
        .expect("bounded critical SC configuration section");
    let configure = &source[configure_start..configure_end];
    let standard = configure
        .find("sel4::set_tcb_sched_params_mcs(")
        .expect("standard fault endpoint installation");
    let timeout_guard = configure
        .find("if requires_timeout_endpoint(task.timeout_policy)")
        .expect("policy-controlled timeout endpoint installation");
    let timeout = configure
        .find("sel4::set_tcb_timeout_endpoint(tcb, timeout_fault_cap)")
        .expect("timeout endpoint installation path");
    assert!(standard < timeout_guard && timeout_guard < timeout);
    assert!(configure[standard..timeout_guard].contains("standard_fault_cap"));
    assert!(configure.contains("!matches!(policy, TimeoutPolicy::NaturalPostpone)"));

    let classifier_start = source
        .find("fn handle_target_fault(")
        .expect("critical fault classifier");
    let classifier_end = source[classifier_start..]
        .find("\nextern \"C\" fn root_fault_entry")
        .map(|offset| classifier_start + offset)
        .expect("bounded critical fault classifier");
    let classifier = &source[classifier_start..classifier_end];
    let root_control = classifier
        .find("TemporalTaskKind::RootControl")
        .expect("root-control standard-fault class");
    let service = classifier[root_control..]
        .find("TemporalTaskKind::Service")
        .map(|offset| root_control + offset)
        .expect("service class after critical duties");
    assert!(classifier[root_control..service].contains("FaultReplyDisposition::CriticalTerminal {"));
}

#[test]
fn driver_tcb_constructor_honors_natural_postpone_policy() {
    let source = include_str!("../src/hal/mod.rs");
    let configure_start = source
        .find("fn configure_driver_tcb_priority_for_boot(")
        .expect("driver TCB MCS configuration helper");
    let configure_end = source[configure_start..]
        .find("\n#[cfg(feature = \"kernel\")]\nfn restore_driver_tcb_steady_priority(")
        .map(|offset| configure_start + offset)
        .expect("bounded driver TCB configuration section");
    let configure = &source[configure_start..configure_end];

    let standard = configure
        .find("sel4::set_tcb_sched_params_mcs(")
        .expect("standard fault endpoint installation");
    let selected_policy = configure
        .find("driver_task_requires_timeout_endpoint(temporal.timeout_policy)")
        .expect("generated driver timeout policy selection");
    let timeout_guard = configure[selected_policy..]
        .find("if install_timeout_endpoint {")
        .map(|offset| selected_policy + offset)
        .expect("policy-controlled driver timeout endpoint guard");
    let timeout = configure
        .find("sel4::set_tcb_timeout_endpoint(tcb, mcs.timeout_fault_endpoint)")
        .expect("driver timeout endpoint installation path");

    assert!(
        standard < selected_policy && selected_policy < timeout_guard && timeout_guard < timeout
    );
    assert!(configure[standard..selected_policy].contains("mcs.standard_fault_endpoint"));
    assert!(configure.contains("timeout_policy={:?} timeout_endpoint={}"));

    let predicate_start = source
        .find("const fn driver_task_requires_timeout_endpoint(")
        .expect("driver timeout endpoint predicate");
    let predicate_end = source[predicate_start..]
        .find("\n}\n")
        .map(|offset| predicate_start + offset + 2)
        .expect("bounded driver timeout endpoint predicate");
    assert!(source[predicate_start..predicate_end].contains("TimeoutPolicy::NaturalPostpone"));
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

    let activation = root_loop
        .find("activate_root_control_temporal_or_fail(ctx);")
        .expect("serial loop must cross the one-time activation seam");

    let direct_pair = root_loop
        .find("pump.contain_faulted_direct_genet_pair(hal)")
        .expect("direct-GENET pair containment probe must remain first");
    let direct_pair_assignment = root_loop[..direct_pair]
        .rfind("recovery_turn =")
        .expect("direct-GENET pair containment result must own recovery_turn");
    let console_guard = root_loop[direct_pair..]
        .find("if !recovery_turn && hal_ptr != 0 {")
        .map(|offset| direct_pair + offset)
        .expect("console containment must be guarded by the paired result");
    let console = root_loop
        .find("pump.contain_faulted_console_network(hal)")
        .expect("console-network containment probe must follow paired containment");
    let console_assignment = root_loop[console_guard..console]
        .find("recovery_turn =")
        .map(|offset| console_guard + offset)
        .expect("console-network containment result must own recovery_turn");
    let ninedoor_guard = root_loop[console..]
        .find("if !recovery_turn && hal_ptr != 0 {")
        .map(|offset| console + offset)
        .expect("NineDoor containment must be guarded by the console result");
    let ninedoor = root_loop
        .find("pump.contain_faulted_ninedoor(hal)")
        .expect("NineDoor containment probe must remain present");
    let ninedoor_assignment = root_loop[ninedoor_guard..ninedoor]
        .find("recovery_turn =")
        .map(|offset| ninedoor_guard + offset)
        .expect("NineDoor containment result must own recovery_turn");
    let handoff_guard = root_loop[ninedoor..]
        .find("if !recovery_turn && hal_ptr != 0 {")
        .map(|offset| ninedoor + offset)
        .expect("deferred console-network handoff must follow containment");
    let handoff = root_loop
        .find("pump.service_deferred_console_network_handoff(hal)")
        .expect("deferred console-network handoff probe must remain present");
    let handoff_assignment = root_loop[handoff_guard..handoff]
        .find("recovery_turn =")
        .map(|offset| handoff_guard + offset)
        .expect("deferred console-network handoff must own recovery_turn");
    let pump_guard = root_loop
        .find("let explicit_yield_required = if recovery_turn {")
        .expect("ordinary pump must be guarded by both containment results");
    let pump = root_loop
        .find("pump.poll_root_control_quantum()")
        .expect("bounded root-control quantum must remain present");
    let outer_yield = root_loop
        .find("sel4::yield_now();")
        .expect("root-control loop must retain its sole outer yield");

    assert!(
        activation < direct_pair_assignment
            && direct_pair_assignment < direct_pair
            && direct_pair < console_guard
            && console_guard < console_assignment
            && console_assignment < console
            && console < ninedoor_guard
            && ninedoor_guard < ninedoor_assignment
            && ninedoor_assignment < ninedoor
            && ninedoor < handoff_guard
            && handoff_guard < handoff_assignment
            && handoff_assignment < handoff
            && handoff < pump_guard
            && pump_guard < pump
            && pump < outer_yield,
        "containment/pump/yield source order drifted: \
         activation={activation}, direct_pair_assignment={direct_pair_assignment}, \
         direct_pair={direct_pair}, console_guard={console_guard}, \
         console_assignment={console_assignment}, console={console}, \
         ninedoor_guard={ninedoor_guard}, ninedoor_assignment={ninedoor_assignment}, \
         ninedoor={ninedoor}, handoff_guard={handoff_guard}, \
         handoff_assignment={handoff_assignment}, handoff={handoff}, \
         pump_guard={pump_guard}, pump={pump}, yield={outer_yield}",
    );
    assert_eq!(
        root_loop
            .matches("activate_root_control_temporal_or_fail(ctx);")
            .count(),
        1,
        "the serial path crosses the activation seam exactly once before containment",
    );
    assert_eq!(
        root_loop
            .matches("pump.poll_root_control_quantum()")
            .count(),
        1,
    );
    assert_eq!(
        root_loop.matches("pump.poll();").count(),
        0,
        "the root loop must not bypass the platform-gated quantum wrapper",
    );
    assert_eq!(root_loop.matches("sel4::yield_now();").count(), 1);
    assert_eq!(
        root_loop
            .matches("pump.contain_faulted_direct_genet_pair(hal)")
            .count(),
        1,
    );
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
        root_loop
            .matches("pump.service_deferred_console_network_handoff(hal)")
            .count(),
        1,
    );
    assert_eq!(
        root_loop.matches(".unwrap_or(false);").count(),
        4,
        "all optional HAL probes must preserve false-on-no-probe semantics",
    );
    assert!(
        root_loop.contains(
            "let explicit_yield_required = if recovery_turn {\n            true\n        } else {\n            pump.poll_root_control_quantum()\n        };"
        ),
        "either containment result must exclude the ordinary pump from that turn",
    );
}

#[test]
fn console_containment_advances_one_material_unit_per_recovery_turn() {
    let source = include_str!("../src/hal/console_network.rs");
    let latch_start = source
        .find("pub fn begin_containment(&mut self)")
        .expect("console containment latch");
    let latch_end = source[latch_start..]
        .find("/// Mark a critical-lane fault")
        .map(|offset| latch_start + offset)
        .expect("bounded console containment latch");
    let latch = &source[latch_start..latch_end];
    assert!(latch.contains("self.containment_started = true;"));
    for forbidden in ["sel4::", "log::", "cache_clean", "unmap_page_cap", "for "] {
        assert!(!latch.contains(forbidden), "latch contains {forbidden}");
    }

    let adapter = include_str!("../src/net/isolated_console.rs");
    let bridge_start = adapter
        .find("pub fn contain_if_faulted(")
        .expect("console containment bridge");
    let bridge_end = adapter[bridge_start..]
        .find("fn pending_containment_diagnostic(")
        .map(|offset| bridge_start + offset)
        .expect("bounded console containment bridge");
    let bridge = &adapter[bridge_start..bridge_end];
    let contended = bridge
        .find("crate::critical_tcb::FaultHandoffError::Contended")
        .expect("typed mailbox contention branch");
    let contended_return = bridge[contended..]
        .find("return Ok(ConsoleNetworkContainmentTurn::Retry);")
        .map(|offset| contended + offset)
        .expect("mailbox contention must always retry before latch");
    let begin = bridge
        .find("self.runtime.begin_containment()")
        .expect("pure resource latch");
    let first_return = bridge[begin..]
        .find("return Ok(ConsoleNetworkContainmentTurn::InProgress);")
        .map(|offset| begin + offset)
        .expect("latch owns the first Recovery turn");
    let advance = bridge
        .find("self.contain_one_turn(hal)")
        .expect("later Recovery turn advances Suspend");
    assert!(contended < contended_return && contended_return < begin);
    let contended_arm_end = bridge[contended..]
        .find("Err(_) =>")
        .map(|offset| contended + offset)
        .expect("bounded contention arm");
    assert!(!bridge[contended..contended_arm_end].contains("if !faulted"));
    assert!(begin < first_return && first_return < advance);
    assert!(!bridge.contains("log::"));

    let start = source
        .find("pub fn contain_one_turn(")
        .expect("console containment turn entrypoint must exist");
    let end = source[start..]
        .find("/// Root slots permanently reserved")
        .map(|offset| start + offset)
        .expect("console containment turn must have a bounded source section");
    let containment = &source[start..end];

    let select = containment
        .find("let selected = self.containment.select_next();")
        .expect("successor must be committed before selected work");
    let dispatch = containment
        .find("let result = match selected")
        .expect("selected material dispatch");
    let restore = containment
        .find("self.containment.restore_selected(selected);")
        .expect("typed synchronous failures must restore the selected unit");
    assert!(select < dispatch && dispatch < restore);
    assert!(
        !containment[select..restore].contains('?'),
        "no post-commit typed error may bypass restore_selected",
    );
    assert!(!containment.contains("for "));
    assert_eq!(containment.matches("sel4::suspend_tcb_bounded(").count(), 1);
    assert_eq!(
        containment
            .matches("sel4::unbind_sched_context_object(")
            .count(),
        1,
    );
    assert_eq!(
        containment
            .matches("super::cache::cache_clean_bounded(")
            .count(),
        1
    );
    assert_eq!(
        containment.matches(".unmap_page_cap(").count(),
        2,
        "shared-frame and direct-GENET copied-cap unmap arms must remain distinct",
    );
    assert_eq!(
        containment.matches("sel4::cnode_delete_bounded(").count(),
        4,
        "fault, direct IRQ-notification, direct IRQHandler, and direct-GENET copied-cap deletion arms must remain distinct",
    );
    assert_eq!(
        containment
            .matches(".revoke_anchor_descendants_and_reset_vspace(")
            .count(),
        1,
    );
    assert_eq!(containment.matches("select_next()").count(), 1);
    assert_eq!(containment.matches("restore_selected(selected)").count(), 1);
    assert!(!containment.contains("yield_now"));
    assert!(!containment.contains("unsafe"));
    assert!(!containment.contains("log::"));
    assert!(!containment.contains("self.audit"));

    let clean_start = containment
        .find("ConsoleNetworkContainmentUnit::ScrubCleanSharedFrame(frame_index) =>")
        .expect("indexed shared-frame clean arm");
    let unmap_start = containment
        .find("ConsoleNetworkContainmentUnit::UnmapSharedFrame(frame_index) =>")
        .expect("separate indexed shared-frame unmap arm");
    let cap_start = containment
        .find("ConsoleNetworkContainmentUnit::DeleteFaultCap(cap_index) =>")
        .expect("indexed fault-cap arm");
    let clean_arm = &containment[clean_start..unmap_start];
    let clean_index = clean_arm
        .find(".get_mut(frame_index)")
        .expect("one indexed shared-frame clean unit");
    let fill = clean_arm.find(".fill(0)").expect("bounded frame zero");
    let clean = clean_arm
        .find("cache_clean_bounded(")
        .expect("bounded cache clean");
    assert!(clean_index < fill && fill < clean);
    assert!(!clean_arm.contains("unmap_page_cap"));
    let unmap_arm = &containment[unmap_start..cap_start];
    assert!(unmap_arm.find(".unmap_page_cap(frame.cap())").is_some());
    assert!(!unmap_arm.contains("fill(0)"));
    assert!(!unmap_arm.contains("cache_clean"));

    let revoke_start = containment
        .find("ConsoleNetworkContainmentUnit::RevokeAnchor =>")
        .expect("anchor revoke arm");
    let cap_arm = &containment[cap_start..revoke_start];
    let cap_delete = cap_arm
        .find("sel4::cnode_delete_bounded(")
        .expect("one indexed fault-cap delete unit");
    let cap_error = cap_arm
        .find("if error == sel4_sys::seL4_NoError")
        .expect("typed cap-delete error boundary");
    assert!(cap_delete < cap_error);

    let finalize_start = containment
        .find("ConsoleNetworkContainmentUnit::Finalize | ConsoleNetworkContainmentUnit::Complete")
        .expect("final proof arm");
    let revoke_arm = &containment[revoke_start..finalize_start];
    let revoke = revoke_arm
        .find(".revoke_anchor_descendants_and_reset_vspace(")
        .expect("anchor revoke unit");
    assert!(revoke < revoke_arm.len());
}

#[test]
fn console_containment_progress_is_exclusive_and_final_proof_is_exact() {
    let event_source = include_str!("../src/event/mod.rs");
    let event_start = event_source
        .find("pub fn contain_faulted_console_network(")
        .expect("EventPump console containment hook");
    let event_end = event_source[event_start..]
        .find("/// Consume and contain one terminal isolated NineDoor service fault")
        .map(|offset| event_start + offset)
        .expect("bounded EventPump console containment section");
    let event_containment = &event_source[event_start..event_end];
    assert!(event_containment.contains("Ok(ConsoleNetworkContainmentTurn::Idle) => false"));
    assert!(event_containment.contains("Ok(ConsoleNetworkContainmentTurn::Retry) => true"));
    assert!(event_containment.contains("Ok(ConsoleNetworkContainmentTurn::InProgress)"));
    assert!(event_containment.contains("self.fence_console_network_authority_quiet();"));
    for forbidden in ["log::", "self.audit", "quarantine_network_service_after"] {
        assert!(
            !event_containment.contains(forbidden),
            "Recovery contains {forbidden}"
        );
    }
    let fence_start = event_source
        .find("fn fence_console_network_authority_quiet(&mut self)")
        .expect("quiet console-network authority fence");
    let fence = &event_source[fence_start..event_start];
    assert!(fence.contains("self.network_service_quarantined = true;"));
    assert!(fence.contains("self.console_network_quarantine_cleanup_pending = true;"));
    assert!(fence.contains("if net_session {"));
    assert!(fence.contains("crate::serial::invalidate_prompt_input_shadow_quiet();"));
    for forbidden in ["log::", "self.audit", "format_message", "end_session("] {
        assert!(
            !fence.contains(forbidden),
            "quiet fence contains {forbidden}"
        );
    }

    let cleanup_start = event_source
        .find("fn service_deferred_console_network_quarantine_cleanup(&mut self)")
        .expect("persistent quiet console-network cleanup");
    let cleanup_end = event_source[cleanup_start..]
        .find("/// Attach the preserved reason")
        .map(|offset| cleanup_start + offset)
        .expect("bounded quiet cleanup section");
    let cleanup = &event_source[cleanup_start..cleanup_end];
    let select = cleanup.find("let selected =").unwrap();
    let successor = cleanup
        .find("self.console_network_quarantine_cleanup_unit = selected.next();")
        .unwrap();
    let dispatch = cleanup.find("match selected {").unwrap();
    assert!(select < successor && successor < dispatch);
    for unit in [
        "RootSessionTicket",
        "RootTicketUsage",
        "NineDoorSessionTicket",
        "NineDoorSessionScope",
        "NineDoorSessionBinds",
        "PendingStreamCursor",
        "PendingStreamCacheSnapshot",
        "PendingStream",
        "Finalize",
        "Complete",
    ] {
        assert!(cleanup.contains(unit), "missing cleanup unit {unit}");
    }
    for forbidden in [
        "log::",
        "self.audit",
        "format_message",
        "end_session(",
        "reset_session(",
        "quarantine_network_service_after",
    ] {
        assert!(
            !cleanup.contains(forbidden),
            "quiet cleanup contains {forbidden}"
        );
    }

    let serial_source = include_str!("../src/serial/mod.rs");
    let invalidate_start = serial_source
        .find("pub(crate) fn invalidate_prompt_input_shadow_quiet()")
        .expect("lock-free prompt-shadow invalidation");
    let invalidate_end = serial_source[invalidate_start..]
        .find("pub(crate) fn emit_prompt_refresh_with_input_shadow_unlocked")
        .map(|offset| invalidate_start + offset)
        .expect("bounded prompt-shadow invalidation");
    let invalidate = &serial_source[invalidate_start..invalidate_end];
    assert!(invalidate.contains("SERIAL_PROMPT_INPUT_SHADOW_VALID.store"));
    for forbidden in [".lock()", "log::", "format", "debug_put"] {
        assert!(
            !invalidate.contains(forbidden),
            "prompt invalidation contains {forbidden}"
        );
    }

    let hal_source = include_str!("../src/hal/console_network.rs");
    let final_start = hal_source
        .find("ConsoleNetworkContainmentUnit::Finalize | ConsoleNetworkContainmentUnit::Complete")
        .expect("pure final and complete dispatch");
    let final_end = hal_source[final_start..]
        .find("if let Err(error) = result")
        .map(|offset| final_start + offset)
        .expect("bounded final dispatch");
    let final_step = &hal_source[final_start..final_end];
    assert!(!final_step.contains("ConsoleNetworkContainmentProof"));
    let proof_step = &hal_source[final_end
        ..hal_source
            .find("/// Root slots permanently reserved")
            .expect("bounded containment function")];
    for field in [
        "tcb_suspended: true",
        "scheduling_context_unbound: true",
        "mappings_scrubbed: true",
        "capabilities_revoked: true",
        "objects_deleted: true",
        "generation_fenced: true",
    ] {
        assert!(
            proof_step.contains(field),
            "missing exact proof field {field}"
        );
    }
    assert!(proof_step.contains("if selected != ConsoleNetworkContainmentUnit::Complete"));
    assert!(proof_step.contains("Ok(ConsoleNetworkContainmentTurn::Complete("));
    assert!(!proof_step.contains("log::"));
}

#[test]
fn ninedoor_containment_latches_then_advances_one_successor_committed_unit() {
    let hal_source = include_str!("../src/hal/ninedoor_service.rs");
    let latch_start = hal_source
        .find("pub fn begin_containment(")
        .expect("NineDoor containment resource latch");
    let latch_end = hal_source[latch_start..]
        .find("pub fn contain_one_turn(")
        .map(|offset| latch_start + offset)
        .expect("bounded NineDoor containment latch");
    let latch_section = &hal_source[latch_start..latch_end];
    let take_resources = latch_section
        .find(".take_target_resources_for_containment()")
        .expect("transport fence and resource transfer");
    let store_resources = latch_section
        .find("self.containment_resources = Some(")
        .expect("persistent containment resource latch");
    assert!(take_resources < store_resources);
    assert!(!latch_section.contains("sel4::"));
    assert!(!latch_section.contains("cache_clean"));
    assert!(!latch_section.contains("for "));

    let turn_start = hal_source
        .find("pub fn contain_one_turn(")
        .expect("NineDoor containment turn entrypoint");
    let turn_end = hal_source[turn_start..]
        .find("fn scrub_clean_request_frame(")
        .map(|offset| turn_start + offset)
        .expect("bounded NineDoor containment turn");
    let turn = &hal_source[turn_start..turn_end];
    let select = turn
        .find("let selected = self.containment.select_next();")
        .expect("successor-before-work selection");
    let dispatch = turn
        .find("let result = match selected {")
        .expect("single selected-unit dispatch");
    let restore = turn
        .find("self.containment.restore_selected(selected);")
        .expect("typed synchronous error restore");
    assert!(select < dispatch && dispatch < restore);
    assert!(!turn.contains("for "));
    assert!(!turn.contains("while "));
    assert!(!turn.contains("log::"));
    assert_eq!(turn.matches("self.containment.select_next();").count(), 1);
    assert_eq!(turn.matches("match selected {").count(), 1);
    assert_eq!(
        turn.matches("sel4::suspend_tcb_bounded(self.tcb)").count(),
        1
    );
    assert!(!turn.contains("sel4::suspend_tcb(self.tcb)"));
    for unit in [
        "NineDoorContainmentUnit::SuspendTcb",
        "NineDoorContainmentUnit::ScrubCleanRequestFrame(frame_index)",
        "NineDoorContainmentUnit::UnmapRequestFrame(frame_index)",
        "NineDoorContainmentUnit::UnmapResponseRead(frame_index)",
        "NineDoorContainmentUnit::MapResponseWritable(frame_index)",
        "NineDoorContainmentUnit::ScrubCleanResponseWritable(frame_index)",
        "NineDoorContainmentUnit::UnmapResponseWritable(frame_index)",
        "NineDoorContainmentUnit::RevokeRecoveryReply",
        "NineDoorContainmentUnit::DeleteFaultCap(cap_index)",
        "NineDoorContainmentUnit::RevokeAnchor",
        "NineDoorContainmentUnit::Finalize",
        "NineDoorContainmentUnit::Complete",
    ] {
        assert!(turn.contains(unit), "missing selected unit {unit}");
    }
    let finalize_dispatch = turn
        .find("NineDoorContainmentUnit::Finalize | NineDoorContainmentUnit::Complete => Ok(()),")
        .expect("fallible-free Finalize/Complete dispatch");
    let complete_gate = turn
        .find("if selected != NineDoorContainmentUnit::Complete {")
        .expect("Finalize must reserve a separate completion turn");
    let mark_contained = turn
        .find("self.contained = true;")
        .expect("idempotent Complete publication");
    assert!(finalize_dispatch < complete_gate && complete_gate < mark_contained);

    let request_start = hal_source
        .find("fn scrub_clean_request_frame(")
        .expect("request scrub-clean unit");
    let response_start = hal_source
        .find("fn unmap_response_read(")
        .expect("response read-unmap unit");
    let request = &hal_source[request_start..response_start];
    let request_scrub = request.find("fn scrub_clean_request_frame(").unwrap();
    let request_unmap = request.find("fn unmap_request_frame(").unwrap();
    assert!(request_scrub < request_unmap);
    let request_scrub_unit = &request[request_scrub..request_unmap];
    let request_unmap_unit = &request[request_unmap..];
    assert_eq!(
        request_scrub_unit
            .matches("scrub_clean_root_mapping(frame)")
            .count(),
        1,
    );
    assert!(!request_scrub_unit.contains("unmap_page_cap"));
    assert_eq!(
        request_unmap_unit
            .matches(".unmap_page_cap(frame.cap())")
            .count(),
        1,
    );
    assert!(!request_unmap_unit.contains("scrub_clean_root_mapping"));

    let response_end = hal_source[response_start..]
        .find("fn delete_fault_cap(")
        .map(|offset| response_start + offset)
        .expect("bounded response lifecycle helpers");
    let response = &hal_source[response_start..response_end];
    let unmap = response.find("fn unmap_response_read(").unwrap();
    let remap = response.find("fn map_response_writable(").unwrap();
    let scrub = response.find("fn scrub_clean_response_writable(").unwrap();
    let writable_unmap = response.find("fn unmap_response_writable(").unwrap();
    assert!(unmap < remap && remap < scrub && scrub < writable_unmap);
    let unmap_unit = &response[unmap..remap];
    let remap_unit = &response[remap..scrub];
    let scrub_unit = &response[scrub..writable_unmap];
    let writable_unmap_unit = &response[writable_unmap..];
    assert_eq!(
        unmap_unit.matches(".unmap_page_cap(frame.cap())").count(),
        1
    );
    assert!(!unmap_unit.contains("map_page_into_vspace"));
    assert!(!unmap_unit.contains("scrub_clean_root_mapping"));
    assert_eq!(
        remap_unit
            .matches("sel4::map_page_into_vspace_bounded(")
            .count(),
        1,
    );
    assert!(!remap_unit.contains("sel4::map_page_into_vspace("));
    assert!(remap_unit.contains("let root_vaddr = frame.ptr().as_ptr() as usize;"));
    assert!(!remap_unit.contains("page_get_address"));
    assert!(!remap_unit.contains("remap_revoke_anchor_frame_in_root"));
    assert!(!remap_unit.contains("scrub_clean_root_mapping"));
    assert!(!remap_unit.contains("unmap_page_cap"));
    assert_eq!(
        scrub_unit
            .matches("scrub_clean_root_mapping(frame)")
            .count(),
        1
    );
    assert!(!scrub_unit.contains("unmap_page_cap"));
    assert!(!scrub_unit.contains("map_page_into_vspace"));
    assert!(!scrub_unit.contains("remap_revoke_anchor_frame_in_root"));
    assert_eq!(
        writable_unmap_unit
            .matches(".unmap_page_cap(source_cap)")
            .count(),
        1,
    );
    assert!(!writable_unmap_unit.contains("scrub_clean_root_mapping"));
    assert!(!writable_unmap_unit.contains("map_page_into_vspace"));

    let containment_scrub_start = hal_source
        .find("fn scrub_clean_root_mapping(")
        .expect("containment scrub-clean helper");
    let containment_scrub_end = hal_source[containment_scrub_start..]
        .find("#[allow(clippy::too_many_arguments)]")
        .map(|offset| containment_scrub_start + offset)
        .expect("bounded containment scrub-clean helper");
    let containment_scrub = &hal_source[containment_scrub_start..containment_scrub_end];
    assert_eq!(containment_scrub.matches("cache_clean_bounded(").count(), 1);
    assert!(!containment_scrub.contains("cache_clean("));
    assert_eq!(hal_source.matches("cache_clean_bounded(").count(), 1);
    let init_start = hal_source
        .find("fn map_init_descriptor(")
        .expect("NineDoor init descriptor construction");
    let init_end = hal_source[init_start..]
        .find("fn map_shared_frames(")
        .map(|offset| init_start + offset)
        .expect("bounded init descriptor construction");
    let init = &hal_source[init_start..init_end];
    assert_eq!(init.matches("super::cache::cache_clean(").count(), 1);
    assert!(!init.contains("cache_clean_bounded"));

    let delete_start = hal_source
        .find("fn delete_fault_cap(")
        .expect("bounded fault-cap delete helper");
    let delete_end = hal_source[delete_start..]
        .find("impl<'a> KernelHal<'a>")
        .map(|offset| delete_start + offset)
        .expect("bounded fault-cap delete helper section");
    let delete = &hal_source[delete_start..delete_end];
    assert_eq!(delete.matches("sel4::cnode_delete_bounded(").count(), 1);
    assert!(!delete.contains("sel4::cnode_delete("));
    assert!(turn.contains("revoke_target_service_recovery_reply_bounded"));

    let sel4_source = include_str!("../src/sel4.rs");
    let bounded_map_start = sel4_source
        .find("pub(crate) fn map_page_into_vspace_bounded(")
        .expect("bounded page-map helper");
    let bounded_map_end = sel4_source[bounded_map_start..]
        .find("/// Maps a page-table capability")
        .map(|offset| bounded_map_start + offset)
        .expect("bounded page-map helper section");
    let bounded_map = &sel4_source[bounded_map_start..bounded_map_end];
    assert_eq!(bounded_map.matches("seL4_ARM_Page_Map(").count(), 1);
    assert!(!bounded_map.contains("log::"));
    assert!(!bounded_map.contains("error_name"));
    assert!(!bounded_map.contains("assert_page_aligned"));
    assert!(!bounded_map.contains("expect("));
    assert!(!bounded_map.contains("format"));

    let bounded_suspend_start = sel4_source
        .find("pub(crate) fn suspend_tcb_bounded(")
        .expect("bounded TCB suspend helper");
    let bounded_suspend_end = sel4_source[bounded_suspend_start..]
        .find("/// Resumes a suspended TCB")
        .map(|offset| bounded_suspend_start + offset)
        .expect("bounded TCB suspend helper section");
    let bounded_suspend = &sel4_source[bounded_suspend_start..bounded_suspend_end];
    assert_eq!(bounded_suspend.matches("seL4_TCB_Suspend(").count(), 1);
    assert!(!bounded_suspend.contains("log::"));
    assert!(!bounded_suspend.contains("error_name"));
    assert!(!bounded_suspend.contains("guard_cptr"));
    assert!(!bounded_suspend.contains("format"));
    assert!(!bounded_suspend.contains("uart"));

    let bounded_delete_start = sel4_source
        .find("pub(crate) fn cnode_delete_bounded(")
        .expect("bounded CNode delete helper");
    let bounded_delete_end = sel4_source[bounded_delete_start..]
        .find("/// Safe projection of `seL4_CNode_Revoke`")
        .map(|offset| bounded_delete_start + offset)
        .expect("bounded CNode delete section");
    let bounded_delete = &sel4_source[bounded_delete_start..bounded_delete_end];
    assert_eq!(bounded_delete.matches("seL4_CNode_Delete(").count(), 1);
    for forbidden in ["debug_put_char", "log::", "for ", "while ", "loop "] {
        assert!(
            !bounded_delete.contains(forbidden),
            "bounded CNode delete contains {forbidden}",
        );
    }

    let bridge_source = include_str!("../src/ninedoor.rs");
    let bridge_start = bridge_source
        .find("pub fn contain_target_service_if_faulted(")
        .expect("NineDoor bridge containment entrypoint");
    let bridge_end = bridge_source[bridge_start..]
        .find("fn prepare_namespace<'a>(")
        .map(|offset| bridge_start + offset)
        .expect("bounded NineDoor bridge containment");
    let bridge = &bridge_source[bridge_start..bridge_end];
    let active_guard = bridge.find("if !runtime.containment_active() {").unwrap();
    let mailbox = bridge.find("take_target_service_fault(").unwrap();
    let latch = bridge
        .find("if let Err(error) = runtime.begin_containment(&mut self.namespace_service) {")
        .unwrap();
    let latch_return = bridge[latch..]
        .find("return Ok(NineDoorContainmentTurn::InProgress);")
        .map(|offset| latch + offset)
        .unwrap();
    let one_turn = bridge
        .find("let turn = match runtime.contain_one_turn(hal) {")
        .unwrap();
    let complete = bridge
        .find("NineDoorContainmentTurn::Complete(proof) if proof.complete()")
        .unwrap();
    let remove = bridge.find("self.target_service = None;").unwrap();
    assert!(
        active_guard < mailbox
            && mailbox < latch
            && latch < latch_return
            && latch_return < one_turn
            && one_turn < complete
            && complete < remove,
    );
    assert_eq!(bridge.matches("take_target_service_fault(").count(), 1);
    assert_eq!(
        bridge
            .matches("NineDoorContainmentTurn::Complete(proof) if proof.complete()")
            .count(),
        1
    );
    assert_eq!(bridge.matches("self.target_service = None;").count(), 1);
    assert!(!bridge.contains("log::"));
    assert!(bridge.contains("self.pending_containment_fault_diagnostic = diagnostic;"));
    assert!(bridge.contains("NineDoorContainmentDiagnostic::ContainmentFailed"));
    assert!(bridge.contains("NineDoorContainmentDiagnostic::IncompleteProof"));
    assert!(bridge.contains("self.pending_containment_teardown_diagnostic ="));

    let cache_source = include_str!("../src/hal/cache.rs");
    let bounded_start = cache_source
        .find("pub(crate) fn cache_clean_bounded(")
        .expect("bounded cache-clean helper");
    let bounded_end = cache_source[bounded_start..]
        .find("fn invoke_cache_op(")
        .map(|offset| bounded_start + offset)
        .expect("bounded cache-clean helper section");
    let bounded = &cache_source[bounded_start..bounded_end];
    assert_eq!(bounded.matches("invoke_cache_op(").count(), 1);
    for forbidden in ["CACHE_LOG", "timebase", "log::", "for ", "while ", "loop "] {
        assert!(
            !bounded.contains(forbidden),
            "bounded cache-clean contains {forbidden}",
        );
    }
    let ordinary_start = cache_source
        .find("pub fn cache_clean(")
        .expect("ordinary cache-clean helper");
    let ordinary_end = cache_source[ordinary_start..]
        .find("pub fn cache_invalidate(")
        .map(|offset| ordinary_start + offset)
        .expect("bounded ordinary cache-clean helper");
    let ordinary = &cache_source[ordinary_start..ordinary_end];
    assert!(ordinary.contains("call_cache_op("));
}

#[test]
fn ninedoor_in_progress_and_errors_exclude_the_ordinary_pump() {
    let source = include_str!("../src/event/mod.rs");
    let start = source
        .find("pub fn contain_faulted_ninedoor(")
        .expect("EventPump NineDoor containment hook");
    let end = source[start..]
        .find("/// Quarantine an attached Wi-Fi stack")
        .map(|offset| start + offset)
        .expect("bounded EventPump NineDoor containment hook");
    let containment = &source[start..end];
    assert!(containment.contains("Ok(NineDoorContainmentTurn::Idle) => false"));
    assert!(containment.contains("Ok(NineDoorContainmentTurn::InProgress) => true"));
    assert!(
        containment.contains("Ok(NineDoorContainmentTurn::Complete(proof)) if proof.complete()")
    );
    assert!(containment.contains("Ok(NineDoorContainmentTurn::Complete(_)) | Err(_) => true"));
    assert!(!containment.contains("pump.poll"));
    assert!(!containment.contains("yield_now"));
    assert!(!containment.contains("log::"));
    assert!(!containment.contains("self.audit"));
}

#[test]
fn ninedoor_diagnostics_queue_then_flush_in_later_ordinary_turns() {
    let source = include_str!("../src/event/mod.rs");
    let queue_start = source
        .find("fn queue_containment_diagnostic(")
        .expect("bounded NineDoor diagnostic queue admission");
    let queue_end = source[queue_start..]
        .find("fn containment_diagnostic_pending(")
        .map(|offset| queue_start + offset)
        .expect("bounded NineDoor diagnostic queue section");
    let queue = &source[queue_start..queue_end];
    assert!(queue.contains("PendingConsoleOutputKind::ContainmentLine"));
    assert!(queue.contains("self.pending_console_output.push(output)"));
    for forbidden in [
        "queue_physical_console_output(",
        ".remove(",
        ".insert(",
        ".iter(",
        "emit_serial",
        "flush_pending",
    ] {
        assert!(
            !queue.contains(forbidden),
            "NineDoor queue admission contains {forbidden}",
        );
    }

    let admit_start = source
        .find("fn admit_one_containment_diagnostic(")
        .expect("ordinary NineDoor diagnostic admission leaf");
    let admit_end = source[admit_start..]
        .find("/// Discard only nonessential")
        .map(|offset| admit_start + offset)
        .expect("bounded ordinary NineDoor diagnostic admission leaf");
    let admit = &source[admit_start..admit_end];
    let render = admit.find("let Ok(line) = diagnostic.render()").unwrap();
    let enqueue = admit[render..]
        .find("self.queue_containment_diagnostic(line.as_str())")
        .map(|offset| render + offset)
        .unwrap();
    let commit = admit
        .find("bridge.commit_containment_diagnostic(diagnostic);")
        .unwrap();
    assert!(render < enqueue && enqueue < commit);
    assert!(!admit.contains("flush_pending_console_output_if_idle"));
    assert!(!admit.contains("emit_serial"));

    let flush_start = source
        .find("fn flush_pending_console_output_if_idle(")
        .expect("ordinary retained-output flush");
    let flush_end = source[flush_start..]
        .find("fn queue_pending_console_output_for_linked_serial(")
        .map(|offset| flush_start + offset)
        .expect("bounded ordinary retained-output flush");
    let flush = &source[flush_start..flush_end];
    assert!(flush.contains("ContainmentDiagnosticOutputPhase::Admit"));
    assert!(flush.contains("ContainmentDiagnosticOutputPhase::DrainSpace"));
    let failed_admit = flush
        .find("if self.admit_one_containment_diagnostic()")
        .unwrap();
    let select_drain = flush[failed_admit..]
        .find("ContainmentDiagnosticOutputPhase::DrainSpace")
        .map(|offset| failed_admit + offset)
        .unwrap();
    let failed_return = flush[select_drain..]
        .find("return true;")
        .map(|offset| select_drain + offset)
        .unwrap();
    let first_remove = flush.find("self.pending_console_output.remove(0)").unwrap();
    assert!(
        failed_admit < select_drain && select_drain < failed_return && failed_return < first_remove
    );
    let admission_return = flush[failed_admit..select_drain]
        .find("return true;")
        .map(|offset| failed_admit + offset)
        .unwrap();
    let output_remove = flush.find("self.pending_console_output.remove(0)").unwrap();
    assert!(failed_admit < admission_return && admission_return < output_remove);

    let linked_start = source
        .find("fn queue_pending_console_output_for_linked_serial(")
        .expect("linked serial retained-output leaf");
    let linked_end = source[linked_start..]
        .find("fn reconcile_physical_response_barrier(")
        .map(|offset| linked_start + offset)
        .expect("bounded linked serial retained-output leaf");
    let linked = &source[linked_start..linked_end];
    let linked_admission = linked
        .find("if self.admit_one_containment_diagnostic()")
        .unwrap();
    let linked_return = linked[linked_admission..]
        .find("return true;")
        .map(|offset| linked_admission + offset)
        .unwrap();
    let linked_remove = linked
        .find("self.pending_console_output.remove(0)")
        .unwrap();
    assert!(linked_admission < linked_return && linked_return < linked_remove);

    let selector_start = source
        .find("fn poll_one_split_ordinary_virtio_operator_unit(")
        .expect("compact Operator selector");
    let selector_end = source[selector_start..]
        .find("fn poll_split_ordinary_virtio_serial_io_unit(")
        .map(|offset| selector_start + offset)
        .expect("bounded compact Operator selector");
    let selector = &source[selector_start..selector_end];
    let local = selector
        .find("if self.split_ordinary_virtio_local_seat_input_pending()")
        .unwrap();
    let response = selector
        .find("if self.physical_console_response_pending()")
        .unwrap();
    let diagnostic = selector
        .find("if self.deferred_containment_work_pending()")
        .unwrap();
    let net_event = selector.find("console_event_pending()").unwrap();
    let net_line = selector.find("buffered_console_lines_pending()").unwrap();
    assert!(
        local < response && response < diagnostic && diagnostic < net_event && net_event < net_line
    );

    let linked_poll_start = source
        .find("fn poll_with_linked_serial_runtime(")
        .expect("linked Pi ordinary scheduler");
    let linked_poll_end = source[linked_poll_start..]
        .find("fn linked_runtime_cyw43_rx_admission_can_follow_serial(")
        .map(|offset| linked_poll_start + offset)
        .expect("bounded linked Pi ordinary scheduler");
    let linked_poll = &source[linked_poll_start..linked_poll_end];
    assert!(linked_poll.contains("LinkedRuntimeServicePhase::ContainmentDiagnostic =>"));
    assert!(linked_poll.contains("self.deferred_containment_work_pending()"));
    assert!(linked_poll.contains("queue_pending_console_output_for_linked_serial(true)"));

    let bootstrap_start = source
        .find("pub fn poll_cyw43_bootstrap_supervisor_event_turn(")
        .expect("Pi bootstrap operator scheduler");
    let bootstrap_end = source[bootstrap_start..]
        .find("/// Promote one due gate frontier")
        .map(|offset| bootstrap_start + offset)
        .expect("bounded Pi bootstrap operator scheduler");
    let bootstrap = &source[bootstrap_start..bootstrap_end];
    assert!(bootstrap.contains("self.cyw43_bootstrap_containment_diagnostic_due = true;"));
    assert!(bootstrap.contains("queue_pending_console_output_for_linked_serial(true)"));
    assert!(bootstrap.contains("self.serial_console_turn_active = false;"));
    assert!(bootstrap.contains("self.cyw43_bootstrap_operator_turn_active = false;"));
    let due = bootstrap
        .find("if self.cyw43_bootstrap_containment_diagnostic_due")
        .unwrap();
    let serial_probe = bootstrap[due..]
        .find("self.serial.service_linked_runtime_only_turn()")
        .map(|offset| due + offset)
        .unwrap();
    let serial_input = bootstrap[due..]
        .find("self.consume_serial()")
        .map(|offset| due + offset)
        .unwrap();
    let local_input = bootstrap[due..]
        .find("self.consume_local_seat_buffered_only(LocalSeatConsumePhase::PreRuntime)")
        .map(|offset| due + offset)
        .unwrap();
    let marker = bootstrap[due..]
        .find("self.queue_pending_console_output_for_linked_serial(true)")
        .map(|offset| due + offset)
        .unwrap();
    assert!(
        due < serial_probe
            && serial_probe < serial_input
            && serial_input < local_input
            && local_input < marker
    );

    let event_start = source
        .find("pub fn contain_faulted_console_network(")
        .expect("console-network Recovery hook");
    let event_end = source[event_start..]
        .find("/// Consume and contain one terminal isolated NineDoor service fault")
        .map(|offset| event_start + offset)
        .expect("bounded console-network Recovery hook");
    let event = &source[event_start..event_end];
    assert!(event.contains("self.fence_console_network_authority_quiet();"));
    assert!(!event.contains("log::"));
    assert!(!event.contains("self.audit"));
    assert!(!event.contains("quarantine_network_service_after"));
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

#[test]
fn attach_commits_only_after_ninedoor_success() {
    let event = include_str!("../src/event/mod.rs");
    let branch_start = event
        .find("Command::Attach { role, ticket } => match self.handle_attach(role, ticket)")
        .expect("ATTACH dispatch branch");
    let branch_end = event[branch_start..]
        .find("Command::Tail {")
        .map(|offset| branch_start + offset)
        .expect("ATTACH branch must end before TAIL");
    let branch = &event[branch_start..branch_end];
    assert!(branch.contains("result = Err(err);"));
    assert!(
        !branch.contains("forwarded"),
        "ATTACH must not be forwarded again after its transaction completes",
    );

    let handler_start = event
        .find("fn handle_attach(")
        .expect("root ATTACH transaction helper");
    let handler_end = event[handler_start..]
        .find("\n}\n\n#[cfg(test)]")
        .map(|offset| handler_start + offset)
        .expect("bounded ATTACH helper section");
    let handler = &event[handler_start..handler_end];
    assert!(handler.contains(") -> Result<bool, CommandDispatchError>"));
    let namespace_attach = handler
        .find("bridge\n                    .attach(")
        .expect("NineDoor namespace attach must be part of the transaction");
    let session_commit = handler
        .find("self.session = SessionRole::from_role(requested_role);")
        .expect("root session commit");
    let success_response = handler
        .find("self.emit_ack_ok(ConsoleVerb::Attach.ack_label()")
        .expect("ATTACH success response");
    assert!(
        namespace_attach < session_commit && session_commit < success_response,
        "NineDoor success must precede root authority and OK ATTACH",
    );

    let ninedoor = include_str!("../src/ninedoor.rs");
    let attach_start = ninedoor
        .find("pub fn attach(\n")
        .expect("NineDoor ATTACH implementation");
    let attach_end = ninedoor[attach_start..]
        .find("/// Handle a `tail` request.")
        .map(|offset| attach_start + offset)
        .expect("bounded NineDoor ATTACH section");
    let attach = &ninedoor[attach_start..attach_end];
    let prepare = attach
        .find("self.prepare_namespace(NamespaceOpcode::Attach")
        .expect("target namespace preparation");
    let logger_notice = attach
        .find("boot_log::notify_bridge_attached();")
        .expect("post-namespace logger notification");
    let bridge_commit = attach
        .find("self.attached = true;")
        .expect("NineDoor session commit");
    let session_commit = attach
        .find("self.update_session_context(role, ticket);")
        .expect("NineDoor session context commit");
    let audit_notice = attach
        .find("audit.info(message.as_str());")
        .expect("post-commit attach audit");
    let tracer_notice = attach
        .find("boot_tracer().advance(BootPhase::EPAttachOk);")
        .expect("post-commit attach tracer");
    assert!(
        prepare < session_commit
            && session_commit < bridge_commit
            && bridge_commit < audit_notice
            && audit_notice < logger_notice
            && logger_notice < tracer_notice,
        "local NineDoor authority must commit before attach diagnostics",
    );
    assert!(
        !attach.contains("return Err(NineDoorBridgeError::AttachTimeout)"),
        "optional logger EP-only promotion cannot veto a completed namespace attach",
    );
}
