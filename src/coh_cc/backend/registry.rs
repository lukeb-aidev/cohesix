// CLASSIFICATION: COMMUNITY
// Filename: registry.rs v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-18

use std::path::Path;

use crate::coh_cc::parser::input_type::CohInput;

pub trait CompilerBackend {
    fn compile(
        &self,
        input: &CohInput,
        out_path: &Path,
        target: &str,
        sysroot: &Path,
    ) -> anyhow::Result<()>;
}

#[derive(Debug, Clone, Copy)]
pub enum BackendType {
    Tcc,
    Zig,
    Cranelift,
}

pub fn get_backend(name: &str) -> anyhow::Result<Box<dyn CompilerBackend>> {
    match name {
        "" | "tcc" => Ok(Box::new(crate::coh_cc::backend::tcc::TccBackend)),
        "zig" => Err(anyhow::anyhow!("Zig backend not implemented")),
        "cranelift" => Err(anyhow::anyhow!("Cranelift backend not implemented")),
        other => Err(anyhow::anyhow!("Unknown backend {other}")),
    }
}
