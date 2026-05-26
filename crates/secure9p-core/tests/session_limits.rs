// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate Secure9P session window and short-write policy behavior.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use secure9p_core::{
    FidError, QueueDepth, QueueError, SessionLimits, ShardedFidTable, ShortWritePolicy, TagError,
    TagWindow, DEFAULT_SHORT_WRITE_BACKOFF_MS, DEFAULT_SHORT_WRITE_RETRIES,
};

#[test]
fn tag_window_enforces_capacity() {
    let mut window = TagWindow::new(2);
    assert_eq!(window.reserve(10), Ok(()));
    assert_eq!(window.reserve(11), Ok(()));
    assert_eq!(window.reserve(12), Err(TagError::WindowFull));
    window.release(10);
    assert_eq!(window.reserve(12), Ok(()));
    assert_eq!(window.reserve(11), Err(TagError::InUse));
}

#[test]
fn short_write_policy_backoff_is_bounded() {
    let policy = ShortWritePolicy::Retry;
    assert_eq!(
        policy.retry_delay_ms(0),
        Some(DEFAULT_SHORT_WRITE_BACKOFF_MS)
    );
    assert_eq!(
        policy.retry_delay_ms(1),
        Some(DEFAULT_SHORT_WRITE_BACKOFF_MS * 2)
    );
    assert_eq!(
        policy.retry_delay_ms(2),
        Some(DEFAULT_SHORT_WRITE_BACKOFF_MS * 4)
    );
    assert_eq!(policy.retry_delay_ms(DEFAULT_SHORT_WRITE_RETRIES), None);
    assert_eq!(ShortWritePolicy::Reject.retry_delay_ms(0), None);
}

#[test]
fn sharded_fid_table_rejects_reuse_after_clunk() {
    let table = ShardedFidTable::new(4);
    assert_eq!(table.insert(7, "root"), Ok(()));
    assert!(table.contains(7));
    assert_eq!(table.insert(7, "duplicate"), Err(FidError::InUse));
    assert_eq!(table.get(7), Some("root"));
    assert_eq!(table.remove(7), Some("root"));
    assert!(table.contains(7));
    assert_eq!(table.get(7), None);
    assert_eq!(table.insert(7, "reused"), Err(FidError::Retired));
}

#[test]
fn thousand_worker_secure9p_pressure_keeps_windows_and_fids_bounded() {
    const WORKERS: usize = 1_000;
    const TAGS_PER_SESSION: u16 = 16;
    const FIDS_PER_WORKER: u32 = 4;

    let limits = SessionLimits {
        tags_per_session: TAGS_PER_SESSION,
        batch_frames: TAGS_PER_SESSION as usize,
        short_write_policy: ShortWritePolicy::Retry,
    };
    assert_eq!(limits.queue_depth_limit(), TAGS_PER_SESSION as usize);

    let fids = ShardedFidTable::new(64);
    let mut total_reserved_tags = 0usize;
    let mut total_completed_frames = 0usize;

    for worker in 0..WORKERS {
        let mut window = TagWindow::new(limits.tags_per_session);
        let mut queue = QueueDepth::new(limits.queue_depth_limit());
        queue
            .reserve(limits.queue_depth_limit())
            .expect("reserve full per-session queue depth");
        assert_eq!(queue.current(), limits.queue_depth_limit());
        assert_eq!(
            queue.reserve(1),
            Err(QueueError::Full),
            "worker {worker} admitted work beyond its bounded queue"
        );

        for tag in 0..limits.tags_per_session {
            window.reserve(tag).expect("reserve bounded tag");
            total_reserved_tags = total_reserved_tags.saturating_add(1);
        }
        assert_eq!(window.active_count(), limits.tags_per_session as usize);
        assert_eq!(
            window.reserve(limits.tags_per_session),
            Err(TagError::WindowFull)
        );

        for tag in 0..limits.tags_per_session {
            window.release(tag);
            queue.release(1);
            total_completed_frames = total_completed_frames.saturating_add(1);
        }
        assert_eq!(window.active_count(), 0);
        assert_eq!(queue.current(), 0);

        let base_fid = (worker as u32).saturating_mul(16).saturating_add(1);
        for offset in 0..FIDS_PER_WORKER {
            let fid = base_fid + offset;
            fids.insert(fid, worker).expect("insert worker fid");
            assert_eq!(fids.get(fid), Some(worker));
        }
        for offset in 0..FIDS_PER_WORKER {
            let fid = base_fid + offset;
            assert_eq!(fids.remove(fid), Some(worker));
            assert_eq!(fids.insert(fid, worker), Err(FidError::Retired));
        }
    }

    assert_eq!(total_reserved_tags, WORKERS * TAGS_PER_SESSION as usize);
    assert_eq!(total_completed_frames, total_reserved_tags);
}
