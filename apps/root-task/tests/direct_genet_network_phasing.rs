// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Guard the one-way Pi GENET direct-data-plane handoff and containment order.
// Author: Lukas Bower

const STACK_SOURCE: &str = include_str!("../src/net/stack.rs");
const ISOLATED_CONSOLE_SOURCE: &str = include_str!("../src/net/isolated_console.rs");
const DRIVER_SOURCE: &str = include_str!("../src/drivers/driver_task_net.rs");
const RUNTIME_SOURCE: &str = include_str!("../../pi4-driver-runtime/src/lib.rs");
const HAL_SOURCE: &str = include_str!("../src/hal/mod.rs");
const CONSOLE_HAL_SOURCE: &str = include_str!("../src/hal/console_network.rs");
const CONSOLE_KERNEL_SOURCE: &str = include_str!("../../console-network-runtime/src/kernel.rs");
const DRIVER_HAL_SOURCE: &str = include_str!("../src/hal/driver_task.rs");
const EVENT_SOURCE: &str = include_str!("../src/event/mod.rs");
const USERLAND_SOURCE: &str = include_str!("../src/userland/mod.rs");
const KERNEL_SOURCE: &str = include_str!("../src/kernel.rs");

#[test]
fn direct_genet_final_sleep_check_adopts_only_durable_physical_work() {
    let adopt = RUNTIME_SOURCE
        .find("fn genet_runtime_adopt_direct_level")
        .expect("direct GENET durable-level admission exists");
    let adopt = &RUNTIME_SOURCE[adopt..];
    let sample_raw = adopt
        .find("let raw_before_mask = genet_irq_raw_sources()")
        .expect("raw owned level is sampled first");
    let sample_rx = adopt
        .find("let rx_pending = genet_rx_hardware_pending(state)")
        .expect("durable RDMA producer is sampled");
    let sample_tx = adopt
        .find("let tx_pending = genet_tx_hardware_completion_pending(state)")
        .expect("durable TDMA completion is sampled");
    let no_work = adopt
        .find("if raw_before_mask == 0 && !rx_pending && !tx_pending")
        .expect("software hints alone cannot adopt an IRQ episode");
    let mask = adopt
        .find("genet_irq_mask_sources()")
        .expect("owned source is masked before service");
    let barrier = adopt[mask..]
        .find("device_store_completion_barrier()")
        .map(|offset| mask + offset)
        .expect("mask store completes before source clear");
    let clear = adopt[barrier..]
        .find("genet_irq_clear_sources(status)")
        .map(|offset| barrier + offset)
        .expect("only the sampled owned source is cleared");
    let retain = adopt[clear..]
        .find("state.irq_ack_pending = true")
        .map(|offset| clear + offset)
        .expect("adopted episode is retained before bounded service");
    assert!(sample_raw < sample_rx && sample_rx < sample_tx && sample_tx < no_work);
    assert!(no_work < mask && mask < barrier && barrier < clear && clear < retain);

    let bounded_final = RUNTIME_SOURCE
        .find("fn genet_runtime_condition_before_sleep_once()")
        .expect("target final check has an MCS refill boundary");
    let bounded_final = &RUNTIME_SOURCE[bounded_final..];
    let begin = bounded_final
        .find("genet_runtime_begin_direct_quantum()")
        .expect("final physical check enters the shared dense MCS window");
    let condition = bounded_final
        .find("genet_runtime_condition_before_sleep_route")
        .expect("final check classifies its exact physical quantum");
    let finish = bounded_final
        .find("genet_runtime_apply_post_quantum_route(route)")
        .expect("final check applies the exact post-quantum route");
    assert!(begin < condition && condition < finish);

    let yield_route = RUNTIME_SOURCE
        .find("fn genet_runtime_apply_post_quantum_route")
        .expect("dense MCS post-quantum router exists");
    let yield_route = &RUNTIME_SOURCE[yield_route..];
    let record = yield_route
        .find("genet_runtime_record_mcs_yield(reason)")
        .expect("the exact yield reason is retained first");
    let yield_call = yield_route
        .find("runtime_yield_current_tcb()")
        .expect("the selected MCS refill boundary yields on target");
    assert!(record < yield_call);

    let loop_body = RUNTIME_SOURCE
        .find("if runtime_idle_prewait_route(\n                notification_route,")
        .expect("final command-ring admission check exists");
    let loop_body = &RUNTIME_SOURCE[loop_body..];
    let command_check = loop_body
        .find("RuntimeIdlePrewaitRoute::ReenterCommandPoll")
        .expect("sequence-last command check completes first");
    let physical_check = loop_body
        .find("genet_runtime_condition_before_sleep_once()")
        .expect("GENET physical condition is checked through the bounded final boundary");
    let block = loop_body
        .find("wait_runtime_command_or_notification(")
        .expect("runtime eventually blocks on the generated endpoint/notification");
    assert!(command_check < physical_check && physical_check < block);
}

