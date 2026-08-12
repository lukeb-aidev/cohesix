// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Guard bounded QEMU VirtIO Runtime/Network-unit ordering and device ownership.
// Author: Lukas Bower

#[path = "../src/net/isolated_network_turn.rs"]
mod isolated_network_turn;

fn section<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start = source
        .find(start)
        .unwrap_or_else(|| unreachable!("source must contain {start}"));
    let end = source[start..]
        .find(end)
        .map(|offset| start + offset)
        .unwrap_or_else(|| unreachable!("source section must end at {end}"));
    &source[start..end]
}

fn marker(source: &str, value: &str) -> usize {
    source
        .find(value)
        .unwrap_or_else(|| unreachable!("source must contain {value}"))
}

#[test]
fn split_runtime_commits_then_uses_only_the_compact_timer_reconcile_prelude() {
    let source = include_str!("../src/event/mod.rs");
    let timer = section(
        source,
        "fn poll_runtime_timer_prelude(&mut self)",
        "fn poll_split_ordinary_virtio_runtime_prelude(&mut self)",
    );
    let snapshot = marker(
        timer,
        "let timebase_now_ms = crate::hal::timebase().now_ms();",
    );
    let poll = marker(timer, "self.timer.poll(timebase_now_ms)");
    let update_now = marker(timer, "self.now_ms = tick.now_ms;");
    let update_metrics = marker(timer, "self.metrics.timer_ticks =");
    let publish_timebase = marker(timer, "crate::hal::set_timebase_now_ms(self.now_ms);");
    let trace_condition = marker(timer, "if tick.tick % 8_000 == 0 {");
    assert!(
        snapshot < poll
            && poll < update_now
            && update_now < update_metrics
            && update_metrics < publish_timebase
            && publish_timebase < trace_condition
    );

    let prelude = section(
        source,
        "fn poll_split_ordinary_virtio_runtime_prelude(&mut self)",
        "fn poll_split_ordinary_virtio_runtime_turn(&mut self)",
    );
    assert_eq!(
        prelude
            .matches("self.poll_runtime_timer_prelude();")
            .count(),
        1
    );
    assert_eq!(
        prelude
            .matches("self.reconcile_cyw43_network_ready_hdmi();")
            .count(),
        1,
    );
    assert!(!prelude.contains("poll_runtime_inner"));
    assert!(!prelude.contains("poll_runtime_without_control_tail"));
    assert!(!prelude.contains("net_poll"));

    let turn = section(
        source,
        "fn poll_split_ordinary_virtio_runtime_turn(&mut self)",
        "fn poll_runtime_inner(",
    );
    let selected = marker(turn, "let unit = self.ordinary_runtime_unit;");
    let successor = marker(turn, "self.ordinary_runtime_unit = unit.next();");
    let compact_prelude = marker(turn, "self.poll_split_ordinary_virtio_runtime_prelude();");
    let dispatch = marker(turn, "match unit {");
    assert!(selected < successor && successor < compact_prelude && compact_prelude < dispatch);
    assert!(!turn.contains("poll_runtime_inner"));
    assert!(!turn.contains("poll_runtime_without_control_tail"));

    let generic = section(
        source,
        "fn poll_runtime_inner(",
        "fn service_pending_reboot(",
    );
    assert_eq!(
        generic
            .matches("self.poll_runtime_timer_prelude();")
            .count(),
        1,
        "split and generic paths must retain one shared timer/timebase contract",
    );
}

