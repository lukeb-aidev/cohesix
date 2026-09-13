// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Bind the operator evidence allowance while preserving explicit ticket attenuation.
// Author: Lukas Bower

use coh_rtc::ir::{load_manifest, TicketLimits};
use std::path::PathBuf;

#[test]
fn default_session_budget_covers_evidence_and_shared_operator_traffic() {
    // SECURITY.md specifies 8 MiB, including three maximum retained-log
    // captures and 2 MiB of other shared operator traffic.
    let budget = 8_388_608;
    let retained_log = (2048 + 64) * (256 + 1);
    assert!(3 * retained_log + 2 * 1024 * 1024 < budget);
    let omitted: TicketLimits = toml::from_str("").expect("default ticket limits");
    assert_eq!(omitted.bandwidth_bytes, budget);
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    for manifest in [
        "root_task.toml",
        "root_task_regression.toml",
        "root_task_pi4_uboot_aarch64.toml",
        "root_task_uefi_aarch64.toml",
        "root_task_uefi_aarch64_no_local_seat.toml",
    ] {
        let selected = load_manifest(&root.join("configs").join(manifest))
            .expect("valid selected source manifest");
        assert_eq!(selected.ticket_limits.bandwidth_bytes, budget, "{manifest}");
    }
}

#[test]
fn explicit_smaller_session_budget_survives_default_resolution() {
    let explicit: TicketLimits =
        toml::from_str("bandwidth_bytes = 4096").expect("bounded explicit quota");
    assert_eq!(explicit.bandwidth_bytes, 4096);
    assert_eq!(explicit.max_scope_rate_per_s, 64);
    assert_eq!(explicit.cursor_resumes, 16);
    assert_eq!(explicit.cursor_advances, 256);
}