#[test]
fn direct_genet_dense_mcs_boundary_resets_after_yield_and_wraps_every_entry() {
    let (production, _) = RUNTIME_SOURCE
        .split_once("#[cfg(test)]\nmod tests {")
        .expect("the runtime production prefix is distinct from its unit tests");

    let yield_start = RUNTIME_SOURCE
        .find("fn runtime_yield_current_tcb()")
        .expect("target runtime exposes one cooperative-yield boundary");
    let yield_body = &RUNTIME_SOURCE[yield_start..];
    let yield_end = yield_body
        .find("\n}\n\n")
        .expect("target cooperative-yield function is bounded");
    let yield_body = &yield_body[..yield_end];
    let syscall = yield_body
        .find("sel4_sys::seL4_Yield()")
        .expect("selected MCS boundary invokes seL4 Yield");
    let reset = yield_body
        .find("genet_runtime_complete_mcs_yield()")
        .expect("GENET software accounting resets after replenishment");
    assert!(
        syscall < reset,
        "clearing before seL4 Yield returns can treat an unrefilled activation as fresh",
    );
    assert_eq!(
        yield_body
            .matches("genet_runtime_complete_mcs_yield()")
            .count(),
        1,
        "the target yield boundary resets GENET accounting exactly once",
    );

    let notification_start = RUNTIME_SOURCE
        .find("fn genet_runtime_service_notification(badge: u32) -> bool")
        .expect("GENET notification entry exists");
    let notification = &RUNTIME_SOURCE[notification_start..];
    let notification_end = notification
        .find("\n}\n\n")
        .expect("GENET notification entry is bounded");
    let notification = &notification[..notification_end];
    let begin = notification
        .find("genet_runtime_begin_direct_quantum()")
        .expect("notification entry performs the shared pre-service guard");
    let service = notification
        .find("genet_runtime_run_dpc_quantum(state, service_badge)")
        .expect("notification entry runs one guarded material DPC quantum");
    let reenter = notification
        .find("route == GenetDirectMcsQuantumRoute::Reenter")
        .expect("only a retained durable condition may select a successor slice");
    let apply = notification
        .find("genet_runtime_apply_post_quantum_route(route)")
        .expect("notification entry applies the shared post-service route");
    assert!(begin < service && service < reenter && reenter < apply);

    let quantum_start = RUNTIME_SOURCE
        .find("fn genet_runtime_run_dpc_quantum(")
        .expect("guarded direct-GENET quantum helper exists");
    let quantum = &RUNTIME_SOURCE[quantum_start..];
    let quantum_end = quantum
        .find("\n}\n\n")
        .expect("guarded direct-GENET quantum helper is bounded");
    let quantum = &quantum[..quantum_end];
    let physical = quantum
        .find("genet_runtime_service_dpc_quantum_state(state, badge)")
        .expect("one physical quantum runs inside the shared guard");
    let classify = quantum
        .find("genet_runtime_finish_dpc_quantum(state, productive)")
        .expect("physical progress is classified before the route returns");
    assert!(physical < classify);

    let window_start = RUNTIME_SOURCE
        .find("impl GenetDirectMcsWindow")
        .expect("dense direct-GENET MCS window exists");
    let window = &RUNTIME_SOURCE[window_start..];
    let finish_start = window
        .find("fn finish(\n        &mut self,")
        .expect("dense-window finish classifier exists");
    let finish_body = &window[finish_start..];
    let quiescent = finish_body
        .find("if !durable_work_ready")
        .expect("quiescence has an explicit MCS route");
    let guard = finish_body
        .find("if now.wrapping_sub(self.start_ticks) >= guard_cycles")
        .expect("elapsed guard remains explicit");
    let cap = finish_body
        .find("if self.attempts >= GENET_DIRECT_MCS_QUANTUM_CAP")
        .expect("attempt cap remains explicit");
    assert!(quiescent < guard && guard < cap);

    let persistent_start = RUNTIME_SOURCE
        .find("fn service_runtime_persistent_source_once(")
        .expect("runtime persistent-source dispatcher exists");
    let persistent = &RUNTIME_SOURCE[persistent_start..];
    let persistent_end = persistent
        .find("\n}\n\n")
        .expect("runtime persistent-source dispatcher is bounded");
    let persistent = &persistent[..persistent_end];
    assert!(persistent
        .contains("RuntimeNotificationRoute::Genet => genet_runtime_service_notification(badge)"));
    assert!(
        !persistent.contains("genet_runtime_service_dpc_state("),
        "the production dispatcher cannot bypass dense-window accounting",
    );

    let final_start = RUNTIME_SOURCE
        .find("fn genet_runtime_condition_before_sleep_once() -> bool")
        .expect("target final physical check exists");
    let final_check = &RUNTIME_SOURCE[final_start..];
    let final_end = final_check
        .find("\n}\n\n")
        .expect("target final physical check is bounded");
    let final_check = &final_check[..final_end];
    assert!(final_check.contains("genet_runtime_begin_direct_quantum()"));
    assert!(final_check.contains("genet_runtime_condition_before_sleep_route"));
    assert!(final_check.contains("genet_runtime_apply_post_quantum_route(route)"));
    assert!(
        !final_check.contains("genet_runtime_service_dpc_state("),
        "the final production entry cannot bypass the shared guarded route",
    );

    let legacy_start = RUNTIME_SOURCE
        .find("fn service_genet_runtime(command: DriverTaskCommandRecord)")
        .expect("GENET endpoint service exists");
    let legacy = &RUNTIME_SOURCE[legacy_start..];
    let direct_fence = legacy
        .find("state.direct_genet_active || state.direct_genet_faulted")
        .expect("direct ownership permanently fences the legacy endpoint path");
    let reject = legacy[direct_fence..]
        .find("return DriverTaskCompletionRecord::fault(command.sequence, FAULT_REJECTED_COMMAND)")
        .map(|offset| direct_fence + offset)
        .expect("direct ownership rejects legacy endpoint service");
    let legacy_poll = legacy
        .find("genet_runtime_poll_rx(state, command.budget)")
        .expect("legacy packet polling remains available before direct handoff");
    assert!(direct_fence < reject && reject < legacy_poll);

    assert_eq!(
        production
            .matches("genet_runtime_service_notification(")
            .count(),
        2,
        "the notification wrapper may appear only at its definition and production dispatcher",
    );
    assert_eq!(
        production
            .matches("genet_runtime_condition_before_sleep_once()")
            .count(),
        2,
        "the final-check wrapper may appear only at its definition and production wait loop",
    );
    assert_eq!(
        production
            .matches("genet_runtime_service_dpc_state(")
            .count(),
        3,
        "the legacy wrapper is limited to its definition and two polls fenced before direct ownership; guarded direct paths use the quantum helper",
    );
}

