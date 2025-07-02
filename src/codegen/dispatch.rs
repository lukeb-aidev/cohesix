// CLASSIFICATION: COMMUNITY
// Filename: dispatch.rs v1.0
// Date Modified: 2025-05-27
// Author: Lukas Bower

/// Codegen dispatcher chooses the appropriate backend (C or WASM) to emit code.
use crate::ir::Module;
use crate::prelude::*;

/// Supported code generation backends.
#[derive(Clone, Copy, Debug)]
pub enum Backend {
    /// Generate C source
    C,
    /// Generate WebAssembly text format
    Wasm,
}

/// Dispatches IR `module` to the specified backend, returning the generated code as a String.
pub fn dispatch(module: &Module, backend: Backend) -> String {
    match backend {
        Backend::C => {
            // Defer to C backend
            crate::codegen::c::generate_c(module)
        }
        Backend::Wasm => {
            // Defer to WASM backend
            crate::codegen::wasm::generate_wasm(module)
        }
    }
}

/// Helper to infer backend from file extension.
/// `.c` → C, `.wat` or `.wasm` → Wasm.
pub fn infer_backend_from_path(path: &str) -> Option<Backend> {
    if path.ends_with(".c") {
        Some(Backend::C)
    } else if path.ends_with(".wat") || path.ends_with(".wasm") {
        Some(Backend::Wasm)
    } else {
        None
    }
}
