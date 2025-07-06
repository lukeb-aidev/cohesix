// CLASSIFICATION: COMMUNITY
// Filename: mod.rs v0.9
// Author: Lukas Bower
// Date Modified: 2025-12-08

#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
use crate::{coh_error, CohError};
pub mod backend;
pub mod config;
pub mod guard;
pub mod ir;
pub mod logging;
pub mod parser;
pub mod rust_wrapper;
pub mod toolchain;

/// Compile a source file using the default shell configuration.
/// Returns the path to the generated output on success.
pub fn compile(source: &str) -> Result<Vec<u8>, CohError> {
    let out = std::env::temp_dir().join("cohcc_shell.out");
    let out_str = out
        .to_str()
        .ok_or_else(|| coh_error!("non-UTF8 temp path"))?;
    crate::compile_from_file(source, out_str)?;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"COHB");
    bytes.push(1);
    bytes.extend_from_slice(&std::fs::read(&out)?);
    Ok(bytes)
}