#[test]
fn diagnostic_probe_is_one_replay_with_pre_and_post_sequence_last_evidence() {
    let root_probe = STACK_SOURCE
        .find("fn refresh_direct_genet_diagnostics")
        .expect("wired stack exposes the bounded diagnostic probe");
    let root_probe = &STACK_SOURCE[root_probe..];
    let before = root_probe
        .find("let previous = console.direct_genet_runtime_diagnostic()")
        .expect("prior child publication is acquired before the probe");
    let replay = root_probe
        .find("refresh_genet_direct_diagnostic(generation)")
        .expect("one exact DGHO replay requests a fresh snapshot");
    let after = root_probe[replay..]
        .find("let snapshot = console.direct_genet_runtime_diagnostic()")
        .map(|offset| replay + offset)
        .expect("fresh child publication is acquired after the probe");
    assert!(before < replay && replay < after);

    let transport = DRIVER_SOURCE
        .find("fn refresh_genet_direct_diagnostic")
        .expect("diagnostic transport exists");
    let transport = &DRIVER_SOURCE[transport..];
    assert!(transport.contains("run_driver_task_ring_service_prompt_slice("));
    assert!(transport.contains("driver_runtime_direct_genet_handoff_completion_exact("));
    assert!(
        !transport[..transport.find("#[cfg(feature").unwrap_or(transport.len())]
            .contains("service_genet_driver_task_pre_poll")
    );

    let handler = RUNTIME_SOURCE
        .find("fn genet_direct_handoff_completion")
        .expect("direct GENET handoff handler exists");
    let handler = &RUNTIME_SOURCE[handler..];
    let active = handler
        .find("if state.direct_genet_active")
        .expect("active replay branch exists");
    let publish = handler[active..]
        .find("genet_direct_publish_runtime_diagnostic(state)")
        .map(|offset| active + offset)
        .expect("active replay publishes before returning READY");
    let ready = handler[publish..]
        .find("DRIVER_RUNTIME_DIRECT_GENET_HANDOFF_DETAIL_READY")
        .map(|offset| publish + offset)
        .expect("snapshot precedes the READY terminal");
    assert!(active < publish && publish < ready);
    let active_branch = &handler[active..ready];
    for forbidden in [
        "genet_direct_drain_rx_hardware(",
        "genet_direct_service_tx(",
        "genet_runtime_service_dpc_state(",
        "genet_irq_clear_sources(",
        "runtime_irq_handler_ack(",
    ] {
        assert!(
            !active_branch.contains(forbidden),
            "DGHO handler unexpectedly services hardware via {forbidden}",
        );
    }

    let reader = CONSOLE_HAL_SOURCE
        .find("fn sample_direct_genet_runtime_diagnostic")
        .expect("root stable reader exists");
    let reader = &CONSOLE_HAL_SOURCE[reader..];
    let first = reader
        .find("first_commit")
        .expect("reader acquires the first commit");
    let prefix = reader
        .find("while offset < DIRECT_GENET_RUNTIME_DIAGNOSTIC_COMMIT_OFFSET")
        .expect("reader copies only the atomic prefix");
    let second = reader
        .find("second_commit")
        .expect("reader rechecks the commit");
    let decode = reader
        .find("DirectGenetRuntimeDiagnostic::decode")
        .expect("reader validates generation and layout");
    assert!(first < prefix && prefix < second && second < decode);
}

#[test]
fn default_stack_delegates_the_direct_genet_probe_without_cross_backend_fallback() {
    let implementation = STACK_SOURCE
        .split_once("impl NetPoller for DefaultNetStack")
        .map(|(_, tail)| tail)
        .expect("default network wrapper implementation exists");
    let probe = implementation
        .split_once("fn refresh_direct_genet_diagnostics")
        .map(|(_, tail)| tail)
        .expect("default wrapper preserves the direct GENET probe");
    let probe = probe
        .split_once("fn status_report")
        .map(|(body, _)| body)
        .expect("direct probe ends before status delegation");

    assert!(
        probe.contains("Self::GenetDriverTask(stack) => stack.refresh_direct_genet_diagnostics()")
    );
    assert!(probe.contains("Self::Rtl8139(_) | Self::Cyw43DriverTask(_) => None"));
    assert!(probe.contains("Self::Virtio(_) => None"));
    assert_eq!(
        probe
            .matches("stack.refresh_direct_genet_diagnostics()")
            .count(),
        1,
        "one netstats request must issue at most one concrete GENET probe",
    );
}

#[test]
fn isolated_direct_genet_listener_is_not_end_to_end_tcp_proof() {
    assert!(STACK_SOURCE.contains("(\"bcmgenet-v5-direct\", \"wired\")"));
    let status = ISOLATED_CONSOLE_SOURCE
        .split_once("fn status_report(&self) -> NetStatusReport")
        .map(|(_, tail)| tail)
        .expect("isolated status report exists");
    let status = status
        .split_once("fn contain_faulted_console_service")
        .map(|(body, _)| body)
        .expect("isolated status report has a finite body");
    assert!(status.contains("super::stack::net_status_tcp_ready("));
    assert!(status.contains("self.console_listener_ready()"));
    assert!(status.contains("self.counters"));
    assert!(!status.contains("tcp_ready: self.console_listener_ready(),"));
}

