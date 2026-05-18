// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate cache maintenance helpers and DMA audit log ordering.
// Author: Lukas Bower

#![cfg(feature = "cache-maintenance")]

use root_task::hal::cache::{set_test_error, CacheErrorKind, CacheMaintenance};
use root_task::hal::dma;
use sel4_sys::{seL4_InvalidArgument, seL4_NoError, seL4_RangeError};
use std::sync::Mutex;

static DMA_TEST_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn cache_maintenance_helpers_surface_success_and_error_paths() {
    let _guard = DMA_TEST_LOCK.lock().expect("cache test lock");
    let maintenance = CacheMaintenance::init_thread();

    set_test_error(Some(seL4_InvalidArgument));
    let err = maintenance
        .clean(0x1000, 64)
        .expect_err("expected invalid argument error");
    assert_eq!(err.kind(), CacheErrorKind::InvalidArgument);

    set_test_error(Some(seL4_RangeError));
    let err = maintenance
        .invalidate(usize::MAX - 32, 64)
        .expect_err("expected range error");
    assert_eq!(err.kind(), CacheErrorKind::Range);

    set_test_error(Some(seL4_NoError));
    maintenance
        .clean_invalidate(0x2000, 128)
        .expect("expected cache operation success");
}

#[test]
fn cache_maintenance_dma_audit_logs_flush_before_share_ready() {
    let _guard = DMA_TEST_LOCK.lock().expect("cache test lock");
    let _ = dma::take_audit_log();
    set_test_error(None);

    let range = dma::pin(0x2000, 0x4000, 0x80, "test-share").expect("pin");
    let lines = dma::take_audit_log();
    let clean_idx = lines
        .iter()
        .position(|line| line.contains("[dma][cache] clean-before-share"))
        .expect("clean log");
    let ready_idx = lines
        .iter()
        .position(|line| line.contains("[dma][share] ready"))
        .expect("ready log");
    assert!(
        clean_idx < ready_idx,
        "cache clean should occur before share ready"
    );

    let _ = dma::unpin(&range).expect("unpin");
    let unpin_lines = dma::take_audit_log();
    let reclaim_idx = unpin_lines
        .iter()
        .position(|line| line.contains("[dma][share] reclaim"))
        .expect("reclaim log");
    let invalidate_idx = unpin_lines
        .iter()
        .position(|line| line.contains("[dma][cache] invalidate-after-reclaim"))
        .expect("invalidate log");
    let reclaimed_idx = unpin_lines
        .iter()
        .position(|line| line.contains("[dma][share] reclaimed"))
        .expect("reclaimed log");
    assert!(
        reclaim_idx < invalidate_idx && invalidate_idx < reclaimed_idx,
        "cache invalidate should occur between reclaim and reclaimed logs"
    );
}

#[test]
fn cache_maintenance_dma_sync_for_cpu_invalidates_before_ready() {
    let _guard = DMA_TEST_LOCK.lock().expect("cache test lock");
    let _ = dma::take_audit_log();
    set_test_error(None);

    let _range = dma::sync_for_cpu(0x3000, 0x5000, 0x40, "test-cpu-sync").expect("sync for cpu");
    let lines = dma::take_audit_log();
    let sync_idx = lines
        .iter()
        .position(|line| line.contains("[dma][share] sync-for-cpu"))
        .expect("sync log");
    let invalidate_idx = lines
        .iter()
        .position(|line| line.contains("[dma][cache] invalidate-before-cpu-read"))
        .expect("invalidate log");
    let ready_idx = lines
        .iter()
        .position(|line| line.contains("[dma][share] cpu-ready"))
        .expect("ready log");
    assert!(
        sync_idx < invalidate_idx && invalidate_idx < ready_idx,
        "cache invalidate should occur before CPU-ready log"
    );

    set_test_error(Some(seL4_InvalidArgument));
    let err = dma::sync_for_cpu(0x3000, 0x5000, 0x40, "test-cpu-sync-error")
        .expect_err("expected injected cache failure");
    assert_eq!(
        err,
        dma::PinError::CacheFailure(root_task::hal::cache::CacheError::new(seL4_InvalidArgument))
    );
    set_test_error(None);
}

#[test]
fn cache_maintenance_dma_sync_for_cpu_can_suppress_hot_poll_audit() {
    let _guard = DMA_TEST_LOCK.lock().expect("cache test lock");
    let _ = dma::take_audit_log();
    set_test_error(None);

    let range =
        dma::sync_for_cpu(0x3000, 0x5000, 0x40, "xhci-event-ring-poll-fast").expect("sync for cpu");
    assert_eq!(range.label(), "xhci-event-ring-poll-fast");

    let lines = dma::take_audit_log();
    assert!(
        lines.is_empty(),
        "fast xHCI event-ring polling keeps cache maintenance but suppresses UART audit lines"
    );
}

#[test]
fn cache_maintenance_dma_pin_handles_xhci_audit_profiles() {
    let _guard = DMA_TEST_LOCK.lock().expect("cache test lock");
    let _ = dma::take_audit_log();
    set_test_error(None);

    let range = dma::pin(0x4000, 0x6000, 0x1000, "xhci-scratchpad-page").expect("scratchpad pin");
    assert_eq!(range.label(), "xhci-scratchpad-page");
    assert!(
        dma::take_audit_log().is_empty(),
        "xHCI scratchpad page sharing keeps cache maintenance but suppresses repetitive UART audit lines"
    );

    let range = dma::pin(0x5000, 0x7000, 0x1000, "xhci-cmd-ring-submit-full")
        .expect("command ring submit pin");
    assert_eq!(range.label(), "xhci-cmd-ring-submit-full");
    let lines = dma::take_audit_log();
    assert!(
        lines
            .iter()
            .any(|line| line.contains("[dma][cache] clean-invalidate-before-share")),
        "xHCI command-ring publication keeps the audit visible and uses clean+invalidate: {lines:?}"
    );
    assert!(
        !lines
            .iter()
            .any(|line| line.contains("[dma][cache] clean-before-share")),
        "xHCI command-ring publication must not use clean-only publication: {lines:?}"
    );
}

#[test]
fn cache_maintenance_dma_can_suppress_bcmgenet_hot_path_audit() {
    let _guard = DMA_TEST_LOCK.lock().expect("cache test lock");
    let _ = dma::take_audit_log();
    set_test_error(None);

    let rx_rearm = dma::pin(0x6000, 0x8000, 0x800, "bcmgenet-rx-rearm").expect("rx rearm pin");
    dma::unpin(&rx_rearm).expect("rx rearm unpin");
    let rx_complete =
        dma::sync_for_cpu(0x7000, 0x9000, 0x800, "bcmgenet-rx-complete").expect("rx sync");
    let tx_submit = dma::pin(0x8000, 0xa000, 0x600, "bcmgenet-tx-submit").expect("tx submit pin");
    dma::unpin(&tx_submit).expect("tx submit unpin");

    assert_eq!(rx_complete.label(), "bcmgenet-rx-complete");
    let lines = dma::take_audit_log();
    assert!(
        lines.is_empty(),
        "GENET packet-rate DMA paths keep cache maintenance but suppress repetitive UART audit lines: {lines:?}"
    );
}
