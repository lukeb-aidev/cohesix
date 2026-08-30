// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Verify AI LoRA authority and the retired radio-sidecar schema boundary.
// Author: Lukas Bower

use coh_rtc::{compile, compile_with_timer_clock_hz, CompileOptions};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

const PROFILE_MANIFESTS: [&str; 5] = [
    "configs/root_task.toml",
    "configs/root_task_pi4_uboot_aarch64.toml",
    "configs/root_task_regression.toml",
    "configs/root_task_uefi_aarch64.toml",
    "configs/root_task_uefi_aarch64_no_local_seat.toml",
];

fn repo_path(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("coh-rtc must have a tools parent")
        .parent()
        .expect("tools must have a repository parent")
        .join(path)
}

fn options_for(manifest_path: PathBuf, work_root: &Path) -> CompileOptions {
    fs::create_dir_all(work_root).expect("create test output directory");
    CompileOptions {
        manifest_path,
        out_dir: work_root.join("generated"),
        manifest_out: work_root.join("root_task_resolved.json"),
        cas_manifest_template_out: work_root.join("cas_manifest_template.json"),
        cli_script_out: work_root.join("boot_v0.coh"),
        doc_snippet_out: work_root.join("root_task_manifest.md"),
        gpu_breadcrumbs_snippet_out: work_root.join("gpu_breadcrumbs.md"),
        observability_interfaces_snippet_out: work_root.join("observability_interfaces.md"),
        observability_security_snippet_out: work_root.join("observability_security.md"),
        ticket_quotas_snippet_out: work_root.join("ticket_quotas.md"),
        trace_policy_snippet_out: work_root.join("trace_policy.md"),
        cas_interfaces_snippet_out: work_root.join("cas_interfaces.md"),
        cas_security_snippet_out: work_root.join("cas_security.md"),
        cbor_snippet_out: work_root.join("telemetry_cbor.md"),
        cohesix_py_defaults_out: work_root.join("cohesix_py_defaults.py"),
        cohesix_py_doc_out: work_root.join("cohesix_py_defaults.md"),
        coh_doctor_doc_out: work_root.join("coh_doctor_checks.md"),
        cohsh_policy_out: work_root.join("cohsh_policy.toml"),
        cohsh_policy_rust_out: work_root.join("cohsh_policy.rs"),
        cohsh_policy_doc_out: work_root.join("cohsh_policy.md"),
        cohsh_client_rust_out: work_root.join("cohsh_client.rs"),
        cohsh_client_doc_out: work_root.join("cohsh_client.md"),
        cohsh_grammar_doc_out: work_root.join("cohsh_grammar.md"),
        cohsh_ticket_policy_doc_out: work_root.join("cohsh_ticket_policy.md"),
        coh_policy_out: work_root.join("coh_policy.toml"),
        coh_policy_rust_out: work_root.join("coh_policy.rs"),
        coh_policy_doc_out: work_root.join("coh_policy.md"),
        swarmui_defaults_out: work_root.join("swarmui_defaults.toml"),
        swarmui_defaults_rust_out: work_root.join("swarmui_defaults.rs"),
        swarmui_defaults_doc_out: work_root.join("swarmui_defaults.md"),
    }
}

#[test]
fn removed_radio_sidecar_schema_is_rejected() {
    let temp_dir = TempDir::new().expect("create tempdir");
    let manifest_path = temp_dir.path().join("radio-sidecar.toml");
    let mut manifest =
        fs::read_to_string(repo_path("configs/root_task.toml")).expect("read default manifest");
    manifest.push_str(
        r#"

[sidecars.lora]
enable = false
mount_at = "/lora"
"#,
    );
    fs::write(&manifest_path, manifest).expect("write invalid radio-sidecar manifest");

    let options = options_for(manifest_path, &temp_dir.path().join("invalid"));
    let error = compile(&options).expect_err("removed sidecars.lora schema must be rejected");
    let message = format!("{error:#}");
    assert!(
        message.contains("unknown field `lora`"),
        "unexpected rejection: {message}"
    );
}