#[test]
fn console_remains_suspended_until_exact_genet_ready_terminal() {
    let arm = STACK_SOURCE
        .find("runtime.arm_direct_genet()?")
        .expect("direct pages armed");
    let finalize = STACK_SOURCE[arm..]
        .find("runtime.finalize_descriptor(")
        .map(|offset| arm + offset)
        .expect("descriptor finalized");
    let armed_state = STACK_SOURCE[finalize..]
        .find("GenetNetState::DirectArmed { stack, runtime }")
        .map(|offset| finalize + offset)
        .expect("suspended direct-armed state");
    let service = STACK_SOURCE[armed_state..]
        .find("fn service_direct_genet_handoff")
        .map(|offset| armed_state + offset)
        .expect("handoff service");
    assert!(STACK_SOURCE[finalize..service]
        .find("runtime.activate()")
        .is_none());

    let exact_ready = STACK_SOURCE[service..]
        .find("service_genet_direct_link_handoff(generation)")
        .map(|offset| service + offset)
        .expect("exact driver handoff");
    let activate = STACK_SOURCE[exact_ready..]
        .find("runtime.activate()")
        .map(|offset| exact_ready + offset)
        .expect("post-handoff console activation");
    let isolated = STACK_SOURCE[activate..]
        .find("GenetNetState::Isolated")
        .map(|offset| activate + offset)
        .expect("direct isolated owner state");
    assert!(arm < finalize && finalize < armed_state && armed_state < service);
    assert!(service < exact_ready && exact_ready < activate && activate < isolated);
}

#[test]
fn quiescing_is_retry_only_and_malformed_handoff_fails_closed() {
    assert!(
        DRIVER_SOURCE.contains("driver_runtime_direct_genet_handoff_quiescing_completion_exact(")
    );
    assert!(DRIVER_SOURCE.contains("return DriverTaskRetainedServiceTurn::Pending;"));
    assert!(DRIVER_SOURCE.contains("DriverTaskRetainedServiceTurn::Failed"));
    assert!(STACK_SOURCE.contains("runtime.begin_containment().is_ok()"));
    assert!(STACK_SOURCE.contains("action=contain-no-fallback"));
}

#[test]
fn paired_containment_suspends_genet_and_removes_both_signal_caps() {
    let fence = HAL_SOURCE
        .find("fn fence_direct_genet_peer")
        .expect("paired direct-GENET fence");
    let suspend = HAL_SOURCE[fence..]
        .find("suspend_tcb(owner.tcb)")
        .map(|offset| fence + offset)
        .expect("GENET owner suspended");
    let delete = HAL_SOURCE[suspend..]
        .find("cnode_delete(cap.cnode, cap.slot, cap.depth)")
        .map(|offset| suspend + offset)
        .expect("reciprocal cap deleted");
    let publish_fence = HAL_SOURCE[delete..]
        .find("fence_genet_direct_link()")
        .map(|offset| delete + offset)
        .expect("old generation permanently fenced");
    assert!(suspend < delete && delete < publish_fence);
}

#[test]
fn reciprocal_caps_remain_raii_guarded_until_construction_cannot_fail() {
    let outer_constructor = CONSOLE_HAL_SOURCE
        .find("fn construct_generation(")
        .expect("outer generation constructor");
    let boundary = CONSOLE_HAL_SOURCE[outer_constructor..]
        .find("ConsoleNetworkBoundary::new(generation)")
        .map(|offset| outer_constructor + offset)
        .expect("generation validated before construction");
    let install_call = CONSOLE_HAL_SOURCE[boundary..]
        .find("let (root_wake_caps, standard_fault_cap, timeout_fault_cap) = install_caps_and_mcs(")
        .map(|offset| boundary + offset)
        .expect("cap and MCS installation");
    assert!(boundary < install_call);

    let install = CONSOLE_HAL_SOURCE
        .find("let direct_genet_peer_caps =")
        .expect("reciprocal capabilities retained by a guard");
    let fault_cap = CONSOLE_HAL_SOURCE[install..]
        .find("let fault_origin =")
        .map(|offset| install + offset)
        .expect("later fallible fault-cap construction");
    let fault_registration = CONSOLE_HAL_SOURCE[fault_cap..]
        .find("register_target_fault_source(")
        .map(|offset| fault_cap + offset)
        .expect("later fallible fault registration");
    let commit = CONSOLE_HAL_SOURCE[fault_registration..]
        .find("commit_direct_genet_peer_notifications(direct_genet_peer_caps)")
        .map(|offset| fault_registration + offset)
        .expect("peer-cap metadata committed last");
    let success = CONSOLE_HAL_SOURCE[commit..]
        .find("Ok((root_wake_caps, standard_fault_cap, timeout_fault_cap))")
        .map(|offset| commit + offset)
        .expect("construction succeeds immediately after commit");
    assert!(install < fault_cap && fault_cap < fault_registration);
    assert!(fault_registration < commit && commit < success);
    assert!(HAL_SOURCE.contains("impl Drop for ReciprocalLinkCapGuard"));
    assert!(HAL_SOURCE.contains("filter_map(Option::take)"));
}

