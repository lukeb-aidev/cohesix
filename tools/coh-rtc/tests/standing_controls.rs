// Author: Lukas Bower
// Purpose: Verify generated standing-authority ceilings and their host-ticket prerequisites.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0

use coh_rtc::ir::load_manifest;
use cohesix_authority::standing::{StandingControls, STANDING_CONTROLS_SCHEMA};
use std::path::PathBuf;

fn selected() -> (PathBuf, coh_rtc::ir::Manifest) {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../configs");
    let manifest = load_manifest(&base.join("root_task.toml")).expect("selected manifest");
    (base, manifest)
}

fn enabled() -> StandingControls {
    StandingControls {
        schema: STANDING_CONTROLS_SCHEMA.into(),
        enabled: true,
        actions: vec!["gpu.workload.submit".into(), "systemd.restart".into()],
        max_scopes: 2,
        max_jobs: 256,
        max_job_units: 1,
        max_total_units: 256,
        max_concurrent: 2,
        max_retries: 2,
        max_cooldown_ms: 60_000,
        max_fact_age_ms: 5_000,
        max_decision_ttl_ms: 5_000,
    }
}

#[test]
fn absent_standing_policy_is_closed_and_valid_selected_ceiling_compiles() {
    let (base, mut manifest) = selected();
    assert!(manifest.standing_authority.enabled);
    assert_eq!(
        StandingControls::from_resolved_manifest(br#"{}"#).expect("older manifest"),
        StandingControls::default()
    );
    manifest.standing_authority = enabled();
    manifest
        .validate_with_base(Some(&base))
        .expect("narrow standing ceiling");
}

#[test]
fn missing_host_ticket_dependency_and_runtime_action_widening_refuse() {
    let (base, mut manifest) = selected();
    manifest.standing_authority = enabled();
    // Isolate standing-authority validation from the selected MCP gateway,
    // which independently rejects a missing execution WAL first.
    manifest.gateway.agent_protocols.enabled = false;
    manifest.authority.execution_wal_required = false;
    assert!(manifest
        .validate_with_base(Some(&base))
        .expect_err("execution WAL required")
        .to_string()
        .contains("standing authority requires"));
    manifest.authority.execution_wal_required = true;
    manifest
        .standing_authority
        .actions
        .push("docker.run".into());
    assert!(manifest.validate_with_base(Some(&base)).is_err());
}
