// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Defines the bootinfo_layout module for root-task.
// Author: Lukas Bower
//! Shared bootinfo snapshot layout helpers.

/// Number of bytes reserved for the trailing post-canary.
pub const POST_CANARY_BYTES: usize = core::mem::size_of::<u64>();

/// Returns the offset at which the post-canary should be written for a
/// snapshot payload of the given length.
#[inline(always)]
pub const fn post_canary_offset(backing_len: usize) -> usize {
    backing_len
}