#[test]
fn handoff_transport_and_quiescing_phases_separate_command_from_legacy_drain() {
    let handoff = DRIVER_SOURCE
        .find("fn service_genet_direct_link_handoff")
        .expect("direct handoff service");
    let pending = DRIVER_SOURCE[handoff..]
        .find("let pending_state = u64::from(token) | GENET_DIRECT_HANDOFF_PENDING")
        .map(|offset| handoff + offset)
        .expect("handoff-pending generation");
    let quiescing = DRIVER_SOURCE[pending..]
        .find("let quiescing_state = u64::from(token) | GENET_DIRECT_HANDOFF_QUIESCING")
        .map(|offset| pending + offset)
        .expect("distinct legacy-drain generation");
    let publish = DRIVER_SOURCE[pending..]
        .find("compare_exchange(0, pending_state")
        .map(|offset| pending + offset)
        .expect("pending state published before the command");
    let issue = DRIVER_SOURCE[publish..]
        .find("run_driver_task_ring_service_retained_service_turn(")
        .map(|offset| publish + offset)
        .expect("handoff command issued");
    let activate = DRIVER_SOURCE[issue..]
        .find("pending_state,\n                        u64::from(token)")
        .map(|offset| issue + offset)
        .expect("READY acceptance removes the pending phase");
    assert!(pending < quiescing && quiescing < publish);
    assert!(publish < issue && issue < activate);
    assert!(DRIVER_SOURCE.contains("state | GENET_DIRECT_PAIR_FAULT_PENDING"));
    let drain_retry = DRIVER_SOURCE[publish..issue]
        .find("state if state == quiescing_state")
        .map(|offset| publish + offset)
        .expect("only the QUIESCING phase admits a drain retry");
    let retry_gate = DRIVER_SOURCE[drain_retry..issue]
        .find("genet_root_mediation_frontiers_quiescent()")
        .expect("QUIESCING retry waits for every legacy frontier");
    let reserve_before_retry = DRIVER_SOURCE[drain_retry..issue]
        .find("quiescing_state,\n                    pending_state")
        .expect("READY retry reserves the retained transport before issue");
    assert!(retry_gate < issue - drain_retry);
    assert!(reserve_before_retry < issue - drain_retry);

    let exact_quiescing = DRIVER_SOURCE[issue..activate]
        .find("driver_runtime_direct_genet_handoff_quiescing_completion_exact")
        .map(|offset| issue + offset)
        .expect("exact QUIESCING terminal validation");
    let publish_drain = DRIVER_SOURCE[exact_quiescing..activate]
        .find("pending_state,\n                        quiescing_state")
        .expect("QUIESCING publishes legacy-drain authority");
    assert!(publish_drain < activate - exact_quiescing);

    let pending_branch = STACK_SOURCE
        .find("DriverTaskRetainedServiceTurn::Pending =>")
        .expect("root handoff pending branch");
    let reservation = STACK_SOURCE[pending_branch..]
        .find("genet_direct_handoff_requires_reserved_turn(")
        .expect("transport Pending reserves the outer root turn");
    assert!(reservation < 512);
}

#[test]
fn direct_genet_cutover_rearms_irq_between_quiescence_and_direct_resume() {
    let cutover = RUNTIME_SOURCE
        .find("fn genet_direct_advance_cutover")
        .expect("direct GENET cutover state machine exists");
    let direct_phase = RUNTIME_SOURCE[cutover..]
        .find("GenetDirectCutoverPhase::Direct =>")
        .map(|offset| cutover + offset)
        .expect("cutover has a finite direct terminal phase");
    let body = &RUNTIME_SOURCE[cutover..direct_phase];

    let frozen = body
        .find("GenetDirectCutoverPhase::RdmaFrozen =>")
        .expect("cutover has a finite frozen ownership boundary");
    let mask = body[frozen..]
        .find("genet_irq_mask_sources();")
        .map(|offset| frozen + offset)
        .expect("the frozen boundary masks packet IRQ sources");
    let clear = body[mask..]
        .find("genet_irq_clear_sources(genet_irq_raw_sources());")
        .map(|offset| mask + offset)
        .expect("the frozen boundary clears the masked source");
    let empty = body[clear..]
        .find("genet_irq_raw_sources() != 0 || genet_rx_hardware_pending(state)")
        .map(|offset| clear + offset)
        .expect("raw source and durable RX state are rechecked");
    let rearm = body[empty..]
        .find("runtime_irq_handler_ack(state.irq_handler_slot)")
        .map(|offset| empty + offset)
        .expect("the finite boundary rearms a queued but unobserved IRQ lifetime");
    let generation = body[rearm..]
        .find("state.direct_genet_generation = generation;")
        .map(|offset| rearm + offset)
        .expect("direct generation publishes after handler rearm");
    let rdma_resume = body[generation..]
        .find("GENET_RDMA_REG_BASE + GENET_DMA_CTRL")
        .map(|offset| generation + offset)
        .expect("RDMA resumes after direct publication");
    let mac_resume = body[rdma_resume..]
        .find("GENET_UMAC_CMD")
        .map(|offset| rdma_resume + offset)
        .expect("MAC RX resumes after RDMA");
    let unmask = body[mac_resume..]
        .find("genet_irq_unmask_sources()")
        .map(|offset| mac_resume + offset)
        .expect("packet IRQ sources unmask last");

    assert!(mask < clear && clear < empty && empty < rearm);
    assert!(rearm < generation && generation < rdma_resume);
    assert!(rdma_resume < mac_resume && mac_resume < unmask);
}

#[test]
fn genet_standard_or_timeout_fault_latches_coupled_console_containment() {
    let supervisor = DRIVER_HAL_SOURCE
        .find("fn root_driver_supervisor_contain_fault")
        .expect("driver supervisor containment");
    let class_match = DRIVER_HAL_SOURCE[supervisor..]
        .find("match record.fault_class")
        .map(|offset| supervisor + offset)
        .expect("standard and timeout faults share containment");
    let revoked = DRIVER_HAL_SOURCE[class_match..]
        .find("DRIVER_TASK_MCS_CALL_REVOKED")
        .map(|offset| class_match + offset)
        .expect("GENET generic containment completes first");
    let pair_latch = DRIVER_HAL_SOURCE[revoked..]
        .find("publish_genet_direct_pair_fault()")
        .map(|offset| revoked + offset)
        .expect("direct peer-fault latch");
    assert!(class_match < revoked && revoked < pair_latch);

    assert!(EVENT_SOURCE.contains("fn contain_faulted_direct_genet_pair"));
    assert!(EVENT_SOURCE.contains("begin_direct_genet_peer_fault_containment()"));
    assert!(EVENT_SOURCE.contains("acknowledge_genet_direct_pair_fault()"));
    let paired = USERLAND_SOURCE
        .find("pump.contain_faulted_direct_genet_pair(hal)")
        .expect("paired fault recovery turn");
    let ordinary = USERLAND_SOURCE[paired..]
        .find("pump.contain_faulted_console_network(hal)")
        .map(|offset| paired + offset)
        .expect("ordinary console fault recovery follows");
    assert!(paired < ordinary);
}

