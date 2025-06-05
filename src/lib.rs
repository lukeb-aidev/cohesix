// CLASSIFICATION: COMMUNITY
// Filename: lib.rs v1.0
// Date Modified: 2025-05-27
// Author: Lukas Bower

//! Root library for the Coh_CC compiler and platform integrations.

/// Intermediate Representation (IR) core types and utilities
pub mod ir;

/// IR pass framework and passes
pub mod pass_framework;
pub mod passes;

/// Code generation backends (C, WASM) and dispatch logic
pub mod codegen;

/// CLI interface for compiler invocation
pub mod cli;

/// Core dependencies validation and management
pub mod dependencies;

/// Utilities and common helpers used across modules
pub mod utils;

/// Runtime subsystem modules
pub mod runtime;

/// Runtime services (telemetry, sandbox, health, ipc)
pub mod services;

/// Compile from input file to output path using the CLI entry point
pub fn compile_from_file(input: &str, output: &str) -> anyhow::Result<()> {
    use std::fs;
    use crate::ir::Module;
    use crate::codegen::{dispatch, Backend};

    let module = Module::new(input);
    let code = dispatch(&module, Backend::C);
    fs::write(output, code)?;
    Ok(())
}
