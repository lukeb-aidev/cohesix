// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Guard bounded VirtIO ordering, ownership, and platform-specific DMA containment.
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
fn non_virtio_containment_skips_direct_dma_and_rejects_invalid_cursor() {
    let hal = include_str!("../src/hal/console_network.rs");
    let non_virtio_inventory = section(
        hal,
        "#[cfg(not(feature = \"net-backend-virtio\"))]\nconst DIRECT_DMA_FRAME_COUNT",
        "#[cfg(feature = \"net-backend-virtio\")]\nconst DIRECT_DEVICE_SLOT_COUNT",
    );
    assert!(non_virtio_inventory.contains("DIRECT_DMA_FRAME_COUNT: usize = 0;"));

    let construction = section(
        hal,
        "containment: ConsoleNetworkContainmentCursor::with_direct_frames(",
        "\n    })\n}",
    );
    assert!(construction.contains("DIRECT_DMA_FRAME_COUNT as u8"));

    let cursor = include_str!("../src/console_network_service.rs");
    let transition = section(
        cursor,
        "ConsoleNetworkContainmentUnit::UnmapSharedFrame(_) if self.direct_frame_count != 0",
        "ConsoleNetworkContainmentUnit::ClearDirectIrq =>",
    );
    assert!(transition.contains("ConsoleNetworkContainmentUnit::ClearDirectIrq"));
    assert!(transition.contains("ConsoleNetworkContainmentUnit::DeleteFaultCap(0)"));

    let invalid_cursor = section(
        hal,
        "#[cfg(not(feature = \"net-backend-virtio\"))]\n    fn scrub_direct_dma_frame(",
        "\n    /// Grant one publication credit",
    );
    assert!(invalid_cursor.contains("console-network-direct-virtio-disabled"));
    assert!(invalid_cursor.contains("Err(HalError::Unsupported("));
    assert!(!invalid_cursor.contains("map_revoke_anchor_frame_in_root"));
    assert!(!invalid_cursor.contains("cache_clean_bounded"));
}

