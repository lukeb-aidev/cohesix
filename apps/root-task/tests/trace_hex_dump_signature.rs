// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate the public trace hex dump API stays slice-only without compile-fail harness cost.
// Author: Lukas Bower

#[test]
fn hex_dump_slice_signature_accepts_only_slices() {
    fn assert_signature(_func: fn(&str, &[u8], usize)) {}
    assert_signature(root_task::trace::hex_dump_slice);
}