#[test]
fn split_virtio_dispatch_bypasses_both_generic_eventpump_frames() {
    let source = include_str!("../src/event/mod.rs");
    let dispatcher = section(
        source,
        "pub fn poll(&mut self)",
        "fn isolated_virtio_compact_path_attached(&self)",
    );
    let compact = marker(dispatcher, "self.poll_split_ordinary_virtio_compact();");
    let generic = marker(dispatcher, "self.poll_generic();");
    assert!(compact < generic);
    assert!(!dispatcher.contains("poll_ordinary_operator_turn"));

    let split = section(
        source,
        "fn poll_split_ordinary_virtio_compact(&mut self)",
        "fn poll_split_ordinary_virtio_compact_operator_turn(&mut self)",
    );
    let reconcile_guard = marker(
        split,
        "if self.physical_response_barrier == PhysicalResponseBarrier::TailInFlight",
    );
    let reconcile = marker(split, "self.reconcile_physical_response_barrier();");
    let reconcile_progress = marker(
        split,
        "if self.physical_response_barrier != PhysicalResponseBarrier::TailInFlight",
    );
    let reconcile_progress_return =
        marker(&split[reconcile_progress..], "return;") + reconcile_progress;
    let reconcile_operator = marker(
        &split[reconcile_progress_return..],
        "self.poll_split_ordinary_virtio_compact_operator_turn();",
    ) + reconcile_progress_return;
    let reconcile_return = marker(&split[reconcile_operator..], "return;") + reconcile_operator;
    let stream_tail_guard = marker(
        split,
        "if !self.stream_end_pending && self.stream_prompt_pending",
    );
    let stream_tail = marker(split, "self.queue_stream_prompt_tail_if_ready();");
    let stream_tail_progress = marker(split, "if !self.stream_prompt_pending");
    let stream_tail_progress_return =
        marker(&split[stream_tail_progress..], "return;") + stream_tail_progress;
    let stream_tail_operator = marker(
        &split[stream_tail_progress_return..],
        "self.poll_split_ordinary_virtio_compact_operator_turn();",
    ) + stream_tail_progress_return;
    let stream_tail_return =
        marker(&split[stream_tail_operator..], "return;") + stream_tail_operator;
    let reboot = marker(split, "self.service_pending_reboot();");
    let reboot_return = marker(&split[reboot..], "return;") + reboot;
    let selected = marker(split, "let phase = self.ordinary_service_phase;");
    let successor = marker(split, "self.ordinary_service_phase = phase.next();");
    let dispatch = marker(split, "match phase {");
    assert!(
        reconcile_guard < reconcile
            && reconcile < reconcile_progress
            && reconcile_progress < reconcile_progress_return
            && reconcile_progress_return < reconcile_operator
            && reconcile_operator < reconcile_return
            && reconcile_return < stream_tail_guard
            && stream_tail_guard < stream_tail
            && stream_tail < stream_tail_progress
            && stream_tail_progress < stream_tail_progress_return
            && stream_tail_progress_return < stream_tail_operator
            && stream_tail_operator < stream_tail_return
            && stream_tail_return < reboot
            && reboot < reboot_return
            && reboot_return < selected
            && selected < successor
            && successor < dispatch
    );
    assert_eq!(
        split
            .matches("self.poll_split_ordinary_virtio_compact_operator_turn();")
            .count(),
        2,
        "only the two no-progress predispatch paths may admit Operator work",
    );
    for forbidden in [
        "begin_cyw43_outer_event_turn",
        "poll_driver_task_sdio_deadline_fault_hint",
        "poll_linked_runtime_cyw43_rx_admission",
        "poll_ordinary_operator_turn",
        "poll_generic",
    ] {
        assert!(
            !split.contains(forbidden),
            "split path composed {forbidden}"
        );
    }

    let generic = section(
        source,
        "fn poll_generic(&mut self)",
        "/// Run one isolated QEMU VirtIO predispatch or phase unit",
    );
    assert!(generic.contains(
        "self.reconcile_physical_response_barrier();\n        self.queue_stream_prompt_tail_if_ready();"
    ));
}

