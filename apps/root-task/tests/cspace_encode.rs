// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines tests for root-task cspace_encode.
// Author: Lukas Bower

#![cfg(feature = "kernel")]

use root_task::bootstrap::cspace_encode::{bits_u8_or_13, encode_slot_for_wordbits, WORD_BITS};

#[test]
fn encode_slot_for_wordbits_is_identity() {
    for &slot in &[0u32, 1, 0x1234, 0xFFFF] {
        let (encoded, depth) = encode_slot_for_wordbits(slot);
        assert_eq!(encoded, u64::from(slot));
        assert_eq!(depth, WORD_BITS);
    }
}

#[test]
fn bits_u8_or_13_clamps_large_values() {
    assert_eq!(bits_u8_or_13(12), 12);
    assert_eq!(bits_u8_or_13(usize::from(WORD_BITS)), WORD_BITS);
    assert_eq!(bits_u8_or_13(usize::MAX), 13);
}
