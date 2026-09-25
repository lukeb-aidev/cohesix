// Author: Lukas Bower
// Purpose: Check compiler-owned selected agent protocol flags, conjunctions, and prerequisites.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

use coh_rtc::ir::load_manifest;
use std::path::PathBuf;

#[test]
fn agent_protocol_flags_enforce_all_eight_combinations() {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../configs");
    let original = load_manifest(&base.join("root_task.toml")).expect("selected manifest");
    assert!(original.gateway.effective_mcp());
    assert!(original.gateway.effective_a2a());
    for mask in 0..8 {
        let mut manifest = original.clone();
        manifest.gateway.agent_protocols.enabled = mask & 1 != 0;
        manifest.gateway.mcp.enabled = mask & 2 != 0;
        manifest.gateway.a2a.enabled = mask & 4 != 0;
        manifest
            .validate_with_base(Some(&base))
            .expect("valid selection");
        assert_eq!(manifest.gateway.effective_mcp(), mask & 3 == 3);
        assert_eq!(manifest.gateway.effective_a2a(), mask & 5 == 5);
    }
}

#[test]
fn enabled_protocol_requires_current_host_authority_dependencies() {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../configs");
    let mut manifest = load_manifest(&base.join("root_task.toml")).expect("selected manifest");
    manifest.gateway.agent_protocols.enabled = true;
    manifest.gateway.mcp.enabled = true;
    manifest.authority.execution_wal_required = false;
    let error = manifest
        .validate_with_base(Some(&base))
        .expect_err("missing dependency must refuse");
    assert!(
        error
            .to_string()
            .contains("enabled gateway agent protocols"),
        "{error}"
    );
}

#[test]
fn unknown_control_schema_refuses_instead_of_enabling() {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../configs");
    let mut manifest = load_manifest(&base.join("root_task.toml")).expect("selected manifest");
    manifest.gateway.schema = "future".into();
    let error = manifest
        .validate_with_base(Some(&base))
        .expect_err("future schema must refuse");
    assert!(error.to_string().contains("unsupported gateway protocol"));
}