#[test]
fn selected_profile_timer_is_exact_in_resolved_and_rust_outputs() {
    let temp_dir = TempDir::new().expect("create tempdir");
    let options = options_for(
        repo_path("configs/root_task.toml"),
        &temp_dir.path().join("kvm-timer"),
    );

    compile_with_timer_clock_hz(&options, Some(31_250_000))
        .expect("compile KVM timer-resolved manifest");
    let resolved: Value = serde_json::from_slice(
        &fs::read(&options.manifest_out).expect("read resolved KVM manifest"),
    )
    .expect("parse resolved KVM manifest");
    assert_eq!(
        resolved["console_network_service"]["timer_clock_hz"],
        31_250_000
    );
    let bootstrap = fs::read_to_string(options.out_dir.join("bootstrap.rs"))
        .expect("read generated KVM bootstrap");
    assert!(bootstrap.contains("timer_clock_hz: 31250000"));

    let zero_options = options_for(
        repo_path("configs/root_task.toml"),
        &temp_dir.path().join("zero-timer"),
    );
    let error = compile_with_timer_clock_hz(&zero_options, Some(0))
        .expect_err("zero profile timer must fail closed");
    assert!(format!("{error:#}").contains("timer_clock_hz must be nonzero"));
}

#[test]
fn schema_1_10_is_rejected_after_operator_serial_contract_change() {
    let temp_dir = TempDir::new().expect("create tempdir");
    let manifest_path = temp_dir.path().join("schema-1.10.toml");
    let manifest = fs::read_to_string(repo_path("configs/root_task.toml"))
        .expect("read default manifest")
        .replacen("schema = \"1.15\"", "schema = \"1.10\"", 1);
    fs::write(&manifest_path, manifest).expect("write legacy-schema manifest");

    let options = options_for(manifest_path, &temp_dir.path().join("legacy"));
    let error = compile(&options).expect_err("schema 1.10 must be rejected");
    let message = format!("{error:#}");
    assert!(
        message.contains("unsupported root_task.schema 1.10 (expected 1.15)"),
        "unexpected rejection: {message}"
    );
}

#[test]
fn schema_1_11_is_rejected_after_publication_ack_contract_change() {
    let temp_dir = TempDir::new().expect("create tempdir");
    let manifest_path = temp_dir.path().join("schema-1.11.toml");
    let manifest = fs::read_to_string(repo_path("configs/root_task.toml"))
        .expect("read default manifest")
        .replacen("schema = \"1.15\"", "schema = \"1.11\"", 1);
    fs::write(&manifest_path, manifest).expect("write legacy-schema manifest");

    let options = options_for(manifest_path, &temp_dir.path().join("legacy-ack"));
    let error = compile(&options).expect_err("schema 1.11 must be rejected");
    let message = format!("{error:#}");
    assert!(
        message.contains("unsupported root_task.schema 1.11 (expected 1.15)"),
        "unexpected rejection: {message}"
    );
}

#[test]
fn schema_1_12_is_rejected_after_send_batch_contract_change() {
    let temp_dir = TempDir::new().expect("create tempdir");
    let manifest_path = temp_dir.path().join("schema-1.12.toml");
    let manifest = fs::read_to_string(repo_path("configs/root_task.toml"))
        .expect("read default manifest")
        .replacen("schema = \"1.15\"", "schema = \"1.12\"", 1);
    fs::write(&manifest_path, manifest).expect("write legacy-schema manifest");

    let options = options_for(manifest_path, &temp_dir.path().join("legacy-batch"));
    let error = compile(&options).expect_err("schema 1.12 must be rejected");
    let message = format!("{error:#}");
    assert!(
        message.contains("unsupported root_task.schema 1.12 (expected 1.15)"),
        "unexpected rejection: {message}"
    );
}

