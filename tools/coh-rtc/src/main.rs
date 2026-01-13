// Copyright © 2025 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
// Purpose: CLI entry point for the Cohesix root-task compiler.
// Author: Lukas Bower

use anyhow::Result;
use clap::Parser;
use coh_rtc::{compile, default_cbor_snippet_path, default_doc_snippet_path, CompileOptions};
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
    /// Output path for the baseline cohsh CLI script.
    #[arg(long)]
    cli_script: PathBuf,
    /// Output path for the manifest schema snippet.
    #[arg(long, default_value_os_t = default_doc_snippet_path())]
    doc_snippet: PathBuf,
    /// Output path for the CBOR telemetry schema snippet.
    #[arg(long, default_value_os_t = default_cbor_snippet_path())]
    cbor_snippet: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let options = CompileOptions {
        manifest_path: args.manifest,
        out_dir: args.out,
        manifest_out: args.manifest_out,
        cli_script_out: args.cli_script,
        doc_snippet_out: args.doc_snippet,
        cbor_snippet_out: args.cbor_snippet,
    };
    let output = compile(&options)?;
    println!("coh-rtc: wrote {}", output.summary());
    Ok(())
}