#[test]
fn direct_armed_async_driver_fault_starts_peer_containment_before_ack() {
    let begin = STACK_SOURCE
        .find("fn begin_direct_pair_fault_containment")
        .expect("direct pair-fault dispatcher");
    let end = STACK_SOURCE[begin..]
        .find("fn containment_required")
        .map(|offset| begin + offset)
        .expect("end of direct pair-fault dispatcher");
    let direct_armed = STACK_SOURCE[begin..end]
        .find("GenetNetState::DirectArmed { runtime, .. }")
        .map(|offset| begin + offset)
        .expect("suspended direct-armed state is explicitly contained");
    let containment = STACK_SOURCE[direct_armed..end]
        .find("runtime.begin_containment()?")
        .map(|offset| direct_armed + offset)
        .expect("console containment begins in the direct-armed window");
    let failed = STACK_SOURCE[containment..end]
        .find("self.state = GenetNetState::Failed")
        .map(|offset| containment + offset)
        .expect("direct-armed resources and policy move into failed containment");
    let begun = STACK_SOURCE[failed..end]
        .find("return Ok(true)")
        .map(|offset| failed + offset)
        .expect("pair handler may acknowledge only after containment begins");
    assert!(direct_armed < containment && containment < failed && failed < begun);

    let pair_handler = EVENT_SOURCE
        .find("fn contain_faulted_direct_genet_pair")
        .expect("root pair-fault handler");
    let begin_peer = EVENT_SOURCE[pair_handler..]
        .find("begin_direct_genet_peer_fault_containment()")
        .map(|offset| pair_handler + offset)
        .expect("peer containment dispatch");
    let acknowledge = EVENT_SOURCE[begin_peer..]
        .find("acknowledge_genet_direct_pair_fault()")
        .map(|offset| begin_peer + offset)
        .expect("pair-fault acknowledgment");
    assert!(begin_peer < acknowledge);
}

#[test]
fn wifi_shell_never_maps_or_installs_the_wired_direct_link() {
    let wifi = KERNEL_SOURCE
        .find("if net_stack.is_none() && net_deferred_config.is_some()")
        .expect("deferred WiFi shell branch");
    let wifi_constructor = KERNEL_SOURCE[wifi..]
        .find("construct_console_network_runtime_shell(1)")
        .map(|offset| wifi + offset)
        .expect("WiFi uses the non-direct shell");
    let wired = KERNEL_SOURCE[wifi_constructor..]
        .find("requires_preseal_console_network_runtime")
        .map(|offset| wifi_constructor + offset)
        .expect("wired preseal branch");
    let wired_constructor = KERNEL_SOURCE[wired..]
        .find("construct_direct_genet_console_network_runtime_shell(1)")
        .map(|offset| wired + offset)
        .expect("wired uses the direct GENET shell");
    assert!(wifi < wifi_constructor && wifi_constructor < wired);
    assert!(wired < wired_constructor);

    assert!(CONSOLE_HAL_SOURCE.contains("let (direct_genet_layout, direct_genet_root_ptrs) ="));
    assert!(CONSOLE_HAL_SOURCE.contains("generation,\n        direct_genet,\n    )?;"));
    assert!(CONSOLE_HAL_SOURCE.contains(
        "install_direct_genet_peer_notifications(\n        child_cnode,\n        root_to_child_notification,\n        direct_genet,"
    ));
    assert!(CONSOLE_HAL_SOURCE.contains("if direct_genet {"));
    assert!(CONSOLE_HAL_SOURCE.contains("direct_genet_frame_count"));
}

