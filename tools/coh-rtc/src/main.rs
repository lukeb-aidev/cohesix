// Author: Lukas Bower
// Purpose: CLI entry point for the Cohesix root-task compiler.

use anyhow::Result;
use clap::Parser;
use coh_rtc::{compile, default_doc_snippet_path, CompileOptions};
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
}

fn main() -> Result<()> {
    let args = Args::parse();
    let options = CompileOptions {
        manifest_path: args.manifest,
        out_dir: args.out,
        manifest_out: args.manifest_out,
        cli_script_out: args.cli_script,
        doc_snippet_out: args.doc_snippet,
    };
    let output = compile(&options)?;
    println!("coh-rtc: wrote {}", output.summary());
    Ok(())
}