#[test]
fn compact_operator_preserves_priority_and_never_calls_the_generic_body() {
    let source = include_str!("../src/event/mod.rs");
    let selector = section(
        source,
        "fn poll_one_split_ordinary_virtio_operator_unit(&mut self)",
        "fn poll_split_ordinary_virtio_serial_io_unit(",
    );
    let serial = marker(selector, "self.poll_split_ordinary_virtio_serial_io_unit");
    let response = marker(selector, "if self.physical_console_response_pending() {");
    let event = marker(selector, "net.console_event_pending()");
    let line = marker(selector, "net.buffered_console_lines_pending()");
    let output = marker(selector, "if !self.pending_console_output.is_empty() {");
    let local = marker(
        selector,
        "if self.split_ordinary_virtio_local_seat_input_pending()",
    );
    let display = marker(
        selector,
        "if self.split_ordinary_virtio_display_attach_pending()",
    );
    assert!(serial < local && local < response);
    assert!(response < event && event < line && line < output && output < display);
    assert!(!selector.contains("poll_ordinary_operator_turn"));
    assert!(!selector.contains("poll_runtime"));
}

#[test]
fn compact_serial_probe_and_dispatch_are_distinct_operator_units() {
    let event = include_str!("../src/event/mod.rs");
    let selector = section(
        event,
        "fn poll_one_split_ordinary_virtio_operator_unit(&mut self)",
        "fn poll_split_ordinary_virtio_serial_io_unit(",
    );
    let retained_dispatch = marker(
        selector,
        "if self.ordinary_operator_unit == OrdinaryOperatorUnit::SerialDispatch",
    );
    let dispatch_successor = marker(
        selector,
        "self.ordinary_operator_unit = OrdinaryOperatorUnit::SerialDispatch.next();",
    );
    let dispatch = marker(
        selector,
        "self.poll_split_ordinary_virtio_serial_dispatch_unit();",
    );
    let tx_snapshot = marker(selector, "let serial_tx_pending_at_entry =");
    let probe_successor = marker(
        selector,
        "self.ordinary_operator_unit = OrdinaryOperatorUnit::SerialIo.next();",
    );
    let probe = marker(
        selector,
        "self.poll_split_ordinary_virtio_serial_io_unit(serial_tx_pending_at_entry)",
    );
    let idle_reset = selector[probe..]
        .find("self.ordinary_operator_unit = OrdinaryOperatorUnit::SerialIo;")
        .map(|offset| probe + offset)
        .unwrap_or_else(|| unreachable!("idle SerialIo must reset its continuation"));
    let local_seat = marker(
        selector,
        "if self.split_ordinary_virtio_local_seat_input_pending()",
    );
    assert!(
        retained_dispatch < dispatch_successor
            && dispatch_successor < dispatch
            && dispatch < tx_snapshot
            && tx_snapshot < probe_successor
            && probe_successor < probe
            && probe < idle_reset
            && idle_reset < local_seat,
        "retained SerialDispatch must commit its successor and return before a new RX probe",
    );

    let serial_io = section(
        event,
        "fn poll_split_ordinary_virtio_serial_io_unit(",
        "fn poll_split_ordinary_virtio_serial_dispatch_unit(",
    );
    assert_eq!(serial_io.matches("self.serial.poll_rx_only()").count(), 1);
    for forbidden in [
        "self.serial.poll_io()",
        "self.serial.flush_tx()",
        "poll_split_ordinary_virtio_serial_dispatch_unit",
        "consume_serial",
    ] {
        assert!(
            !serial_io.contains(forbidden),
            "SerialIo composed forbidden work: {forbidden}",
        );
    }

    let dispatch_unit = section(
        event,
        "fn poll_split_ordinary_virtio_serial_dispatch_unit(",
        "fn poll_split_ordinary_virtio_retained_output_unit(",
    );
    assert_eq!(dispatch_unit.matches("self.consume_serial()").count(), 1);
    assert_eq!(dispatch_unit.matches("self.serial.flush_tx()").count(), 1);
    assert!(!dispatch_unit.contains("poll_rx_only"));
    assert!(!dispatch_unit.contains("poll_io"));

    let serial = include_str!("../src/serial/mod.rs");
    let rx_only = section(
        serial,
        "pub(crate) fn poll_rx_only(&mut self) -> bool",
        "fn poll_root_context_io(",
    );
    assert_eq!(
        rx_only
            .matches("self.poll_root_context_io(contract, true)")
            .count(),
        1,
    );
    assert_eq!(
        rx_only
            .matches("self.poll_rx_only_current_tcb(contract)")
            .count(),
        1,
    );
    assert!(!rx_only.contains("poll_root_context_io(contract, false)"));
    assert!(!rx_only.contains("poll_io_current_tcb"));
    assert!(!rx_only.contains("serial_poll_io_driver_task"));
    assert!(!rx_only.contains("serial_ring_service_driver_task"));
    assert!(!rx_only.contains("unsafe"));
    assert!(!rx_only.contains("flush_tx"));

    let root_context = section(
        serial,
        "fn poll_root_context_io(",
        "pub fn poll_io_linked_runtime_only(&mut self) -> bool",
    );
    assert_eq!(
        root_context
            .matches("serial_ring_service_driver_task::<D, RX, TX, LINE>")
            .count(),
        1,
    );
    assert_eq!(
        root_context
            .matches("serial_poll_io_driver_task::<D, RX, TX, LINE>")
            .count(),
        1,
    );
    assert_eq!(
        root_context
            .matches("self.root_context_rx_only_service = rx_only;")
            .count(),
        2,
    );
    assert_eq!(
        root_context
            .matches("self.root_context_rx_only_service = false;")
            .count(),
        2,
    );

    let rx_current = section(
        serial,
        "fn poll_rx_only_current_tcb(&mut self, contract: DriverTaskContract) -> bool",
        "fn poll_io_current_tcb(&mut self, contract: DriverTaskContract) -> bool",
    );
    assert_eq!(rx_current.matches("self.poll_rx_current_tcb(").count(), 1);
    assert!(!rx_current.contains("flush_tx"));

    let shared_rx = section(
        serial,
        "fn poll_rx_current_tcb(&mut self, budget: &mut DriverServiceBudget)",
        "fn flush_tx_locked(&mut self)",
    );
    let trace_guard = marker(shared_rx, "if serial_input_poll_trace_allowed(");
    let active_mode = marker(shared_rx, "self.ordinary_root_control_turn.active()");
    let trace = marker(shared_rx, "emit_serial_input_poll_trace(");
    assert!(trace_guard < active_mode && active_mode < trace);
    assert!(!shared_rx.contains("flush_tx"));

    let callback_selector = section(
        serial,
        "fn poll_root_context_callback_current_tcb(&mut self, contract: DriverTaskContract) -> bool",
        "fn poll_io_current_tcb(&mut self, contract: DriverTaskContract) -> bool",
    );
    let mode = marker(callback_selector, "if self.root_context_rx_only_service");
    let rx = marker(
        callback_selector,
        "return self.poll_rx_only_current_tcb(contract);",
    );
    let combined = marker(callback_selector, "self.poll_io_current_tcb(contract)");
    assert!(mode < rx && rx < combined);
    assert!(!callback_selector.contains("flush_tx"));

    let ring_callback = section(
        serial,
        "unsafe fn serial_ring_service_driver_task",
        "unsafe fn serial_poll_io_driver_task",
    );
    let ring_service = section(
        ring_callback,
        "if command.opcode == crate::hal::driver_task::DriverTaskOpcode::Service.as_u16()",
        "if command.opcode == crate::hal::driver_task::DriverTaskOpcode::Flush.as_u16()",
    );
    assert_eq!(
        ring_service
            .matches("port.poll_root_context_callback_current_tcb(contract)")
            .count(),
        1,
    );
    assert!(!ring_service.contains("poll_io_current_tcb"));
    assert!(!ring_service.contains("flush_tx"));

    let compat_callback = section(
        serial,
        "unsafe fn serial_poll_io_driver_task",
        "unsafe fn serial_flush_tx_driver_task",
    );
    assert_eq!(
        compat_callback
            .matches("port.poll_root_context_callback_current_tcb(contract)")
            .count(),
        1,
    );
    assert!(!compat_callback.contains("poll_io_current_tcb"));
    assert!(!compat_callback.contains("flush_tx"));
}

