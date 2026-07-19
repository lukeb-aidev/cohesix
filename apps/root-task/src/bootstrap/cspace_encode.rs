// Copyright © 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the bootstrap/cspace_encode module for root-task.
// Author: Lukas Bower
#![allow(dead_code)]

use core::convert::TryFrom;

/// AArch64 WordBits for seL4 (cap depth)
pub const WORD_BITS: u8 = 64;

/// Encode a raw slot number into a CPtr index suitable for use with depth=WORD_BITS.
///
/// This is intentionally trivial (identity) but typed and checked to keep all
/// call sites consistent and future-proof.
#[inline]
pub fn encode_slot_for_wordbits(slot: u32) -> (u64, u8) {
    // For 64-bit CPtrs, treating the slot as the fully-encoded index works with depth=WORD_BITS.
    (slot as u64, WORD_BITS)
}

/// Convert a bootinfo-provided radix to `u8`, rejecting unrepresentable values.
#[inline]
pub fn bits_u8(bits: usize) -> Option<u8> {
    u8::try_from(bits).ok()
}

#[cfg(test)]
mod tests {
    use super::{bits_u8, encode_slot_for_wordbits, WORD_BITS};

    #[test]
    fn word_depth_slot_encoding_is_identity() {
        assert_eq!(encode_slot_for_wordbits(0x1234), (0x1234, WORD_BITS));
    }

    #[test]
    fn radix_conversion_preserves_profile_values() {
        assert_eq!(bits_u8(13), Some(13));
        assert_eq!(bits_u8(14), Some(14));
    }

    #[test]
    fn radix_conversion_rejects_unrepresentable_values() {
        assert_eq!(bits_u8(usize::MAX), None);
    }
}
