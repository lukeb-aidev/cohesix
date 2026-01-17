// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Guard SwarmUI defaults docs against coh-rtc output.
// Author: Lukas Bower

use coh_rtc::{compile, CompileOptions};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

fn repo_path(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(path)
}

fn extract_snippet<'a>(contents: &'a str, start_marker: &str, end_marker: &str) -> &'a str {
    let start = contents
        .find(start_marker)
        .expect("start marker missing")
        + start_marker.len();
    let end = contents[start..]
        .find(end_marker)
        .map(|idx| start + idx)
        .expect("end marker missing");
    contents[start..end].trim()
}

fn compile_swarmui_defaults(temp_dir: &TempDir) -> PathBuf {
    let manifest_path = repo_path("configs/root_task.toml");
    let options = CompileOptions {
        manifest_path,
        out_dir: temp_dir.path().join("generated"),
        manifest_out: temp_dir.path().join("root_task_resolved.json"),
        cas_manifest_template_out: temp_dir.path().join("cas_manifest_template.json"),
        cli_script_out: temp_dir.path().join("boot_v0.coh"),
        doc_snippet_out: temp_dir.path().join("snippet.md"),
        observability_interfaces_snippet_out: temp_dir.path().join("observability_interfaces.md"),
        observability_security_snippet_out: temp_dir.path().join("observability_security.md"),
        cas_interfaces_snippet_out: temp_dir.path().join("cas_interfaces.md"),
        cas_security_snippet_out: temp_dir.path().join("cas_security.md"),
        cbor_snippet_out: temp_dir.path().join("telemetry_cbor.md"),
        cohsh_policy_out: temp_dir.path().join("cohsh_policy.toml"),
        cohsh_policy_rust_out: temp_dir.path().join("cohsh_policy.rs"),
        cohsh_policy_doc_out: temp_dir.path().join("cohsh_policy.md"),
        cohsh_client_rust_out: temp_dir.path().join("cohsh_client.rs"),
        cohsh_client_doc_out: temp_dir.path().join("cohsh_client.md"),
        cohsh_grammar_doc_out: temp_dir.path().join("cohsh_grammar.md"),
        cohsh_ticket_policy_doc_out: temp_dir.path().join("cohsh_ticket_policy.md"),
        swarmui_defaults_out: temp_dir.path().join("swarmui_defaults.toml"),
        swarmui_defaults_rust_out: temp_dir.path().join("swarmui_defaults.rs"),
        swarmui_defaults_doc_out: temp_dir.path().join("swarmui_defaults.md"),
    };
    compile(&options).expect("compile manifest");
    temp_dir.path().join("swarmui_defaults.md")
}

#[test]
fn swarmui_defaults_snippet_matches_codegen() {
    let temp_dir = TempDir::new().expect("tempdir");
    let generated_path = compile_swarmui_defaults(&temp_dir);
    let generated = fs::read_to_string(&generated_path).expect("read generated swarmui defaults");
    let repo_snippet = fs::read_to_string(repo_path("docs/snippets/swarmui_defaults.md"))
        .expect("read repo swarmui defaults");
    assert_eq!(generated.trim(), repo_snippet.trim());
}

#[test]
fn userland_swarmui_snippet_matches_repo() {
    let userland_path = repo_path("docs/USERLAND_AND_CLI.md");
    let contents = fs::read_to_string(&userland_path).expect("read userland docs");
    let extracted = extract_snippet(
        &contents,
        "<!-- coh-rtc:swarmui-defaults:start -->",
        "<!-- coh-rtc:swarmui-defaults:end -->",
    );
    let repo_snippet = fs::read_to_string(repo_path("docs/snippets/swarmui_defaults.md"))
        .expect("read repo swarmui defaults");
    assert_eq!(extracted.trim(), repo_snippet.trim());
}