#[test]
fn isolated_network_poll_maps_one_selected_unit_in_strict_source_order() {
    let source = include_str!("../src/net/isolated_console.rs");
    let poll = section(
        source,
        "impl NetPoller for IsolatedVirtioConsole",
        "fn driver_task_contract",
    );

    let selector = marker(poll, "select_isolated_network_turn(");
    let successor_commit = marker(poll, "self.lower_cursor = selection.successor();");
    let dispatch = marker(poll, "let outcome = match selection.unit() {");
    let deferred_guard = marker(poll, "IsolatedNetworkTurnUnit::DeferredDiagnostic =>");
    let deferred = marker(poll, "self.poll_deferred_diagnostic_unit()");
    let transmit_guard = marker(poll, "IsolatedNetworkTurnUnit::TransmitEgress =>");
    let transmit = marker(poll, "self.poll_transmit_egress_unit(now_ms)");
    let observe = marker(poll, "self.poll_observe_child_unit()");
    let output = marker(poll, "self.poll_stage_output_unit()");
    let disconnect = marker(poll, "self.poll_disconnect_unit()");
    let ingress = marker(poll, "self.poll_ingress_unit()");
    let tick = marker(poll, "self.poll_service_tick_unit()");
    let outcome_commit = marker(
        poll,
        "let (lower_cursor, activity) = selection.finish(outcome);",
    );
    let cursor_commit = marker(poll, "self.lower_cursor = lower_cursor;");

    assert!(
        selector < successor_commit
            && successor_commit < dispatch
            && dispatch < deferred_guard
            && deferred_guard < deferred
            && deferred < transmit_guard
            && transmit_guard < transmit
            && transmit < observe
            && observe < output
            && output < disconnect
            && disconnect < ingress
            && ingress < tick
            && tick < outcome_commit
            && outcome_commit < cursor_commit,
        "isolated Network unit priority drifted",
    );
    let selected_visit = &poll[selector..cursor_commit];
    assert_eq!(poll.matches("select_isolated_network_turn(").count(), 1);
    assert!(!selected_visit.contains("|unit|"));
    assert!(!selected_visit.contains("execute_isolated_network_turn"));

    let implementation = include_str!("../src/net/isolated_console.rs");
    for helper in [
        "fn poll_deferred_diagnostic_unit",
        "fn poll_transmit_egress_unit",
        "fn poll_observe_child_unit",
        "fn poll_stage_output_unit",
        "fn poll_disconnect_unit",
        "fn poll_ingress_unit",
        "fn poll_service_tick_unit",
    ] {
        let position = marker(implementation, helper);
        let prefix = &implementation[..position];
        assert!(
            prefix.trim_end().ends_with("#[inline(never)]"),
            "{helper} must remain a separate target frame"
        );
    }
}