#[test]
fn schema_1_13_is_rejected_after_natural_postpone_contract_change() {
    let temp_dir = TempDir::new().expect("create tempdir");
    let manifest_path = temp_dir.path().join("schema-1.13.toml");
    let manifest = fs::read_to_string(repo_path("configs/root_task.toml"))
        .expect("read default manifest")
        .replacen("schema = \"1.15\"", "schema = \"1.13\"", 1);
    fs::write(&manifest_path, manifest).expect("write legacy-schema manifest");

    let options = options_for(manifest_path, &temp_dir.path().join("legacy-timeout"));
    let error = compile(&options).expect_err("schema 1.13 must be rejected");
    let message = format!("{error:#}");
    assert!(
        message.contains("unsupported root_task.schema 1.13 (expected 1.15)"),
        "unexpected rejection: {message}"
    );
}

#[test]
fn schema_1_14_is_rejected_after_worker_execution_contract_change() {
    let temp_dir = TempDir::new().expect("create tempdir");
    let manifest_path = temp_dir.path().join("schema-1.14.toml");
    let manifest = fs::read_to_string(repo_path("configs/root_task.toml"))
        .expect("read default manifest")
        .replacen("schema = \"1.15\"", "schema = \"1.14\"", 1);
    fs::write(&manifest_path, manifest).expect("write legacy-schema manifest");

    let options = options_for(manifest_path, &temp_dir.path().join("legacy-worker"));
    let error = compile(&options).expect_err("schema 1.14 must be rejected");
    let message = format!("{error:#}");
    assert!(
        message.contains("unsupported root_task.schema 1.14 (expected 1.15)"),
        "unexpected rejection: {message}"
    );
}

#[test]
fn checked_in_profiles_compile_without_radio_sidecar_output() {
    let temp_dir = TempDir::new().expect("create tempdir");

    for (index, profile) in PROFILE_MANIFESTS.iter().enumerate() {
        let work_root = temp_dir.path().join(format!("profile-{index}"));
        let options = options_for(repo_path(profile), &work_root);
        compile(&options).unwrap_or_else(|error| panic!("{profile} must compile: {error:#}"));

        let generated = fs::read_to_string(options.out_dir.join("mod.rs"))
            .unwrap_or_else(|error| panic!("read generated module for {profile}: {error}"));
        let bootstrap = fs::read_to_string(options.out_dir.join("bootstrap.rs"))
            .unwrap_or_else(|error| panic!("read generated bootstrap for {profile}: {error}"));
        let resolved = fs::read_to_string(&options.manifest_out)
            .unwrap_or_else(|error| panic!("read resolved manifest for {profile}: {error}"));
        let snippet = fs::read_to_string(&options.doc_snippet_out)
            .unwrap_or_else(|error| panic!("read manifest snippet for {profile}: {error}"));

        assert!(!generated.contains("SidecarLora"), "{profile}");
        assert!(generated.contains("pub direct_genet: bool"), "{profile}");
        assert!(!resolved.contains("\"lora\": {"), "{profile}");
        assert!(!snippet.contains("sidecars.lora"), "{profile}");

        let resolved: Value = serde_json::from_str(&resolved)
            .unwrap_or_else(|error| panic!("parse resolved manifest for {profile}: {error}"));
        assert_eq!(resolved["root_task"]["schema"], "1.15", "{profile}");
        assert_eq!(
            resolved["console_network_service"]["abi_version"], 5,
            "{profile}"
        );
        let worker_lora = resolved["worker_runtime"]["roles"]
            .as_array()
            .and_then(|roles| roles.iter().find(|role| role["role"] == "worker-lora"))
            .unwrap_or_else(|| panic!("worker-lora role missing from {profile}"));
        assert_eq!(worker_lora["ticket_scope"], "/worker", "{profile}");
        assert_eq!(worker_lora["lease_path_template"], "", "{profile}");

        if matches!(
            *profile,
            "configs/root_task.toml" | "configs/root_task_pi4_uboot_aarch64.toml"
        ) {
            let tasks = resolved["temporal_authority"]["tasks"]
                .as_array()
                .unwrap_or_else(|| panic!("temporal tasks missing from {profile}"));
            let root = tasks
                .iter()
                .find(|task| task["id"] == "root-control")
                .unwrap_or_else(|| panic!("root-control missing from {profile}"));
            let child = tasks
                .iter()
                .find(|task| task["id"] == "console-network-service")
                .unwrap_or_else(|| panic!("console-network-service missing from {profile}"));
            let (
                expected_root,
                expected_budget,
                expected_root_response,
                expected_child_priority,
                expected_child_mcp,
                expected_child_response,
                expected_timer_clock_hz,
                expected_direct_virtio,
                expected_direct_genet,
                expected_console_cspace_slots,
            ) = if *profile == "configs/root_task.toml" {
                (
                    "m26e-qemu-root-dedicated-core-bounded-quantum-v1",
                    9_000,
                    8_500,
                    180,
                    200,
                    3_000,
                    24_000_000,
                    true,
                    false,
                    162,
                )
            } else {
                (
                    "m26e-pi4-root-cross-core-console-parallel-candidate-v25",
                    5_500,
                    5_100,
                    200,
                    200,
                    3_000,
                    54_000_000,
                    false,
                    true,
                    161,
                )
            };
            assert_eq!(root["wcet_provenance"], expected_root, "{profile}");
            assert_eq!(root["timeout_policy"], "natural-postpone", "{profile}");
            assert_eq!(root["budget_us"], expected_budget, "{profile}");
            assert_eq!(root["period_us"], 10_000, "{profile}");
            assert_eq!(root["max_refills"], 2, "{profile}");
            assert_eq!(
                root["response_time_us"], expected_root_response,
                "{profile}"
            );
            assert_eq!(
                resolved["console_network_service"]["timer_clock_hz"], expected_timer_clock_hz,
                "{profile}"
            );
            assert_eq!(
                child["wcet_provenance"],
                if *profile == "configs/root_task.toml" {
                    "m26e-qemu-console-received-progress-retention-candidate-v18"
                } else {
                    "m26e-pi4-console-cross-core-signal-only-candidate-v19"
                },
                "{profile}"
            );
            assert_eq!(child["timeout_policy"], "natural-postpone", "{profile}");
            assert_eq!(child["wcet_us"], 3_000, "{profile}");
            assert_eq!(child["priority"], expected_child_priority, "{profile}");
            assert_eq!(child["mcp"], expected_child_mcp, "{profile}");
            assert_eq!(
                child["response_time_us"], expected_child_response,
                "{profile}"
            );
            assert_eq!(
                resolved["console_network_service"]["direct_virtio"], expected_direct_virtio,
                "{profile}"
            );
            assert_eq!(
                resolved["console_network_service"]["objects"]["cspace_slots"],
                expected_console_cspace_slots,
                "{profile}"
            );
            assert!(
                bootstrap.contains(&format!("direct_virtio: {expected_direct_virtio},")),
                "{profile}"
            );
            assert!(
                bootstrap.contains(&format!("direct_genet: {expected_direct_genet},")),
                "{profile}"
            );
        }
    }
}

