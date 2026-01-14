// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate ingest metrics counters and allocation-free hot paths.
// Author: Lukas Bower

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use root_task::observe::IngestMetrics;

struct CountingAlloc;

static ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::SeqCst);
        System.alloc(layout)
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::SeqCst);
        System.alloc_zeroed(layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::SeqCst);
        System.realloc(ptr, layout, new_size)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
    }
}

#[global_allocator]
static GLOBAL_ALLOC: CountingAlloc = CountingAlloc;

fn reset_alloc_count() {
    ALLOC_COUNT.store(0, Ordering::SeqCst);
}

fn alloc_count() -> usize {
    ALLOC_COUNT.load(Ordering::SeqCst)
}

#[test]
fn ingest_metrics_updates_without_allocations() {
    let mut metrics = IngestMetrics::default();
    reset_alloc_count();

    for sample_ms in [1u64, 5, 10, 20, 30, 40] {
        metrics.record_latency_ms(sample_ms);
    }
    metrics.record_backpressure();
    metrics.record_drop();
    let snapshot = metrics.snapshot(3);

    assert_eq!(
        alloc_count(),
        0,
        "ingest metrics allocated on hot path"
    );
    assert_eq!(snapshot.backpressure, 1);
    assert_eq!(snapshot.dropped, 1);
    assert_eq!(snapshot.queued, 3);
    assert!(snapshot.p50_ms <= snapshot.p95_ms);
}

#[test]
fn ingest_metrics_percentiles_are_deterministic() {
    let mut metrics = IngestMetrics::default();
    for sample_ms in [5u64, 10, 15, 20] {
        metrics.record_latency_ms(sample_ms);
    }
    let snapshot = metrics.snapshot(0);

    assert_eq!(snapshot.p50_ms, 10);
    assert_eq!(snapshot.p95_ms, 15);
}
