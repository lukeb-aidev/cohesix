// pass_framework/ir_pass_framework/ir.rs – façade (v9)
// All IR types now live under crate::ir; older modules re-export them here.
pub use crate::ir::function::Function;
pub use crate::ir::module::Module;
pub use crate::ir::context::IRContext;
