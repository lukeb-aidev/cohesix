// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Verify target-neutral Python defaults and exact target-qualified profile contracts.
// Author: Lukas Bower

use coh_rtc::codegen::{cohesix_py, hash_bytes};
use coh_rtc::ir::HostTicketAction;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("coh-rtc lives under tools/")
        .to_path_buf()
}

fn render(manifest_name: &str, profile: &str, target: &str) -> (Vec<u8>, Value) {
    let manifest_path = repo_root().join("configs").join(manifest_name);
    let manifest = coh_rtc::ir::load_manifest(&manifest_path).expect("load manifest");
    manifest
        .validate_with_base(manifest_path.parent())
        .expect("validate manifest");
    let resolved = coh_rtc::ir::serialize_manifest(&manifest).expect("serialize manifest");
    let bytes =
        cohesix_py::render_profile_contract(&manifest, &hash_bytes(&resolved), profile, target)
            .expect("render Python contract");
    let value = serde_json::from_slice(&bytes).expect("parse Python contract");
    (bytes, value)
}

#[test]
fn target_neutral_defaults_cannot_name_live_target_or_proof() {
    let manifest_path = repo_root().join("configs/root_task.toml");
    let manifest = coh_rtc::ir::load_manifest(&manifest_path).expect("load manifest");
    let resolved = coh_rtc::ir::serialize_manifest(&manifest).expect("serialize manifest");
    let rendered = cohesix_py::render_defaults(&manifest, &hash_bytes(&resolved));
    assert!(rendered
        .python
        .contains("\"contract_kind\": \"target-neutral-fallback\""));
    assert!(rendered.python.contains("\"manifest_sha256\": None"));
    assert!(rendered.python.contains("\"execution_proof\": \"none\""));
}

#[test]
fn qemu_pi_and_regression_contracts_bind_distinct_selected_manifests() {
    let (qemu_bytes, qemu) = render("root_task.toml", "qemu_smp_production", "qemu");
    let (pi_bytes, pi) = render("root_task_pi4_uboot_aarch64.toml", "pi4_production", "pi4");
    let (_, regression) = render("root_task_regression.toml", "qemu_smp_production", "qemu");

    assert_eq!(qemu["schema"], "cohesix-python-profile/v2");
    assert_eq!(qemu["target"], "qemu");
    assert_eq!(pi["target"], "pi4");
    assert_ne!(qemu["manifest_sha256"], pi["manifest_sha256"]);
    assert_ne!(qemu["manifest_sha256"], regression["manifest_sha256"]);
    assert_ne!(qemu_bytes, pi_bytes);
    assert_eq!(qemu["worker"]["maximum_live_tasks"], 256);
    assert_eq!(pi["worker"]["maximum_live_tasks"], 256);
    assert_eq!(
        pi["worker"]["executable_roles"],
        qemu["worker"]["executable_roles"]
    );
    for contract in [&qemu, &pi, &regression] {
        assert_eq!(
            contract["receipts"]["gpu_actions"],
            serde_json::json!([
                "gpu.lease.grant",
                "gpu.lease.renew",
                "gpu.lease.release",
                "gpu.workload.submit",
                "gpu.workload.cancel",
                "gpu.workload.observe"
            ])
        );
    }
    assert_eq!(
        qemu["receipts"]["peft_actions"],
        serde_json::json!([
            "peft.export",
            "peft.import",
            "peft.activate",
            "peft.rollback",
            "peft.release"
        ])
    );
    assert_eq!(
        qemu["proof_boundary"]["python_projection_is_authority"],
        false
    );
    assert_eq!(
        pi["receipts"]["peft_actions"],
        serde_json::json!([
            "peft.export",
            "peft.import",
            "peft.activate",
            "peft.rollback",
            "peft.release"
        ])
    );
}

#[test]
fn native_kvm_contract_retains_its_selected_profile() {
    let (_, kvm) = render("root_task.toml", "qemu_smp_kvm_production", "qemu");
    assert_eq!(kvm["target"], "qemu");
    assert_eq!(kvm["target_profile"], "qemu_smp_kvm_production");
    assert_eq!(kvm["manifest_profile"], "virt-aarch64");
    assert_eq!(
        kvm["proof_boundary"]["python_projection_is_authority"],
        false
    );
}

