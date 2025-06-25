// CLASSIFICATION: COMMUNITY
// Filename: registry.rs v0.4
// Author: Lukas Bower
// Date Modified: 2025-07-18

use std::{fs, string::String, vec::Vec, boxed::Box};
use std::path::Path;

use crate::coh_cc::parser::input_type::CohInput;
use crate::coh_cc::toolchain::Toolchain;

pub trait CompilerBackend {
    fn compile(
        &self,
        input: &CohInput,
        out_path: &Path,
        target: &str,
        sysroot: &Path,
        toolchain: &Toolchain,
    ) -> anyhow::Result<()>;
}

#[derive(Debug, Clone, Copy)]
pub enum BackendType {
    Tcc,
    Zig,
    Cranelift,
    // Future backends may include LLVM or WASM implementations.
}

pub fn get_backend(name: &str) -> anyhow::Result<Box<dyn CompilerBackend>> {
    match name {
        "" | "tcc" => Ok(Box::new(crate::coh_cc::backend::tcc::TccBackend)),
        "zig" => Ok(Box::new(crate::coh_cc::backend::zig::ZigBackend)),
        "cranelift" => Err(anyhow::anyhow!("Cranelift backend not implemented")),
        other => Err(anyhow::anyhow!("Unknown backend {other}")),
    }
}
