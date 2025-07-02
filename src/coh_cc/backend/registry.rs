// CLASSIFICATION: COMMUNITY
// Filename: registry.rs v0.4
// Author: Lukas Bower
// Date Modified: 2025-07-18

use crate::prelude::*;
use crate::{coh_error, CohError};
use alloc::boxed::Box;
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
    ) -> Result<(), CohError>;
}

#[derive(Debug, Clone, Copy)]
pub enum BackendType {
    Tcc,
    Zig,
    Cranelift,
    // Future backends may include LLVM or WASM implementations.
}

pub fn get_backend(name: &str) -> Result<Box<dyn CompilerBackend>, CohError> {
    match name {
        "" | "tcc" => Ok(Box::new(crate::coh_cc::backend::tcc::TccBackend)),
        "zig" => Ok(Box::new(crate::coh_cc::backend::zig::ZigBackend)),
        "cranelift" => Err(coh_error!("Cranelift backend not implemented")),
        other => Err(coh_error!("Unknown backend {other}")),
    }
}
