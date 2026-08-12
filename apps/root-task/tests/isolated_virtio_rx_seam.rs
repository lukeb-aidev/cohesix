// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Guard the bounded silent isolated VirtIO RX ownership seam.
// Author: Lukas Bower

const SOURCE: &str = include_str!("../src/drivers/virtio/net.rs");

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

#[test]
fn isolated_rx_exposes_the_dedicated_one_record_seam() {
    let wrapper = section(SOURCE, "impl VirtioNetStatic {", "impl VirtioNet {");
    assert!(wrapper.contains("pub(crate) fn receive_isolated(&mut self) -> Option<VirtioRxToken>"));

    let receive = section(
        SOURCE,
        "    fn receive_isolated(&mut self) -> Option<VirtioRxToken> {",
        "    fn poll_rx_interrupt_without_tx_work",
    );
    assert_eq!(
        receive.matches("pop_rx_with_routine_diagnostics").count(),
        1,
        "one isolated RX visit may inspect or pop only one used record",
    );
    for iteration in ["for ", "while ", "loop {"] {
        assert!(
            !receive.contains(iteration),
            "isolated RX must not iterate over the used ring via {iteration}",
        );
    }
    assert!(receive.contains("RxRoutineDiagnostics::Suppressed"));
    assert!(receive.contains("VirtioRxToken {"));
}

#[test]
fn isolated_rx_does_not_reclaim_or_reserve_tx_ownership() {
    let receive = section(
        SOURCE,
        "    fn receive_isolated(&mut self) -> Option<VirtioRxToken> {",
        "    fn transmit_without_diagnostic_drain",
    );
    assert!(receive.contains("acknowledge_interrupts"));
    assert!(receive.contains("check_device_health"));
    for forbidden in [
        "emit_one_deferred_tx_diagnostic",
        "reclaim_tx",
        "tx_reclaim_used",
        "prepare_tx_token",
        "reserve_tx_slot",
        "cancel_tx_slot",
        "VirtioTxToken {",
        "info!(",
        "debug!(",
    ] {
        assert!(
            !receive.contains(forbidden),
            "isolated RX must not compose {forbidden}",
        );
    }
}

#[test]
fn isolated_rx_preserves_copy_requeue_and_publish_ownership() {
    let consume = section(
        SOURCE,
        "impl RxToken for VirtioRxToken",
        "/// Transmit token that queues frames",
    );
    assert!(consume.contains("let payload ="));
    assert!(consume.contains("let result = f(payload);"));
    assert_eq!(
        consume
            .matches("requeue_rx_with_routine_diagnostics")
            .count(),
        2,
        "both the short-frame and consumed-frame paths must return RX ownership",
    );
    let healthy_diagnostics = section(
        consume,
        "        if self.diagnostics.enabled() {",
        "        driver.rx_packets",
    );
    for routine in ["log_tcp_dest_port_once", "log_tcp_trace", "log::debug!("] {
        assert!(
            healthy_diagnostics.contains(routine),
            "routine RX formatting must remain behind the token policy: {routine}",
        );
    }

    let requeue = section(
        SOURCE,
        "    fn requeue_rx_with_routine_diagnostics",
        "    fn prepare_tx_token",
    );
    for retained in [
        "check_device_health",
        "assert_dma_region",
        "sync_rx_slot_for_device",
        "enqueue_rx_chain_checked_with_routine_diagnostics",
        "rx_header_len negotiated as zero",
        "rx_payload_len resolved to zero",
        "rx requeue cache clean failed",
        "missing buffer entry",
    ] {
        assert!(
            requeue.contains(retained),
            "isolated RX must retain {retained}",
        );
    }
    let dma_log = requeue
        .find("log_dma_programming")
        .expect("generic RX retains its routine DMA breadcrumb");
    assert!(
        requeue[..dma_log].ends_with("            if diagnostics.enabled() {\n                "),
        "routine DMA formatting must be directly policy-gated",
    );

    let publish = section(
        SOURCE,
        "    fn enqueue_rx_chain_checked_with_routine_diagnostics",
        "    fn enqueue_tx_chain_checked",
    );
    for retained in [
        "validate_chain_nonzero",
        "setup_descriptor",
        "virtq_publish_barrier",
        "verify_descriptor_write",
        "sync_descriptor_table_for_device",
        "validate_chain_pre_publish",
        "push_avail",
        "sync_avail_ring_for_device",
        "notify_with_routine_diagnostics",
    ] {
        assert!(
            publish.contains(retained),
            "RX descriptor publication must retain {retained}",
        );
    }
    let forensic_log = publish
        .find("self.log_publish_transaction")
        .expect("generic RX retains bounded publish formatting");
    assert!(publish[..forensic_log].contains("if diagnostics.enabled()"));
}

#[test]
fn silent_rx_pop_and_notify_retain_anomaly_paths() {
    let pop = section(
        SOURCE,
        "    fn pop_used_with_routine_diagnostics",
        "\n}\n\n#[repr(C)]",
    );
    for retained in [
        "invalidate_used_header_for_cpu",
        "used ring advanced beyond queue size",
        "pop_used invariant violation",
        "invalidate_used_elem_for_cpu",
        "used len zero after re-read",
        "ForensicFaultReason::UsedLenZeroRepeat",
        "ForensicFaultReason::UsedIdOutOfRange",
        "ForensicFaultReason::UsedDescriptorZero",
    ] {
        assert!(
            pop.contains(retained),
            "silent RX pop must retain {retained}"
        );
    }
    let healthy_pop_log = pop
        .find("[virtio-net] pop_used:")
        .expect("generic pop retains its healthy debug breadcrumb");
    assert!(pop[..healthy_pop_log].contains("if routine_diagnostics"));

    let notify = section(
        SOURCE,
        "    fn notify_with_routine_diagnostics",
        "    fn sync_descriptor_table_for_device",
    );
    for retained in [
        "dma_barrier",
        "virtq_notify_barrier",
        "regs.notify(queue)",
        "NET_DIAG.record_rx_kick()",
    ] {
        assert!(
            notify.contains(retained),
            "silent RX notify must retain {retained}",
        );
    }
    assert!(notify.contains("if routine_diagnostics"));
    assert!(notify.contains("notify queue={queue} (RX)"));
}
