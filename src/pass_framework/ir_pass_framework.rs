// CLASSIFICATION: COMMUNITY
// Filename: ir_pass_framework.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

//! Legacy IR pass façade for compatibility with older call-sites.
//! Re-exports core IR structures (Function, Module, IRContext) for transitional use.

// pass_framework/ir_pass_framework.rs – façade (auto-patch v8)
//
// We now treat IR types canonically under crate::ir; this file merely
// re-exports them for older call-sites.

pub use crate::ir::function::Function;
pub use crate::ir::module::Module;

// Re-export IRContext too, in case something still points here.
pub use crate::ir::context::IRContext;
