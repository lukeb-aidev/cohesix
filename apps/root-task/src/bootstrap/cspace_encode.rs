// Copyright © 2025 Lukas Bower
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

/// Defensive clamp for initBits -> u8, with a hard fallback to 13.
#[inline]
pub fn bits_u8_or_13(bits: usize) -> u8 {
    u8::try_from(bits).unwrap_or(13)
}
