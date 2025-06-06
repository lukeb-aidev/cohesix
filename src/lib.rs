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

/// Common cross-module types.
pub mod cohesix_types;

/// Sandbox helpers (profiles, syscall queueing).
pub mod sandbox;

/// Kernel modules and drivers
pub mod kernel;

/// Compile from an input IR file to the specified output path.
///
/// This helper loads the IR text, constructs a minimal [`ir::Module`],
/// selects a backend based on the `output` extension and writes the generated
/// code to disk.
pub fn compile_from_file(input: &str, output: &str) -> anyhow::Result<()> {
    use std::fs;

    // Read the IR text from disk. Return an error if the file is missing.
    let _ir_text = fs::read_to_string(input)?;

    // TODO: parse IR once a format is available. For now create a stub Module.
    let module = ir::Module::new(input);

    // Choose backend based on output path.
    let backend = codegen::infer_backend_from_path(output).unwrap_or(codegen::Backend::C);

    // Dispatch code generation and write to file.
    let code = codegen::dispatch(&module, backend);
    fs::write(output, code)?;

    Ok(())
}
