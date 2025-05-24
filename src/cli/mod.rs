

// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v1.0
// Date Modified: 2025-05-27
// Author: Lukas Bower

//! CLI module for Coh_CC compiler. Exports argument parser and main entry.

pub mod args;

use crate::cli::args::build_cli;
use crate::codegen::dispatch::{dispatch, Backend};
use crate::ir::Module;
use std::fs;

/// Entry point for the CLI. Parses arguments, reads IR, dispatches codegen, and writes output.
pub fn run() -> anyhow::Result<()> {
    let matches = build_cli().get_matches();
    let input_path = matches.value_of("input").unwrap();
    let output_path = matches.value_of("output").unwrap();
    let timeout: u64 = matches
        .value_of("timeout")
        .unwrap()
        .parse()
        .unwrap_or(5000);

    // Load IR from file (TODO: parse actual IR format)
    let ir_text = fs::read_to_string(input_path)?;
    // For now, stub: create empty module with name
    let module = Module::new(input_path);

    // Infer backend based on output extension
    let backend = Backend::C; // TODO: infer from output_path using infer_backend_from_path

    let code = dispatch(&module, backend);
    fs::write(output_path, code)?;

    println!("Generated {} (timeout: {} ms)", output_path, timeout);
    Ok(())
}