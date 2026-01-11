// Author: Lukas Bower
// Purpose: Validate cache maintenance helpers and DMA audit log ordering.

#![cfg(feature = "cache-maintenance")]

use root_task::hal::cache::{set_test_error, CacheErrorKind, CacheMaintenance};
use root_task::hal::dma;
use sel4_sys::{seL4_InvalidArgument, seL4_NoError, seL4_RangeError};

#[test]
fn cache_maintenance_helpers_surface_success_and_error_paths() {
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
