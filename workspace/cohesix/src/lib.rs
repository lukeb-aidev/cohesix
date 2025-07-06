// CLASSIFICATION: COMMUNITY
// Filename: lib.rs v1.11
// Date Modified: 2026-12-30
// Conditional `cfg(not(target_os = "uefi"))` sections were removed.
// The library now always compiles all modules for UEFI.
// Author: Lukas Bower
#![cfg_attr(not(feature = "std"), no_std)]
//! Core Cohesix runtime library.

extern crate alloc;
#[cfg(test)]
extern crate std;

/// Cohesix runtime error type.
pub type CohError = alloc::boxed::Box<dyn core::error::Error + Send + Sync>;

/// Helper to create a boxed error from a string.
pub fn new_err(msg: impl Into<String>) -> CohError {
    alloc::boxed::Box::new(StringError(msg.into()))
}

#[macro_export]
macro_rules! coh_bail {
    ($($arg:tt)+) => {
        return Err($crate::new_err(format!($($arg)+)));
    };
}

#[macro_export]
macro_rules! coh_error {
    ($($arg:tt)+) => {
        $crate::new_err(format!($($arg)+))
    };
}

pub mod printk;

/// Prelude re-exporting common `alloc` types for no_std modules
pub mod prelude {
    pub use alloc::{boxed::Box, string::String, vec::Vec};
}

/// Root library for the Coh_CC compiler and platform integrations.
/// Intermediate Representation (IR) core types and utilities
pub mod ir;

/// IR pass framework
pub mod pass_framework;
/// Individual optimization passes
pub mod passes;
/// Code generation backends (C, WASM) and dispatch logic
pub mod codegen;
/// CLI interface for compiler invocation
pub mod cli;
/// Minimal sandbox-safe compiler wrapper
pub mod coh_cc;
/// Core dependencies validation and management
pub mod dependencies;

/// Low-level logging and debugging helpers
pub mod util;
/// Utilities and common helpers used across modules
pub mod utils;

/// Standalone agent helpers
pub mod agent;
/// Migration control-plane helpers
pub mod agent_migration;
/// Transport implementation for migrations
pub mod agent_transport;
/// Agent runtime modules
pub mod agents;
/// Physical sensor modules
pub mod physical;
/// Queen orchestrator modules
pub mod queen;
/// Runtime subsystem modules
pub mod runtime;
/// Swarm runtime modules for distributed deployments
pub mod swarm;
/// Telemetry subsystem utilities
pub mod telemetry;
pub mod metrics;
/// Trace recording modules
pub mod trace;

/// Boot helper modules
pub mod boot;

/// Security modules (capabilities, sandbox enforcement)
pub mod security;

/// Runtime services (telemetry, sandbox, health, ipc)
pub mod services;

/// Common cross-module types.
pub mod cohesix_types;

/// Worker role modules
pub mod worker;

/// Sandbox helpers (profiles, syscall queueing).
pub mod sandbox;

/// Syscall permission guard helpers
pub mod syscall;

/// Kernel modules and drivers
pub mod kernel;

/// CUDA runtime helpers
pub mod cuda;
/// Secure launch module helpers
pub mod slm;

/// Physics simulation bridge
pub mod sim;

/// Cloud integration helpers
pub mod cloud;
/// 9P multiplexer utilities
pub mod p9;

/// Interactive shell loop
pub mod sh_loop;
/// Shell helpers
pub mod shell;

/// Plan 9 userland helpers
#[cfg(feature = "std")]
pub mod plan9;

/// POSIX compatibility helpers
pub mod posix;

/// Filesystem overlay helpers
pub mod fs;

/// Shared world model structures
pub mod world_model;

/// Federation utilities
pub mod federation;
/// Distributed orchestration modules
pub mod orchestrator;
/// Runtime rule validator
pub mod validator;
/// Watchdog daemon module
pub mod watchdogd;

/// Role modules
pub mod roles;

/// Bootloader subcrate utilities
pub mod bootloader;

/// Hardware abstraction layer
pub mod hal;

/// rc style init parser
pub mod rc {
    /// Parser for rc-style init scripts
    pub mod init;
}

#[allow(non_snake_case)]
/// seL4 kernel bindings
pub mod seL4;

/// Role-specific initialization hooks
pub mod init;

/// Compile from an input IR file to the specified output path.
///
/// This helper loads the IR text, constructs a minimal [`ir::Module`],
/// selects a backend based on the `output` extension and writes the generated
/// code to disk.
pub fn compile_from_file(input: &str, output: &str) -> Result<(), CohError> {
    use std::fs;

    // Read the IR text from disk. Return an error if the file is missing.
    let _ir_text = fs::read_to_string(input)?;

    // Parsing of the IR format will be added later; create a stub Module for now.
    // FIXME: parse IR once a format is available. For now create a stub Module.
    let module = ir::Module::new(input);

    // Choose backend based on output path.
    let backend = codegen::infer_backend_from_path(output).unwrap_or(codegen::Backend::C);

    // Dispatch code generation and write to file.
    let code = codegen::dispatch(&module, backend);
    fs::write(output, code)?;

    Ok(())
}

/// Compile an IR file targeting the specified architecture.
pub fn compile_from_file_with_target(
    input: &str,
    output: &str,
    target: &str,
) -> Result<(), CohError> {
    use std::process::Command;

    compile_from_file(input, output)?;

    if output.ends_with(".c") {
        let compiler = std::env::var("CC").unwrap_or_else(|_| "clang".into());
        let status = Command::new(compiler)
            .arg("--target")
            .arg(format!("{}-unknown-linux-gnu", target))
            .arg(output)
            .arg("-c")
            .arg("-o")
            .arg("a.out")
            .status()?;
        if !status.success() {
            eprintln!("Warning: cross compile step failed");
        }
    }
    Ok(())
}

/// Simple string error type for boxed errors.
#[derive(Debug)]
pub struct StringError(String);

impl core::fmt::Display for StringError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl core::error::Error for StringError {}

/// Trait implemented by runtime components that can boot themselves.
pub trait BootableRuntime {
    /// Boot the runtime component.
    fn boot() -> Result<(), CohError>;
}

/// Binary helper modules
pub mod binlib;