#[test]
fn compact_direct_genet_successor_composes_only_the_exact_adapter_response() {
    let successor_start = EVENT_SOURCE
        .find("fn service_direct_genet_compact_command_successor")
        .expect("compact direct-GENET successor exists");
    let successor_end = EVENT_SOURCE[successor_start..]
        .find("fn linked_runtime_direct_genet_response_stage_ready")
        .map(|offset| successor_start + offset)
        .expect("compact successor has a finite body");
    let successor = &EVENT_SOURCE[successor_start..successor_end];
    let dispatch = successor
        .find("let dispatched = self.dispatch_one_buffered_network_line()")
        .expect("one buffered command is dispatched");
    let consume = successor
        .find("command_before.command_queue.checked_sub(1) == Some(after.command_queue)")
        .expect("command consumption is proven from the exact child queue");
    let compose = successor
        .find("self.compose_direct_genet_sync_response_adapter_only(expected)")
        .expect("the sealed response is moved through the adapter-only composer");
    let control = successor
        .find("net.service_direct_genet_command_control_with_budget(")
        .expect("the affirmative isolated control-only hook follows composition");
    assert!(dispatch < consume && consume < compose && compose < control);
    assert!(
        !successor.contains("accepted_commands_after"),
        "a denied command still owns its typed ERR response and must not be excluded by an accepted-only delta",
    );

    let adapter_start = EVENT_SOURCE
        .find("fn try_enqueue_direct_genet_sync_line_adapter_only")
        .expect("adapter-only line enqueue exists");
    let adapter_end = EVENT_SOURCE[adapter_start..]
        .find("fn refill_sync_cache_line")
        .map(|offset| adapter_start + offset)
        .expect("adapter-only line enqueue has a finite body");
    let adapter = &EVENT_SOURCE[adapter_start..adapter_end];
    assert!(adapter.contains("net.send_console_line(line)"));
    assert!(adapter.contains("net.send_console_terminal_line(line)"));
    for forbidden in [
        "mirror_local_seat_network_line_if_ready(",
        "poll_runtime(",
        "poll_with_budget(",
        "service_linked_runtime_only_turn(",
        "poll_local_seat_backend_for_ingress(",
    ] {
        assert!(
            !adapter.contains(forbidden),
            "compact adapter unexpectedly enters side-effecting work via {forbidden}",
        );
    }

    let composer_start = EVENT_SOURCE
        .find("fn compose_direct_genet_sync_response_adapter_only")
        .expect("bounded adapter-only composer exists");
    let composer_end = EVENT_SOURCE[composer_start..]
        .find("fn flush_pending_sync_response")
        .map(|offset| composer_start + offset)
        .expect("adapter-only composer has a finite body");
    let composer = &EVENT_SOURCE[composer_start..composer_end];
    assert!(composer.contains("pending.sync_sealed()"));
    assert!(composer.contains("console_network_abi::SEND_BATCH_MAX_RECORDS"));
    assert!(composer.contains("terminal.as_str(),\n                true,"));
    assert!(composer.contains("try_enqueue_direct_genet_sync_line_adapter_only"));
    assert!(!composer.contains("mirror_local_seat_network_line_if_ready("));
    assert!(!composer.contains("poll_runtime("));

    let identity_check = composer
        .find("if pending.response_identity != Some(expected)")
        .expect("composer rechecks the retained response identity");
    let restore = composer[identity_check..]
        .find("self.pending_stream = Some(pending)")
        .map(|offset| identity_check + offset)
        .expect("identity drift restores the retained response");
    let identity_drift = composer[restore..]
        .find("DirectGenetResponseComposeOutcome::IdentityDrift")
        .map(|offset| restore + offset)
        .expect("identity drift is causally classified");
    assert!(identity_check < restore && restore < identity_drift);

    let compose_outcome = successor
        .find(
            "let compose_outcome = self.compose_direct_genet_sync_response_adapter_only(expected)",
        )
        .expect("compact successor consumes the typed compose result");
    let drift_arm = successor[compose_outcome..]
        .find("DirectGenetResponseComposeOutcome::IdentityDrift")
        .map(|offset| compose_outcome + offset)
        .expect("successor handles response identity drift explicitly");
    let fence = successor[drift_arm..]
        .find("self.fence_console_network_authority_quiet()")
        .map(|offset| drift_arm + offset)
        .expect("response identity drift enters quiet containment");
    let fault = successor[fence..]
        .find("crate::net::DirectGenetCommandControlOutcome::Fault")
        .map(|offset| fence + offset)
        .expect("identity drift returns the typed compact fault");
    assert!(compose_outcome < drift_arm && drift_arm < fence && fence < fault);
    for (compose, deferred) in [
        (
            "DirectGenetResponseComposeOutcome::NotSealed =>",
            "DirectGenetCommandControlDeferReason::ComposeOpen",
        ),
        (
            "DirectGenetResponseComposeOutcome::Backpressure =>",
            "DirectGenetCommandControlDeferReason::ComposeBackpressure",
        ),
    ] {
        assert!(successor.contains(compose), "missing {compose}");
        assert!(successor.contains(deferred), "missing {deferred}");
    }

    let no_pending = successor
        .find("DirectGenetResponseComposeOutcome::NoPending")
        .expect("non-SyncCapture responses have an explicit compact outcome");
    let identity_drift = successor[no_pending..]
        .find("DirectGenetResponseComposeOutcome::IdentityDrift")
        .map(|offset| no_pending + offset)
        .expect("identity drift follows the non-SyncCapture response arms");
    let no_pending_arms = &successor[no_pending..identity_drift];
    assert!(
        no_pending_arms.contains("linked_runtime_direct_genet_no_pending_terminal_stage_ready()")
    );
    assert!(no_pending_arms.contains("DirectGenetCommandControlDeferReason::OutputMissing"));
    assert!(
        !no_pending_arms.contains("fence_console_network_authority_quiet"),
        "absence of SyncCapture is not a containment event when the exact response lane is ready",
    );
}

#[test]
fn compact_direct_genet_denial_and_passive_command_keep_exact_terminal_cuts() {
    let refusal_start = EVENT_SOURCE
        .find("fn emit_refusal(&mut self")
        .expect("typed refusal renderer exists");
    let refusal_end = EVENT_SOURCE[refusal_start..]
        .find("fn net_disabled_refusal_detail")
        .map(|offset| refusal_start + offset)
        .expect("typed refusal renderer has a finite body");
    let refusal = &EVENT_SOURCE[refusal_start..refusal_end];
    assert!(refusal.contains("self.emit_ack_err(verb, Some(detail.as_str()))"));

    let terminal_start = EVENT_SOURCE
        .find("fn emit_terminal_console_line")
        .expect("terminal response capture exists");
    let terminal_end = EVENT_SOURCE[terminal_start..]
        .find("fn try_emit_sync_response_line")
        .map(|offset| terminal_start + offset)
        .expect("terminal response capture has a finite body");
    let terminal = &EVENT_SOURCE[terminal_start..terminal_end];
    assert!(terminal.contains("PendingStreamMode::SyncCapture"));
    assert!(terminal.contains("pending.terminal_line = HeaplessString::try_from(line).ok()"));

    let quantum_start = EVENT_SOURCE
        .find("fn poll_root_control_quantum_for_state")
        .expect("Pi root-control quantum exists");
    let quantum_end = EVENT_SOURCE[quantum_start..]
        .find("fn isolated_virtio_compact_path_attached")
        .map(|offset| quantum_start + offset)
        .expect("Pi root-control quantum has a finite body");
    let quantum = &EVENT_SOURCE[quantum_start..quantum_end];
    let poll = quantum.find("self.poll();").expect("one rotor unit runs");
    let pre_compact_passive = quantum[poll..]
        .find("if self.pi_root_control_passive_admission_pending()")
        .map(|offset| poll + offset)
        .expect("ordinary dispatch stops at a passive command");
    let compact = quantum[pre_compact_passive..]
        .find("self.service_direct_genet_compact_command_successor()")
        .map(|offset| pre_compact_passive + offset)
        .expect("compact command successor follows the ordinary cut");
    let post_compact_passive = quantum[compact..]
        .find("if self.pi_root_control_passive_admission_pending()")
        .map(|offset| compact + offset)
        .expect("compact dispatch rechecks the passive cut");
    let compact_metrics = quantum[post_compact_passive..]
        .find("net_direct_genet_immediate_dispatches")
        .map(|offset| post_compact_passive + offset)
        .expect("metrics follow the passive cut");
    assert!(poll < pre_compact_passive);
    assert!(pre_compact_passive < compact && compact < post_compact_passive);
    assert!(post_compact_passive < compact_metrics);

    let successor_start = EVENT_SOURCE
        .find("fn service_direct_genet_compact_command_successor")
        .expect("compact direct-GENET successor exists");
    let successor_end = EVENT_SOURCE[successor_start..]
        .find("fn linked_runtime_direct_genet_response_stage_ready")
        .map(|offset| successor_start + offset)
        .expect("compact successor has a finite body");
    let successor = &EVENT_SOURCE[successor_start..successor_end];
    let dispatch = successor
        .find("let dispatched = self.dispatch_one_buffered_network_line()")
        .expect("compact successor dispatches one command");
    let passive = successor[dispatch..]
        .find("if self.pi_root_control_passive_admission_pending()")
        .map(|offset| dispatch + offset)
        .expect("passive command is cut after classification");
    let command = successor[passive..]
        .find("if !command_consumed")
        .map(|offset| passive + offset)
        .expect("unconsumed command is classified separately");
    let compose = successor[command..]
        .find("self.compose_direct_genet_sync_response_adapter_only(expected)")
        .map(|offset| command + offset)
        .expect("response composition follows both command cuts");
    assert!(dispatch < passive && passive < command && command < compose);
}

