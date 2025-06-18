// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v0.7
// Author: Lukas Bower
// Date Modified: 2025-09-25

pub mod backend;
pub mod config;
pub mod guard;
pub mod ir;
pub mod logging;
pub mod parser;
pub mod rust_wrapper;
pub mod toolchain;

use std::path::PathBuf;

/// Compile a source file using the default shell configuration.
/// Returns the path to the generated output on success.
pub fn compile(source: &str) -> anyhow::Result<Vec<u8>> {
    let out = std::env::temp_dir().join("cohcc_shell.out");
    crate::compile_from_file(source, out.to_str().unwrap())?;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"COHB");
    bytes.push(1);
    bytes.extend_from_slice(&std::fs::read(&out)?);
    Ok(bytes)
}
