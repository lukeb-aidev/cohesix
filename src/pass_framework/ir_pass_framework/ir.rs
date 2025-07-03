// CLASSIFICATION: COMMUNITY
// Filename: ir.rs v1.0
// Author: Lukas Bower
// Date Modified: 2025-05-31

#[allow(unused_imports)]
use alloc::{boxed::Box, string::String, vec::Vec};
/// Façade module for IR types used in the pass framework.
/// Re-exports core IR components for pass-level access and orchestration.

// pass_framework/ir_pass_framework/ir.rs – façade (v9)
// All IR types now live under crate::ir; older modules re-export them here.
pub use crate::ir::function::Function;
pub use crate::ir::module::Module;
pub use crate::ir::context::IRContext;