#[test]
fn split_network_commits_refill_cursor_before_timer_or_nic_work() {
    let source = include_str!("../src/event/mod.rs");
    let runtime_prelude = section(
        source,
        "fn poll_split_ordinary_virtio_runtime_prelude(&mut self)",
        "fn poll_split_ordinary_virtio_runtime_turn(&mut self)",
    );
    assert_eq!(
        runtime_prelude
            .matches("self.reconcile_cyw43_network_ready_hdmi();")
            .count(),
        1,
        "the evidenced Network cut must not change the Runtime reconciliation contract",
    );

    let turn = section(
        source,
        "fn poll_split_ordinary_virtio_network_turn(&mut self)",
        "fn poll_one_split_ordinary_virtio_network_unit(&mut self)",
    );
    let pending = marker(turn, "self.pending_ordinary_virtio_net_diag.take()");
    let diagnostic = marker(turn, "self.log_net_diag_observation(observation);");
    let diagnostic_return = marker(&turn[diagnostic..], "return;") + diagnostic;
    let selected = marker(turn, "let unit = self.ordinary_virtio_network_unit;");
    let successor = marker(turn, "self.ordinary_virtio_network_unit = unit.next();");
    let dispatch = marker(turn, "match unit {");
    let timer = marker(
        turn,
        "OrdinaryVirtioNetworkUnit::Timer => self.poll_runtime_timer_prelude(),",
    );
    let nic_dispatch = marker(
        turn,
        "OrdinaryVirtioNetworkUnit::Nic => self.poll_one_split_ordinary_virtio_network_unit(),",
    );
    assert!(
        pending < diagnostic
            && diagnostic < diagnostic_return
            && diagnostic_return < selected
            && selected < successor
            && successor < dispatch
            && dispatch < timer
            && timer < nic_dispatch,
        "NETDIAG must preempt without advancing the cursor, then the ordinary successor must commit before work",
    );
    assert_eq!(turn.matches("self.poll_runtime_timer_prelude()").count(), 1);
    assert_eq!(
        turn.matches("self.poll_one_split_ordinary_virtio_network_unit()")
            .count(),
        1
    );
    assert!(!turn[..selected].contains("self.ordinary_virtio_network_unit ="));
    assert!(!source.contains("fn poll_split_ordinary_virtio_network_prelude"));
    assert!(!turn.contains("poll_runtime_inner"));
    assert!(!turn.contains("poll_runtime_without_control_tail"));
    assert!(!turn.contains("dispatch_one_buffered_network_line"));
    assert!(!turn.contains("drain_net_console_events"));

    let nic = section(
        source,
        "fn poll_one_split_ordinary_virtio_network_unit(&mut self)",
        "fn poll_runtime_inner(",
    );
    assert_eq!(
        nic.matches("PendingOrdinaryVirtioNetDiag::capture(")
            .count(),
        1,
        "each featured NIC visit must retain exactly one later diagnostic",
    );
    let retain = marker(nic, "PendingOrdinaryVirtioNetDiag::capture(");
    let ninedoor = marker(nic, "bridge.update_ingest_snapshot(ingest_snapshot);");
    assert!(
        retain < ninedoor,
        "NineDoor ingest truth remains immediate while NETDIAG is deferred",
    );

    let outer = section(
        source,
        "if phase == OrdinaryServicePhase::Network",
        "if phase == OrdinaryServicePhase::Runtime",
    );
    assert_eq!(
        outer
            .matches("self.poll_split_ordinary_virtio_network_turn();")
            .count(),
        1
    );
    assert!(!outer.contains("poll_runtime_without_control_tail"));
}