#[test]
fn compact_control_hook_is_genet_only_and_virtio_fails_closed() {
    let wrapper_start = STACK_SOURCE
        .find("impl NetPoller for DefaultNetStack")
        .expect("default stack wrapper exists");
    let hook_start = STACK_SOURCE[wrapper_start..]
        .find("fn service_direct_genet_command_control_with_budget")
        .map(|offset| wrapper_start + offset)
        .expect("default wrapper exposes the compact control hook");
    let hook_end = STACK_SOURCE[hook_start..]
        .find("fn request_disconnect")
        .map(|offset| hook_start + offset)
        .expect("compact control hook has a finite body");
    let hook = &STACK_SOURCE[hook_start..hook_end];
    assert!(hook.contains("Self::GenetDriverTask(stack) =>"));
    assert!(hook.contains(
        "Self::Rtl8139(_) | Self::Cyw43DriverTask(_) => {\n                Ok(super::DirectGenetCommandControlOutcome::Unsupported)"
    ));
    assert!(hook
        .contains("Self::Virtio(_) => Ok(super::DirectGenetCommandControlOutcome::Unsupported)"));
    assert_eq!(
        hook.matches("stack.service_direct_genet_command_control_with_budget")
            .count(),
        1,
        "only the concrete GENET backend may delegate the affirmative hook",
    );

    let isolated_start = ISOLATED_CONSOLE_SOURCE
        .find("fn service_direct_genet_command_control_with_budget")
        .expect("isolated compact control implementation exists");
    let isolated_end = ISOLATED_CONSOLE_SOURCE[isolated_start..]
        .find("fn driver_task_contract")
        .map(|offset| isolated_start + offset)
        .expect("isolated compact control implementation has a finite body");
    let isolated = &ISOLATED_CONSOLE_SOURCE[isolated_start..isolated_end];
    assert!(isolated.contains(
        "D::driver_task_contract() != crate::hal::driver_task::GENET_DRIVER_TASK_CONTRACT"
    ));
    assert!(isolated.contains("return Ok(DirectGenetCommandControlOutcome::Unsupported)"));
    for reason in [
        "DirectGenetCommandControlDeferReason::PriorBatch",
        "DirectGenetCommandControlDeferReason::ControlBusy",
        "DirectGenetCommandControlDeferReason::OutputMissing",
        "DirectGenetCommandControlDeferReason::StageBackpressure",
    ] {
        assert!(isolated.contains(reason), "missing exact {reason} cut");
    }
    assert!(isolated.contains("direct-genet-command-control-response-lane"));

    let successor_start = EVENT_SOURCE
        .find("fn service_direct_genet_compact_command_successor")
        .expect("compact successor exists");
    let successor_end = EVENT_SOURCE[successor_start..]
        .find("fn linked_runtime_direct_genet_response_stage_ready")
        .map(|offset| successor_start + offset)
        .expect("compact successor has a finite body");
    let successor = &EVENT_SOURCE[successor_start..successor_end];
    let unsupported = successor
        .find("crate::net::DirectGenetCommandControlOutcome::Unsupported")
        .expect("unsupported backend is explicitly classified");
    let fence = successor[unsupported..]
        .find("self.fence_console_network_authority_quiet()")
        .map(|offset| unsupported + offset)
        .expect("unsupported compact execution fails closed");
    assert!(unsupported < fence);
}

#[test]
fn direct_genet_command_quiesce_fences_both_direct_service_entry_cuts() {
    let pre_wait = CONSOLE_KERNEL_SOURCE
        .find("ChildTurnUnit::Idle => direct_service_pending && !direct_genet_command_quiesced")
        .expect("command quiesce fences the pre-wait direct-service admission");
    let post_wait = CONSOLE_KERNEL_SOURCE
        .find("&& !direct_genet_command_quiesced\n            && unit == ChildTurnUnit::Idle")
        .expect("command quiesce fences the post-wait direct-service admission");
    let command_publication = CONSOLE_KERNEL_SOURCE
        .find("awaiting_root_command_control = true")
        .expect("command publication closes the causal fence");
    let exact_control = CONSOLE_KERNEL_SOURCE
        .find("if release_command_quiesce")
        .expect("only the exact applied control reopens the causal fence");
    assert!(pre_wait < command_publication);
    assert!(post_wait < command_publication);
    assert!(command_publication < exact_control);
}
