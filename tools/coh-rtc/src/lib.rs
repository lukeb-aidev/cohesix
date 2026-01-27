// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: Expose coh-rtc manifest compilation helpers for tests and the CLI.
// Author: Lukas Bower

pub mod codegen;
pub mod ir;

use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct CompileOptions {
    pub manifest_path: PathBuf,
    pub out_dir: PathBuf,
    pub manifest_out: PathBuf,
    pub cas_manifest_template_out: PathBuf,
    pub cli_script_out: PathBuf,
    pub doc_snippet_out: PathBuf,
    pub gpu_breadcrumbs_snippet_out: PathBuf,
    pub observability_interfaces_snippet_out: PathBuf,
    pub observability_security_snippet_out: PathBuf,
    pub ticket_quotas_snippet_out: PathBuf,
    pub trace_policy_snippet_out: PathBuf,
    pub cas_interfaces_snippet_out: PathBuf,
    pub cas_security_snippet_out: PathBuf,
    pub cbor_snippet_out: PathBuf,
    pub cohesix_py_defaults_out: PathBuf,
    pub cohesix_py_doc_out: PathBuf,
    pub coh_doctor_doc_out: PathBuf,
    pub cohsh_policy_out: PathBuf,
    pub cohsh_policy_rust_out: PathBuf,
    pub cohsh_policy_doc_out: PathBuf,
    pub cohsh_client_rust_out: PathBuf,
    pub cohsh_client_doc_out: PathBuf,
    pub cohsh_grammar_doc_out: PathBuf,
    pub cohsh_ticket_policy_doc_out: PathBuf,
    pub coh_policy_out: PathBuf,
    pub coh_policy_rust_out: PathBuf,
    pub coh_policy_doc_out: PathBuf,
    pub swarmui_defaults_out: PathBuf,
    pub swarmui_defaults_rust_out: PathBuf,
    pub swarmui_defaults_doc_out: PathBuf,
}

pub fn compile(options: &CompileOptions) -> Result<codegen::GeneratedArtifacts> {
    if !options.manifest_path.is_file() {
        bail!(
            "manifest path does not exist or is not a file: {}",
            options.manifest_path.display()
        );
    }

    let manifest = ir::load_manifest(&options.manifest_path)?;
    let manifest_dir = options.manifest_path.parent();
    manifest.validate_with_base(manifest_dir)?;

    let resolved_json = ir::serialize_manifest(&manifest)?;
    let manifest_hash = codegen::hash_bytes(&resolved_json);

    let py_defaults = codegen::cohesix_py::render_defaults(&manifest, &manifest_hash);
    let docs = codegen::DocFragments::from_manifest(&manifest, &manifest_hash, &py_defaults);

    codegen::emit_all(
        &manifest,
        &manifest_hash,
        &resolved_json,
        options,
        &docs,
        &py_defaults,
    )
}

pub fn default_doc_snippet_path() -> PathBuf {
    Path::new("docs")
        .join("snippets")
        .join("root_task_manifest.md")
}

pub fn default_gpu_breadcrumbs_snippet_path() -> PathBuf {
    Path::new("docs")
        .join("snippets")
        .join("gpu_breadcrumbs.md")
}

pub fn default_cli_script_path() -> PathBuf {
    Path::new("scripts").join("cohsh").join("boot_v0.coh")
}

pub fn default_cas_manifest_template_path() -> PathBuf {
    Path::new("out").join("cas_manifest_template.json")
}

pub fn default_cbor_snippet_path() -> PathBuf {
    Path::new("docs")
        .join("snippets")
        .join("telemetry_cbor_schema.md")
}

pub fn default_cohesix_py_defaults_path() -> PathBuf {
    Path::new("tools")
        .join("cohesix-py")
        .join("cohesix")
        .join("generated.py")
}

pub fn default_cohesix_py_doc_path() -> PathBuf {
    Path::new("docs")
        .join("snippets")
        .join("cohesix_py_defaults.md")
}

pub fn default_coh_doctor_doc_path() -> PathBuf {
    Path::new("docs")
        .join("snippets")
        .join("coh_doctor_checks.md")
}

pub fn default_observability_interfaces_snippet_path() -> PathBuf {
    Path::new("docs")
        .join("snippets")
        .join("observability_interfaces.md")
}

pub fn default_observability_security_snippet_path() -> PathBuf {
    Path::new("docs")
        .join("snippets")
        .join("observability_security.md")
}

pub fn default_ticket_quotas_snippet_path() -> PathBuf {
    Path::new("docs").join("snippets").join("ticket_quotas.md")
}

pub fn default_trace_policy_snippet_path() -> PathBuf {
    Path::new("docs").join("snippets").join("trace_policy.md")
}

pub fn default_cas_interfaces_snippet_path() -> PathBuf {
    Path::new("docs").join("snippets").join("cas_interfaces.md")
}

pub fn default_cas_security_snippet_path() -> PathBuf {
    Path::new("docs").join("snippets").join("cas_security.md")
}

pub fn default_cohsh_policy_path() -> PathBuf {
    Path::new("out").join("cohsh_policy.toml")
}

pub fn default_cohsh_policy_rust_path() -> PathBuf {
    Path::new("apps")
        .join("cohsh")
        .join("src")
        .join("generated")
        .join("policy.rs")
}

pub fn default_cohsh_policy_doc_path() -> PathBuf {
    Path::new("docs").join("snippets").join("cohsh_policy.md")
}

pub fn default_cohsh_client_rust_path() -> PathBuf {
    Path::new("apps")
        .join("cohsh")
        .join("src")
        .join("generated")
        .join("client.rs")
}

pub fn default_cohsh_client_doc_path() -> PathBuf {
    Path::new("docs").join("snippets").join("cohsh_client.md")
}

pub fn default_cohsh_grammar_doc_path() -> PathBuf {
    Path::new("docs").join("snippets").join("cohsh_grammar.md")
}

pub fn default_cohsh_ticket_policy_doc_path() -> PathBuf {
    Path::new("docs")
        .join("snippets")
        .join("cohsh_ticket_policy.md")
}

pub fn default_coh_policy_path() -> PathBuf {
    Path::new("out").join("coh_policy.toml")
}

pub fn default_coh_policy_rust_path() -> PathBuf {
    Path::new("apps")
        .join("coh")
        .join("src")
        .join("generated")
        .join("policy.rs")
}

pub fn default_coh_policy_doc_path() -> PathBuf {
    Path::new("docs").join("snippets").join("coh_policy.md")
}

pub fn default_swarmui_defaults_path() -> PathBuf {
    Path::new("out").join("swarmui_defaults.toml")
}

pub fn default_swarmui_defaults_rust_path() -> PathBuf {
    Path::new("apps")
        .join("swarmui")
        .join("src")
        .join("generated.rs")
}

pub fn default_swarmui_defaults_doc_path() -> PathBuf {
    Path::new("docs")
        .join("snippets")
        .join("swarmui_defaults.md")
}