#[test]
fn regression_profile_preserves_selected_m26e_operational_topology() {
    let temp_dir = TempDir::new().expect("create tempdir");
    let mut resolved_profiles = Vec::new();

    for (label, manifest) in [
        ("base", "configs/root_task.toml"),
        ("regression", "configs/root_task_regression.toml"),
    ] {
        let work_root = temp_dir.path().join(label);
        let options = options_for(repo_path(manifest), &work_root);
        compile(&options).unwrap_or_else(|error| panic!("{manifest} must compile: {error:#}"));
        let resolved = fs::read_to_string(&options.manifest_out)
            .unwrap_or_else(|error| panic!("read resolved manifest for {manifest}: {error}"));
        resolved_profiles.push(
            serde_json::from_str::<Value>(&resolved)
                .unwrap_or_else(|error| panic!("parse resolved manifest for {manifest}: {error}")),
        );
    }

    let base = &resolved_profiles[0];
    let regression = &resolved_profiles[1];
    for section in [
        "root_task",
        "worker_runtime",
        "temporal_authority",
        "ninedoor_service",
        "console_network_service",
        "worker_resource_admission",
    ] {
        assert_eq!(
            regression[section], base[section],
            "regression manifest must preserve selected M26e operational section {section}"
        );
    }
    assert_eq!(
        regression["console_network_service"]["timer_clock_hz"],
        24_000_000
    );
}