#[test]
fn generic_net_diag_preserves_snapshot_first_and_late_progress_read() {
    let source = include_str!("../src/event/mod.rs");
    let generic = section(
        source,
        "fn log_net_diag(&mut self, telemetry: NetTelemetry)",
        "fn log_net_diag_observation(&mut self, observation: PendingOrdinaryVirtioNetDiag)",
    );
    assert!(generic.contains("let snapshot = NET_DIAG.snapshot();"));
    assert!(generic.contains("self.log_net_diag_snapshot(telemetry, snapshot, self.now_ms, None);"));
    assert!(!generic.contains("PendingOrdinaryVirtioNetDiag::capture"));
    assert!(!generic.contains("NET_DIAG.last_rx_used_change_ms"));

    let progress = section(
        source,
        "fn check_net_diag_progress(",
        "#[cfg(feature = \"kernel\")]\n    /// Run the bootstrap probe loop",
    );
    assert_eq!(
        progress
            .matches("NET_DIAG.last_rx_used_change_ms()")
            .count(),
        1,
        "the generic path must retain one late conditional progress read",
    );
}

#[test]
fn empty_stage_output_commits_disconnect_without_forcing_child_observation() {
    use isolated_network_turn::{
        select_isolated_network_turn, IsolatedNetworkLowerCursor, IsolatedNetworkLowerUnit,
        IsolatedNetworkTurnOutcome, IsolatedNetworkTurnUnit,
    };

    let observe = select_isolated_network_turn(false, false, IsolatedNetworkLowerCursor::new());
    assert_eq!(
        observe.unit(),
        IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::ObserveChild),
    );
    let (stage_output, activity) = observe.finish(IsolatedNetworkTurnOutcome::complete(false));
    assert!(!activity);
    assert_eq!(stage_output.unit(), IsolatedNetworkLowerUnit::StageOutput);

    let selected = select_isolated_network_turn(false, false, stage_output);
    assert_eq!(
        selected.unit(),
        IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::StageOutput),
    );
    assert_eq!(
        selected.successor().unit(),
        IsolatedNetworkLowerUnit::Disconnect,
        "the successor is committed before the empty StageOutput attempt",
    );
    let (disconnect, activity) =
        selected.finish(IsolatedNetworkTurnOutcome::child_signal_attempt(false));
    assert!(!activity);
    assert_eq!(disconnect.unit(), IsolatedNetworkLowerUnit::Disconnect);
}

