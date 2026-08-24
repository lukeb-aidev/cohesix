// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: CLI entry point for the Cohesix root-task compiler.
// Author: Lukas Bower

use anyhow::Result;
use clap::Parser;
use coh_rtc::{
    compile_with_timer_clock_hz, default_cas_interfaces_snippet_path,
    default_cas_manifest_template_path, default_cas_security_snippet_path,
    default_cbor_snippet_path, default_cli_script_path, default_coh_doctor_doc_path,
    default_coh_policy_doc_path, default_coh_policy_path, default_coh_policy_rust_path,
    default_cohesix_py_defaults_path, default_cohesix_py_doc_path, default_cohsh_client_doc_path,
    default_cohsh_client_rust_path, default_cohsh_grammar_doc_path, default_cohsh_policy_doc_path,
    default_cohsh_policy_path, default_cohsh_policy_rust_path,
    default_cohsh_ticket_policy_doc_path, default_doc_snippet_path,
    default_gpu_breadcrumbs_snippet_path, default_host_integration_doc_path,
    default_host_integration_graph_path, default_host_integration_source_path,
    default_observability_interfaces_snippet_path, default_observability_security_snippet_path,
    default_swarmui_defaults_doc_path, default_swarmui_defaults_path,
    default_swarmui_defaults_rust_path, default_ticket_quotas_snippet_path,
    default_trace_policy_snippet_path, CompileOptions,
};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(author, version, about)]
struct Args {
    /// Path to the root-task manifest TOML file.
    manifest: PathBuf,
    /// Output directory for generated Rust modules.
    #[arg(long)]
    out: PathBuf,
    /// Output path for the resolved manifest JSON.
    #[arg(long = "manifest", alias = "manifest-out")]
    manifest_out: PathBuf,
    /// Console-network counter frequency resolved from the validated target
    /// profile. Omit only when the manifest value is already target-exact.
    #[arg(long)]
    timer_clock_hz: Option<u64>,
    /// Output path for the CAS manifest template JSON.
    #[arg(long, default_value_os_t = default_cas_manifest_template_path())]
    cas_manifest_template: PathBuf,
    /// Output path for the baseline cohsh CLI script.
    #[arg(long, default_value_os_t = default_cli_script_path())]
    cli_script: PathBuf,
    /// Output path for the manifest schema snippet.
    #[arg(long, default_value_os_t = default_doc_snippet_path())]
    doc_snippet: PathBuf,
    /// Output path for the GPU breadcrumb schema snippet.
    #[arg(long, default_value_os_t = default_gpu_breadcrumbs_snippet_path())]
    gpu_breadcrumbs_snippet: PathBuf,
    /// Output path for the observability interfaces snippet.
    #[arg(long, default_value_os_t = default_observability_interfaces_snippet_path())]
    observability_interfaces_snippet: PathBuf,
    /// Output path for the observability security snippet.
    #[arg(long, default_value_os_t = default_observability_security_snippet_path())]
    observability_security_snippet: PathBuf,
    /// Output path for the ticket quota snippet.
    #[arg(long, default_value_os_t = default_ticket_quotas_snippet_path())]
    ticket_quotas_snippet: PathBuf,
    /// Output path for the trace policy snippet.
    #[arg(long, default_value_os_t = default_trace_policy_snippet_path())]
    trace_policy_snippet: PathBuf,
    /// Output path for the CAS interfaces snippet.
    #[arg(long, default_value_os_t = default_cas_interfaces_snippet_path())]
    cas_interfaces_snippet: PathBuf,
    /// Output path for the CAS security snippet.
    #[arg(long, default_value_os_t = default_cas_security_snippet_path())]
    cas_security_snippet: PathBuf,
    /// Output path for the CBOR telemetry schema snippet.
    #[arg(long, default_value_os_t = default_cbor_snippet_path())]
    cbor_snippet: PathBuf,
    /// Output path for Cohesix Python defaults.
    #[arg(long, default_value_os_t = default_cohesix_py_defaults_path())]
    cohesix_py_defaults: PathBuf,
    /// Output path for Cohesix Python defaults doc snippet.
    #[arg(long, default_value_os_t = default_cohesix_py_doc_path())]
    cohesix_py_doc: PathBuf,
    /// Output path for coh doctor doc snippet.
    #[arg(long, default_value_os_t = default_coh_doctor_doc_path())]
    coh_doctor_doc: PathBuf,
    /// Output path for the cohsh policy TOML.
    #[arg(long, default_value_os_t = default_cohsh_policy_path())]
    cohsh_policy: PathBuf,
    /// Output path for the cohsh policy Rust constants.
    #[arg(long, default_value_os_t = default_cohsh_policy_rust_path())]
    cohsh_policy_rust: PathBuf,
    /// Output path for the cohsh policy doc snippet.
    #[arg(long, default_value_os_t = default_cohsh_policy_doc_path())]
    cohsh_policy_doc: PathBuf,
    /// Output path for the cohsh client Rust defaults.
    #[arg(long, default_value_os_t = default_cohsh_client_rust_path())]
    cohsh_client_rust: PathBuf,
    /// Output path for the cohsh client doc snippet.
    #[arg(long, default_value_os_t = default_cohsh_client_doc_path())]
    cohsh_client_doc: PathBuf,
    /// Output path for the cohsh grammar doc snippet.
    #[arg(long, default_value_os_t = default_cohsh_grammar_doc_path())]
    cohsh_grammar_doc: PathBuf,
    /// Output path for the cohsh ticket policy doc snippet.
    #[arg(long, default_value_os_t = default_cohsh_ticket_policy_doc_path())]
    cohsh_ticket_policy_doc: PathBuf,
    /// Output path for the coh policy TOML.
    #[arg(long, default_value_os_t = default_coh_policy_path())]
    coh_policy: PathBuf,
    /// Output path for the coh policy Rust constants.
    #[arg(long, default_value_os_t = default_coh_policy_rust_path())]
    coh_policy_rust: PathBuf,
    /// Output path for the coh policy doc snippet.
    #[arg(long, default_value_os_t = default_coh_policy_doc_path())]
    coh_policy_doc: PathBuf,
    /// Output path for the SwarmUI defaults TOML.
    #[arg(long, default_value_os_t = default_swarmui_defaults_path())]
    swarmui_defaults: PathBuf,
    /// Output path for the SwarmUI defaults Rust constants.
    #[arg(long, default_value_os_t = default_swarmui_defaults_rust_path())]
    swarmui_defaults_rust: PathBuf,
    /// Output path for the SwarmUI defaults doc snippet.
    #[arg(long, default_value_os_t = default_swarmui_defaults_doc_path())]
    swarmui_defaults_doc: PathBuf,
    /// Compiler source for the implementation-surface inventory.
    #[arg(long, default_value = "configs/implementation_surfaces.toml")]
    implementation_surfaces: PathBuf,
    /// Output path for the generated implementation-surface inventory.
    #[arg(
        long,
        default_value = "configs/generated/implementation_surface_inventory.json"
    )]
    implementation_surface_inventory: PathBuf,
    /// Compiler source for host-integration dependency and acceptance truth.
    #[arg(long, default_value_os_t = default_host_integration_source_path())]
    host_integration_source: PathBuf,
    /// Output path for the generated host-integration dependency graph.
    #[arg(long, default_value_os_t = default_host_integration_graph_path())]
    host_integration_graph: PathBuf,
    /// Output path for the generated host-integration documentation table.
    #[arg(long, default_value_os_t = default_host_integration_doc_path())]
    host_integration_doc: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let options = CompileOptions {
        manifest_path: args.manifest.clone(),
        out_dir: args.out,
        manifest_out: args.manifest_out.clone(),
        cas_manifest_template_out: args.cas_manifest_template,
        cli_script_out: args.cli_script,
        doc_snippet_out: args.doc_snippet,
        gpu_breadcrumbs_snippet_out: args.gpu_breadcrumbs_snippet,
        observability_interfaces_snippet_out: args.observability_interfaces_snippet,
        observability_security_snippet_out: args.observability_security_snippet,
        ticket_quotas_snippet_out: args.ticket_quotas_snippet,
        trace_policy_snippet_out: args.trace_policy_snippet,
        cas_interfaces_snippet_out: args.cas_interfaces_snippet,
        cas_security_snippet_out: args.cas_security_snippet,
        cbor_snippet_out: args.cbor_snippet,
        cohesix_py_defaults_out: args.cohesix_py_defaults,
        cohesix_py_doc_out: args.cohesix_py_doc,
        coh_doctor_doc_out: args.coh_doctor_doc,
        cohsh_policy_out: args.cohsh_policy,
        cohsh_policy_rust_out: args.cohsh_policy_rust,
        cohsh_policy_doc_out: args.cohsh_policy_doc,
        cohsh_client_rust_out: args.cohsh_client_rust,
        cohsh_client_doc_out: args.cohsh_client_doc,
        cohsh_grammar_doc_out: args.cohsh_grammar_doc,
        cohsh_ticket_policy_doc_out: args.cohsh_ticket_policy_doc,
        coh_policy_out: args.coh_policy,
        coh_policy_rust_out: args.coh_policy_rust,
        coh_policy_doc_out: args.coh_policy_doc,
        swarmui_defaults_out: args.swarmui_defaults,
        swarmui_defaults_rust_out: args.swarmui_defaults_rust,
        swarmui_defaults_doc_out: args.swarmui_defaults_doc,
    };
    let output = compile_with_timer_clock_hz(&options, args.timer_clock_hz)?;
    let surface_output = coh_rtc::implementation_surface::compile_inventory(
        &args.implementation_surfaces,
        &args.implementation_surface_inventory,
    )?;
    let repo_root = args
        .manifest
        .parent()
        .and_then(std::path::Path::parent)
        .unwrap_or_else(|| std::path::Path::new("."));
    let host_output = coh_rtc::host_integration::compile_graph(
        &args.host_integration_source,
        &args.manifest_out,
        &args.implementation_surface_inventory,
        &repo_root.join("docs/BUILD_PLAN.md"),
        &args.host_integration_graph,
        &args.host_integration_doc,
    )?;
    println!("coh-rtc: wrote {}", output.summary());
    println!(
        "coh-rtc: wrote implementation surfaces {}",
        surface_output.display()
    );
    println!(
        "coh-rtc: wrote host integrations {}",
        host_output.graph.display()
    );
    Ok(())
}
