// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate append-only offset helpers for Secure9P providers.
// Author: Lukas Bower

use secure9p_core::{
    append_only_read_bounds, append_only_write_bounds, AppendOnlyOffsetError,
};

#[test]
fn read_bounds_detect_stale_offsets() {
    let err = append_only_read_bounds(4, 8, 16, 12).expect_err("stale offset");
    assert_eq!(
        err,
        AppendOnlyOffsetError::Stale {
            requested: 4,
            available_start: 8
        }
    );
}

#[test]
fn read_bounds_compute_short_reads() {
    let bounds = append_only_read_bounds(8, 8, 12, 16).expect("valid bounds");
    assert_eq!(bounds.offset, 8);
    assert_eq!(bounds.len, 4);
    assert!(bounds.short);
}

#[test]
fn write_bounds_enforce_expected_offset() {
    let err = append_only_write_bounds(10, 5, 64, 12).expect_err("offset mismatch");
    assert_eq!(
        err,
        AppendOnlyOffsetError::Invalid {
            provided: 5,
            expected: 10
        }
    );
}

#[test]
fn write_bounds_flag_short_write() {
    let bounds = append_only_write_bounds(0, 0, 4, 12).expect("valid bounds");
    assert_eq!(bounds.len, 4);
    assert!(bounds.short);
}