#[test]
fn target_and_sel4_profile_mismatch_fail_closed() {
    let manifest_path = repo_root().join("configs/root_task.toml");
    let manifest = coh_rtc::ir::load_manifest(&manifest_path).expect("load manifest");
    let resolved = coh_rtc::ir::serialize_manifest(&manifest).expect("serialize manifest");
    let error = cohesix_py::render_profile_contract(
        &manifest,
        &hash_bytes(&resolved),
        "pi4_production",
        "pi4",
    )
    .expect_err("QEMU manifest must not become Pi contract");
    assert!(error.to_string().contains("requires manifest profile"));
    for profile in ["pi4_production", "qemu_smp_diagnostic", "unknown"] {
        assert!(cohesix_py::render_profile_contract(
            &manifest,
            &hash_bytes(&resolved),
            profile,
            "qemu",
        )
        .is_err());
    }
}

#[test]
fn host_ticket_v2_schema_and_receipt_matrices_are_exact() {
    let manifest_path = repo_root().join("configs/root_task.toml");
    let manifest = coh_rtc::ir::load_manifest(&manifest_path).expect("load manifest");
    assert_eq!(
        manifest.ecosystem.host.tickets.accepted_request_schemas,
        ["host-ticket/v1", "host-ticket/v2"]
    );
    assert_eq!(
        manifest.ecosystem.host.tickets.accepted_result_schemas,
        ["host-ticket-result/v1", "host-ticket-result/v2"]
    );
    assert_eq!(
        manifest.ecosystem.host.tickets.receipt_action_allowlist,
        [
            HostTicketAction::GpuLeaseGrant,
            HostTicketAction::GpuLeaseRenew,
            HostTicketAction::GpuLeaseRelease,
            HostTicketAction::GpuWorkloadSubmit,
            HostTicketAction::GpuWorkloadCancel,
            HostTicketAction::GpuWorkloadObserve,
            HostTicketAction::PeftExport,
            HostTicketAction::PeftImport,
            HostTicketAction::PeftActivate,
            HostTicketAction::PeftRollback,
            HostTicketAction::PeftRelease,
        ]
    );

    let mut missing_schema = manifest.clone();
    missing_schema
        .ecosystem
        .host
        .tickets
        .accepted_request_schemas
        .pop();
    let error = missing_schema
        .validate_with_base(manifest_path.parent())
        .expect_err("one-sided request schemas must fail");
    assert!(error.to_string().contains("schema matrices must be exact"));

    let mut missing_action = manifest;
    missing_action
        .ecosystem
        .host
        .tickets
        .receipt_action_allowlist
        .pop();
    let error = missing_action
        .validate_with_base(manifest_path.parent())
        .expect_err("incomplete receipt actions must fail");
    assert!(error
        .to_string()
        .contains("the existing GPU/PEFT actions and the complete selected workload action set"));
}

#[test]
fn canonical_targets_share_enrolled_macos_defaults_without_worker_receipts() {
    for name in ["root_task.toml", "root_task_pi4_uboot_aarch64.toml"] {
        let manifest = coh_rtc::ir::load_manifest(&repo_root().join("configs").join(name))
            .expect("load canonical target manifest");
        let host = serde_json::to_value(&manifest.ecosystem.host).expect("serialize host contract");
        let sources = host["snapshots"]["publishers"]
            .as_array()
            .expect("publisher array");
        let mac = sources
            .iter()
            .find(|source| source["source_id"] == "mac-controller")
            .expect("default Mac source");
        assert_eq!(
            mac["providers"],
            serde_json::json!(["launchd", "network", "endpoint_compliance"])
        );
        for action in [
            "launchd.start",
            "launchd.stop",
            "launchd.restart",
            "launchd.status-check",
            "endpoint_compliance.observe",
        ] {
            assert!(
                host["tickets"]["action_allowlist"]
                    .as_array()
                    .expect("action array")
                    .iter()
                    .any(|selected| selected == action),
                "missing {action} in {name}"
            );
            assert!(
                !host["tickets"]["receipt_action_allowlist"]
                    .as_array()
                    .expect("receipt array")
                    .iter()
                    .any(|selected| selected == action),
                "unsupported Worker receipt for {action}"
            );
        }
    }
    let source = fs::read_to_string(repo_root().join("configs/host_integration_acceptance.toml"))
        .expect("read provider source");
    let registry: toml::Value = toml::from_str(&source).expect("parse provider source");
    assert_eq!(
        registry["providers"]["macos_targets"]
            .as_array()
            .expect("macOS targets")
            .len(),
        1
    );
    assert_eq!(
        registry["providers"]["macos_targets"][0]["id"].as_str(),
        Some("local-endpoint-compliance")
    );
    assert_eq!(
        registry["providers"]["macos_targets"][0]["operation"]["action"].as_str(),
        Some("endpoint_compliance.observe")
    );
    assert!(
        registry["providers"].get("launchd_targets").is_none(),
        "services require exact deployment enrollment"
    );
}
