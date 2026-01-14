// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: CLI entry point for the Cohesix root-task compiler.
// Author: Lukas Bower

use anyhow::Result;
use clap::Parser;
use coh_rtc::{
    compile, default_cas_interfaces_snippet_path, default_cas_manifest_template_path,
    default_cas_security_snippet_path, default_cbor_snippet_path, default_cli_script_path,
    default_cohsh_policy_doc_path, default_cohsh_policy_path, default_cohsh_policy_rust_path,
    default_doc_snippet_path, default_observability_interfaces_snippet_path,
    default_observability_security_snippet_path, CompileOptions,
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
    /// Output path for the CAS manifest template JSON.
    #[arg(long, default_value_os_t = default_cas_manifest_template_path())]
    cas_manifest_template: PathBuf,
    /// Output path for the baseline cohsh CLI script.
    #[arg(long, default_value_os_t = default_cli_script_path())]
    cli_script: PathBuf,
    /// Output path for the manifest schema snippet.
    #[arg(long, default_value_os_t = default_doc_snippet_path())]
    doc_snippet: PathBuf,
    /// Output path for the observability interfaces snippet.
    #[arg(long, default_value_os_t = default_observability_interfaces_snippet_path())]
    observability_interfaces_snippet: PathBuf,
    /// Output path for the observability security snippet.
    #[arg(long, default_value_os_t = default_observability_security_snippet_path())]
    observability_security_snippet: PathBuf,
    /// Output path for the CAS interfaces snippet.
    #[arg(long, default_value_os_t = default_cas_interfaces_snippet_path())]
    cas_interfaces_snippet: PathBuf,
    /// Output path for the CAS security snippet.
    #[arg(long, default_value_os_t = default_cas_security_snippet_path())]
    cas_security_snippet: PathBuf,
    /// Output path for the CBOR telemetry schema snippet.
    #[arg(long, default_value_os_t = default_cbor_snippet_path())]
    cbor_snippet: PathBuf,
    /// Output path for the cohsh policy TOML.
    #[arg(long, default_value_os_t = default_cohsh_policy_path())]
    cohsh_policy: PathBuf,
    /// Output path for the cohsh policy Rust constants.
    #[arg(long, default_value_os_t = default_cohsh_policy_rust_path())]
    cohsh_policy_rust: PathBuf,
    /// Output path for the cohsh policy doc snippet.
    #[arg(long, default_value_os_t = default_cohsh_policy_doc_path())]
    cohsh_policy_doc: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let options = CompileOptions {
        manifest_path: args.manifest,
        out_dir: args.out,
        manifest_out: args.manifest_out,
        cas_manifest_template_out: args.cas_manifest_template,
        cli_script_out: args.cli_script,
        doc_snippet_out: args.doc_snippet,
        observability_interfaces_snippet_out: args.observability_interfaces_snippet,
        observability_security_snippet_out: args.observability_security_snippet,
        cas_interfaces_snippet_out: args.cas_interfaces_snippet,
        cas_security_snippet_out: args.cas_security_snippet,
        cbor_snippet_out: args.cbor_snippet,
        cohsh_policy_out: args.cohsh_policy,
        cohsh_policy_rust_out: args.cohsh_policy_rust,
        cohsh_policy_doc_out: args.cohsh_policy_doc,
    };
    let output = compile(&options)?;
    println!("coh-rtc: wrote {}", output.summary());
    Ok(())
}
