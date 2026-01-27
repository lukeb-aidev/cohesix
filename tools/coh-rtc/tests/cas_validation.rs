// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Validate CAS manifest checks in coh-rtc.
// Author: Lukas Bower
#![forbid(unsafe_code)]

use coh_rtc::{compile, CompileOptions};
use std::fs;
use tempfile::TempDir;

fn base_manifest(extra: &str) -> String {
    format!(
        r#"
# Author: Lukas Bower
# Purpose: CAS validation test manifest.
[root_task]
schema = "1.5"

[profile]
name = "virt-aarch64"
kernel = true

[event_pump]
tick_ms = 5

[secure9p]
msize = 8192
walk_depth = 8
tags_per_session = 4
batch_frames = 1

[secure9p.short_write]
policy = "reject"

[features]
net_console = false
serial_console = true
std_console = false
std_host_tools = false

{extra}

[[tickets]]
role = "queen"
secret = "bootstrap"
"#
    )
}

fn compile_error(manifest: &str) -> String {
    let temp_dir = TempDir::new().expect("tempdir");
    let manifest_path = temp_dir.path().join("manifest.toml");
    fs::write(&manifest_path, manifest).expect("write manifest");
    let options = CompileOptions {
        manifest_path,
        out_dir: temp_dir.path().join("out"),
        manifest_out: temp_dir.path().join("resolved.json"),
        cas_manifest_template_out: temp_dir.path().join("cas_manifest_template.json"),
        cli_script_out: temp_dir.path().join("boot_v0.coh"),
        doc_snippet_out: temp_dir.path().join("snippet.md"),
        gpu_breadcrumbs_snippet_out: temp_dir.path().join("gpu_breadcrumbs.md"),
        observability_interfaces_snippet_out: temp_dir.path().join("observability_interfaces.md"),
        observability_security_snippet_out: temp_dir.path().join("observability_security.md"),
        ticket_quotas_snippet_out: temp_dir.path().join("ticket_quotas.md"),
        cas_interfaces_snippet_out: temp_dir.path().join("cas_interfaces.md"),
        cas_security_snippet_out: temp_dir.path().join("cas_security.md"),
        cbor_snippet_out: temp_dir.path().join("telemetry_cbor.md"),
        cohesix_py_defaults_out: temp_dir.path().join("cohesix_py_defaults.py"),
        cohesix_py_doc_out: temp_dir.path().join("cohesix_py_defaults.md"),
        coh_doctor_doc_out: temp_dir.path().join("coh_doctor_checks.md"),
        cohsh_policy_out: temp_dir.path().join("cohsh_policy.toml"),
        cohsh_policy_rust_out: temp_dir.path().join("cohsh_policy.rs"),
        cohsh_policy_doc_out: temp_dir.path().join("cohsh_policy.md"),
        cohsh_client_rust_out: temp_dir.path().join("cohsh_client.rs"),
        cohsh_client_doc_out: temp_dir.path().join("cohsh_client.md"),
        cohsh_grammar_doc_out: temp_dir.path().join("cohsh_grammar.md"),
        cohsh_ticket_policy_doc_out: temp_dir.path().join("cohsh_ticket_policy.md"),
        coh_policy_out: temp_dir.path().join("coh_policy.toml"),
        coh_policy_rust_out: temp_dir.path().join("coh_policy.rs"),
        coh_policy_doc_out: temp_dir.path().join("coh_policy.md"),
        swarmui_defaults_out: temp_dir.path().join("swarmui_defaults.toml"),
        swarmui_defaults_rust_out: temp_dir.path().join("swarmui_defaults.rs"),
        swarmui_defaults_doc_out: temp_dir.path().join("swarmui_defaults.md"),
    };
    let err = compile(&options).expect_err("manifest should be rejected");
    err.to_string()
}

#[test]
fn cas_chunk_bytes_exceeds_msize_rejected() {
    let manifest = base_manifest(
        r#"
[cas]
enable = true

[cas.store]
chunk_bytes = 9000

[cas.delta]
enable = false

[cas.signing]
required = false
"#,
    );
    let err = compile_error(&manifest);
    assert!(err.contains("cas.store.chunk_bytes"));
    assert!(err.contains("secure9p.msize"));
}

#[test]
fn cas_chunk_bytes_exceeds_budget_rejected() {
    let manifest = base_manifest(
        r#"
[cas]
enable = true

[cas.store]
chunk_bytes = 4097

[cas.delta]
enable = false

[cas.signing]
required = false
"#,
    );
    let err = compile_error(&manifest);
    assert!(err.contains("event-pump budget"));
}

#[test]
fn cas_signing_key_required_when_signing_required() {
    let manifest = base_manifest(
        r#"
[cas]
enable = true

[cas.store]
chunk_bytes = 128

[cas.delta]
enable = false

[cas.signing]
required = true
"#,
    );
    let err = compile_error(&manifest);
    assert!(err.contains("cas.signing.key_path"));
}

#[test]
fn cas_signing_section_required_when_enabled() {
    let manifest = base_manifest(
        r#"
[cas]
enable = true

[cas.store]
chunk_bytes = 128

[cas.delta]
enable = false
"#,
    );
    let err = compile_error(&manifest);
    assert!(err.contains("cas.signing section required"));
}

#[test]
fn models_require_cas_enable() {
    let manifest = base_manifest(
        r#"
[ecosystem.models]
enable = true
"#,
    );
    let err = compile_error(&manifest);
    assert!(err.contains("ecosystem.models.enable"));
}

#[test]
fn cas_chunk_bytes_zero_rejected() {
    let manifest = base_manifest(
        r#"
[cas]
enable = true

[cas.store]
chunk_bytes = 0

[cas.delta]
enable = false

[cas.signing]
required = false
"#,
    );
    let err = compile_error(&manifest);
    assert!(err.contains("cas.store.chunk_bytes"));
}
