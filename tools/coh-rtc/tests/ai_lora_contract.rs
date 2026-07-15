// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Verify AI LoRA authority and the retired radio-sidecar schema boundary.
// Author: Lukas Bower

use coh_rtc::{compile, CompileOptions};
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
fn schema_1_5_is_rejected_after_radio_sidecar_removal() {
    let temp_dir = TempDir::new().expect("create tempdir");
    let manifest_path = temp_dir.path().join("schema-1.5.toml");
    let manifest = fs::read_to_string(repo_path("configs/root_task.toml"))
        .expect("read default manifest")
        .replacen("schema = \"1.6\"", "schema = \"1.5\"", 1);
    fs::write(&manifest_path, manifest).expect("write legacy-schema manifest");

    let options = options_for(manifest_path, &temp_dir.path().join("legacy"));
    let error = compile(&options).expect_err("schema 1.5 must be rejected");
    let message = format!("{error:#}");
    assert!(
        message.contains("unsupported root_task.schema 1.5 (expected 1.6)"),
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
        let resolved = fs::read_to_string(&options.manifest_out)
            .unwrap_or_else(|error| panic!("read resolved manifest for {profile}: {error}"));
        let snippet = fs::read_to_string(&options.doc_snippet_out)
            .unwrap_or_else(|error| panic!("read manifest snippet for {profile}: {error}"));

        assert!(!generated.contains("SidecarLora"), "{profile}");
        assert!(!resolved.contains("\"lora\": {"), "{profile}");
        assert!(!snippet.contains("sidecars.lora"), "{profile}");

        let resolved: Value = serde_json::from_str(&resolved)
            .unwrap_or_else(|error| panic!("parse resolved manifest for {profile}: {error}"));
        let worker_lora = resolved["worker_runtime"]["roles"]
            .as_array()
            .and_then(|roles| roles.iter().find(|role| role["role"] == "worker-lora"))
            .unwrap_or_else(|| panic!("worker-lora role missing from {profile}"));
        assert_eq!(worker_lora["ticket_scope"], "/worker", "{profile}");
        assert_eq!(worker_lora["lease_path_template"], "", "{profile}");
    }
}
