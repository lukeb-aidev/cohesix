// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines tests for root-task cspace_bits.
// Author: Lukas Bower

#![cfg(feature = "kernel")]

use root_task::bootstrap::cspace_sys::super_bits_as_u8_for_test;

#[test]
fn bits_fit_common_values() {
    for v in [8usize, 12, 13, 14, 16, 21] {
        let bits = super_bits_as_u8_for_test(v);
        assert_eq!(bits as usize, v);
    }
}

#[test]
#[should_panic(expected = "initThreadCNodeSizeBits must fit in u8")]
fn bits_out_of_range_fails_closed() {
    let _ = super_bits_as_u8_for_test(1_000usize);
}
