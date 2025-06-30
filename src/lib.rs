// CLASSIFICATION: COMMUNITY
// Filename: lib.rs v1.11
// Date Modified: 2026-11-27
// Author: Lukas Bower
#![cfg_attr(not(feature = "std"), no_std)]

//! Root library for the Coh_CC compiler and platform integrations.

/// Intermediate Representation (IR) core types and utilities
pub mod ir;

/// IR pass framework
pub mod pass_framework;
/// Individual optimization passes
pub mod passes;

/// Code generation backends (C, WASM) and dispatch logic
pub mod codegen;

/// CLI interface for compiler invocation
#[cfg(not(target_os = "uefi"))]
pub mod cli;

/// Minimal sandbox-safe compiler wrapper
#[cfg(not(target_os = "uefi"))]
pub mod coh_cc;

/// Core dependencies validation and management
#[cfg(not(target_os = "uefi"))]
pub mod dependencies;

/// Utilities and common helpers used across modules
pub mod utils;
/// Low-level logging and debugging helpers
pub mod util;

/// Runtime subsystem modules
pub mod runtime;
/// Telemetry subsystem utilities
#[cfg(not(target_os = "uefi"))]
pub mod telemetry;
/// Agent runtime modules
#[cfg(not(target_os = "uefi"))]
pub mod agents;
/// Standalone agent helpers
#[cfg(not(target_os = "uefi"))]
pub mod agent;
/// Migration control-plane helpers
#[cfg(not(target_os = "uefi"))]
pub mod agent_migration;
/// Transport implementation for migrations
#[cfg(not(target_os = "uefi"))]
pub mod agent_transport;
/// Queen orchestrator modules
#[cfg(not(target_os = "uefi"))]
pub mod queen;
/// Trace recording modules
#[cfg(not(target_os = "uefi"))]
pub mod trace;
/// Swarm runtime modules for distributed deployments
#[cfg(not(target_os = "uefi"))]
pub mod swarm;
/// Physical sensor modules
#[cfg(not(target_os = "uefi"))]
pub mod physical;

/// Boot helper modules
#[cfg(not(target_os = "uefi"))]
pub mod boot;

/// Security modules (capabilities, sandbox enforcement)
#[cfg(not(target_os = "uefi"))]
pub mod security;

/// Runtime services (telemetry, sandbox, health, ipc)
#[cfg(not(target_os = "uefi"))]
pub mod services;
/// Webcam helpers
#[cfg(not(target_os = "uefi"))]
pub mod webcam;

/// Common cross-module types.
pub mod cohesix_types;

/// Worker role modules
#[cfg(not(target_os = "uefi"))]
pub mod worker;

/// Sandbox helpers (profiles, syscall queueing).
#[cfg(not(target_os = "uefi"))]
pub mod sandbox;

/// Syscall permission guard helpers
#[cfg(not(target_os = "uefi"))]
pub mod syscall;

/// Kernel modules and drivers
#[cfg(not(target_os = "uefi"))]
pub mod kernel;

/// CUDA runtime helpers
#[cfg(feature = "cuda")]
pub mod cuda;
/// Secure launch module helpers
#[cfg(not(target_os = "uefi"))]
pub mod slm;

/// Physics simulation bridge
#[cfg(not(target_os = "uefi"))]
pub mod sim;

/// 9P multiplexer utilities
#[cfg(not(target_os = "uefi"))]
pub mod p9;
/// Cloud integration helpers
#[cfg(not(target_os = "uefi"))]
pub mod cloud;
#[cfg(all(feature = "secure9p", not(target_os = "uefi")))]
pub mod secure9p;
/// Networking daemons
#[cfg(not(target_os = "uefi"))]
pub mod net;

/// Shell helpers
pub mod shell;
/// Interactive shell loop
pub mod sh_loop;

/// Plan 9 userland helpers
pub mod plan9;

/// POSIX compatibility helpers
#[cfg(not(target_os = "uefi"))]
pub mod posix;

/// Filesystem overlay helpers
pub mod fs;

/// Shared world model structures
#[cfg(not(target_os = "uefi"))]
pub mod world_model;

/// Distributed orchestration modules
#[cfg(not(target_os = "uefi"))]
pub mod orchestrator;
/// Federation utilities
#[cfg(not(target_os = "uefi"))]
pub mod federation;
/// Runtime rule validator
#[cfg(not(target_os = "uefi"))]
pub mod validator;
/// Watchdog daemon module
#[cfg(not(target_os = "uefi"))]
pub mod watchdogd;

/// Role modules
#[cfg(not(target_os = "uefi"))]
pub mod roles;

/// Bootloader subcrate utilities
#[cfg(not(target_os = "uefi"))]
pub mod bootloader;

/// Hardware abstraction layer
#[cfg(not(target_os = "uefi"))]
pub mod hal;

/// rc style init parser
pub mod rc {
/// Parser for rc-style init scripts
    pub mod init;
}

#[allow(non_snake_case)]
/// seL4 kernel bindings
#[cfg(not(target_os = "uefi"))]
pub mod seL4;

/// Role-specific initialization hooks
#[cfg(not(target_os = "uefi"))]
pub mod init;

/// Compile from an input IR file to the specified output path.
///
/// This helper loads the IR text, constructs a minimal [`ir::Module`],
/// selects a backend based on the `output` extension and writes the generated
/// code to disk.
pub fn compile_from_file(input: &str, output: &str) -> anyhow::Result<()> {
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
) -> anyhow::Result<()> {
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

/// Cohesix runtime error type.
pub type CohError = Box<dyn std::error::Error + Send + Sync>;

/// Trait implemented by runtime components that can boot themselves.
pub trait BootableRuntime {
    /// Boot the runtime component.
    fn boot() -> Result<(), CohError>;
}

/// Binary helper modules
#[cfg(not(target_os = "uefi"))]
pub mod binlib;

