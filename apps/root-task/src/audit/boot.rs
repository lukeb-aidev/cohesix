// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Centralised boot header emission helper for early kernel logging.
// Author: Lukas Bower

#![cfg(feature = "kernel")]

/// Emit the early boot stage0 header lines using the provided sink.
///
/// The helper preserves ordering and content; callers must invoke it from the
/// same location as the previous inline calls to avoid any behavioural drift.
#[inline(always)]
pub fn emit_stage0_header(mut emit_line: impl FnMut(&'static str)) {
    log::info!("[kernel:entry] about to log stage0 entry");
    emit_line("entered from seL4 (stage0)");
    emit_line("Cohesix boot: root-task online");
}

/// Emit the version banner using the provided sink.
#[inline(always)]
pub fn emit_version_banner(mut emit_line: impl FnMut(&'static str)) {
    emit_line("Cohesix v0 (AArch64/virt)");
}