#[test]
fn isolated_budget_is_charged_before_any_turn_side_effect() {
    let source = include_str!("../src/net/isolated_console.rs");
    let budgeted = section(source, "fn poll_with_budget(", "fn driver_task_contract");
    let ops = marker(budgeted, "budget.charge_ops(1)?;");
    let frames = marker(
        budgeted,
        "budget.charge_frames(ISOLATED_NETWORK_TURN_FRAMES)?;",
    );
    let bytes = marker(
        budgeted,
        "budget.charge_bytes(ISOLATED_NETWORK_TURN_BYTES)?;",
    );
    let poll = marker(budgeted, "Ok(self.poll(now_ms))");
    assert!(ops < frames && frames < bytes && bytes < poll);
    assert!(source.contains("ISOLATED_NETWORK_TURN_FRAMES: u16 = 2;"));
    let turn_bytes = section(
        source,
        "const ISOLATED_NETWORK_TURN_BYTES",
        "/// Construction failure",
    );
    assert!(turn_bytes.contains("console_network_abi::CONSOLE_PAYLOAD_BYTES"));
    assert!(turn_bytes.contains("console_network_abi::ETHERNET_FRAME_BYTES"));
}

#[test]
fn v1_runtime_reclaim_is_capped_and_selftests_request_their_own_bound() {
    let source = include_str!("../src/drivers/virtio/net.rs");
    let wrapper = section(source, "fn reclaim_tx(&mut self)", "fn reclaim_tx_bounded");
    assert!(wrapper.contains("self.reclaim_tx_bounded(TX_RECLAIM_POLL_BUDGET);"));

    let bounded = section(source, "fn reclaim_tx_bounded", "fn tx_reclaim_used");
    assert!(bounded.contains("while processed < budget"));
    assert!(bounded.contains("processed = processed.saturating_add(1);"));
    assert!(
        !bounded.contains("loop {"),
        "normal v1 reclaim must not contain an unbounded used-ring drain",
    );
    assert_eq!(
        source
            .matches("reclaim_tx_bounded(TX_QUEUE_SIZE as u16)")
            .count(),
        2,
        "only the two explicit TX selftests request the queue-size bound",
    );
}

