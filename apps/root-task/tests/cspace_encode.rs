// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines tests for root-task cspace_encode.
// Author: Lukas Bower

#![cfg(feature = "kernel")]

use root_task::bootstrap::cspace_encode::{bits_u8, encode_slot_for_wordbits, WORD_BITS};

#[test]
fn encode_slot_for_wordbits_is_identity() {
    for &slot in &[0u32, 1, 0x1234, 0xFFFF] {
        let (encoded, depth) = encode_slot_for_wordbits(slot);
        assert_eq!(encoded, u64::from(slot));
        assert_eq!(depth, WORD_BITS);
    }
}

#[test]
fn bits_u8_preserves_representable_values() {
    assert_eq!(bits_u8(12), Some(12));
    assert_eq!(bits_u8(usize::from(WORD_BITS)), Some(WORD_BITS));
}

#[test]
fn bits_u8_rejects_unrepresentable_values() {
    assert_eq!(bits_u8(usize::MAX), None);
}