#[test]
fn routine_audit_handoff_cannot_delay_terminal_response_admission() {
    let debug_uart = include_str!("../src/debug_uart.rs");
    assert!(!debug_uart.contains("routine_audit_line"));

    let critical = section(
        debug_uart,
        "pub fn debug_uart_line(line: &str)",
        "#[cfg(not(feature = \"kernel\"))]",
    );
    assert!(critical.contains("crate::sel4::debug_put_line_unlocked(line.as_bytes())"));
    assert!(!critical.contains("log_channel_active"));

    let event = include_str!("../src/event/mod.rs");
    let queue = section(
        event,
        "struct RoutineAuditQueue",
        "const fn isolated_routine_audit_drain_allowed",
    );
    assert!(queue.contains("HeaplessDeque<"));
    assert!(queue.contains("self.pending.push_back(record)"));
    assert!(queue.contains("self.dropped = self.dropped.saturating_add(1);"));
    assert!(!queue.contains("log_buffer"));

    let routing = section(
        event,
        "fn retain_routine_audit_line(&mut self",
        "fn end_session(&mut self",
    );
    let isolated = marker(routing, "if self.isolated_virtio_compact_path_attached()");
    let retained = marker(routing, "self.routine_audits.retain(line)");
    let legacy = marker(routing, "crate::debug_uart::debug_uart_line(line);");
    assert!(isolated < retained && retained < legacy);
    assert!(!routing.contains("log_buffer"));

    let routine_audits = section(
        event,
        "fn audit_tcp_cmd_begin(&mut self",
        "fn session_role_label(&self)",
    );
    assert_eq!(
        routine_audits
            .matches("self.retain_routine_audit_line(message.as_str());")
            .count(),
        7,
    );
    assert!(!routine_audits.contains("crate::debug_uart::debug_uart_line"));

    let dispatch = section(
        event,
        "pub(crate) fn handle_command(&mut self",
        "fn forward_to_ninedoor(&mut self",
    );
    let terminal_admission = marker(
        dispatch,
        "if result.is_ok() {\n            self.emit_stream_end_if_pending();",
    );
    let deferred_audit = marker(
        dispatch,
        "self.audit_tcp_cmd_end(conn_id, end_sid, verb_label, cmd_status, term);",
    );
    assert!(terminal_admission < deferred_audit);

    let console_emit = section(
        event,
        "pub fn emit_console_line(&mut self",
        "fn service_local_seat_keyboard_during_output",
    );
    assert!(console_emit.contains("crate::debug_uart::debug_uart_line(message.as_str());"));
    assert!(!console_emit.contains("crate::debug_uart::routine_audit_line"));

    let cat = section(
        event,
        "let detail = format_message(format_args!(\n                                                \"path={} data={}\"",
        "self.metrics.ui_reads =",
    );
    let cat_response = marker(cat, "self.emit_ack_ok(verb_label, Some(detail.as_str()));");
    let cat_audit = marker(cat, "self.retain_routine_audit_line(message.as_str());");
    assert!(cat_response < cat_audit);

    let operator = section(
        event,
        "fn poll_one_split_ordinary_virtio_operator_unit(&mut self)",
        "fn poll_split_ordinary_virtio_serial_io_unit",
    );
    let buffered_command = marker(operator, "self.poll_split_ordinary_virtio_net_line_unit();");
    let retained_output = marker(operator, "if !self.pending_console_output.is_empty()");
    let display = marker(
        operator,
        "self.poll_split_ordinary_virtio_display_attach_unit();",
    );
    let audit_drain = marker(operator, "isolated_routine_audit_drain_allowed(");
    assert!(buffered_command < retained_output);
    assert!(retained_output < display);
    assert!(display < audit_drain);
    assert!(operator.contains("self.network_response_owner_active() || response_lane_active,"));
    assert!(operator.contains("self.pending_net_flush.active(),"));

    let drain = section(
        event,
        "fn poll_split_ordinary_virtio_routine_audit_unit(&mut self)",
        "fn poll_split_ordinary_virtio_operator_turn",
    );
    let nonblocking = marker(
        drain,
        "self.serial.try_enqueue_routine_audit_line_record(line)",
    );
    let commit = marker(drain, "self.routine_audits.pop_front();");
    assert!(nonblocking < commit);
    assert!(!drain.contains("debug_uart"));
    assert!(!drain.contains("log_buffer"));

    let legacy_stack = include_str!("../src/net/stack.rs");
    assert!(!legacy_stack.contains("routine_audit_line"));

    let critical_tcb = include_str!("../src/hal/critical_tcb.rs");
    let fail_stop = section(
        critical_tcb,
        "fn target_fail_stop(reason: &'static str",
        "fn publish_target_worker_fault",
    );
    assert!(fail_stop.contains("crate::debug_uart::debug_uart_line(reason);"));
    assert!(!fail_stop.contains("routine_audit_line"));
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
        3,
        "only two no-progress predispatch paths and the bounded completed-response burst may admit Operator work",
    );
    assert!(split.contains("let completed_response_pipeline_dispatch_due = response_lane"));
    assert!(split.contains("!lane.producer_open && lane.available_lines != 0"));
    assert!(split.contains("if completed_response_pipeline_dispatch_due {"));
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
        "self.poll_split_ordinary_virtio_serial_io_unit(\n            serial_tx_pending_at_entry,\n            routine_audit_only_pending,\n        )",
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
        "impl<D: NetDevice> NetPoller for IsolatedNetworkConsole<D>",
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
fn exact_lower_cursor_constructor_preserves_selected_unit() {
    use isolated_network_turn::{IsolatedNetworkLowerCursor, IsolatedNetworkLowerUnit};

    for unit in [
        IsolatedNetworkLowerUnit::ObserveChild,
        IsolatedNetworkLowerUnit::StageOutput,
        IsolatedNetworkLowerUnit::Disconnect,
        IsolatedNetworkLowerUnit::Ingress,
        IsolatedNetworkLowerUnit::ServiceTick,
    ] {
        assert_eq!(IsolatedNetworkLowerCursor::for_unit(unit).unit(), unit);
    }
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
fn disconnect_control_is_one_shot_and_releases_ingress_after_completion() {
    use isolated_network_turn::{
        select_isolated_network_turn, IsolatedNetworkLowerCursor, IsolatedNetworkLowerUnit,
        IsolatedNetworkTurnOutcome, IsolatedNetworkTurnUnit,
    };

    #[derive(Clone, Copy)]
    struct DisconnectTransaction {
        requested: bool,
        issued: bool,
        response_lane_active: bool,
        attempts: usize,
    }

    impl DisconnectTransaction {
        fn attempt(&mut self, stage_succeeds: bool) -> bool {
            if !self.requested || self.issued || self.response_lane_active {
                return false;
            }
            self.attempts += 1;
            if !stage_succeeds {
                return false;
            }
            self.issued = true;
            true
        }
    }

    let source = include_str!("../src/net/isolated_console.rs");
    let fields = section(
        source,
        "pub struct IsolatedNetworkConsole<D: NetDevice>",
        "impl<D: NetDevice> IsolatedNetworkConsole<D>",
    );
    assert!(fields.contains("disconnect_requested: bool,"));
    assert!(fields.contains("disconnect_issued: bool,"));

    let stage = section(
        source,
        "fn stage_disconnect_if_drained(&mut self) -> bool",
        "fn refresh_device_counters(&mut self)",
    );
    let issued_guard = marker(stage, "|| self.disconnect_issued");
    let response_lane_guard = marker(stage, "|| self.response_lane.is_some()");
    let output_guard = marker(stage, "|| !self.output.is_empty()");
    let publish = marker(
        stage,
        "match self.runtime.stage_disconnect(self.last_now_ms)",
    );
    let issue_commit = marker(stage, "self.disconnect_issued = true;");
    let backpressure = marker(stage, "Err(BoundaryError::Backpressure) => false,");
    assert!(issued_guard < response_lane_guard);
    assert!(response_lane_guard < output_guard);
    assert!(output_guard < publish && publish < issue_commit && issue_commit < backpressure);
    assert_eq!(stage.matches("self.disconnect_issued = true;").count(), 1);

    let mut transaction = DisconnectTransaction {
        requested: true,
        issued: false,
        response_lane_active: true,
        attempts: 0,
    };
    assert!(
        !transaction.attempt(true),
        "QUIT must not publish Disconnect while its terminal response lane still owns completion identity"
    );
    assert_eq!(transaction.attempts, 0);

    transaction.response_lane_active = false;
    assert!(!transaction.attempt(false), "backpressure is not issuance");
    assert!(!transaction.issued);
    assert_eq!(transaction.attempts, 1);

    let mut cursor = IsolatedNetworkLowerCursor::new();
    for expected in [
        IsolatedNetworkLowerUnit::ObserveChild,
        IsolatedNetworkLowerUnit::StageOutput,
    ] {
        let selected = select_isolated_network_turn(false, false, cursor);
        assert_eq!(selected.unit(), IsolatedNetworkTurnUnit::Lower(expected));
        (cursor, _) = selected.finish(IsolatedNetworkTurnOutcome::complete(false));
    }
    assert_eq!(cursor.unit(), IsolatedNetworkLowerUnit::Disconnect);

    let selected = select_isolated_network_turn(false, false, cursor);
    assert!(transaction.attempt(true));
    (cursor, _) = selected.finish(IsolatedNetworkTurnOutcome::child_signal_attempt(true));
    assert!(transaction.issued);
    assert_eq!(transaction.attempts, 2);
    assert_eq!(cursor.unit(), IsolatedNetworkLowerUnit::ObserveChild);

    // The control watermark and OutputDrained retire the boundary records,
    // but do not reopen the semantic Disconnect transaction for this connection.
    for expected in [
        IsolatedNetworkLowerUnit::ObserveChild,
        IsolatedNetworkLowerUnit::StageOutput,
    ] {
        let selected = select_isolated_network_turn(false, false, cursor);
        assert_eq!(selected.unit(), IsolatedNetworkTurnUnit::Lower(expected));
        (cursor, _) = selected.finish(IsolatedNetworkTurnOutcome::complete(false));
    }
    assert_eq!(cursor.unit(), IsolatedNetworkLowerUnit::Disconnect);

    let selected = select_isolated_network_turn(false, false, cursor);
    assert!(
        !transaction.attempt(true),
        "Disconnect must not be restaged"
    );
    (cursor, _) = selected.finish(IsolatedNetworkTurnOutcome::child_signal_attempt(false));
    assert_eq!(transaction.attempts, 2);
    assert_eq!(cursor.unit(), IsolatedNetworkLowerUnit::Ingress);

    let selected = select_isolated_network_turn(false, false, cursor);
    assert_eq!(
        selected.unit(),
        IsolatedNetworkTurnUnit::Lower(IsolatedNetworkLowerUnit::Ingress)
    );
    (cursor, _) = selected.finish(IsolatedNetworkTurnOutcome::child_signal_attempt(false));
    assert_eq!(cursor.unit(), IsolatedNetworkLowerUnit::ServiceTick);
}

#[test]
fn console_publication_ack_is_exactly_once_and_post_retention() {
    let hal = include_str!("../src/hal/console_network.rs");
    let pending = section(
        hal,
        "pub const fn publication_ack_pending(&self)",
        "pub fn poll_turn(&mut self)",
    );
    assert!(pending.contains("self.activated && !self.contained && self.publication_ack_owed"));

    let poll = section(
        hal,
        "pub fn poll_turn(&mut self)",
        "pub fn acknowledge_publication(&mut self)",
    );
    let owed_guard = marker(poll, "|| self.publication_ack_owed");
    let empty_badge = marker(poll, "if badge == 0");
    let event_gate = marker(
        poll,
        "let event_ready = badge & seL4_Word::from(WAKE_EVENT_READY) != 0;",
    );
    let watermarks = marker(poll, ".accept_completion_watermarks(");
    let event_pending = marker(poll, ".event_publication_pending(");
    let event_accept = marker(poll, ".accept_event(");
    let egress_accept = marker(poll, ".accept_egress(");
    let latch = marker(
        poll,
        "self.publication_ack_owed = event.is_some() || egress.is_some();",
    );
    assert!(
        owed_guard < empty_badge
            && empty_badge < event_gate
            && event_gate < watermarks
            && watermarks < event_pending
            && event_pending < event_accept
            && event_accept < egress_accept
            && egress_accept < latch
    );
    assert!(poll.contains("let input_completions = if event_ready"));
    assert!(!poll.contains("signal_unchecked"));

    let acknowledge = section(
        hal,
        "pub fn acknowledge_publication(&mut self)",
        "pub fn retire_terminal_publication(&mut self)",
    );
    let requires_owed = marker(acknowledge, "|| !self.publication_ack_owed");
    let clear = marker(acknowledge, "self.publication_ack_owed = false;");
    let release = marker(acknowledge, "fence(Ordering::Release);");
    let signal = marker(
        acknowledge,
        "signal_unchecked(self.root_wake_caps[ROOT_PUBLICATION_ACK_WAKE_INDEX])",
    );
    assert!(requires_owed < clear && clear < release && release < signal);
    assert_eq!(
        acknowledge
            .matches("self.publication_ack_owed = false;")
            .count(),
        1,
        "one successful ACK must consume the latch exactly once",
    );
    assert_eq!(acknowledge.matches("signal_unchecked").count(), 1);

    let supervisor_fault = section(
        hal,
        "pub fn record_supervisor_fault(&mut self)",
        "pub fn activate(&mut self)",
    );
    let record_fault = marker(supervisor_fault, "self.boundary.record_fault();");
    let clear_debt = marker(supervisor_fault, "self.publication_ack_owed = false;");
    assert!(record_fault < clear_debt);

    let revoke = section(
        hal,
        "pub fn signal_revoke(&mut self)",
        "pub const fn publication_ack_pending(&self)",
    );
    let revoke_clear = marker(revoke, "self.publication_ack_owed = false;");
    let revoke_release = marker(revoke, "fence(Ordering::Release);");
    let revoke_signal = marker(
        revoke,
        "signal_unchecked(self.root_wake_caps[ROOT_REVOKE_WAKE_INDEX])",
    );
    assert!(revoke_clear < revoke_release && revoke_release < revoke_signal);

    let terminal = section(
        hal,
        "pub fn retire_terminal_publication(&mut self)",
        "pub fn contain_one_turn(",
    );
    assert!(terminal.contains("self.boundary.state() != ServiceState::Terminal"));
    assert!(terminal.contains("self.publication_ack_owed = false;"));
    assert!(!terminal.contains("signal_unchecked"));

    let adapter = include_str!("../src/net/isolated_console.rs");
    let output = section(
        adapter,
        "fn poll_child_output(&mut self)",
        "fn transmit_pending_egress",
    );
    let publication_snapshot = marker(
        output,
        "let publication_observed = turn.publication_observed();",
    );
    let completion = marker(output, "self.handle_control_completed(sequence);");
    let handle = marker(output, "self.handle_event(event);");
    let completion_fault_gate = marker(output, "if self.faulted {");
    let terminal_retire = marker(output, "self.runtime.retire_terminal_publication()");
    let containment_start = marker(output, "self.runtime.begin_containment()");
    let overwrite_guard = marker(output, "if self.pending_egress.is_some()");
    let retain_egress = marker(output, "self.pending_egress = Some(egress);");
    let inline_ack = marker(output, "self.runtime.acknowledge_publication()");
    assert!(
        publication_snapshot < completion
            && completion < completion_fault_gate
            && completion_fault_gate < handle
            && handle < terminal_retire
            && terminal_retire < containment_start
            && containment_start < overwrite_guard
            && overwrite_guard < retain_egress
            && retain_egress < inline_ack
    );
    assert!(output.contains("self.fail_closed(\"egress-overwrite\")"));
    assert_eq!(output.matches("acknowledge_publication").count(), 1);
    assert!(output.contains("self.fail_closed(\"publication-ack\")"));
    assert!(!output.contains("signal_unchecked"));

    let fail_closed = section(
        adapter,
        "fn fail_closed(&mut self",
        "fn handle_event(&mut self",
    );
    assert!(fail_closed.contains("self.runtime.signal_revoke()"));

    let observe = section(
        adapter,
        "fn poll_observe_child_unit(&mut self)",
        "fn poll_stage_output_unit(&mut self)",
    );
    assert!(observe.contains("self.poll_child_output()"));
    assert!(!observe.contains("acknowledge_publication"));

    assert!(output.contains("IsolatedNetworkTurnOutcome::child_signaled(activity)"));

    let adapter_poll = section(
        adapter,
        "impl<D: NetDevice> NetPoller for IsolatedNetworkConsole<D>",
        "fn driver_task_contract",
    );
    assert!(!adapter_poll.contains("AcknowledgePublication"));
    assert!(!adapter_poll.contains("poll_acknowledge_publication_unit"));

    let turn_selector = include_str!("../src/net/isolated_network_turn.rs");
    let selection = section(
        turn_selector,
        "pub(crate) fn select_isolated_network_turn(",
        "#[cfg(test)]",
    );
    let diagnostic_priority = marker(selection, "if deferred_diagnostic");
    let egress_priority = marker(selection, "else if pending_egress");
    assert!(diagnostic_priority < egress_priority);
    assert!(!selection.contains("publication_ack_pending"));
    assert!(!selection.contains("AcknowledgePublication"));

    let terminal_handler = section(adapter, "fn handle_event(&mut self", "fn poll_child_output");
    assert!(terminal_handler.contains("self.fail_closed("));
    let shutdown = terminal_handler
        .split("ExchangeKind::ShutdownComplete => {")
        .nth(1)
        .expect("ShutdownComplete handler")
        .split("ExchangeKind::SendLine")
        .next()
        .expect("bounded ShutdownComplete arm");
    assert!(shutdown.contains("self.graceful_teardown_pending = true;"));
    assert!(!shutdown.contains("self.terminal = true;"));
    assert!(!shutdown.contains("acknowledge_publication"));

    let containment = section(
        adapter,
        "pub fn contain_one_turn(",
        "pub fn contain_if_faulted(",
    );
    let proof_complete = marker(containment, "proof.complete()");
    let clear_pending = marker(containment, "self.graceful_teardown_pending = false;");
    let mark_terminal = marker(containment, "self.terminal = true;");
    assert!(proof_complete < clear_pending && clear_pending < mark_terminal);

    let gate = section(
        adapter,
        "pub const fn containment_required(&self)",
        "pub fn contain_one_turn(",
    );
    assert!(gate.contains("!self.terminal"));
    assert!(gate.contains("self.graceful_teardown_pending"));
    assert!(gate.contains("self.runtime.containment_active()"));
    let stack = include_str!("../src/net/stack.rs");
    let stack_gate = section(
        stack,
        "pub fn console_network_child_faulted(&self)",
        "pub fn contain_console_network_child(",
    );
    assert!(stack_gate.contains("stack.containment_required()"));
    assert!(!stack_gate.contains("stack.faulted()"));
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
fn isolated_output_backpressures_until_a_connection_is_authenticated() {
    let source = include_str!("../src/net/isolated_console.rs");
    let output = section(
        source,
        "fn queue_console_output(&mut self, line: &str, terminal: bool) -> bool",
        "fn complete_response_lane_if_drained(&mut self)",
    );
    let admission = marker(output, "if self.faulted || self.terminal {");
    let rejection = marker(output, "return false;");
    let identity = marker(
        output,
        "let connection_id = match self.authenticated_connection",
    );
    let unauthenticated = marker(output, "None => return false,");
    let normalization = marker(output, "let line = line.trim_end_matches");
    let queue = marker(output, ".push_back(QueuedConsoleOutput");

    assert!(admission < rejection);
    assert!(rejection < identity && identity < unauthenticated);
    assert!(unauthenticated < normalization && normalization < queue);
    assert!(!output.contains("return !self.faulted && !self.terminal;"));

    let trait_output = section(
        source,
        "fn send_console_line(&mut self, line: &str) -> bool",
        "fn send_console_terminal_line(&mut self, line: &str) -> bool",
    );
    assert!(trait_output.contains("self.queue_console_output(line, false)"));
}

#[test]
fn default_net_stack_preserves_every_isolated_response_hook() {
    let source = include_str!("../src/net/stack.rs");
    let implementation = section(
        source,
        "impl NetPoller for DefaultNetStack",
        "/// Cooperative polling loop",
    );

    for signature in [
        "fn send_console_terminal_line(&mut self, line: &str) -> bool",
        "fn bounded_console_response_identity(&self) -> Option<ConsoleResponseIdentity>",
        "fn console_response_lane(&self) -> Option<ConsoleResponseLane>",
        "fn poll_console_response_with_budget(",
        "fn console_event_pending(&self) -> bool",
    ] {
        assert!(
            implementation.contains(signature),
            "DefaultNetStack must preserve {signature}"
        );
    }
    for delegated in [
        "Self::Virtio(stack) => stack.send_console_terminal_line(line)",
        "Self::Virtio(stack) => stack.bounded_console_response_identity()",
        "Self::Virtio(stack) => stack.console_response_lane()",
        "Self::Virtio(stack) => stack.poll_console_response_with_budget(now_ms, budget)",
        "Self::Virtio(stack) => stack.console_event_pending()",
    ] {
        assert!(
            implementation.contains(delegated),
            "selected Virtio wrapper must preserve {delegated}"
        );
    }
}

#[test]
fn isolated_console_input_never_crosses_connection_identity_boundaries() {
    let source = include_str!("../src/net/isolated_console.rs");
    let handler = section(source, "fn handle_event(&mut self", "fn poll_child_output");
    let connected = handler
        .split("ExchangeKind::Connected => {")
        .nth(1)
        .expect("Connected handler")
        .split("ExchangeKind::Authenticated")
        .next()
        .expect("bounded Connected arm");
    let authenticated = handler
        .split("ExchangeKind::Authenticated => {")
        .nth(1)
        .expect("Authenticated handler")
        .split("ExchangeKind::Command")
        .next()
        .expect("bounded Authenticated arm");
    let command = handler
        .split("ExchangeKind::Command => {")
        .nth(1)
        .expect("Command handler")
        .split("ExchangeKind::CommandBatch")
        .next()
        .expect("bounded Command arm");
    let command_batch = handler
        .split("ExchangeKind::CommandBatch => {")
        .nth(1)
        .expect("CommandBatch handler")
        .split("ExchangeKind::Disconnected")
        .next()
        .expect("bounded CommandBatch arm");
    let disconnected = handler
        .split("ExchangeKind::Disconnected => {")
        .nth(1)
        .expect("Disconnected handler")
        .split("ExchangeKind::Backpressure")
        .next()
        .expect("bounded Disconnected arm");
    let shutdown = handler
        .split("ExchangeKind::ShutdownComplete => {")
        .nth(1)
        .expect("ShutdownComplete handler")
        .split("ExchangeKind::SendLine")
        .next()
        .expect("bounded ShutdownComplete arm");

    for (boundary, arm) in [
        ("Connected", connected),
        ("Disconnected", disconnected),
        ("ShutdownComplete", shutdown),
    ] {
        assert_eq!(
            arm.matches("self.lines.clear();").count(),
            1,
            "{boundary} must retire every retained command line",
        );
    }
    assert!(
        marker(connected, "self.lines.clear();")
            < marker(connected, "self.active_connection = Some(connection_id);")
    );
    assert!(
        marker(disconnected, "self.lines.clear();")
            < marker(disconnected, "self.active_connection = None;")
    );
    assert!(
        marker(shutdown, "self.lines.clear();")
            < marker(shutdown, "self.active_connection = None;")
    );

    // A replacement identity starts unauthenticated, can authenticate only
    // against its exact active connection, and can then enqueue fresh input.
    assert!(connected.contains("self.authenticated_connection = None;"));
    assert!(authenticated.contains("self.active_connection != Some(connection_id)"));
    assert!(authenticated.contains("self.authenticated_connection = Some(connection_id);"));
    let command_identity = marker(
        command,
        "self.authenticated_connection != Some(connection_id)",
    );
    let command_enqueue = marker(command, ".push_back(ConsoleLine::for_connection(");
    assert!(command_identity < command_enqueue);
    assert!(command.contains("event.now_ms(),"));
    assert!(command.contains("connection_id,"));
    let batch_identity = marker(
        command_batch,
        "self.authenticated_connection != Some(connection_id)",
    );
    let batch_capacity = marker(
        command_batch,
        "cursor.remaining() > LINE_QUEUE_DEPTH.saturating_sub(self.lines.len())",
    );
    let batch_enqueue = marker(command_batch, ".push_back(ConsoleLine::for_connection(");
    assert!(batch_identity < batch_capacity);
    assert!(batch_capacity < batch_enqueue);
    assert!(command_batch.contains("let (now_ms, command) = command;"));
    assert!(command_batch.contains("ConsoleLine::for_connection(line, now_ms, connection_id)"));
}

#[test]
fn exact_console_drain_includes_root_retained_egress_and_response_lane() {
    let source = include_str!("../src/net/isolated_console.rs");
    let drain = section(
        source,
        "fn console_output_drained(&self, connection_id: u64) -> bool",
        "fn drain_console_events",
    );
    let local_output = marker(drain, "self.output.is_empty()");
    let copied_egress = marker(drain, "self.pending_egress.is_none()");
    let response_lane = marker(drain, "self.response_lane.is_none()");
    let child_drain = marker(drain, "self.runtime.console_output_drained(connection_id)");
    assert!(local_output < copied_egress);
    assert!(copied_egress < response_lane);
    assert!(response_lane < child_drain);
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

    let submit = section(source, "fn submit_tx(", "fn submit_tx_v2");
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
fn isolated_tx_visit_neither_selects_nor_drains_routine_diagnostic_inline() {
    let source = include_str!("../src/net/isolated_console.rs");
    let transmit = section(source, "fn transmit_pending_egress", "fn stage_one_ingress");
    assert!(transmit.contains(".transmit_isolated_frame(timestamp, frame.as_slice())"));
    assert!(!transmit.contains("emit_one_deferred_tx_diagnostic"));
    assert!(!transmit.contains("deferred_tx_diagnostic_pending()"));

    let response = section(
        source,
        "fn poll_response_turn",
        "impl<D: NetDevice> NetPoller for IsolatedNetworkConsole<D>",
    );
    assert!(!response.contains("deferred_tx_diagnostic_pending()"));
    assert!(!response.contains("poll_deferred_diagnostic_unit"));
}

#[test]
fn isolated_ingress_uses_the_single_silent_rx_seam() {
    let source = include_str!("../src/net/isolated_console.rs");
    let ingress = section(source, "fn stage_one_ingress", "fn stage_one_output");
    assert!(ingress.contains("self.device.consume_isolated_rx("));
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

#[test]
fn direct_dma_containment_is_compiled_only_for_the_virtio_backend() {
    let source = include_str!("../src/hal/console_network.rs");

    assert!(source.contains(
        "#[cfg(feature = \"net-backend-virtio\")]\nconst DIRECT_DMA_FRAME_COUNT: usize ="
    ));
    assert!(source.contains(
        "#[cfg(not(feature = \"net-backend-virtio\"))]\nconst DIRECT_DMA_FRAME_COUNT: usize = 0;"
    ));
    assert_eq!(
        source
            .matches("fn scrub_direct_dma_frame(\n        &mut self,")
            .count(),
        2,
    );
    assert!(source.contains("console-network-direct-virtio-disabled"));
    assert!(source.contains(
        "ConsoleNetworkContainmentCursor::with_direct_frames(\n            DIRECT_DMA_FRAME_COUNT as u8,"
    ));
}