#[test]
fn successful_publish_defers_one_record_and_never_scrubs_after_notify() {
    let source = include_str!("../src/drivers/virtio/net.rs");
    let publish = section(
        source,
        "fn enqueue_tx_chain_checked",
        "fn initialise_queues",
    );
    assert!(publish.contains(".notify(&mut self.regs, TX_QUEUE_INDEX)"));
    assert!(
        !publish.contains("info!("),
        "healthy atomic publication must not synchronously format Info records",
    );

    let submit = section(source, "fn submit_tx(&mut self", "fn submit_tx_v2");
    let enqueue = marker(submit, ".enqueue_tx_chain_checked");
    let deferred = marker(submit, "self.queue_deferred_tx_diagnostic");
    assert!(
        enqueue < deferred,
        "success is recorded only after notify returns"
    );
    let committed_suffix = &submit[enqueue..];
    assert!(!committed_suffix.contains("as_mut_slice"));
    assert!(!committed_suffix.contains(".fill("));
    assert!(!committed_suffix.contains("for byte in"));
    assert!(submit.contains("descriptor's validated `len` is the complete device-owned"));
}

#[test]
fn isolated_tx_visit_cannot_drain_its_own_success_diagnostic() {
    let source = include_str!("../src/net/isolated_console.rs");
    let transmit = section(source, "fn transmit_pending_egress", "fn stage_one_ingress");
    assert!(transmit.contains("self.device.transmit_isolated(timestamp)"));
    assert!(!transmit.contains("emit_one_deferred_tx_diagnostic"));
    assert!(transmit.contains("deferred_tx_diagnostic_pending()"));
}

#[test]
fn isolated_ingress_uses_the_single_silent_rx_seam() {
    let source = include_str!("../src/net/isolated_console.rs");
    let ingress = section(source, "fn stage_one_ingress", "fn stage_one_output");
    assert!(ingress.contains("self.device.receive_isolated()"));
    assert!(!ingress.contains("self.device.receive("));
    assert!(!ingress.contains("drop(transmit)"));
    assert!(ingress.contains("self.device.begin_smoltcp_rx_transaction()"));
    assert!(ingress.contains("self.device.end_smoltcp_rx_transaction()"));
}

#[test]
fn isolated_tx_reclaim_path_suppresses_all_routine_info_formatting() {
    let source = include_str!("../src/drivers/virtio/net.rs");
    let transmit = section(
        source,
        "fn transmit_without_diagnostic_drain",
        "pub fn debug_snapshot",
    );
    assert!(transmit.contains("self.poll_interrupts_without_routine_diagnostics()"));
    assert!(transmit.contains("self.prepare_tx_token_without_routine_diagnostics()"));
    assert!(!transmit.contains("self.poll_interrupts();"));
    assert!(!transmit.contains("info!("));

    let silent_poll = section(
        source,
        "fn poll_interrupts_without_routine_diagnostics",
        "fn poll_interrupts_with_routine_diagnostics",
    );
    assert!(silent_poll.contains("self.poll_interrupts_with_routine_diagnostics(false);"));
    assert!(!silent_poll.contains("info!("));

    let poll_core = section(
        source,
        "fn poll_interrupts_with_routine_diagnostics",
        "fn check_device_health",
    );
    assert!(poll_core.matches("if routine_diagnostics {").count() >= 3);
    assert_eq!(
        poll_core.matches("self.log_tx_stats_snapshot();").count(),
        2
    );
    assert!(!poll_core.contains("info!("));

    let reclaim = section(
        source,
        "fn reclaim_tx_bounded_with_routine_diagnostics",
        "fn tx_reclaim_used(&mut self",
    );
    assert!(reclaim.contains("let should_log = routine_diagnostics"));
    assert!(!reclaim.contains("info!("));

    let invalidate = section(
        source,
        "fn invalidate_used_elem_for_cpu",
        "fn debug_descriptors",
    );
    assert!(invalidate.contains("log::debug!("));
    assert!(!invalidate.contains("info!("));
}
