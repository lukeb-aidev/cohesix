// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate Secure9P session window and short-write policy behavior.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use secure9p_core::{
    ShortWritePolicy, TagError, TagWindow, DEFAULT_SHORT_WRITE_BACKOFF_MS,
    DEFAULT_SHORT_WRITE_RETRIES,
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
    assert_eq!(policy.retry_delay_ms(0), Some(DEFAULT_SHORT_WRITE_BACKOFF_MS));
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
