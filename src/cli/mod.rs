// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v1.3
// Date Modified: 2026-10-08
// Author: Lukas Bower

use crate::prelude::*;
//! CLI module for Coh_CC compiler. Exports argument parser and main entry.

pub mod args;
pub mod federation;
pub mod cohtrace;

use crate::cli::args::build_cli;
use crate::codegen::dispatch::{dispatch, infer_backend_from_path, Backend};
use crate::pass_framework::ir_pass_framework::Module;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::env;
use crate::coh_cc::ir::schema::load_ir_from_file;

/// Entry point for the CLI. Parses arguments, reads IR, dispatches codegen, and writes output.
pub fn run() -> anyhow::Result<()> {
    let mut args: Vec<String> = env::args().collect();
    let exe = Path::new(&args[0])
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("cohcc")
        .to_string();
    let cmd = if exe == "cohesix" && args.len() > 1 {
        args.remove(1)
    } else {
        exe.clone()
    };
    let remaining = if exe == "cohesix" { args.split_off(2) } else { args.split_off(1) };

    match cmd.as_str() {
        "cohcc" => {
            let matches = build_cli().get_matches_from(remaining);
            let input_path = matches.get_one::<String>("input").expect("required");
            let output_path = matches.get_one::<String>("output").expect("defaulted");
            let timeout: u64 = matches
                .get_one::<String>("timeout")
                .expect("defaulted")
                .parse()
                .unwrap_or(5000);

            let _ir = load_ir_from_file(Path::new(input_path))?;
            let module = Module::new(input_path);
            let backend = infer_backend_from_path(output_path).unwrap_or(Backend::C);
            let code = dispatch(&module, backend);
            fs::write(output_path, code)?;
            println!("Generated {} (timeout: {} ms)", output_path, timeout);
        }
        "cohtrace" => {
            cohtrace::run_cohtrace(&remaining).map_err(anyhow::Error::msg)?;
        }
        "cohcap" => {
            let status = Command::new("/usr/bin/cohcap").args(&remaining).status()?;
            if !status.success() {
                anyhow::bail!("cohcap exited with {:?}", status.code());
            }
        }
        other => {
            eprintln!("unknown cli command: {}", other);
        }
    }
    Ok(())
}
